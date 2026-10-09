//! `cargo xtask bench records-memory` (landing S10): what a first look at a big
//! Claude transcript costs, release-built, each measure in a process of its own
//! (a process remembers the most memory it ever held). The measures are the
//! ignored tests of `crates/cf-harness/src/claude/record/tests/memory.rs`, which
//! say what each reads: a first look and the memory it peaks at; the same look in
//! its parts, each timed apart; the look after it, which finds nothing new; and
//! the look after a record that has the transcript read again.
//!
//! The transcript is a synthetic one, built like a 327 MB one of 125,000 lines.
//! Nothing reaches the network, and nothing is written but the synthetic
//! transcript, in a temporary folder.
//!
//! - `--transcript FILE` names a real one to read instead (read only; its file
//!   name is its session).
//! - `--lines N` sets the lines of the synthetic one.
//! - `--runs N` repeats the first look (and the look in its parts): 1 is the
//!   default, and 0 leaves those two out.
//! - `--repo DIR` names another checkout of this repository to measure, one from
//!   before a change: it needs this module's files (`tests/memory.rs`,
//!   `tests/synthetic.rs`, and their `mod` lines in `tests.rs`), and the line
//!   `read_on_with(… Transcript::parse …)` of `first_look_parts` read as
//!   `read_on(…)` where the code is from before the parser of a line was chosen.
//!   A relative one is from the checkout's root.
//!
//! Each measure's heading is said when it starts, and what it printed, indented,
//! when it ends; its own lines only, not cargo's or the test harness's. A measure
//! that fails has all it wrote said on the standard error, and the status is 1.

use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::context::Context;
use crate::dispatch::{Command, Console, Failure, Run};
use crate::process::{self, Captured, Invocation};

/// The tests that are the measures, by their path in cf-harness's library.
const MODULE: &str = "claude::record::tests::memory";
/// The measures that repeat with `--runs`, and the ones that run once after them.
const REPEATED: [&str; 2] = ["first_look", "first_look_parts"];
const ONCE: [&str; 2] = ["unchanged_look", "reread"];
const USAGE: &str = "[--runs N] [--lines N] [--transcript FILE] [--repo DIR]";

pub const COMMANDS: &[Command] = &[Command {
    words: &["bench", "records-memory"],
    about: "Measure what a first look at a big Claude transcript costs, release-built",
    usage: USAGE,
    run: Run::Native(run),
}];

fn run(context: &Context, args: &[OsString], console: &mut Console) -> Result<i32, Failure> {
    let options = Options::read(args)?;
    let repo = options.repo_in(context);
    report(
        options.runs,
        |name| process::capture(&measure(&repo, name, &options), &context.env),
        console.out,
        console.err,
    )
}

/// What the command line gave.
struct Options {
    runs: u32,
    /// For the variables the measures read, as they were given.
    lines: Option<OsString>,
    transcript: Option<OsString>,
    repo: Option<PathBuf>,
}

impl Options {
    /// Reads `args`: each option as `--name VALUE` or `--name=VALUE`, the last of
    /// one given twice. Another word, an option with no value, and a number of
    /// runs that is not a whole number are refused.
    fn read(args: &[OsString]) -> Result<Self, Failure> {
        let mut options = Self {
            runs: 1,
            lines: None,
            transcript: None,
            repo: None,
        };
        let mut rest = args.iter();
        while let Some(arg) = rest.next() {
            let Some((name, joined)) = option(arg) else {
                return Err(refused(arg));
            };
            let value = match joined {
                Some(value) => value,
                None => value_after(name, rest.next())?,
            };
            match name {
                "--runs" => options.runs = whole_number(&value)?,
                "--lines" => options.lines = Some(value),
                "--transcript" => options.transcript = Some(value),
                _ => options.repo = Some(PathBuf::from(value)),
            }
        }
        Ok(options)
    }

    /// Where the measures run: the checkout this is, or the one `--repo` names
    /// (from the checkout's root, if it is a relative path).
    fn repo_in(&self, context: &Context) -> PathBuf {
        self.repo
            .as_ref()
            .map_or_else(|| context.root.clone(), |dir| context.root.join(dir))
    }
}

/// The options the command takes.
const OPTIONS: [&str; 4] = ["--runs", "--lines", "--transcript", "--repo"];

/// `arg` as one of the options, `--name` or `--name=VALUE`: the name, and the
/// value when it came with it.
fn option(arg: &OsStr) -> Option<(&'static str, Option<OsString>)> {
    let text = arg.to_str()?;
    let (name, joined) = match text.split_once('=') {
        Some((name, value)) => (name, Some(OsString::from(value))),
        None => (text, None),
    };
    let known = OPTIONS.into_iter().find(|known| *known == name)?;
    Some((known, joined))
}

/// The value of the option `name`, the word after it. A word that begins with a
/// dash is another option, not a value: `--name=-1` is how a value begins with one.
fn value_after(name: &str, word: Option<&OsString>) -> Result<OsString, Failure> {
    match word {
        Some(word) if !word.to_string_lossy().starts_with('-') => Ok(word.clone()),
        _ => Err(Failure::Usage(format!(
            "bench records-memory: {name} needs a value"
        ))),
    }
}

/// A word that is not one the command takes: what it takes instead.
fn refused(word: &OsStr) -> Failure {
    Failure::Usage(format!(
        "bench records-memory takes {USAGE}, not {}",
        word.to_string_lossy()
    ))
}

/// The number of runs `value` says.
fn whole_number(value: &OsStr) -> Result<u32, Failure> {
    value
        .to_str()
        .and_then(|text| text.parse().ok())
        .ok_or_else(|| {
            Failure::Usage(format!(
                "bench records-memory: --runs takes a whole number, not {}",
                value.to_string_lossy()
            ))
        })
}

/// One measure, run alone: the test `name`, release-built and ignored by
/// default, from `repo`, with the variables the options set.
fn measure(repo: &Path, name: &str, options: &Options) -> Invocation {
    let mut invocation = Invocation::new("cargo", repo)
        .args([
            "test",
            "--offline",
            "--release",
            "-p",
            "cf-harness",
            "--lib",
        ])
        .arg(format!("{MODULE}::{name}"))
        .args(["--", "--ignored", "--nocapture", "--exact"]);
    if let Some(lines) = &options.lines {
        invocation = invocation.var("CF_RECORDS_MEMORY_LINES", lines);
    }
    if let Some(transcript) = &options.transcript {
        invocation = invocation.var("CF_RECORDS_MEMORY_TRANSCRIPT", transcript);
    }
    invocation
}

/// Says each measure's heading, runs it with `capture` and says what it printed,
/// the first look and the look in its parts `runs` times each, then the other
/// two. The status is 0, or 1 at the first measure that fails.
fn report(
    runs: u32,
    mut capture: impl FnMut(&str) -> Result<Captured, process::Failure>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<i32, Failure> {
    for name in REPEATED {
        for run in 1..=runs {
            let which = if runs > 1 {
                format!(" (run {run})")
            } else {
                String::new()
            };
            if !look(name, &which, &mut capture, out, err)? {
                return Ok(1);
            }
        }
    }
    for name in ONCE {
        if !look(name, "", &mut capture, out, err)? {
            return Ok(1);
        }
    }
    Ok(0)
}

/// One measure: its heading, then its lines. False when it failed, which it
/// says on `err` with everything the run wrote.
fn look(
    name: &str,
    which: &str,
    capture: &mut impl FnMut(&str) -> Result<Captured, process::Failure>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<bool, Failure> {
    writeln!(out, "{}{which}:", name.replace('_', " "))?;
    let done = capture(name)?;
    if done.code != 0 {
        write!(err, "{}{}", done.stdout, done.stderr)?;
        writeln!(
            err,
            "xtask: {name} failed: its cargo test ended with status {}",
            done.code
        )?;
        return Ok(false);
    }
    let printed = done.stdout.split('\n').filter(|line| {
        !line.is_empty() && !line.starts_with("running") && !line.starts_with("test ")
    });
    for line in printed {
        writeln!(out, "  {line}")?;
    }
    Ok(true)
}

#[cfg(test)]
mod tests;

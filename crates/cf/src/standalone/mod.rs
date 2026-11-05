//! The standalone verbs of the CLI, answered here as Node answered them, word
//! for word: the usage, the version, and `setup` and `doctor`, which wire the launcher, the stale hooks and the
//! app's preparation (`cf_launcher`, `cf_harness`). `tests/cli_goldens` holds
//! them to what Node said, file by file (the recording was made from Node and
//! is fixed since: `tests/goldens/README.md`).
//!
//! They are what `cf` answers for a tokenless command (a window has its
//! participant's token, and there `cf` is the board). `ui` is the daemon's,
//! which `main` runs before any of this (`crate::native_ui`): reaching this
//! module it is an unknown command, like any word that is no verb.
//!
//! A verb says what it prints as it goes, and what stops it as `cf: <words>`
//! with exit code 1: Node's `fail` and every error `main` caught.
//!
//! Every verb answers `--help` and `-h` with its usage and exit code 0, and
//! reads and writes nothing of the home, as Node never did (it refused the
//! word as an option no verb has). None of the verbs takes text of its own: a
//! name or a value is the most it takes, so the word is a request for help
//! wherever it stands ahead of a `--`, and a name after a `--` is a name.

mod doctor;
mod setup;

use std::ffi::OsString;
use std::io::{self, Write};
use std::path::PathBuf;

use cf_base::env::Env;
use cf_base::home::config_root;
use cf_base::js;
use cf_base::refusal::Refusal;
use cf_daemon::machine;
use cf_harness::detect::detect_harnesses;
use serde_json::Value;

/// Said when the environment names no folder to keep ConsensFlow's things in.
/// Node asked the system for the user's home then; here it is not asked for.
const NO_HOME: &str =
    "ConsensFlow has no folder to keep its things in: set CONSENSFLOW_HOME, or HOME";

/// What `cf help` prints, with the version where `{version}` stands.
const USAGE: &str = include_str!("usage.txt");

/// What stops a verb: words it says, or the output it could not write.
enum Stop {
    /// `cf: <words>`, and exit code 1.
    Said(String),
    /// The reader of the output went away, or the output failed.
    Io(io::Error),
}

impl From<io::Error> for Stop {
    fn from(failed: io::Error) -> Self {
        Self::Io(failed)
    }
}

impl From<Refusal> for Stop {
    fn from(refusal: Refusal) -> Self {
        Self::Said(refusal.message)
    }
}

/// How a verb ended.
type Done = Result<(), Stop>;

/// Runs `args` as a standalone verb: its exit code, and a word that is none is
/// an unknown command. The caller has found no window's token. Only a failure
/// to write is an error.
pub fn run(
    env: &Env,
    args: &[OsString],
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> io::Result<u8> {
    // Node reads its arguments as UTF-8, and a byte that is none as U+FFFD.
    let words: Vec<String> = args
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    let (command, rest) = words
        .split_first()
        .map_or((None, &[][..]), |(command, rest)| {
            (Some(command.as_str()), rest)
        });
    let done = match command {
        None | Some("help" | "--help" | "-h") => writeln!(out, "{}", usage()).map_err(Stop::from),
        Some("--version" | "-v" | "version") => {
            writeln!(out, "{}", env!("CARGO_PKG_VERSION")).map_err(Stop::from)
        }
        // The word that asks for a verb's usage is answered before the verb
        // reads its words or the home, whatever else follows it.
        Some(verb @ ("setup" | "doctor" | "ui")) if asks_for_help(rest) => {
            writeln!(out, "{}", usage_of(&[verb])).map_err(Stop::from)
        }
        Some("setup") => setup::run(env, rest, out),
        // Whatever words follow it are no matter, as in Node.
        Some("doctor") => doctor::run(env, out),
        Some(other) => Err(Stop::Said(format!(
            "unknown command {} — run `cf help`",
            js::stringify(&Value::from(other))
        ))),
    };
    match done {
        Ok(()) => Ok(0),
        Err(Stop::Said(words)) => {
            writeln!(err, "cf: {words}")?;
            Ok(1)
        }
        Err(Stop::Io(failed)) => Err(failed),
    }
}

/// The usage, as Node's CLI printed it: it ends with a line break of its own,
/// and `cf help` adds another.
fn usage() -> String {
    USAGE.replace("{version}", env!("CARGO_PKG_VERSION"))
}

/// Whether `words`, what follows a verb, ask for its usage: `--help` or `-h`
/// stands among them ahead of any `--`.
pub(crate) fn asks_for_help(words: &[String]) -> bool {
    words
        .iter()
        .take_while(|word| *word != "--")
        .any(|word| word == "--help" || word == "-h")
}

/// The lines of the usage that belong to the command `cf <path…>` (`["setup"]`),
/// with the lines that go on from them.
fn usage_of(path: &[&str]) -> String {
    let usage = usage();
    let mut kept = false;
    let mut lines = Vec::new();
    for line in usage.lines() {
        match line.strip_prefix("  ") {
            Some(command) if !command.starts_with(' ') => {
                let mut words = command.split_whitespace();
                kept = path.iter().all(|wanted| words.next() == Some(*wanted));
            }
            Some(_) => {}
            None => kept = false,
        }
        if kept {
            lines.push(line);
        }
    }
    lines.join("\n")
}

/// What stops a verb that needs ConsensFlow's folder where none is named.
fn no_home() -> Stop {
    Stop::Said(NO_HOME.to_owned())
}

/// ConsensFlow's folder, which the verbs that keep something in it refuse to
/// go on without.
fn home(env: &Env) -> Result<PathBuf, Stop> {
    config_root(env).ok_or_else(no_home)
}

/// The harnesses installed here, by id, in the order detection lists them.
fn harness_ids(env: &Env) -> Vec<&'static str> {
    detect_harnesses(env)
        .iter()
        .map(|found| found.id.as_str())
        .collect()
}

/// The `cf` of the bundle this program is in: the one a launcher runs, and
/// the one `doctor` asks a launcher about.
fn own_cf() -> Result<PathBuf, Stop> {
    match std::env::current_exe() {
        Ok(exe) => Ok(machine::bundle_of(&exe).cf),
        Err(cause) => Err(Stop::Said(format!(
            "cannot tell which folder it is in: {cause}"
        ))),
    }
}

#[cfg(test)]
mod tests;

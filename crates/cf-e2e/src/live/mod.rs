//! What the opt-in live tests share. A live test runs real harnesses on the
//! machine's own logins and spends real quota, so it is ignored unless asked
//! for (`cargo xtask live <name>`, which runs `tests/live_<name>.rs`), and it
//! needs the machine to be set up for it: the harnesses installed and logged in.
//! The rest is as the rig's cases: the `cf` and the pane host built from this
//! checkout, in a home of the case's own that is never `~/.consensflow`.
//!
//! The support is a module for each thing it does, and a live test is made of
//! them as the shape of `tests/live_paste.rs` shows:
//!
//! - [`machine`] is this machine: the harnesses it has, found as a shell finds
//!   them with the home each reads its login from, and the rig whose windows run
//!   them; the programs Claude Code is started through; the folders a live test
//!   works in. [`who`] says which of them a check runs, a harness or the
//!   stand-in agent.
//! - [`agents`] says which model each harness runs on in a live test, and what
//!   its roster row is.
//! - [`window`] opens a real harness's window as the app opens one, trusts its
//!   folder for Claude Code, and has it take a paste and answer.
//! - [`claude`] reads what Claude Code keeps of a session (its status, its
//!   transcript), and the processes of the machine.
//! - [`codex`] shuts Codex's MCP servers out of a run, and drives its app-server.
//! - [`trace`] reads what the daemon wrote as it went (its event file).
//! - [`ask`] asks the harness code of the product what a test needs of it (its
//!   own record of a conversation, a window's command line, the text a window
//!   takes, a member's Devin launch whole), through the test binary `harness-ask`.
//! - [`calls`] is the stand-in `cf` of a window that has no daemon to reach, and
//!   what it wrote down of the calls it was given.
//! - [`members`] is the rig of the checks that play the chief with its token and
//!   run real members.
//! - [`tree`] says what a worker's turn left running: the processes its window's
//!   tree started in it, as the pane host lists them.
//! - [`human`] plays the human at a project whose chief is a real harness (the
//!   page's calls, and the keys typed into the chief's window), and [`cleanup`]
//!   has the harnesses forget what they kept of a run's folder.
//! - [`wire`] says Devin's wire log in runs of what its records are, and how it ends.
//! - [`report`] says what each check of a run came to, and ends the run with
//!   them ([`report::conclude`]).
//!
//! What a run is asked to do is in variables that `cargo xtask live` sets from
//! its flags ([`option`], [`list`], and the shared [`KEEP`], [`HARNESS`] and
//! [`STEPS`]); what a live run says as it goes, and how it waits, is here too:
//! [`Log`] says what the run is doing with the seconds since it began (and,
//! with [`Log::found`], which harness it found and where), [`version_of`] names
//! a harness's version, [`wait`] and [`until`] say what they wait for and for how
//! long, and [`text`], [`cut`] and [`last_lines`] make what the product answered,
//! or what a window showed, fit a line.

pub mod agents;
pub mod ask;
pub mod calls;
pub mod claude;
pub mod cleanup;
pub mod codex;
pub mod human;
pub mod machine;
pub mod members;
pub mod report;
pub mod trace;
pub mod tree;
pub mod who;
pub mod window;
pub mod wire;

#[cfg(test)]
mod tests;

use std::fmt::Display;
use std::io::{self, Write};
use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::process::{own_var, Run};
use crate::rig::Rig;
use crate::{files, Error, Result};

pub use machine::{
    codex, command, installed, isolated_claude, rig, rig_with_stand_ins, shimmed_claude,
    shimmed_claude_with_servers, Harness, Installed, ISOLATION,
};
pub use who::{between, Who};

/// What every variable a live run reads its options by begins with: `cargo xtask
/// live <name> --some-flag value` sets `CONSENSFLOW_LIVE_SOME_FLAG` to `value`
/// (see [`variable`]), and the flag given more than once sets it to all its values
/// with commas between them.
pub const PREFIX: &str = "CONSENSFLOW_LIVE_";

/// The variable that leaves a live run's home and project where they are, for a
/// person to look at (the flag `--keep`, which takes no value).
pub const KEEP: &str = "CONSENSFLOW_LIVE_KEEP";

/// The variable that limits a live run to some harnesses, `claude,pi` (the flag
/// `--harness`, and the words that name harnesses with no flag in front of them).
pub const HARNESS: &str = "CONSENSFLOW_LIVE_HARNESS";

/// The variable that limits a live run to some of its steps or cases,
/// `worker,question` (the flag `--steps`).
pub const STEPS: &str = "CONSENSFLOW_LIVE_STEPS";

/// The variable the flag `flag` sets: `PREFIX`, then the flag in capitals with
/// its dashes as underscores (`--input-box` is `CONSENSFLOW_LIVE_INPUT_BOX`).
pub fn variable(flag: &str) -> String {
    format!("{PREFIX}{}", flag.to_ascii_uppercase().replace('-', "_"))
}

/// What the variable `name` is set to, when it is set to something.
pub fn requested(name: &str) -> Option<String> {
    own_var(name).filter(|value| !value.trim().is_empty())
}

/// What the flag `flag` of the run was given (`--hold 90` is `Some("90")`), when
/// it was given any.
pub fn option(flag: &str) -> Option<String> {
    requested(&variable(flag))
}

/// What the flag `flag` was given as a list, in the order given: `--harness a
/// --harness b`, and `--harness a,b`, are both `["a", "b"]`; none when it was
/// given none.
pub fn list(flag: &str) -> Vec<String> {
    words(option(flag).as_deref().unwrap_or_default())
}

/// The words of a list a flag was given: the pieces between commas, trimmed,
/// without the empty ones.
fn words(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|word| !word.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Whether the run is to leave what it made: [`KEEP`] is set to something that
/// is not empty or `0`.
pub fn keep_requested() -> bool {
    kept(own_var(KEEP).as_deref())
}

fn kept(value: Option<&str>) -> bool {
    value.is_some_and(|value| !value.is_empty() && value != "0")
}

/// What a live run says as it goes, on the error output (which a test's
/// capture does not hold back), with the seconds since it began.
pub struct Log {
    name: String,
    started: Instant,
}

impl Log {
    /// A run called `name` (`live designer`), beginning now.
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            started: Instant::now(),
        }
    }

    /// The same run, saying `scope` after its name: one project of it.
    #[must_use]
    pub fn scoped(&self, scope: impl Display) -> Self {
        Self {
            name: format!("{} {scope}", self.name),
            started: self.started,
        }
    }

    /// Says `text`, with the seconds since the run began.
    pub fn say(&self, text: impl Display) {
        let seconds = self.started.elapsed().as_secs_f64();
        let _ = writeln!(io::stderr(), "[{seconds:>6.1} s] {}: {text}", self.name);
    }

    /// The time since the run began, which a check notes what it saw at.
    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    /// `title`, then `body` a line at a time, indented.
    pub fn block(&self, title: &str, body: &str) {
        let indented: Vec<String> = body.lines().map(|line| format!("    {line}")).collect();
        self.say(format!("{title}:\n{}", indented.join("\n")));
    }

    /// Finds `harness` on the machine as a person's shell does ([`installed`]: one
    /// that is not there, or not logged in, is the error, before anything is
    /// spent) and says what was found.
    pub fn found(&self, harness: Harness) -> Result<Installed> {
        let install = installed(harness)?;
        self.say(found_line(&install, &install.version()));
        Ok(install)
    }
}

/// The version of `harness` as its program on this machine says it, or `unknown`
/// and why where it cannot be found or run. A harness may update itself while a
/// run goes, so a report names the version before and after.
pub fn version_of(harness: Harness) -> String {
    said_version(installed(harness))
}

/// The version of the harness `found` says it is, or why there is none.
fn said_version(found: Result<Installed>) -> String {
    found.map_or_else(
        |failed| format!("unknown ({failed})"),
        |install| install.version(),
    )
}

/// What a run says of a harness it found: where its program is, the version it
/// says it is, and where its login is read from.
fn found_line(install: &Installed, version: &str) -> String {
    format!(
        "{} is {} ({version}); its login is read from {}",
        install.harness().name(),
        install.command().display(),
        install.home().display()
    )
}

/// A JSON string as the text it is; anything else as JSON writes it.
pub fn text(value: &Value) -> String {
    value
        .as_str()
        .map_or_else(|| value.to_string(), str::to_owned)
}

/// `text` on one line, and no longer than `most` characters.
pub fn cut(text: &str, most: usize) -> String {
    let line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.chars().count() > most {
        let head: String = line.chars().take(most).collect();
        format!("{head}…")
    } else {
        line
    }
}

/// The last lines of what a window printed, as a line: the last eight that
/// are not blank, trimmed and joined with `⏎`. Enough of its screen for a
/// report to say why a window did not open or did not answer.
pub fn last_lines(text: &str) -> String {
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    lines[lines.len().saturating_sub(8)..].join(" ⏎ ")
}

/// A span of milliseconds as seconds with a decimal, `12.3`.
pub fn seconds(milliseconds: i64) -> String {
    format!("{:.1}", milliseconds as f64 / 1000.0)
}

/// A span of time as seconds with a decimal, `12.3`.
pub fn secs(span: Duration) -> String {
    format!("{:.1}", span.as_secs_f64())
}

/// A read of the daemon as indented JSON, or why it could not be made.
pub fn pretty(read: Result<Value>) -> String {
    match read {
        Ok(value) => serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string()),
        Err(failed) => format!("(could not be read: {failed})"),
    }
}

/// How many of the daemon's log's lines that are warnings or errors a report shows.
const WARNINGS: usize = 20;

/// The lines of the daemon's log that are warnings or errors, the first few.
pub fn warnings(log: &str) -> Vec<&str> {
    log.lines()
        .filter(|line| line.contains(" warn ") || line.contains(" error "))
        .take(WARNINGS)
        .collect()
}

/// How much of an item of a transcript a report shows, how many of a
/// transcript's last items, and how much of an item's id (which tells two
/// items that read alike apart).
const SHOWN: usize = 400;
const ITEMS: usize = 20;
const ID: usize = 14;

/// What the window that had the task `task` of the project `project` recorded of
/// it, as the page's drawer shows it: each item by its role, the last few.
pub fn transcript(rig: &Rig, project: i64, task: i64) -> String {
    let read = rig.page(
        "task.transcript",
        serde_json::json!({ "project": project, "task": task }),
    );
    let items = match read {
        Ok(answer) => answer["items"].as_array().cloned().unwrap_or_default(),
        Err(failed) => return format!("(could not be read: {failed})"),
    };
    if items.is_empty() {
        return "(nothing recorded)".to_owned();
    }
    let skipped = items.len().saturating_sub(ITEMS);
    let mut lines = Vec::new();
    if skipped > 0 {
        lines.push(format!("({skipped} earlier items not shown)"));
    }
    lines.extend(items.iter().skip(skipped).map(|item| {
        format!(
            "[{} {}] {}",
            text(&item["role"]),
            cut(&text(&item["id"]), ID),
            cut(&text(&item["text"]), SHOWN)
        )
    }));
    lines.join("\n")
}

/// How often the daemon is asked how it is getting on while a run waits.
const LOOK: Duration = Duration::from_secs(1);

/// `rig.wait_for`, looking once a second (each look asks the daemon), which
/// says what it waits for, and for how long, before it does.
pub fn wait(
    rig: &Rig,
    log: &Log,
    what: &str,
    within: Duration,
    mut check: impl FnMut() -> Result<bool>,
) -> Result {
    log.say(format!("waiting up to {} s for {what}", within.as_secs()));
    let mut looked: Option<Instant> = None;
    rig.wait_for(what, within, || {
        if looked.is_some_and(|at| at.elapsed() < LOOK) {
            return Ok(false);
        }
        looked = Some(Instant::now());
        check()
    })
}

/// What `look` first finds within `within`, looking again every `step`: none if
/// it finds nothing in that time. A wait that asks nothing of the daemon (a
/// window's record, a file) and ends with what it found, not with an error.
pub fn until<T>(
    within: Duration,
    step: Duration,
    mut look: impl FnMut() -> Option<T>,
) -> Option<T> {
    let started = Instant::now();
    loop {
        if let Some(found) = look() {
            return Some(found);
        }
        if started.elapsed() >= within {
            return None;
        }
        std::thread::sleep(step);
    }
}

/// Makes `folder` a git repository, as `git init` does, in the folders made for
/// it: what a live test's demo project begins as.
pub fn git_repository(folder: &Path) -> Result {
    files::make_dir(folder)?;
    let ran = Run::new(command("git")?)
        .args(["init", "--quiet"])
        .cwd(folder)
        .inheriting_env()
        .run()?;
    if ran.code != Some(0) {
        return Err(Error::Program {
            action: "run",
            program: "git init".to_owned(),
            source: io::Error::other(ran.to_string()),
        });
    }
    Ok(())
}

//! Claude Code's own live status of each of its running processes, from the
//! `sessions/<pid>.json` files it keeps: the conversation the process shows,
//! and busy, idle or waiting, with the reason (a permission prompt, input
//! needed, a dialog). A file whose process is gone says nothing.

use std::fs;
use std::io::Read as _;
use std::path::Path;
use std::time::{Duration, SystemTime};

use cf_base::env::Env;
use cf_base::json::from_slice_lossy;
use cf_base::path;
use serde_json::Value;

use crate::shared::paths::home;
use crate::shared::record::find::entries;

/// What Claude says of one of its processes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    /// The conversation the process shows.
    pub(crate) session: String,
    pub(crate) state: State,
}

/// What Claude Code's status says a window waits on while its question tool's dialog is open
/// (`waitingFor`, 2.1.317): the window's own question, never a dialog of Claude Code's (T-1183).
pub(super) const QUESTION_TOOL: &str = "input needed";

/// Whether a Claude is at work, at its prompt, or waiting on its own dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    Working,
    Idle,
    /// Waiting, and on what where Claude said.
    Waiting(Option<String>),
}

/// The folder Claude keeps its statuses in: `sessions` in its config folder,
/// which is `CLAUDE_CONFIG_DIR` (an empty one too), else `.claude` in the
/// home, made whole against the working folder now (`path.resolve`, its
/// `..` taken off by the join after it).
///
/// Kept from Node on purpose: the home is the environment's `HOME`, else
/// its `USERPROFILE` (Windows' own), as every home of the harnesses here
/// is, and an environment that names neither has none. Node's adapter read
/// the process's own home (`os.homedir()`) when `HOME` was not set, which
/// is that same `USERPROFILE` when the environment is the process's.
pub(super) fn folder(env: &Env) -> Result<String, String> {
    let root = match env.os("CLAUDE_CONFIG_DIR") {
        Some(root) => root.to_string_lossy().into_owned(),
        None => path::join(&[&home(env)?, ".claude"]),
    };
    let named = if root.is_empty() { "." } else { &root };
    let whole = std::path::absolute(named).map_err(|failed| failed.to_string())?;
    Ok(path::join(&[&whole.to_string_lossy(), "sessions"]))
}

/// The status of each Claude running, by its process id, as one look reads it.
pub(super) fn statuses(folder: &str) -> Vec<(u32, Status)> {
    Scanner::default().statuses(folder)
}

/// The Claudes running that show the conversation `session`, by process id: none
/// where the folder cannot be read, which says nothing of who runs. A parked job
/// (`parkedJobId`) is no holder of it, as Claude Code's own count of a
/// conversation's live sessions passes over one (2.1.310). Each of them writes
/// the conversation's transcript as it goes, and Claude Code lets two show one
/// conversation at once: one the human started outside ConsensFlow, beside a
/// window's (`claude --resume <id>`).
pub(super) fn holders(folder: &str, session: &str) -> Option<Vec<u32>> {
    Scanner::default().holders(folder, session)
}

/// Claude's statuses, read each look afresh: nothing about a file is kept from one look to the
/// next (P22-1, T-2029: on Windows each look opened every process a file named, and the files of
/// ended windows pile up; T-2052 and T-2081: what a look remembered of a file went wrong where a
/// number was reused or a file rewritten).
///
/// A look lists the folder and asks of each status file's name (`<pid>.json`, the number Claude
/// writes it under) whether that process is alive; a file whose number is not is skipped without
/// being opened, as the Claude that wrote it is gone. Every other file is read as main reads it:
/// its bytes, then its time from the same handle (a link's target is judged by the target's own
/// time), then the process the file says. So a look makes the calls of the listing and of the
/// files of live numbers, and one process question for each name. Asking whether a number is
/// alive is no file-system call (a signal of no effect on Unix, a handle that fails at once for a
/// number nobody holds on Windows), and costs microseconds a name.
///
/// Known limit: a live Claude that writes its status under a name that is not its own number is
/// not seen while that name's number is not alive. Claude Code names its file `<pid>.json`.
#[derive(Debug)]
pub(crate) struct Scanner {
    /// Whether a process is alive: the system's, and a test's own where it must name numbers.
    alive: fn(u32) -> bool,
    /// How many status files were opened, for a test to count.
    pub(crate) reads: usize,
    /// Every call made to the file system: a listing, an open, a metadata call, a read.
    pub(crate) fs_calls: usize,
    /// How many times a process was asked of whether it is alive, apart from the file system.
    pub(crate) alive_asks: usize,
}

impl Default for Scanner {
    fn default() -> Self {
        Self::asking(cf_process::alive)
    }
}

/// What one status file says.
struct Seen {
    pid: u32,
    status: Status,
    parked: bool,
}

impl Scanner {
    /// A scanner that asks `alive` whether a process is: a test's, where it must say which
    /// numbers are.
    pub(crate) fn asking(alive: fn(u32) -> bool) -> Self {
        Self {
            alive,
            reads: 0,
            fs_calls: 0,
            alive_asks: 0,
        }
    }

    /// The status of each Claude running, by its process id, in the order
    /// Node's `readdir` lists their files: a second file naming the same
    /// process puts its status where the first was, as a `Map` keeps a key's
    /// place. A folder that cannot be read holds none.
    pub(crate) fn statuses(&mut self, folder: &str) -> Vec<(u32, Status)> {
        let mut statuses: Vec<(u32, Status)> = Vec::new();
        for Seen { pid, status, .. } in self.scan(folder).unwrap_or_default() {
            match statuses.iter_mut().find(|(held, _)| *held == pid) {
                Some(held) => held.1 = status,
                None => statuses.push((pid, status)),
            }
        }
        statuses
    }

    /// The holders of `session` ([`holders`]).
    pub(crate) fn holders(&mut self, folder: &str, session: &str) -> Option<Vec<u32>> {
        let mut holders: Vec<u32> = Vec::new();
        for Seen {
            pid,
            status,
            parked,
        } in self.scan(folder)?
        {
            if status.session == session && !parked && !holders.contains(&pid) {
                holders.push(pid);
            }
        }
        Some(holders)
    }

    /// Whether the process `pid` is alive, asked of `alive` and counted.
    fn alive(&mut self, pid: u32) -> bool {
        self.alive_asks += 1;
        (self.alive)(pid)
    }

    /// Each file of a live Claude, in the folder's order; none where the folder cannot be read.
    fn scan(&mut self, folder: &str) -> Option<Vec<Seen>> {
        self.fs_calls += 1;
        let listed = entries(Path::new(folder)).ok()?;
        let mut found = Vec::new();
        for entry in listed {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !is_status_file(&name) {
                continue;
            }
            // The number a file is named by is alive, or the file is of a Claude that is gone
            // and is not opened.
            let named = name
                .strip_suffix(".json")
                .and_then(|stem| stem.parse().ok());
            if !named.is_some_and(|named| self.alive(named)) {
                continue;
            }
            self.reads += 1;
            self.fs_calls += 1;
            let Ok(mut file) = fs::File::open(entry.path()) else {
                continue;
            };
            let mut bytes = Vec::new();
            self.fs_calls += 1;
            if file.read_to_end(&mut bytes).is_err() {
                continue;
            }
            // The time of the file whose bytes were read, taken after them: the target of a
            // link, and no older than the content.
            self.fs_calls += 1;
            let written = file.metadata().ok().and_then(|meta| meta.modified().ok());
            if let Read::Live(pid, status) = self.read(&bytes, written) {
                let parked = from_slice_lossy(&bytes)
                    .is_ok_and(|row| row.get("parkedJobId").is_some_and(|job| !job.is_null()));
                found.push(Seen {
                    pid,
                    status,
                    parked,
                });
            }
        }
        Some(found)
    }
}

/// How much later than its status file's last write a process may have begun and still be the
/// one that wrote it: the file's time and the process's are read from two clocks of the system.
const SLACK: Duration = Duration::from_secs(2);

/// `<digits>.json`, the name Claude gives a status file.
fn is_status_file(name: &str) -> bool {
    name.strip_suffix(".json")
        .is_some_and(|stem| !stem.is_empty() && stem.bytes().all(|byte| byte.is_ascii_digit()))
}

impl Scanner {
    /// What a status file says, if it is one of a live process: JSON as
    /// `JSON.parse` reads it, the conversation's id a text, the process id one
    /// of a process alive, and the status one of Claude's own four words.
    ///
    /// Kept from Node on purpose, for files Claude never writes: a process id
    /// below 0 says nothing here, where Node asked the system of a process
    /// group (0 is `alive`'s to refuse); a status that is not one of the four
    /// words names no state, where Node, which looked the word up in an
    /// object, took a name every object has (`constructor`) for a state
    /// neither idle nor waiting, read a list as its text (`["idle"]`), and
    /// failed the look on an object with a `toString` of its own; and JSON
    /// that holds a number past a double's range or nests past 127 levels says
    /// nothing, where Node read it.
    ///
    /// A file last written (`written`) before the process now holding its number began is another
    /// program's number: the Claude that wrote it ended, as a window the host ended does, without
    /// taking its file away, and the system gave the number to a program begun since. Windows does
    /// so at once and to any program (T-1226: a window's record read as shared with the earlier
    /// window's Claude, and no answer of it taken for its task's result). Claude Code says the same
    /// of a number by when its process began (its own `procStart`, `procStartFt` on Windows).
    fn read(&mut self, bytes: &[u8], written: Option<SystemTime>) -> Read {
        let Ok(row) = from_slice_lossy(bytes) else {
            return Read::Nothing;
        };
        let (Some(session), Some(pid)) = (
            row.get("sessionId").and_then(Value::as_str),
            row.get("pid").and_then(process),
        ) else {
            return Read::Nothing;
        };
        if !self.alive(pid) {
            return Read::Nothing;
        }
        if written.is_some_and(|written| cf_process::began_after(pid, written + SLACK)) {
            return Read::Nothing;
        }
        let state = match row.get("status").and_then(Value::as_str) {
            Some("busy") => State::Working,
            Some("waiting") => State::Waiting(
                row.get("waitingFor")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            ),
            Some("idle" | "shell") => State::Idle,
            _ => return Read::Nothing,
        };
        Read::Live(
            pid,
            Status {
                session: session.to_owned(),
                state,
            },
        )
    }
}

/// What a status file came to.
enum Read {
    /// The file of a live Claude.
    Live(u32, Status),
    /// A file that says nothing of a live Claude: one of a process that is gone or that a
    /// later program's number holds, or one of no status.
    Nothing,
}

/// The process a status names: a whole number a process id may be, which
/// `alive` asks the system of as `process.kill` did (Node took any safe
/// integer, `Number.isSafeInteger`).
fn process(pid: &Value) -> Option<u32> {
    let pid = pid.as_f64()?;
    (pid.fract() == 0.0 && (0.0..=f64::from(u32::MAX)).contains(&pid)).then_some(pid as u32)
}

#[cfg(test)]
mod tests;

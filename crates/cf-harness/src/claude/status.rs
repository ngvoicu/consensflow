//! Claude Code's own live status of each of its running processes, from the
//! `sessions/<pid>.json` files it keeps: the conversation the process shows,
//! and busy, idle or waiting, with the reason (a permission prompt, input
//! needed, a dialog). A file whose process is gone says nothing.

use std::fs;
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

/// The status of each Claude running, by its process id, read once and not remembered
/// ([`Scanner::statuses`] remembers).
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

/// A status file as it stood when it was found to be of no live Claude: its last write and
/// its size. A file written since is another file.
type Stamp = (Option<SystemTime>, u64);

/// Claude's statuses, read by one owner that remembers which files it found to be of no live
/// process (P22-1, T-2029: on Windows each look opened every process a file named, and the
/// files of ended windows pile up). A file found so is not opened again until it is written:
/// its process is gone, and a number the system gives another program since begins after the
/// file was written, which [`read`] already refuses. A look then costs the live Claudes, not
/// the files. A file that is not found gone (one torn mid-write, one of a status that is no
/// state) and every file of a live process is read again each look, as it always was.
#[derive(Debug, Default)]
pub(crate) struct Scanner {
    /// By folder, the files found of no live process, by name.
    gone: std::collections::HashMap<String, std::collections::HashMap<String, Stamp>>,
    /// How many status files were opened, for a test to count.
    pub(crate) reads: usize,
}

/// What one status file says.
struct Seen {
    pid: u32,
    status: Status,
    parked: bool,
}

impl Scanner {
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

    /// Each file of a live Claude, in the folder's order; none where the folder cannot be read.
    fn scan(&mut self, folder: &str) -> Option<Vec<Seen>> {
        let listed = entries(Path::new(folder)).ok()?;
        let remembered = self.gone.entry(folder.to_owned()).or_default();
        let mut found = Vec::new();
        let mut present = std::collections::HashMap::new();
        for entry in listed {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !is_status_file(&name) {
                continue;
            }
            // Where the system lists a file it has its time and size (Windows), or a call
            // each (Unix): neither opens the file, nor its process.
            let meta = entry.metadata().ok();
            let written = meta.as_ref().and_then(|meta| meta.modified().ok());
            let stamp = (written, meta.as_ref().map_or(0, fs::Metadata::len));
            if let Some(held) = remembered.get(&name).filter(|held| **held == stamp) {
                present.insert(name, *held);
                continue;
            }
            self.reads += 1;
            let Ok(bytes) = fs::read(entry.path()) else {
                continue;
            };
            match read(&bytes, written) {
                Read::Live(pid, status) => {
                    let parked = from_slice_lossy(&bytes)
                        .is_ok_and(|row| row.get("parkedJobId").is_some_and(|job| !job.is_null()));
                    found.push(Seen {
                        pid,
                        status,
                        parked,
                    });
                }
                Read::Gone => {
                    present.insert(name, stamp);
                }
                Read::Nothing => {}
            }
        }
        // What is no longer listed is forgotten.
        *remembered = present;
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
fn read(bytes: &[u8], written: Option<SystemTime>) -> Read {
    let Ok(row) = from_slice_lossy(bytes) else {
        return Read::Nothing;
    };
    let (Some(session), Some(pid)) = (
        row.get("sessionId").and_then(Value::as_str),
        row.get("pid").and_then(process),
    ) else {
        return Read::Nothing;
    };
    if !cf_process::alive(pid) {
        return Read::Gone;
    }
    if written.is_some_and(|written| cf_process::began_after(pid, written + SLACK)) {
        return Read::Gone;
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

/// What a status file came to.
enum Read {
    /// The file of a live Claude.
    Live(u32, Status),
    /// The file of a process that is gone (or that a later program's number took).
    Gone,
    /// A file that says nothing of a Claude, whatever its process.
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

//! Claude Code's own live status of each of its running processes, from the
//! `sessions/<pid>.json` files it keeps: the conversation the process shows,
//! and busy, idle or waiting, with the reason (a permission prompt, input
//! needed, a dialog). A file whose process is gone says nothing.

use std::fs;
use std::path::Path;

use cf_base::env::Env;
use cf_base::json::from_slice_lossy;
use cf_base::path;
use serde_json::Value;

use crate::shared::paths::home;
use crate::shared::record::find::entries;

/// What Claude says of one of its processes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Status {
    /// The conversation the process shows.
    pub(super) session: String,
    pub(super) state: State,
}

/// Whether a Claude is at work, at its prompt, or waiting on its own dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum State {
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

/// The status of each Claude running, by its process id, in the order
/// Node's `readdir` lists their files: a second file naming the same
/// process puts its status where the first was, as a `Map` keeps a key's
/// place. A folder that cannot be read holds none.
pub(super) fn statuses(folder: &str) -> Vec<(u32, Status)> {
    let mut statuses: Vec<(u32, Status)> = Vec::new();
    for entry in entries(Path::new(folder)).unwrap_or_default() {
        if !is_status_file(&entry.file_name().to_string_lossy()) {
            continue;
        }
        let Some((pid, status)) = fs::read(entry.path()).ok().and_then(|bytes| read(&bytes)) else {
            continue;
        };
        match statuses.iter_mut().find(|(held, _)| *held == pid) {
            Some(held) => held.1 = status,
            None => statuses.push((pid, status)),
        }
    }
    statuses
}

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
fn read(bytes: &[u8]) -> Option<(u32, Status)> {
    let row = from_slice_lossy(bytes).ok()?;
    let session = row.get("sessionId")?.as_str()?.to_owned();
    let pid = process(row.get("pid")?)?;
    if !cf_process::alive(pid) {
        return None;
    }
    let state = match row.get("status")?.as_str()? {
        "busy" => State::Working,
        "waiting" => State::Waiting(
            row.get("waitingFor")
                .and_then(Value::as_str)
                .map(str::to_owned),
        ),
        "idle" | "shell" => State::Idle,
        _ => return None,
    };
    Some((pid, Status { session, state }))
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

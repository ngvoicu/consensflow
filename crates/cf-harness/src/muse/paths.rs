//! Where Muse Code keeps its things: its data folder, `muse` in `XDG_DATA_HOME`, else
//! `.local/share/muse` in the home, on a Mac too (Muse 1.4.13; its own session reader,
//! which the binary carries, reads the same). An `XDG_DATA_HOME` set empty is none, as
//! Muse reads it, where the other harnesses' XDG places keep it ([`data_home`]).
//!
//! In it, one folder per session by the local date it began,
//! `sessions/YYYY/MM/DD/<session>/`, holds the session's log, `session.jsonl`, and the
//! logs of its child agents under `subagent/`; `session-index.db` beside `sessions` is
//! Muse's index of them ([`super::index`]). Windows keeps the same places in the user's
//! home, as far as Muse's docs say: not yet seen on a Windows machine (MUS-20).
//!
//! [`data_home`]: crate::shared::paths::data_home

use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use cf_base::env::Env;
use cf_base::file::real_path;
use cf_base::path;

use crate::shared::paths::{home, set};

/// The log of a session in its folder.
const LOG: &str = "session.jsonl";

/// The folder Muse's data folder is in, as the environment names it.
fn data_home(env: &Env) -> Result<PathBuf, String> {
    Ok(PathBuf::from(match set(env, "XDG_DATA_HOME") {
        Some(data) => data,
        None => path::join(&[&home(env)?, ".local", "share"]),
    }))
}

/// Muse's data folder, by its real path: where a window's Muse writes when it is given
/// that path ([`data_home_given`]).
pub(crate) fn data(env: &Env) -> Result<PathBuf, String> {
    Ok(real_path(&data_home(env)?).join("muse"))
}

/// What a window's Muse is told its data folder's folder is (`XDG_DATA_HOME`): its real
/// path, where the environment names it through a link. Muse 1.4.14 takes a data folder
/// reached through one for unsafe (`unsafe_registry_root`, a Mac's `/tmp`) and writes
/// no `route_facts`, so a fresh window would never learn its session (T-829's probe,
/// T-931). None where the path is its real one.
pub(crate) fn data_home_given(env: &Env) -> Option<String> {
    let given = data_home(env).ok()?;
    let real = real_path(&given);
    (real != given).then(|| real.to_string_lossy().into_owned())
}

/// Muse's settings, `muse/settings.json` in `XDG_CONFIG_HOME`, else in `.config` in the
/// home, on a Mac too: the owner's, which ConsensFlow reads and never writes.
pub(crate) fn settings(env: &Env) -> Result<PathBuf, String> {
    let base = match set(env, "XDG_CONFIG_HOME") {
        Some(config) => config,
        None => path::join(&[&home(env)?, ".config"]),
    };
    Ok(PathBuf::from(base).join("muse").join("settings.json"))
}

/// The folder of every session's own folder.
pub(crate) fn sessions(env: &Env) -> Result<PathBuf, String> {
    Ok(data(env)?.join("sessions"))
}

/// Muse's index of its sessions.
pub(crate) fn index(env: &Env) -> Result<PathBuf, String> {
    Ok(data(env)?.join("session-index.db"))
}

/// The log of the session `session`: `session.jsonl` in the folder named for it, three
/// folders of a date (`YYYY/MM/DD`) under `sessions`, the first such in their order.
/// The folder's name is the session's as written, as Claude's and Codex's are, on a
/// disk that folds case too (a Mac's): no folder is named by an empty id, by a dot or
/// two, or by a path. None where there is none. A child agent's log
/// (`subagent/<child>/`) is never a session's.
pub(crate) fn transcript(session: &str, env: &Env) -> Result<Option<PathBuf>, String> {
    let root = sessions(env)?;
    for year in dated(&root, 4) {
        for month in dated(&year, 2) {
            for day in dated(&month, 2) {
                let named = fs::read_dir(&day)
                    .into_iter()
                    .flatten()
                    .filter_map(Result::ok)
                    .any(|entry| entry.file_name() == session);
                let log = day.join(session).join(LOG);
                if named && log.is_file() {
                    return Ok(Some(log));
                }
            }
        }
    }
    Ok(None)
}

/// How much of the head of a session's log is read for the process that opened it:
/// its `route_facts` is the third record (Muse 1.4.13), well inside this.
const HEAD: u64 = 64 * 1024;

/// How long before a window's launch the session it opens may say it began, for a
/// clock read twice: the session's id and the launch's.
const SKEW_MS: i64 = 5_000;

/// What the head of a session's log says of where the session came from: the Muse
/// process that opened it (`runtime.session.route_facts`), and the session it was
/// started from, by a `/clear` in its window (`session.started`'s
/// `previous_session_stream`).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct Head {
    pid: Option<u64>,
    previous: Option<String>,
}

/// The heads of the session logs a window has read, by session id ([`Head`]). A head
/// that says where its session came from (its process, or the session a `/clear`
/// started it from) never changes, so it is read once; one that says neither yet (a log
/// Muse has only begun) is read again at the next look.
#[derive(Default)]
pub(crate) struct Heads(RefCell<HashMap<String, Head>>);

impl Heads {
    /// What the head of the session `id`'s log `log` says.
    fn of(&self, id: &str, log: &Path) -> Head {
        if let Some(known) = self.0.borrow().get(id) {
            return known.clone();
        }
        let read = head(log);
        if read.pid.is_some() || read.previous.is_some() {
            self.0.borrow_mut().insert(id.to_owned(), read.clone());
        }
        read
    }
}

/// The sessions begun since `launched_ms` (a time in ms since the epoch), each by its id
/// with what the head of its log says ([`Head`]), oldest first: those whose ids (UUIDv7,
/// the time they began in their first 48 bits) began no earlier, for a session older
/// than a window's launch is never its own, though its process had the same pid. One
/// whose id is no UUIDv7 began before 1.4 named them so.
fn begun_since(launched_ms: i64, env: &Env, heads: &Heads) -> Vec<(String, Head)> {
    let Ok(root) = sessions(env) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for year in dated(&root, 4) {
        for month in dated(&year, 2) {
            for day in dated(&month, 2) {
                for entry in fs::read_dir(&day).into_iter().flatten().flatten() {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    if begun_ms(&name).is_some_and(|ms| ms >= launched_ms - SKEW_MS) {
                        let head = heads.of(&name, &entry.path().join(LOG));
                        found.push((name, head));
                    }
                }
            }
        }
    }
    found.sort_by(|a, b| a.0.cmp(&b.0));
    found
}

/// The session the Muse process `pid` opened since `launched_ms`, where a log says it:
/// the newest whose route facts name `pid` ([`begun_since`]). Muse writes the log as its
/// TUI opens, a second in, and its SessionStart hook only at the window's first prompt
/// (Muse 1.4.13, T-787's probe): a window opened with no first message would say nothing
/// of itself until one came.
pub(crate) fn opened_by(pid: u32, launched_ms: i64, env: &Env, heads: &Heads) -> Option<String> {
    begun_since(launched_ms, env, heads)
        .into_iter()
        .rev()
        .find(|(_, head)| head.pid == Some(u64::from(pid)))
        .map(|(id, _)| id)
}

/// The session a window that shows `session` shows now, where a `/clear` in it started
/// others since `launched_ms`: the last of the chain of sessions each started from the
/// one before (`previous_session_stream`), or `session` itself. A cleared session's log
/// names no process, and Muse runs no SessionStart hook for it (Muse 1.4.13, T-787's
/// probe on echo): the chain is the one word of it.
pub(crate) fn cleared_into(session: &str, launched_ms: i64, env: &Env, heads: &Heads) -> String {
    let begun = begun_since(launched_ms, env, heads);
    let mut shown = session.to_owned();
    // A chain is as long as the sessions begun, and a loop in it (no Muse writes one)
    // ends there.
    for _ in 0..begun.len() {
        match begun
            .iter()
            .rev()
            .find(|(_, head)| head.previous.as_deref() == Some(shown.as_str()))
        {
            Some((next, _)) => shown.clone_from(next),
            None => break,
        }
    }
    shown
}

/// The time a UUIDv7 `id` says it was made at, in ms since the epoch: none for any
/// other id.
pub(crate) fn begun_ms(id: &str) -> Option<i64> {
    let bytes = id.as_bytes();
    let shaped = id.len() == 36
        && [8, 13, 18, 23].iter().all(|&at| bytes[at] == b'-')
        && bytes.get(14) == Some(&b'7')
        && id
            .bytes()
            .enumerate()
            .all(|(at, byte)| [8, 13, 18, 23].contains(&at) || byte.is_ascii_hexdigit());
    if !shaped {
        return None;
    }
    i64::from_str_radix(&format!("{}{}", &id[..8], &id[9..13]), 16).ok()
}

/// What the head of the log `log` says ([`Head`]): nothing where it cannot be read.
fn head(log: &Path) -> Head {
    use std::io::{BufRead, BufReader, Read};

    let mut head = Head::default();
    let Ok(file) = fs::File::open(log) else {
        return head;
    };
    for line in BufReader::new(file.take(HEAD))
        .lines()
        .map_while(Result::ok)
    {
        if !line.contains("\"route_facts\"") && !line.contains("\"session_started\"") {
            continue;
        }
        let Ok(record) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        let payload = &record["payload"];
        match payload["kind"].as_str() {
            Some("route_facts") => head.pid = head.pid.or(payload["record"]["pid"].as_u64()),
            Some("session_started") => {
                head.previous = head.previous.take().or_else(|| {
                    payload["previous_session_stream"]["id"]
                        .as_str()
                        .map(str::to_owned)
                });
            }
            _ => {}
        }
    }
    head
}

/// The folders in `folder` whose names are `digits` digits, in their order; none where
/// it cannot be read.
fn dated(folder: &Path, digits: usize) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut found: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .filter(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name.len() == digits && name.bytes().all(|byte| byte.is_ascii_digit())
        })
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    found.sort();
    found
}

#[cfg(test)]
mod tests;

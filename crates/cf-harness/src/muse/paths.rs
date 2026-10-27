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

use std::fs;
use std::path::{Path, PathBuf};

use cf_base::env::Env;
use cf_base::path;

use crate::shared::paths::{home, set};

/// The log of a session in its folder.
const LOG: &str = "session.jsonl";

/// Muse's data folder.
pub(crate) fn data(env: &Env) -> Result<PathBuf, String> {
    let base = match set(env, "XDG_DATA_HOME") {
        Some(data) => data,
        None => path::join(&[&home(env)?, ".local", "share"]),
    };
    Ok(PathBuf::from(base).join("muse"))
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

/// The session the Muse process `pid` opened since `launched_ms` (a time in ms since
/// the epoch), where a log says it: the newest whose `runtime.session.route_facts`
/// names `pid`, of the sessions whose ids (UUIDv7, the time they began in their
/// first 48 bits) began no earlier. Muse writes the log as its TUI opens, a second in,
/// and its SessionStart hook only at the window's first prompt (Muse 1.4.13, T-787's
/// probe): a window opened with no first message would say nothing of itself until
/// one came. A session older than the launch is never the window's, though its
/// process had the same pid; one whose id is no UUIDv7 began before 1.4 named them so.
pub(crate) fn opened_by(pid: u32, launched_ms: i64, env: &Env) -> Option<String> {
    let root = sessions(env).ok()?;
    let mut found: Option<String> = None;
    for year in dated(&root, 4) {
        for month in dated(&year, 2) {
            for day in dated(&month, 2) {
                for entry in fs::read_dir(&day).into_iter().flatten().flatten() {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    let begun = begun_ms(&name).is_some_and(|ms| ms >= launched_ms - SKEW_MS);
                    let newer = found.as_deref().is_none_or(|known| name.as_str() > known);
                    if begun && newer && opened_in(&entry.path().join(LOG), pid) {
                        found = Some(name);
                    }
                }
            }
        }
    }
    found
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

/// Whether the head of the log `log` has a `route_facts` record naming `pid`.
fn opened_in(log: &Path, pid: u32) -> bool {
    use std::io::{BufRead, BufReader, Read};

    let Ok(file) = fs::File::open(log) else {
        return false;
    };
    BufReader::new(file.take(HEAD))
        .lines()
        .map_while(Result::ok)
        .filter(|line| line.contains("\"route_facts\""))
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(&line).ok())
        .any(|record| {
            let payload = &record["payload"];
            payload["kind"] == "route_facts"
                && payload["record"]["pid"].as_u64() == Some(u64::from(pid))
        })
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

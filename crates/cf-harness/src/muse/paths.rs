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
/// disk that folds case too (a Mac's). None where there is none, or `session` could
/// not name a folder (it is empty, or holds a separator or a dot alone). A child
/// agent's log (`subagent/<child>/`) is never a session's.
pub(crate) fn transcript(session: &str, env: &Env) -> Result<Option<PathBuf>, String> {
    if !names_a_folder(session) {
        return Ok(None);
    }
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

/// Whether `session` names one folder.
fn names_a_folder(session: &str) -> bool {
    !session.is_empty() && session != "." && session != ".." && !session.contains(['/', '\\'])
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

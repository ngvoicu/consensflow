//! The newest conversation a harness opened in a folder, found in the harness's
//! own store: a window of Devin, Codex or Muse names its conversation only
//! there (Claude Code and Pi open on an id they are given; a Muse session's log
//! names its folder in its head, `muse::paths::opened_in`). The folder is the
//! run's own, and a window of a run with no staff is its chief's, so the newest
//! conversation opened there since the window was is the window's.
//!
//! A folder is compared as the system names it, links resolved: the one a store
//! says and the one asked for are the same folder however each is spelled (a Mac's
//! `/var/folders/…` is `/private/var/folders/…`, and a window opens in the folder as
//! the system names it, T-783).

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use cf_base::env::Env;
use cf_base::file::real_path;
use cf_proto::agents::Harness;
use rusqlite::{params, Params};
use serde_json::Value;

use crate::shared::record::sqlite::Store;
use crate::{codex, devin, muse};

/// How deep under Codex's `sessions` its rollouts are: `YYYY/MM/DD/rollout-….jsonl`.
const DAYS: usize = 3;

/// The newest top-level conversation the harness `harness` (asked for by its
/// `kind`) opened in `workspace` at or after `since_ms` (milliseconds since the
/// epoch), from its store in the places `env` names; none while there is none.
pub(super) fn newest(
    harness: Harness,
    kind: &str,
    workspace: &str,
    since_ms: i64,
    env: &Env,
) -> Result<Option<String>, String> {
    match harness {
        Harness::Devin => devin_session(workspace, since_ms, env),
        Harness::Codex => codex_session(workspace, since_ms, env),
        Harness::Claude | Harness::Pi => Err(format!(
            "{kind} opens on a session id of its own choosing: none to find"
        )),
        Harness::Muse => Ok(muse::paths::opened_in(workspace, since_ms, env)),
    }
}

/// The `id` of the first row `sql` selects in the store `file` whose `directory` is
/// the folder `workspace`, both as the system names them; none where the store cannot
/// be opened or read (it is not there yet, or it is the harness's to write now) or no
/// row is of the folder.
fn first_id(file: &Path, sql: &str, params: impl Params, workspace: &str) -> Option<String> {
    let store = Store::open(file).ok()?;
    let rows = store.read(|reads| reads.all(sql, params)).ok()?;
    let folder = real_path(Path::new(workspace));
    rows.iter()
        .find(|row| {
            row.get("directory")
                .is_some_and(|directory| real_path(Path::new(&*directory.text())) == folder)
        })?
        .get("id")
        .map(|id| id.text().into_owned())
}

/// Devin keeps a `sessions` row for each, and keeps seconds.
fn devin_session(workspace: &str, since_ms: i64, env: &Env) -> Result<Option<String>, String> {
    Ok(first_id(
        Path::new(&devin::paths::store(env)?),
        "SELECT id, working_directory AS directory FROM sessions WHERE created_at >= ?1 \
         ORDER BY created_at DESC",
        params![since_ms.div_euclid(1000)],
        workspace,
    ))
}

/// Codex writes `sessions/YYYY/MM/DD/rollout-….jsonl`, its first line naming the
/// session and its folder: the newest rollout, written to since the window
/// opened, that names the folder.
fn codex_session(workspace: &str, since_ms: i64, env: &Env) -> Result<Option<String>, String> {
    let mut rollouts = Vec::new();
    rollouts_under(&codex::paths::sessions(env)?, 0, since_ms, &mut rollouts);
    rollouts.sort_by(|(left, _), (right, _)| right.cmp(left));
    Ok(rollouts
        .iter()
        .find_map(|(_, rollout)| session_of(rollout, workspace)))
}

/// The rollouts under `folder`, `depth` folders below the root of the sessions,
/// that were written to at or after `since_ms`, each with the time. A folder's
/// time moves only when an entry is added right inside it, so the year and the
/// month are older than any run: only files are dated.
fn rollouts_under(folder: &Path, depth: usize, since_ms: i64, found: &mut Vec<(i64, PathBuf)>) {
    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let Ok(kind) = fs::metadata(&path) else {
            continue;
        };
        let name = entry.file_name().to_string_lossy().into_owned();
        if kind.is_dir() && depth < DAYS {
            rollouts_under(&path, depth + 1, since_ms, found);
        } else if kind.is_file() && name.starts_with("rollout-") && name.ends_with(".jsonl") {
            let written = kind
                .modified()
                .ok()
                .and_then(|at| at.duration_since(UNIX_EPOCH).ok())
                .and_then(|since| i64::try_from(since.as_millis()).ok());
            if let Some(written) = written.filter(|written| *written >= since_ms) {
                found.push((written, path));
            }
        }
    }
}

/// The session a rollout is of, if its first line says it is of `workspace`.
/// Only the first line is read: a rollout grows with its conversation.
fn session_of(rollout: &Path, workspace: &str) -> Option<String> {
    let mut first = String::new();
    BufReader::new(fs::File::open(rollout).ok()?)
        .read_line(&mut first)
        .ok()?;
    let meta: Value = serde_json::from_str(&first).ok()?;
    let cwd = meta["payload"]["cwd"].as_str()?;
    (meta["type"] == "session_meta" && real_path(Path::new(cwd)) == real_path(Path::new(workspace)))
        .then(|| meta["payload"]["id"].as_str().map(str::to_owned))?
}

#[cfg(test)]
mod tests;

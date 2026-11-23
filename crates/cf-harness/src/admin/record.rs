//! The record of an update that is running: the installer's number and when it began, in a
//! file in ConsensFlow's home, for as long as the installer runs. One update of a harness
//! runs at a time, and this is what says so after the daemon that began it was replaced: the
//! installer outlives an abrupt exit of the daemon (on Unix it leads a group of its own), and
//! a replacement that finds the record of a process still there, as it began, refuses a
//! second one. A record of a process that has ended, or whose number another process has,
//! is stale and goes.

use std::fs;
use std::path::PathBuf;

use cf_base::env::Env;
use cf_base::home::config_root;
use cf_proto::agents::Harness;
use serde_json::{json, Value};

/// The folder the records are in, under the home.
const FOLDER: &str = "updates";

/// Where the record of an update of `harness` is.
pub(super) fn path(env: &Env, harness: Harness) -> Option<PathBuf> {
    Some(
        config_root(env)?
            .join(FOLDER)
            .join(format!("{}.json", harness.as_str())),
    )
}

/// Records that the installer `pid` runs: its number and when it began, as the system says.
/// Nothing is written where the system says nothing of it: its record could never be told
/// from a stale one.
pub(super) fn write(env: &Env, harness: Harness, pid: u32) {
    let (Some(path), Some(began)) = (path(env, harness), cf_process::began_ms(pid)) else {
        return;
    };
    if let Some(folder) = path.parent() {
        let _ = fs::create_dir_all(folder);
    }
    // Whole or not there: a reader never sees half of it and takes it for a stale one.
    let partial = path.with_extension("json.tmp");
    if fs::write(&partial, json!({ "pid": pid, "began": began }).to_string()).is_ok() {
        let _ = fs::rename(partial, path);
    }
}

/// Takes the record away: the installer has ended.
pub(super) fn remove(env: &Env, harness: Harness) {
    if let Some(path) = path(env, harness) {
        let _ = fs::remove_file(path);
    }
}

/// Whether the record of `harness` is of an installer still running: its process is there
/// and began when the record says. A record that is not is removed.
pub(super) fn running(env: &Env, harness: Harness) -> bool {
    let Some(path) = path(env, harness) else {
        return false;
    };
    let Ok(text) = fs::read_to_string(&path) else {
        return false;
    };
    let Ok(record) = serde_json::from_str::<Value>(&text) else {
        return false;
    };
    let pid = record["pid"]
        .as_u64()
        .and_then(|pid| u32::try_from(pid).ok());
    let began = record["began"].as_i64();
    let alive = matches!((pid, began), (Some(pid), Some(began)) if cf_process::began_ms(pid) == Some(began));
    if !alive {
        let _ = fs::remove_file(path);
    }
    alive
}

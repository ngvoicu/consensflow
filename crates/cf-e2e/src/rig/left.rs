//! What the windows left behind when the rig ended: the processes the stand-ins noted
//! (`pids.jsonl`) that are still there once the pane host is gone.
//!
//! A number is no process. A stand-in noted its own, and a process it left is still the
//! one with that number; but a window that ended before the rig did (a chief the case
//! replaced, a worker that finished) gave its number back, and a rig that runs among a
//! gate's (a dozen at once, each starting programs by the hundred) can find another
//! program holding it by the time it looks. A program that began after the last note was
//! written was never a window's: every window's began before its own note, and the last
//! note is the latest of them.

use std::fs;
use std::path::Path;
use std::time::Duration;

use super::wait;
use crate::process;

/// How much younger than the file a process may be and still be taken for a window's:
/// `ps` says a process's age to the second, and the file's time is exact.
#[cfg(unix)]
const SLACK: Duration = Duration::from_secs(2);

/// What is left of the windows noted in `pids_file`, once `within` has passed: the first
/// process still there that is a window's, in words that say what it is; none where they
/// are all gone.
pub(super) fn left_behind(pids_file: &Path, within: Duration) -> Option<String> {
    let noted = fs::read_to_string(pids_file).unwrap_or_default();
    for pid in noted
        .lines()
        .filter_map(|line| line.trim().parse::<u32>().ok())
    {
        if wait::until(within, || !process::is_alive(pid)) {
            continue;
        }
        if began_after(pid, pids_file) {
            continue;
        }
        let what = describe(pid);
        return Some(format!("{pid} ({what})"));
    }
    None
}

/// Whether the process `pid` began after the file `noted` was last written: one that did
/// is another program, which took the number a window gave back.
#[cfg(unix)]
fn began_after(pid: u32, noted: &Path) -> bool {
    let written_ago = fs::metadata(noted)
        .and_then(|found| found.modified())
        .ok()
        .and_then(|written| written.elapsed().ok());
    match (process::age(pid), written_ago) {
        (Some(ran_for), Some(written_ago)) => ran_for + SLACK < written_ago,
        _ => false,
    }
}

/// Windows says no age cheaply: a process there is a window's.
#[cfg(not(unix))]
fn began_after(_: u32, _: &Path) -> bool {
    false
}

#[cfg(unix)]
fn describe(pid: u32) -> String {
    process::described(pid).unwrap_or_else(|| "gone by the time it was looked at".to_owned())
}

#[cfg(not(unix))]
fn describe(_: u32) -> String {
    "no more is said of it here".to_owned()
}

#[cfg(test)]
mod tests;

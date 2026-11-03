//! What the windows left behind when the rig ended: the processes the stand-ins noted
//! (`pids.jsonl`) that are still there once the pane host is gone.
//!
//! A number is no process. A stand-in noted its own, and when its process began, in the
//! words the system says it in (`began.jsonl`): a window that ended gave its number
//! back, and a rig that runs among a gate's (a dozen at once, each starting programs by
//! the hundred) can find another program holding it by the time it looks — one that
//! began at another time than the note says the window's did, which a run of minutes is
//! old enough to meet (T-1077 found three of nine runs failed by one). Where no begin
//! was noted, the file's time is the measure: a program that began after the last note
//! was written was never a window's, every window's began before its own note.

use std::collections::HashMap;
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
    let began = began_beside(pids_file);
    for pid in noted
        .lines()
        .filter_map(|line| line.trim().parse::<u32>().ok())
    {
        if wait::until(within, || !process::is_alive(pid)) {
            continue;
        }
        if another_holds(pid, &began, pids_file) {
            continue;
        }
        let what = describe(pid);
        return Some(format!("{pid} ({what})"));
    }
    None
}

/// Whether the process `pid` is another's than the window the note is of. Where the
/// note says when the window's began, one that began at another time is another
/// program's — the window's ended, and the system gave its number to it. Where none
/// was noted, one that began after the last note was written is no window's.
fn another_holds(pid: u32, began: &HashMap<u32, Vec<String>>, noted: &Path) -> bool {
    let Some(begins) = began.get(&pid).filter(|begins| !begins.is_empty()) else {
        return began_after(pid, noted);
    };
    match process::started(pid) {
        Some(this) => !begins.contains(&this),
        // The system will not say when it began: gone by now it is none of the
        // windows'; still there but not to be told from, it is held to being one.
        None => !process::is_alive(pid),
    }
}

/// When each noted process began, by its number, as `began.jsonl` beside `pids_file`
/// says it: a line for each a stand-in wrote, `pid` and the words the system said its
/// start in (`process::started`), the same words it is looked at in here.
fn began_beside(pids_file: &Path) -> HashMap<u32, Vec<String>> {
    let mut began: HashMap<u32, Vec<String>> = HashMap::new();
    let file = pids_file.with_file_name("began.jsonl");
    for line in fs::read_to_string(file).unwrap_or_default().lines() {
        let Some((pid, start)) = line.split_once('\t') else {
            continue;
        };
        let Ok(pid) = pid.trim().parse::<u32>() else {
            continue;
        };
        if start.is_empty() {
            continue;
        }
        began.entry(pid).or_default().push(start.to_owned());
    }
    began
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

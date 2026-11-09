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

/// The program that notes its number (`fake_agent`'s `note_process`): the stand-in's own,
/// which every window of a rig runs. No other program notes one.
pub(super) const NOTER: &str = "fake-agent";

/// How much younger than the file a process may be and still be taken for a window's:
/// `ps` says a process's age to the second, and the file's time is exact.
#[cfg(unix)]
const SLACK: Duration = Duration::from_secs(2);

/// What is left of the windows noted in `pids_file`, once `within` has passed: the first
/// process still there that is a window's, in words that say what it is; none where they
/// are all gone.
pub(super) fn left_behind(pids_file: &Path, within: Duration, program: &str) -> Option<String> {
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
        if runs_another(pid, program) {
            continue;
        }
        let what = describe(pid);
        return Some(format!("{pid} ({what})"));
    }
    None
}

/// Whether the process `pid` runs another program than `program`, as its command line says:
/// one that does took the number a window gave back, whenever it began. Its start alone does
/// not tell it: the file's last note is the latest window's, and a program that took the
/// number of a window that ended before another window noted its own began before that note
/// (a chief opened again, CI run 41973700452: `sort.exe` held the first chief's number), as
/// one that began within the slack does. Where the command cannot be read, the process is
/// taken for a window's.
fn runs_another(pid: u32, program: &str) -> bool {
    command(pid).is_some_and(|command| !names(&command, program))
}

/// Whether the command line `command` runs `program`: a word of it whose file name is
/// `program`, `.exe` aside, in any case (Windows' names are).
fn names(command: &str, program: &str) -> bool {
    command
        .split(|c: char| c.is_whitespace() || c == '"')
        .filter(|word| !word.is_empty())
        .map(|word| word.rsplit(['/', '\\']).next().unwrap_or(word))
        .map(|name| {
            name.len()
                .checked_sub(4)
                .filter(|&at| name.is_char_boundary(at) && name[at..].eq_ignore_ascii_case(".exe"))
                .map_or(name, |at| &name[..at])
        })
        .any(|name| name.eq_ignore_ascii_case(program))
}

/// The command line of the process `pid`, as `ps` says it: none where it is gone or `ps`
/// cannot say.
#[cfg(unix)]
fn command(pid: u32) -> Option<String> {
    process::command(pid)
}

/// The command line of the process `pid`, as Windows' process table says it (the closing
/// listing's own, `live::members::ThisMachine`): none where it is gone or the table cannot be
/// read.
#[cfg(windows)]
fn command(pid: u32) -> Option<String> {
    use crate::live::members::{Machine, ThisMachine};
    ThisMachine
        .processes()
        .ok()?
        .into_iter()
        .find(|running| running.identity.pid == pid)
        .map(|running| running.command)
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

/// Whether the process `pid` began after the file `noted` was last written, as Windows'
/// process table says when it began (the kit's own listing, `live::members::ThisMachine`):
/// Windows gives a number back at once and to any program, and a busy runner's next program
/// took a window's (CI runs 41787972093 and 41792519144, T-1226). Where the table cannot say,
/// the process is taken for a window's.
#[cfg(windows)]
fn began_after(pid: u32, noted: &Path) -> bool {
    use crate::live::members::{Machine, ThisMachine};
    let written = fs::metadata(noted).and_then(|found| found.modified()).ok();
    let began = ThisMachine
        .identity(pid)
        .ok()
        .flatten()
        .and_then(|identity| from_file_time(identity.started));
    match (began, written) {
        (Some(began), Some(written)) => began_later(began, written),
        _ => false,
    }
}

/// Whether a process that `began` then began after a file `written` then, by more than the
/// slack of the two clocks' reading.
#[cfg(any(windows, test))]
fn began_later(began: std::time::SystemTime, written: std::time::SystemTime) -> bool {
    began
        .duration_since(written)
        .is_ok_and(|after| after > Duration::from_secs(2))
}

/// The time Windows' 100 ns since 1601 say (a FILETIME, as its process table says when a
/// process began).
#[cfg(any(windows, test))]
fn from_file_time(ticks: u64) -> Option<std::time::SystemTime> {
    const UNIX_FROM_1601: u64 = 11_644_473_600;
    let seconds = (ticks / 10_000_000).checked_sub(UNIX_FROM_1601)?;
    let nanos = u32::try_from((ticks % 10_000_000) * 100).ok()?;
    std::time::SystemTime::UNIX_EPOCH.checked_add(Duration::new(seconds, nanos))
}

#[cfg(unix)]
fn describe(pid: u32) -> String {
    process::described(pid).unwrap_or_else(|| "gone by the time it was looked at".to_owned())
}

/// What Windows' process table says the process `pid` runs, its command line.
#[cfg(windows)]
fn describe(pid: u32) -> String {
    use crate::live::members::{Machine, ThisMachine};
    match ThisMachine.processes() {
        Ok(all) => all
            .into_iter()
            .find(|running| running.identity.pid == pid)
            .map_or_else(
                || "gone by the time it was looked at".to_owned(),
                |running| format!("command {}", running.command),
            ),
        Err(failed) => format!("Windows' process table could not be read: {failed}"),
    }
}

#[cfg(test)]
mod tests;

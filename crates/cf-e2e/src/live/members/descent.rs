//! The processes of a window's own tree, as a live check that opened the window reads
//! them to find and end what the window runs: what descends from the window's program,
//! by the process that started each, and nothing else of the machine's. A check that
//! ended every process whose command line held a mark ended the worker that ran it
//! (2026-10-16), and one that went by numbers alone could end a process that took a
//! number the tree had, or take for the tree's a process whose parent's number the
//! window's program had taken since (Astra and Sol on the B1 fix, their second look).
//! So a process is known by its number and when it began ([`Identity`]): a child began
//! no earlier than its parent, and the window's own process is the one the pane host
//! opened, read once as it answered the open and kept (the rig's
//! [`crate::rig::Rig::window_process`]): a window whose process has gone has nothing that
//! descends from it, whatever has its number since (Astra and Sol on round 6).
//!
//! A process is ended only while it is still the one chosen, checked on what holds it
//! ([`end_chosen`]): on Windows a handle to the process, opened first, on which its
//! creation time is read and through which it is ended, so that its number is no other's
//! in between; on macOS and Linux, which have no such handle, its number, when it began
//! read again right before the signal. What could not be ended is said, never dropped
//! ([`Ended`]).
//!
//! When a process began is read to the microsecond on macOS (`proc_pidinfo`), to the
//! 100 ns on Windows (the process's own handle), and to the second on Linux (`ps`'s
//! `lstart`, in the C locale). The machine's list is `ps` on macOS and Linux, and on
//! Windows, which has none, the process table through PowerShell, as JSON: a command
//! line of many lines is one string there, where read as lines it could make up a
//! process. On macOS and Windows each process listed is read again where it is held, for
//! when it began ([`refined_by_bsd`], [`refined_by_handle`]).

use std::io;

#[cfg(any(windows, test))]
use serde_json::Value;

use crate::process::Run;
use crate::{Error, Result};

mod native;
#[cfg(test)]
pub(crate) mod testing;

/// Which process a process is: its number and when it began, which no two processes
/// have together. `started` grows with the time it says, on the machine that said it:
/// microseconds since 1970 on macOS, Windows' 100 ns since 1601 of its creation time,
/// and `YYYYMMDDhhmmss` of `ps`'s `lstart` on Linux.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Identity {
    pub pid: u32,
    pub started: u64,
}

/// A process of the machine as a check reads it: which it is, the number of the process
/// that started it, and its command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Running {
    pub identity: Identity,
    pub ppid: u32,
    pub command: String,
}

/// What a check reads of the machine's processes: the seam by which it finds what
/// descends from a window's own process. Nothing of it ends a process.
pub trait Machine {
    /// Every process of the machine.
    fn processes(&self) -> Result<Vec<Running>>;
    /// Which process has the number `pid` now: none where none has it.
    fn identity(&self, pid: u32) -> Result<Option<Identity>>;
}

/// The machine the check runs on.
#[derive(Debug, Clone, Copy)]
pub struct ThisMachine;

impl Machine for ThisMachine {
    fn processes(&self) -> Result<Vec<Running>> {
        native::processes()
    }

    fn identity(&self, pid: u32) -> Result<Option<Identity>> {
        native::identity(pid)
    }
}

/// What holds a process while a check makes sure it is the one chosen, and ends it: on
/// Windows a handle to it, on macOS and Linux its number.
pub(crate) trait Held {
    /// When the process held began: none where it has ended.
    fn started(&self) -> io::Result<Option<u64>>;
    /// Ends the process held.
    fn end(&self) -> io::Result<()>;
}

/// How a check takes hold of the process that has a number now.
pub(crate) trait Holds {
    /// What holds the process `pid`: none where no process has the number.
    fn hold(&self, pid: u32) -> io::Result<Option<Box<dyn Held>>>;
}

/// What came of ending the processes a check chose: each process ended, gone already
/// (ended, or its number another process's now), or not ended, with why.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Ended {
    pub ended: Vec<Running>,
    pub gone: Vec<Running>,
    /// What may still run: the process, and why it could not be ended.
    pub failed: Vec<(Running, String)>,
}

impl Ended {
    /// What may still run, a problem for the case's verdict each: a process that could not
    /// be reached, read or ended.
    pub fn problems(&self) -> Vec<String> {
        self.failed
            .iter()
            .map(|(process, why)| {
                format!(
                    "process {} ({}) may still run: {why}",
                    process.identity.pid,
                    crate::live::cut(&process.command, 90)
                )
            })
            .collect()
    }
}

/// What came of ending one process.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Outcome {
    Ended,
    Gone,
    Failed(String),
}

/// Ends each of `chosen` (chosen from a window's own tree) that is still the process
/// chosen, checked on what holds it right before its end ([`end_one`]): a number another
/// process took since is not ended, and what could not be reached, read or ended is said
/// ([`Ended::failed`]). This is the one way a live check ends a process: no check ends
/// one by what its command line says, or by its number alone (a guard test of the live
/// checks holds them to it).
pub fn end_chosen(chosen: &[Running]) -> Ended {
    end_chosen_by(&native::Native, chosen)
}

/// [`end_chosen`], each process held by `holds`.
pub(crate) fn end_chosen_by(holds: &dyn Holds, chosen: &[Running]) -> Ended {
    let mut said = Ended::default();
    for process in chosen {
        match end_one(holds, process.identity) {
            Outcome::Ended => said.ended.push(process.clone()),
            Outcome::Gone => said.gone.push(process.clone()),
            Outcome::Failed(why) => said.failed.push((process.clone(), why)),
        }
    }
    said
}

/// Ends the process `identity` names, only while what `holds` holds of its number began
/// when it did: one that has ended, or whose number another process has now, is gone.
fn end_one(holds: &dyn Holds, identity: Identity) -> Outcome {
    let held = match holds.hold(identity.pid) {
        Ok(Some(held)) => held,
        Ok(None) => return Outcome::Gone,
        Err(failed) => return Outcome::Failed(format!("it could not be reached: {failed}")),
    };
    match held.started() {
        Ok(Some(started)) if started == identity.started => match held.end() {
            Ok(()) => Outcome::Ended,
            Err(failed) => Outcome::Failed(format!("its end failed: {failed}")),
        },
        Ok(_) => Outcome::Gone,
        Err(failed) => Outcome::Failed(format!("when it began could not be read again: {failed}")),
    }
}

/// What descends from the process `root` among `all`: the processes it started, and what
/// they started in turn, a child being one that names its parent's number and began no
/// earlier than its parent did. None where `root` is not among them: the window's process
/// has gone, and its number may be another's.
pub fn descending_from(all: &[Running], root: Identity) -> Vec<Running> {
    if !all.iter().any(|process| process.identity == root) {
        return Vec::new();
    }
    let mut found: Vec<Running> = Vec::new();
    let mut parents = vec![root];
    while let Some(parent) = parents.pop() {
        for process in all {
            let child = process.ppid == parent.pid
                && process.identity.started >= parent.started
                && process.identity != root
                && !found.iter().any(|seen| seen.identity == process.identity);
            if child {
                parents.push(process.identity);
                found.push(process.clone());
            }
        }
    }
    found
}

/// What descends from the window's process `window` among `all` and holds `text` in its
/// command line, with what each of those started, each once: what a check ends of a
/// command the window runs. A process outside the window is none of it, whatever its
/// command line says.
pub fn holding(all: &[Running], window: Identity, text: &str) -> Vec<Running> {
    let mut chosen: Vec<Running> = Vec::new();
    for process in descending_from(all, window)
        .into_iter()
        .filter(|process| process.command.contains(text))
    {
        let under = descending_from(all, process.identity);
        for one in std::iter::once(process).chain(under) {
            if !chosen.iter().any(|seen| seen.identity == one.identity) {
                chosen.push(one);
            }
        }
    }
    chosen
}

/// What macOS says of a process, read where it is held (`proc_pidinfo`): the process that
/// started it, and when it began, in microseconds since 1970.
#[cfg(any(target_os = "macos", test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Bsd {
    pub ppid: u32,
    pub started: u64,
}

/// The processes `ps` listed (`listed`), each with when it began as macOS says it to the
/// microsecond (`read`): one the system says nothing of, or whose parent it says is
/// another than `ps` listed, has ended since, and its number may be another's: it is not
/// listed.
#[cfg(any(target_os = "macos", test))]
pub(crate) fn refined_by_bsd(
    listed: Vec<Running>,
    read: impl Fn(u32) -> io::Result<Option<Bsd>>,
) -> Vec<Running> {
    listed
        .into_iter()
        .filter_map(|mut process| {
            let bsd = read(process.identity.pid).ok()??;
            if bsd.ppid != process.ppid {
                return None;
            }
            process.identity.started = bsd.started;
            Some(process)
        })
        .collect()
}

/// How far, in 100 ns, the table's creation time of a process may be from its handle's:
/// the table says it to the microsecond.
#[cfg(any(windows, test))]
const TABLE_GRAIN: u64 = 10;

/// The processes Windows' table listed (`listed`), each with its creation time as its own
/// handle says it (`read`), to the 100 ns. One the handle says has exited, or that no
/// process has the number of, is not listed; nor one whose handle says another creation
/// time than the table, give or take its grain ([`TABLE_GRAIN`]): its number is
/// another's now. One that cannot be opened (the system's own, another user's) keeps
/// what the table said: no check can end it.
#[cfg(any(windows, test))]
pub(crate) fn refined_by_handle(
    listed: Vec<Running>,
    read: impl Fn(u32) -> io::Result<Option<u64>>,
) -> Vec<Running> {
    listed
        .into_iter()
        .filter_map(|mut process| match read(process.identity.pid) {
            Ok(Some(created)) if created.abs_diff(process.identity.started) < TABLE_GRAIN => {
                process.identity.started = created;
                Some(process)
            }
            Ok(_) => None,
            Err(_) => Some(process),
        })
        .collect()
}

/// What `ps` says of every process, or of the process `pid` alone, in the C locale so
/// that its `lstart` is what [`ps_lines`] reads.
#[cfg(unix)]
fn ps(pid: Option<u32>) -> Result<Vec<Running>> {
    let words = match pid {
        Some(pid) => vec![
            "-o".to_owned(),
            "pid=,ppid=,lstart=,command=".to_owned(),
            "-p".to_owned(),
            pid.to_string(),
        ],
        None => vec!["-axo".to_owned(), "pid=,ppid=,lstart=,command=".to_owned()],
    };
    let ran = Run::new("ps")
        .args(words)
        .inheriting_env()
        .var("LC_ALL", "C")
        .run()?;
    // `ps -p` of a number no process has says nothing, and ends with 1.
    let nothing = pid.is_some() && ran.stdout.trim().is_empty();
    if ran.code != Some(0) && !nothing {
        return Err(Error::Program {
            action: "list the processes with",
            program: "ps".to_owned(),
            source: io::Error::other(ran.to_string()),
        });
    }
    Ok(ps_lines(&ran.stdout))
}

/// The processes of a listing `ps -o pid=,ppid=,lstart=,command=` made: on each line the
/// number, the parent's, five words of `lstart` (`Fri Oct  6 04:44:58 2026`) and the
/// command line, as it is spaced.
#[cfg(any(unix, test))]
fn ps_lines(listing: &str) -> Vec<Running> {
    listing
        .lines()
        .filter_map(|line| {
            let mut rest = line.trim_start();
            let mut words = Vec::new();
            for _ in 0..7 {
                let (word, after) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
                words.push(word);
                rest = after.trim_start();
            }
            Some(Running {
                identity: Identity {
                    pid: words[0].parse().ok()?,
                    started: started(&words[2..7])?,
                },
                ppid: words[1].parse().ok()?,
                command: rest.to_owned(),
            })
        })
        .collect()
}

/// The months as `lstart` names them in the C locale.
#[cfg(any(unix, test))]
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// The five words of an `lstart` (`Fri Oct 16 04:44:58 2026`) as `YYYYMMDDhhmmss`.
#[cfg(any(unix, test))]
fn started(lstart: &[&str]) -> Option<u64> {
    let month = MONTHS.iter().position(|name| *name == lstart[1])?;
    let month = u64::try_from(month).ok()? + 1;
    let day: u64 = lstart[2].parse().ok()?;
    let mut clock = lstart[3].split(':').map(|part| part.parse::<u64>().ok());
    let (hour, minute, second) = (clock.next()??, clock.next()??, clock.next()??);
    let year: u64 = lstart[4].parse().ok()?;
    Some(((((year * 100 + month) * 100 + day) * 100 + hour) * 100 + minute) * 100 + second)
}

/// What Windows' process table is asked, through PowerShell, for the processes `which`
/// selects (` -Filter 'ProcessId=N'`, or nothing for all): each as an object of its
/// number, its parent's, its start as a file time (none for the system's own) and its
/// command line, all of them as JSON, in UTF-8.
#[cfg(any(windows, test))]
fn table_script(which: &str) -> String {
    format!(
        "[Console]::OutputEncoding = [Text.Encoding]::UTF8; Get-CimInstance Win32_Process{which} | ForEach-Object {{ [pscustomobject]@{{ pid = $_.ProcessId; ppid = $_.ParentProcessId; started = $(if ($_.CreationDate) {{ $_.CreationDate.ToFileTimeUtc() }} else {{ 0 }}); command = $_.CommandLine }} }} | ConvertTo-Json -Compress"
    )
}

/// What Windows' process table says of every process.
#[cfg(windows)]
fn table() -> Result<Vec<Running>> {
    let ran = Run::new("powershell")
        .args(["-NoProfile", "-Command", &table_script("")])
        .inheriting_env()
        .run()?;
    if ran.code != Some(0) {
        return Err(Error::Program {
            action: "list the processes with",
            program: "powershell".to_owned(),
            source: io::Error::other(ran.to_string()),
        });
    }
    table_of(&ran.stdout)
}

/// The processes of what [`table_script`] printed: a list of them, one alone, or nothing
/// for none, a byte order mark before it where PowerShell writes one.
#[cfg(any(windows, test))]
fn table_of(listing: &str) -> Result<Vec<Running>> {
    let listing = listing
        .trim_start_matches(|letter: char| u32::from(letter) == 0xFEFF)
        .trim();
    if listing.is_empty() {
        return Ok(Vec::new());
    }
    let parsed: Value = serde_json::from_str(listing)
        .map_err(|failed| Error::Daemon(format!("Windows' process table is no JSON: {failed}")))?;
    let entries = match parsed {
        Value::Array(entries) => entries,
        one => vec![one],
    };
    Ok(entries
        .iter()
        .filter_map(|entry| {
            Some(Running {
                identity: Identity {
                    pid: u32::try_from(entry["pid"].as_u64()?).ok()?,
                    started: entry["started"].as_u64()?,
                },
                ppid: u32::try_from(entry["ppid"].as_u64()?).ok()?,
                command: entry["command"].as_str().unwrap_or_default().to_owned(),
            })
        })
        .collect())
}

#[cfg(test)]
mod tests;

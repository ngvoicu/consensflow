//! The processes of a window's own tree, as a live check that opened the window reads
//! them to find and end what the window runs: what descends from the window's program,
//! by the process that started each, and nothing else of the machine's. A check that
//! ended every process whose command line held a mark ended the worker that ran it
//! (2026-10-16), and one that went by numbers alone could end a process that took a
//! number the tree had, or take for the tree's a process whose parent's number the
//! window's program had taken since (Astra and Sol on the B1 fix, their second look).
//! So a process is known by its number and when it began ([`Identity`]): a child began
//! no earlier than its parent, the window's own process is the one the check found when
//! the window opened, and a process is ended only while it is still the one chosen,
//! read again right before (`LongCommand::end_in`).
//!
//! The machine's list is `ps` on macOS and Linux (`lstart`, to the second, in the C
//! locale), and on Windows, which has none, the process table through PowerShell, as
//! JSON: a command line of many lines is one string there, where read as lines it could
//! make up a process.

use serde_json::Value;

use crate::process::{signal, Run, Signal};
use crate::{Error, Result};

/// Which process a process is: its number and when it began, which no two processes
/// have together. `started` grows with the time it says, on the machine that said it:
/// `YYYYMMDDhhmmss` of `ps`'s `lstart`, and Windows' file time of its `CreationDate`.
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

/// What a check reads of the machine's processes, and how it ends one: the seam by which
/// it finds what descends from a window's own process, and ends that alone.
pub trait Machine {
    /// Every process of the machine.
    fn processes(&self) -> Result<Vec<Running>>;
    /// Which process has the number `pid` now: none where none has it.
    fn identity(&self, pid: u32) -> Result<Option<Identity>>;
    /// Ends the process `pid`.
    fn end(&self, pid: u32);
}

/// The machine the check runs on: its own list of processes, and an end that is a kill.
#[derive(Debug, Clone, Copy)]
pub struct ThisMachine;

impl Machine for ThisMachine {
    fn processes(&self) -> Result<Vec<Running>> {
        if cfg!(windows) {
            table(None)
        } else {
            ps(None)
        }
    }

    fn identity(&self, pid: u32) -> Result<Option<Identity>> {
        let found = if cfg!(windows) {
            table(Some(pid))?
        } else {
            ps(Some(pid))?
        };
        Ok(found
            .into_iter()
            .map(|process| process.identity)
            .find(|identity| identity.pid == pid))
    }

    fn end(&self, pid: u32) {
        let _ = signal(pid, Signal::Kill);
    }
}

/// Ends each of `chosen` (chosen from a window's own tree) that is still the process
/// chosen, read again right before its end: a number another process took since is not
/// ended, nor one whose process cannot be read. What it ended. This and a machine's
/// [`Machine::end`] are the only ways a live check ends a process: no check ends one by
/// what its command line says (a guard test of the live suites holds them to it).
pub fn end_chosen(machine: &dyn Machine, chosen: &[Running]) -> Vec<Running> {
    let mut ended = Vec::new();
    for process in chosen {
        let pid = process.identity.pid;
        if matches!(machine.identity(pid), Ok(Some(now)) if now == process.identity) {
            machine.end(pid);
            ended.push(process.clone());
        }
    }
    ended
}

/// Which process the program of a window is, its number `pid` (`pane.open`'s answer):
/// read while the window runs, when its number is its own. None where no process has
/// the number, or the machine cannot be read.
pub fn process_of(pid: u32) -> Option<Identity> {
    ThisMachine.identity(pid).ok().flatten()
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

/// What `ps` says of every process, or of the process `pid` alone, in the C locale so
/// that its `lstart` is what [`ps_lines`] reads.
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
            source: std::io::Error::other(ran.to_string()),
        });
    }
    Ok(ps_lines(&ran.stdout))
}

/// The processes of a listing `ps -o pid=,ppid=,lstart=,command=` made: on each line the
/// number, the parent's, five words of `lstart` (`Fri Oct  6 04:44:58 2026`) and the
/// command line, as it is spaced.
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
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// The five words of an `lstart` (`Fri Oct 16 04:44:58 2026`) as `YYYYMMDDhhmmss`.
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
fn table_script(which: &str) -> String {
    format!(
        "[Console]::OutputEncoding = [Text.Encoding]::UTF8; Get-CimInstance Win32_Process{which} | ForEach-Object {{ [pscustomobject]@{{ pid = $_.ProcessId; ppid = $_.ParentProcessId; started = $(if ($_.CreationDate) {{ $_.CreationDate.ToFileTimeUtc() }} else {{ 0 }}); command = $_.CommandLine }} }} | ConvertTo-Json -Compress"
    )
}

/// What Windows' process table says of every process, or of the process `pid` alone.
fn table(pid: Option<u32>) -> Result<Vec<Running>> {
    let which = pid.map_or_else(String::new, |pid| format!(" -Filter 'ProcessId={pid}'"));
    let ran = Run::new("powershell")
        .args(["-NoProfile", "-Command", &table_script(&which)])
        .inheriting_env()
        .run()?;
    if ran.code != Some(0) {
        return Err(Error::Program {
            action: "list the processes with",
            program: "powershell".to_owned(),
            source: std::io::Error::other(ran.to_string()),
        });
    }
    table_of(&ran.stdout)
}

/// The processes of what [`table_script`] printed: a list of them, one alone, or nothing
/// for none, a byte order mark before it where PowerShell writes one.
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

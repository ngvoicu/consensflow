//! What the process table says, which is the evidence the smoke has of the app,
//! of its daemon and of what outlives either. Only `ps` is asked, and only of
//! processes the smoke started: the app's, its daemon's and the windows' stand-ins.

use std::path::Path;
use std::process::ExitStatus;
use std::thread;
use std::time::{Duration, Instant};

use cf_base::env::Env;
use cf_process::Ending;

use super::{Error, Result};
use crate::process::{self, Invocation};

/// How often a wait looks again.
const POLL: Duration = Duration::from_millis(100);

/// How many rounds of ending what is under a process are made before giving up.
const ROUNDS: usize = 100;

/// The process table is asked of the system's own `ps`.
const PS: &str = "/bin/ps";

/// One row of `ps -axo pid=,ppid=,stat=,command=`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub pid: u32,
    pub ppid: u32,
    pub state: String,
    pub command: String,
}

/// The next word of `text` and what follows it, whitespace before the word gone.
fn word(text: &str) -> Option<(&str, &str)> {
    let text = text.trim_start();
    if text.is_empty() {
        return None;
    }
    Some(text.split_at(text.find(char::is_whitespace).unwrap_or(text.len())))
}

/// The row a line of `ps` says, or none for a line that is no row.
fn parse_row(line: &str) -> Option<Row> {
    let number = |text: &str| {
        text.bytes()
            .all(|byte| byte.is_ascii_digit())
            .then(|| text.parse().ok())
            .flatten()
    };
    let (pid, rest) = word(line)?;
    let (ppid, rest) = word(rest)?;
    let (state, rest) = word(rest)?;
    // The command is the whole of what follows the whitespace after the state.
    if !rest.starts_with(char::is_whitespace) {
        return None;
    }
    Some(Row {
        pid: number(pid)?,
        ppid: number(ppid)?,
        state: state.to_string(),
        command: rest.trim_start().to_string(),
    })
}

/// The rows of `ps -axo pid=,ppid=,stat=,command=`.
pub fn parse_table(text: &str) -> Vec<Row> {
    text.split('\n').filter_map(parse_row).collect()
}

/// The table of processes as the system shows it now.
pub fn process_table() -> Result<Vec<Row>> {
    let ps = Invocation::new(PS, Path::new(".")).args(["-axo", "pid=,ppid=,stat=,command="]);
    let shown = process::capture(&ps, &Env::default())?;
    ensure!(shown.code == 0, "ps failed: {}", shown.stderr.trim());
    Ok(parse_table(&shown.stdout))
}

/// The pids under `root` in `table`: its children, theirs, and so on.
pub fn descendants(table: &[Row], root: u32) -> Vec<u32> {
    let mut found = Vec::new();
    let mut queue = std::collections::VecDeque::from([root]);
    while let Some(parent) = queue.pop_front() {
        for row in table {
            if row.ppid == parent && !found.contains(&row.pid) {
                found.push(row.pid);
                queue.push_back(row.pid);
            }
        }
    }
    found
}

/// Ends the process `pid`, at once: one that is already gone is no failure.
pub fn kill(pid: u32) {
    cf_process::terminate(pid, Ending::Forced);
}

/// Ends the process group that `pid` leads, at once: one that is already gone is
/// no failure. A pid that names more than a group (0 is this program's own, 1 is
/// none, -1 is every process) ends nothing.
pub fn kill_group(pid: u32) {
    #[cfg(unix)]
    if let Some(leader) = i32::try_from(pid).ok().filter(|leader| *leader > 1) {
        let group = nix::unistd::Pid::from_raw(-leader);
        let _ = nix::sys::signal::kill(group, nix::sys::signal::Signal::SIGKILL);
    }
    #[cfg(not(unix))]
    let _ = pid;
}

/// Ends `root` and everything under it, round after round until nothing is left.
/// A process group is not enough where a chain of processes keeps making the next
/// (two programs handing a command to each other for ever did, once): what is
/// under the app is found by the table, and ended before it makes another.
pub fn kill_tree(root: u32) {
    for _ in 0..ROUNDS {
        let under = process_table()
            .map(|table| descendants(&table, root))
            .unwrap_or_default();
        if under.is_empty() && !alive(root) {
            return;
        }
        for pid in std::iter::once(root).chain(under) {
            kill(pid);
        }
    }
}

/// Whether the process runs: signalled, and not a zombie waiting to be reaped.
pub fn alive(pid: u32) -> bool {
    if !cf_process::alive(pid) {
        return false;
    }
    let asked = Invocation::new(PS, Path::new(".")).args(["-p", &pid.to_string(), "-o", "stat="]);
    let Ok(shown) = process::capture(&asked, &Env::default()) else {
        return false;
    };
    let state = shown.stdout.trim();
    shown.code == 0 && !state.is_empty() && !state.starts_with('Z')
}

/// Says a process is gone, which is what the ledger's lock needs of the daemon that held it.
pub fn gone(pid: u32) -> bool {
    !alive(pid)
}

/// The number of the signal that ended a process, where one did.
#[cfg(unix)]
pub fn signal_of(status: &ExitStatus) -> Option<i32> {
    std::os::unix::process::ExitStatusExt::signal(status)
}

/// A system with no signals has none that ended a process.
#[cfg(not(unix))]
pub fn signal_of(_status: &ExitStatus) -> Option<i32> {
    None
}

/// Polls `check` until it answers something, or `timeout` passes. A `check` that
/// fails ends the wait with its failure.
pub fn until<T>(
    label: &str,
    timeout: Duration,
    mut check: impl FnMut() -> Result<Option<T>>,
) -> Result<T> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(found) = check()? {
            return Ok(found);
        }
        if Instant::now() >= deadline {
            return Err(Error::new(format!(
                "{label} did not happen within {} ms",
                timeout.as_millis()
            )));
        }
        thread::sleep(POLL);
    }
}

/// How long the waits of a case are given: each one `each`, and all of them
/// together, `twice` that, as a case is held to.
#[derive(Debug, Clone, Copy)]
pub struct Waits {
    each: Duration,
    deadline: Option<Instant>,
}

impl Waits {
    /// Waits of `each`, with no end to the case they belong to.
    pub fn new(each: Duration) -> Self {
        Self {
            each,
            deadline: None,
        }
    }

    /// Waits of `each` for a case that starts now and is given twice that in all.
    #[must_use]
    pub fn for_a_case(self) -> Self {
        Self {
            deadline: Some(Instant::now() + self.each * 2),
            ..self
        }
    }

    /// How long one wait is given.
    pub fn each(&self) -> Duration {
        self.each
    }

    /// `check` polled for as long as one wait is given, or the case has left.
    pub fn until<T>(&self, label: &str, check: impl FnMut() -> Result<Option<T>>) -> Result<T> {
        let left = self.deadline.map_or(self.each, |end| {
            end.saturating_duration_since(Instant::now())
        });
        until(label, self.each.min(left), check)
    }
}

#[cfg(test)]
mod tests;

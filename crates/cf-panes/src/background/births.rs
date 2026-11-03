//! How a window's processes began, for the one proof a Windows job can give of a job the agent
//! detached (T-1145): that the process that started it was a shell.
//!
//! A job object holds every process a window starts, and a process whose starter has gone has
//! nothing in the job to say who started it: a job the agent detached from its shell
//! (`Start-Process`) and a daemon a build left (Gradle's, which the owner does not want the
//! chief told of: 2026-10-15, `regressions/a_builds_daemon_held_a_workers_task`) both look like
//! a process with no parent. What tells them apart is what the starter was, and that can be read
//! only while the starter lives: the job tells its completion port of each process as it joins
//! (`JOB_OBJECT_MSG_NEW_PROCESS`), a moment after its starter made it, and the starter is read
//! then. This keeps, for each process the job has had, when it began and whether its starter
//! was a shell. A process the host has no birth for, or one whose starter could not be read,
//! is not proved: the host names less.
//!
//! **A birth is kept for as long as its process runs** (T-1153's D2): the job tells its port of
//! each exit too (`JOB_OBJECT_MSG_EXIT_PROCESS`, `ABNORMAL_EXIT_PROCESS`), and the birth is let
//! go of then ([`Births::forget`]). A server the agent leaves at the beginning of a day's work
//! keeps its birth through any number of short-lived processes (a test run starts one for each
//! test). The record is bounded all the same ([`BIRTHS`]), as a backstop for an exit that was not
//! told: the one recorded longest ago is let go of first, a number recorded again counting as
//! recorded now, and a process whose birth was let go of is not proved.

use std::collections::{BTreeMap, HashMap};

use super::ProcessKey;

/// How many births are kept at most: more than the processes a window has at once, in any
/// day's work.
const BIRTHS: usize = 8192;

/// What was read of a process as it joined the job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Birth {
    /// When it began, as the system keeps it: with the number, what tells it from a process
    /// that took the number later.
    began: u64,
    /// Whether the process that started it was a shell, read as it began.
    started_by_a_shell: bool,
    /// When it was recorded, in the record's own order.
    recorded: u64,
}

/// The births of a job's processes.
#[derive(Debug)]
pub(crate) struct Births {
    by_number: HashMap<u32, Birth>,
    /// The numbers by when each was recorded, oldest first, for letting go of them.
    order: BTreeMap<u64, u32>,
    /// What the next record is numbered.
    next: u64,
    /// How many are kept at most.
    cap: usize,
}

impl Default for Births {
    fn default() -> Self {
        Self::with_cap(BIRTHS)
    }
}

impl Births {
    /// A record that keeps `cap` births at most.
    pub fn with_cap(cap: usize) -> Self {
        Self {
            by_number: HashMap::new(),
            order: BTreeMap::new(),
            next: 0,
            cap,
        }
    }

    /// A process joined the job: it is `pid`, it began at `began`, and the process that started
    /// it was, or was not, a shell. A number that is recorded again is the newer process's, and
    /// goes to the back of the line to be let go of.
    pub fn record(&mut self, pid: u32, began: u64, started_by_a_shell: bool) {
        self.forget(pid);
        let recorded = self.next;
        self.next += 1;
        self.by_number.insert(
            pid,
            Birth {
                began,
                started_by_a_shell,
                recorded,
            },
        );
        self.order.insert(recorded, pid);
        while self.order.len() > self.cap {
            let Some((_, oldest)) = self.order.pop_first() else {
                break;
            };
            self.by_number.remove(&oldest);
        }
    }

    /// The process `pid` exited: its birth is let go of. A number nothing was recorded for is no
    /// matter.
    pub fn forget(&mut self, pid: u32) {
        if let Some(birth) = self.by_number.remove(&pid) {
            self.order.remove(&birth.recorded);
        }
    }

    /// Whether the process `key` names was started by a shell, as it was read when it began:
    /// false for a process whose birth is not kept, or is of another process of its number.
    pub fn started_by_a_shell(&self, key: ProcessKey) -> bool {
        self.by_number
            .get(&key.pid)
            .is_some_and(|birth| birth.began == key.began && birth.started_by_a_shell)
    }
}

#[cfg(test)]
mod tests;

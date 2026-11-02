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
//! The record is bounded ([`BIRTHS`]); the oldest is let go of first, and a process whose
//! birth was let go of is not proved.

use std::collections::{HashMap, VecDeque};

use super::ProcessKey;

/// How many births are kept: more than a window's processes in a day's work.
const BIRTHS: usize = 8192;

/// What was read of a process as it joined the job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Birth {
    /// When it began, as the system keeps it: with the number, what tells it from a process
    /// that took the number later.
    began: u64,
    /// Whether the process that started it was a shell, read as it began.
    started_by_a_shell: bool,
}

/// The births of a job's processes.
#[derive(Debug, Default)]
pub(crate) struct Births {
    by_number: HashMap<u32, Birth>,
    /// The numbers, oldest first, for letting go of them.
    order: VecDeque<u32>,
}

impl Births {
    /// A process joined the job: it is `pid`, it began at `began`, and the process that started
    /// it was, or was not, a shell. A number that is recorded again is the newer process's.
    pub fn record(&mut self, pid: u32, began: u64, started_by_a_shell: bool) {
        if self
            .by_number
            .insert(
                pid,
                Birth {
                    began,
                    started_by_a_shell,
                },
            )
            .is_none()
        {
            self.order.push_back(pid);
        }
        while self.order.len() > BIRTHS {
            if let Some(oldest) = self.order.pop_front() {
                self.by_number.remove(&oldest);
            }
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

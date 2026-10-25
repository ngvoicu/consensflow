//! A program this process started, as far as whoever may end it can tell: whether it has
//! been waited for, and the job that holds it and what it started. The program's owner
//! (its `Group`, or its `Child`) holds it, and the enders it hands out hold it weakly: a
//! program no one owns any more is no one's to end.

use std::cell::Cell;

use crate::job::Job;

pub(crate) struct Live {
    /// Whether the program has been waited for: its pid, and the id of its group, may be
    /// another's now.
    exited: Cell<bool>,
    job: Job,
}

impl Live {
    pub(crate) fn new(job: Job) -> Self {
        Self {
            exited: Cell::new(false),
            job,
        }
    }

    pub(crate) fn exited(&self) -> bool {
        self.exited.get()
    }

    /// The program has been waited for: nothing is sent to its pid after this, nor to its
    /// group's id, and what it left running is let go of (its job is released).
    pub(crate) fn set_exited(&self) {
        self.job.release();
        self.exited.set(true);
    }

    pub(crate) fn job(&self) -> &Job {
        &self.job
    }
}

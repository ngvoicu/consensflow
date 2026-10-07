//! A task whose window went away mid-work is paused, not given up: the chief
//! resumes it into the same window, with its memory (`stall` in
//! `src/core/dispatcher.js`). Its requester is told in a pause note, one note
//! for the tasks that stall together. A restart that finds a dozen windows
//! gone, or a project that closes with a dozen open, is one message in the
//! chief's window and one turn, not a dozen.
//!
//! The tasks that stall together are the stalls of one burst: a pass, or the
//! closing of a project's windows. The first stall of a burst for a requester
//! queues its note, as a lone stall always did, and each later one joins that
//! note while it is queued. A note already pasted into the window is read, so
//! the stall after it queues a note of its own, and the next ones join that.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use cf_ledger::{ProjectView, TaskView};

use crate::dispatcher::Dispatcher;
use crate::seams::EngineError;

/// What the dispatcher keeps of the bursts under way.
#[derive(Debug, Default)]
pub(crate) struct StallsState {
    /// How many bursts are under way; stalls are one while any is.
    bursts: Cell<usize>,
    /// The note that told each requester of a project since its burst began,
    /// by project and requester: its message.
    told: RefCell<HashMap<(i64, String), i64>>,
}

/// A burst under way. Dropping it ends it, however its work ends: what the
/// bursts told is forgotten with the last of them.
pub(crate) struct Burst<'a>(&'a StallsState);

impl StallsState {
    /// A pass, or the closing of windows, begins: the stalls it makes are told together.
    pub(crate) fn begin_burst(&self) -> Burst<'_> {
        self.bursts.set(self.bursts.get() + 1);
        Burst(self)
    }
}

impl Drop for Burst<'_> {
    fn drop(&mut self) {
        let left = self.0.bursts.get().saturating_sub(1);
        self.0.bursts.set(left);
        if left == 0 {
            self.0.told.borrow_mut().clear();
        }
    }
}

impl Dispatcher {
    /// A task whose window went away is paused, and its requester told: in a
    /// note of its own, or in the one that told it of the stalls before this
    /// one in the same burst, if its reader has not been given it yet.
    pub(crate) fn stall(
        &self,
        project: &ProjectView,
        task: &TaskView,
        because: &str,
    ) -> Result<(), EngineError> {
        let mut ledger = self.seams.ledger.borrow_mut();
        ledger.pause_task(project.id, task.number, None, Some(because))?;
        let key = (project.id, task.requester.clone());
        let earlier = self.stalls.told.borrow().get(&key).copied();
        if let Some(note) = earlier {
            if ledger
                .join_pause_note(note, task.number, because)?
                .is_some()
            {
                return Ok(());
            }
        }
        let note = ledger.note_pause(project.id, &task.requester, task.number, because)?;
        // A stall that comes alone leaves nothing to join: its note is its own.
        if self.stalls.bursts.get() > 0 {
            self.stalls.told.borrow_mut().insert(key, note.id);
        }
        Ok(())
    }
}

//! The human's wake-up calls, as the dispatcher sends them: a chief that
//! stopped while work is left is nudged on the project's schedule, by a note
//! from the human that goes through the usual queue, so it waits for the chief
//! to be at rest and for the human's unsent words in its window like any note.
//!
//! The ledger says when a wake-up is due (`cf_ledger::Ledger::queue_wake_up_if_due`:
//! the interval, what moved since the last, never two on their way, and none
//! while work is on its way on the board); the engine adds what only it knows.
//! A wake-up is queued when the chief's window is at rest (not in a turn, not
//! waiting on a question, not out of quota), so a wake-up that fell due during
//! a long turn is not pasted in stale right after it, and not while a message
//! already waits for the human's unsent words. Whether the chief then has the
//! words is the delivery's: a window that holds them for text the human typed
//! holds the wake-up too.

use cf_ledger::{ParticipantView, ProjectView};

use crate::dispatcher::Dispatcher;
use crate::seams::EngineError;

impl Dispatcher {
    /// Queues the project's wake-up for its chief if one is due. Called at a
    /// look that found the chief's window at rest: the delivery that follows in
    /// the same step is the wake-up's, if it is the head of the chief's queue.
    pub(crate) fn queue_wake_up_if_due(
        &self,
        project: &ProjectView,
        chief: &ParticipantView,
    ) -> Result<(), EngineError> {
        if project.state != "open" {
            return Ok(());
        }
        // Most projects have no wake-ups: one read of the setting and nothing more.
        if !self.seams.ledger.borrow().wake_up(project.id)?.on {
            return Ok(());
        }
        // Words that wait for the human's own are no news to a chief that is
        // being told of them already.
        if self.holding(chief.id)? {
            return Ok(());
        }
        self.seams
            .ledger
            .borrow_mut()
            .queue_wake_up_if_due(project.id, self.now())?;
        Ok(())
    }

    /// A wake-up that waits for its turn is withdrawn once the project's
    /// windows are closed: one that was being handed over when the window went
    /// came back to the queue as the window's exit settled it, and is no more
    /// use to a chief that will come back at a Resume.
    pub(crate) fn withdraw_wake_up(&self, project: i64) -> Result<(), EngineError> {
        self.seams
            .ledger
            .borrow_mut()
            .withdraw_wake_up(project, "the project was closed")?;
        Ok(())
    }
}

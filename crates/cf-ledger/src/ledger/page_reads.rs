//! What the page reads of the ledger, each in one frame, as its callers hold
//! it.

use super::Ledger;
use crate::{
    page_reads, Board, LatestMessages, LatestTranscript, LedgerError, TaskThatFits, TaskView,
};

impl Ledger {
    /// The board as the page reads it in one frame.
    pub fn board(&self, project_id: i64) -> Result<Board, LedgerError> {
        page_reads::board(&self.store, project_id)
    }

    /// The tasks on the board for a member of their tier.
    pub fn open_tasks(&self, project_id: i64) -> Result<Vec<TaskView>, LedgerError> {
        page_reads::open_tasks(&self.store, project_id)
    }

    /// A task with its thread as the page reads it in one frame; none when there is no such task.
    pub fn task_that_fits(
        &self,
        project_id: i64,
        number: i64,
    ) -> Result<Option<TaskThatFits>, LedgerError> {
        page_reads::task_that_fits(&self.store, project_id, number)
    }

    /// What a task's windows wrote, the last that fit in one frame, at most `limit`.
    pub fn latest_transcript(
        &self,
        project_id: i64,
        number: i64,
        limit: Option<usize>,
    ) -> Result<LatestTranscript, LedgerError> {
        page_reads::latest_transcript(&self.store, project_id, number, limit)
    }

    /// A participant's newest messages that fit in one frame; with `unread`, the notes still queued.
    pub fn latest_messages(
        &self,
        participant_id: i64,
        unread: bool,
    ) -> Result<LatestMessages, LedgerError> {
        page_reads::latest_messages(&self.store, participant_id, unread)
    }
}

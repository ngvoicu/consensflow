//! The ledger's tasks, as its callers hold them (`index.js`, tasks).

use super::Ledger;
use crate::{
    tasks, HeldTask, LedgerError, NewTask, Stop, TaskCreated, TaskMoved, TaskReleased, TaskThread,
    TaskView,
};

impl Ledger {
    /// A task from the chief or the human, by name or for a pool and tier.
    pub fn create_task(
        &mut self,
        project_id: i64,
        request: &NewTask,
    ) -> Result<TaskCreated, LedgerError> {
        tasks::create_task(&mut self.store, project_id, request)
    }

    /// The daemon gives an open task to a new session of a member.
    pub fn assign_task(
        &mut self,
        project_id: i64,
        number: i64,
        participant_id: i64,
    ) -> Result<TaskMoved, LedgerError> {
        tasks::assign_task(&mut self.store, project_id, number, participant_id)
    }

    /// A task given by tier goes back to the board for another member of that tier.
    pub fn release_task(
        &mut self,
        project_id: i64,
        number: i64,
        because: &str,
    ) -> Result<TaskReleased, LedgerError> {
        tasks::release_task(&mut self.store, project_id, number, because)
    }

    /// Whether a task may go back to the board for its tier, without moving it; says why not.
    pub fn check_release(&self, project_id: i64, number: i64) -> Result<(), LedgerError> {
        tasks::check_release(&self.store, project_id, number)
    }

    /// The assignee's answer finishes the task.
    pub fn record_result(
        &mut self,
        project_id: i64,
        number: i64,
        body: &str,
    ) -> Result<TaskMoved, LedgerError> {
        tasks::record_result(&mut self.store, project_id, number, body)
    }

    /// A coordinator accepts a task's result.
    pub fn accept_task(
        &mut self,
        project_id: i64,
        number: i64,
        by: &str,
    ) -> Result<TaskView, LedgerError> {
        tasks::accept_task(&mut self.store, project_id, number, by)
    }

    /// A worker's task stops without ending; `by` none when ConsensFlow pauses it.
    pub fn pause_task(
        &mut self,
        project_id: i64,
        number: i64,
        by: Option<&str>,
        because: Option<&str>,
    ) -> Result<TaskView, LedgerError> {
        #[cfg(feature = "test-support")]
        self.watched("pause_task", Some(number))?;
        tasks::pause_task(&mut self.store, project_id, number, by, because)
    }

    /// The daemon holds a task while its member is out of quota, until `until`.
    pub fn hold_task(
        &mut self,
        project_id: i64,
        number: i64,
        until: &str,
        because: &str,
    ) -> Result<TaskView, LedgerError> {
        tasks::hold_task(&mut self.store, project_id, number, until, because)
    }

    /// The daemon ends a held task's hold without resuming it: the task stays paused.
    pub fn clear_hold(
        &mut self,
        project_id: i64,
        number: i64,
        because: &str,
    ) -> Result<TaskView, LedgerError> {
        tasks::clear_hold(&mut self.store, project_id, number, because)
    }

    /// The held tasks whose time has come at `now`, an ISO time.
    pub fn held_tasks_due(&self, now: &str) -> Result<Vec<HeldTask>, LedgerError> {
        tasks::held_tasks_due(&self.store, now)
    }

    /// The stops asked of a participant's window: the task it works on, and
    /// how many its pauses asked. None for a window that works on none. A
    /// member session holds one task at a time (`require_free`), and is asked
    /// for the stops of that one.
    pub fn stop_of(&self, participant_id: i64) -> Result<Option<Stop>, LedgerError> {
        tasks::stop_of(&self.store, participant_id)
    }

    /// The task a participant's window is on as far as what it sends goes: the
    /// one it holds, else its paused one. A member session holds one task at
    /// a time (`require_free`): this is it.
    pub fn task_in_hand(&self, participant_id: i64) -> Result<Option<TaskThread>, LedgerError> {
        tasks::task_in_hand(&self.store, participant_id)
    }

    /// The paused task a participant still holds, or none.
    pub fn paused_task(&self, participant_id: i64) -> Result<Option<TaskThread>, LedgerError> {
        tasks::paused_task(&self.store, participant_id)
    }

    /// Whether a tell for the task reached the participant's window since the task was paused.
    pub fn told_since_paused(
        &self,
        participant_id: i64,
        task_id: i64,
    ) -> Result<bool, LedgerError> {
        tasks::told_since_paused(&self.store, participant_id, task_id)
    }

    /// A paused task goes on with the words that resume it; `by` none when the daemon does.
    pub fn resume_task(
        &mut self,
        project_id: i64,
        number: i64,
        by: Option<&str>,
        body: &str,
    ) -> Result<TaskMoved, LedgerError> {
        tasks::resume_task(&mut self.store, project_id, number, by, body)
    }

    /// A follow-up on a finished or failed task, back to its assignee. A
    /// question of the window's whose answer did not stand is still to be
    /// answered, and the one who was asked is told so.
    pub fn reopen_task(
        &mut self,
        project_id: i64,
        number: i64,
        by: &str,
        body: &str,
    ) -> Result<TaskMoved, LedgerError> {
        tasks::reopen_task(&mut self.store, project_id, number, by, body)
    }

    /// A task is called off, its requester told.
    pub fn cancel_task(
        &mut self,
        project_id: i64,
        number: i64,
        by: &str,
    ) -> Result<TaskView, LedgerError> {
        tasks::cancel_task(&mut self.store, project_id, number, by)
    }

    /// The daemon gives up on a task.
    pub fn fail_task(
        &mut self,
        project_id: i64,
        number: i64,
        reason: &str,
    ) -> Result<TaskView, LedgerError> {
        tasks::fail_task(&mut self.store, project_id, number, reason)
    }

    /// The human takes finished tasks off the board for good, all or none.
    pub fn delete_tasks(
        &mut self,
        project_id: i64,
        numbers: &[i64],
    ) -> Result<Vec<TaskView>, LedgerError> {
        tasks::delete_tasks(&mut self.store, project_id, numbers)
    }

    /// A task and its whole thread; none when there is no such task.
    pub fn task(&self, project_id: i64, number: i64) -> Result<Option<TaskThread>, LedgerError> {
        tasks::task(&self.store, project_id, number)
    }

    /// The task a participant has in progress; with `queued`, one still arriving too.
    pub fn active_task(
        &self,
        participant_id: i64,
        queued: bool,
    ) -> Result<Option<TaskThread>, LedgerError> {
        tasks::active_task(&self.store, participant_id, queued)
    }

    /// The task of the newest message for a participant, whatever its state now.
    pub fn last_task(&self, participant_id: i64) -> Result<Option<TaskThread>, LedgerError> {
        tasks::last_task(&self.store, participant_id)
    }
}

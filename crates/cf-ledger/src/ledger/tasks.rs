//! The ledger's tasks, as its callers hold them.

use super::Ledger;
use crate::{
    tasks, HeldTask, LedgerError, NewTask, Stop, TaskCreated, TaskEdit, TaskEdited, TaskMoved,
    TaskReleased, TaskRetiered, TaskThread, TaskView,
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

    /// An open task for a tier nobody on the staff holds any more goes to the nearest
    /// one somebody does: which tier it was for, which it is for, and who holds it;
    /// none when it stays as it is.
    pub fn retier_open_task(
        &mut self,
        project_id: i64,
        number: i64,
    ) -> Result<Option<TaskRetiered>, LedgerError> {
        tasks::retier_open_task(&mut self.store, project_id, number)
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

    /// The engine holds a queued follow-up that cannot go on in the conversation of the task it
    /// follows: paused, with the mark that it was held for this.
    pub fn hold_for_context(
        &mut self,
        project_id: i64,
        number: i64,
        because: &str,
    ) -> Result<TaskView, LedgerError> {
        tasks::hold_for_context(&mut self.store, project_id, number, because)
    }

    /// Whether T-`number` was held once for the conversation it could not go on in.
    pub fn held_for_context(&self, project_id: i64, number: i64) -> Result<bool, LedgerError> {
        tasks::held_for_context(&self.store, project_id, number)
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

    /// The chief or the human takes what the worker said last as the task's result: of a
    /// task at work, or of a paused one.
    pub fn record_finished(
        &mut self,
        project_id: i64,
        number: i64,
        body: &str,
    ) -> Result<TaskMoved, LedgerError> {
        tasks::record_finished(&mut self.store, project_id, number, body)
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

    /// A coordinator accepts a failed task on `checked`, the words of what they checked.
    pub fn accept_failed_task(
        &mut self,
        project_id: i64,
        number: i64,
        by: &str,
        checked: &str,
    ) -> Result<TaskView, LedgerError> {
        tasks::accept_failed_task(&mut self.store, project_id, number, by, checked)
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

    /// ConsensFlow relaunches the window of T-`number` that began no turn on message
    /// `message_id`, stopped by its own stop `seq`: that message goes back to the window's
    /// keeping and the task is queued for it, or nothing moves where anything of the stall
    /// changed (`not-relaunched`).
    pub fn relaunch_stalled(
        &mut self,
        project_id: i64,
        number: i64,
        participant_id: i64,
        message_id: i64,
        seq: i64,
    ) -> Result<TaskView, LedgerError> {
        tasks::relaunch_stalled(
            &mut self.store,
            project_id,
            number,
            participant_id,
            message_id,
            seq,
        )
    }

    /// Whether T-`number` was relaunched in its window: a message of it reached that window.
    pub fn task_relaunched(&self, project_id: i64, number: i64) -> Result<bool, LedgerError> {
        tasks::task_relaunched(&self.store, project_id, number)
    }

    /// Whether message `message_id` was relaunched once already.
    pub fn was_relaunched(&self, project_id: i64, message_id: i64) -> Result<bool, LedgerError> {
        tasks::was_relaunched(&self.store, project_id, message_id)
    }

    /// ConsensFlow pauses a worker's task whose window it closed itself; in a
    /// suspended project, the human's Resume of the project resumes it. Says which
    /// stop the pause is ([`Ledger::unmark_pause`]).
    pub fn pause_with_project(
        &mut self,
        project_id: i64,
        number: i64,
        because: &str,
    ) -> Result<Stop, LedgerError> {
        #[cfg(feature = "test-support")]
        self.watched("pause_task", Some(number))?;
        tasks::pause_with_project(&mut self.store, project_id, number, because)
    }

    /// The window of a task the engine paused with its project ended by itself
    /// after all (its kill was refused): no Resume of the project resumes it. Only
    /// the pause that is stop `stop` ([`Ledger::pause_with_project`]).
    pub fn unmark_pause(
        &mut self,
        project_id: i64,
        number: i64,
        stop: i64,
    ) -> Result<(), LedgerError> {
        tasks::unmark_pause(&mut self.store, project_id, number, stop)
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

    /// Who asked the stop `seq` of task `number`: the chief or the human whose
    /// pause counted it, none where ConsensFlow asked it.
    pub fn stop_asked_by(
        &self,
        project_id: i64,
        number: i64,
        seq: i64,
    ) -> Result<Option<String>, LedgerError> {
        tasks::stop_asked_by(&self.store, project_id, number, seq)
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

    /// A task its window went on with by itself, in a turn of its own that got
    /// through: held for quota, it goes on with no words; resumed with the
    /// daemon's bare words still queued, they are withdrawn. Anything else is
    /// refused.
    pub fn go_on_in_window(
        &mut self,
        project_id: i64,
        number: i64,
    ) -> Result<TaskView, LedgerError> {
        tasks::go_on_in_window(&mut self.store, project_id, number)
    }

    /// A resumed task whose window is at work on it reads working, its resume's words
    /// still queued for the window's rest. Refused for any other task.
    pub fn work_while_resumed(
        &mut self,
        project_id: i64,
        number: i64,
    ) -> Result<TaskView, LedgerError> {
        tasks::work_while_resumed(&mut self.store, project_id, number)
    }

    /// A task that read working for the words that resumed it ([`Ledger::work_while_resumed`])
    /// reads queued again, `why` (its window rested, or went) and they have not gone in.
    /// Refused for any other.
    pub fn rest_while_resumed(
        &mut self,
        project_id: i64,
        number: i64,
        why: &str,
    ) -> Result<TaskView, LedgerError> {
        tasks::rest_while_resumed(&mut self.store, project_id, number, why)
    }

    /// The words that resumed the task, where they are queued or held for the human and the
    /// task is queued, working or waiting: what is told its window meanwhile goes in with
    /// them. None once they are being pasted or in.
    pub fn resume_words_not_in(
        &self,
        project_id: i64,
        number: i64,
    ) -> Result<Option<i64>, LedgerError> {
        let task = self.store.task_row(project_id, number)?;
        tasks::resume_words_not_in(&self.store, &task)
    }

    /// The words that resumed the task of a participant's window, if they are on their way
    /// to it still (queued, held for the human or being pasted): their message. None for a
    /// task that was not resumed by words, or whose window has them. The daemon's own bare
    /// words, which a window that goes on by itself withdraws, are none.
    pub fn resume_words_waiting(
        &self,
        project_id: i64,
        number: i64,
    ) -> Result<Option<i64>, LedgerError> {
        let task = self.store.task_row(project_id, number)?;
        Ok(tasks::resume_words(&self.store, &task)?.map(|(message, _)| message))
    }

    /// A task the daemon resumed went on: its window got a turn through after
    /// the words that resumed it. Its requester, told it waits, is told once
    /// that it goes on.
    pub fn tell_it_goes_on(&mut self, project_id: i64, number: i64) -> Result<(), LedgerError> {
        tasks::tell_it_goes_on(&mut self.store, project_id, number)
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

    /// The brief of a task not yet given to a window is replaced; the old one
    /// is kept in the log.
    pub fn edit_task(
        &mut self,
        project_id: i64,
        number: i64,
        by: &str,
        body: &str,
    ) -> Result<TaskView, LedgerError> {
        tasks::edit_task(&mut self.store, project_id, number, by, body)
    }

    /// The brief, the tier, or both, of a task not yet given to a window, in one step.
    pub fn edit_task_with(
        &mut self,
        project_id: i64,
        number: i64,
        by: &str,
        edit: &TaskEdit,
    ) -> Result<TaskEdited, LedgerError> {
        tasks::edit_task_with(&mut self.store, project_id, number, by, edit)
    }

    /// The title of a task in any state is `title`; the old one stays in the log.
    pub fn retitle_task(
        &mut self,
        project_id: i64,
        number: i64,
        by: &str,
        title: &str,
    ) -> Result<TaskView, LedgerError> {
        tasks::retitle_task(&mut self.store, project_id, number, by, title)
    }

    /// `from` tells an open task `words`, which no window has yet: they are added
    /// to its brief and go to whichever window takes the task, once, with it.
    pub fn tell_open(
        &mut self,
        project_id: i64,
        number: i64,
        from: &str,
        words: &str,
    ) -> Result<TaskView, LedgerError> {
        tasks::tell_open(&mut self.store, project_id, number, from, words)
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

    /// A task fails, as Node's ledger failed it: what its recordings call. The
    /// daemon gives up on a task with [`Ledger::give_up_task`].
    pub fn fail_task(
        &mut self,
        project_id: i64,
        number: i64,
        reason: &str,
    ) -> Result<TaskView, LedgerError> {
        tasks::fail_task(&mut self.store, project_id, number, reason)
    }

    /// The daemon gives up on a task, and tells its requester why, with the
    /// words it told the task's window that went with it.
    pub fn give_up_task(
        &mut self,
        project_id: i64,
        number: i64,
        reason: &str,
    ) -> Result<TaskView, LedgerError> {
        tasks::give_up_task(&mut self.store, project_id, number, reason)
    }

    /// What the note that tells `requester` that task `number` failed adds of the tasks it asked
    /// for that wait for it: a sentence led by a space, none where nothing of theirs waits. The
    /// engine writes that note when a delivery that did not arrive failed the task, and the
    /// failure comes first in it, as in the ledger's own (`give_up_task`).
    pub fn waiting_for_it(
        &self,
        project_id: i64,
        number: i64,
        requester: &str,
    ) -> Result<String, LedgerError> {
        tasks::waiting_for_it_of(&self.store, project_id, number, requester)
    }

    /// The requesters of the tasks that wait for the failed task `number`, but `named`, whose
    /// note of the failure names its own ([`Ledger::waiting_for_it`]), are told that they wait
    /// for a failed task: the engine says it last, after the words of the failure.
    pub fn tell_waiting_for_failed(
        &mut self,
        project_id: i64,
        number: i64,
        named: &str,
    ) -> Result<(), LedgerError> {
        tasks::tell_waiting_for_failed(&mut self.store, project_id, number, named)
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

    /// The task a participant's window was last given, whatever its state now: a
    /// task message queued for it since is not given yet.
    pub fn task_given(&self, participant_id: i64) -> Result<Option<TaskThread>, LedgerError> {
        tasks::task_given(&self.store, participant_id)
    }
}

//! Finish: the chief (`cf task finish T-n`, the API's `POST /api/tasks/<n>/finish`) or the
//! human (Finish in the task's drawer) takes what the worker of a working task said last
//! as its result (the owner's decision of 2026-10-18: the chief can finish any working task).
//! A paused task is finished the same way, whichever pause it is in (the chief's explicit
//! call): the window's last complete answer since the latest message pasted into it. After a
//! tell that is the answer its turn on the tell ended with, which the chief is told (`paused`);
//! a pause that is none of a tell's (`cf task pause`, a Close, a stall) has whatever complete
//! answer the window wrote since its last message, if it wrote one, and a harness that marks the
//! text of a turn it was stopped in complete would give the chief those cut words (Claude Code
//! marks it incomplete). While a tell to the window is queued or being pasted Finish is refused:
//! the record holds the turn from before the tell, which is not its answer.
//!
//! It is the way out of a wait that has no end, and of a reader that reads a turn at work
//! that is not (T-442, dip-agentos's T-20): a job that runs for ever (a dev server, a
//! headless browser the harness moved to the background) never wakes its agent, and its
//! task stays working for as long as it runs; waiting is the default, and work has no time
//! limit. A monitor that said it ended (its time ran out, it was stopped, its socket
//! closed) is counted until what runs in its window says so: a monitor of a command is
//! over once a look finds no command of the harness's running in the window
//! (`super::Dispatcher::watch_monitors`); one of a socket or an MCP server, and every one
//! where the host cannot tell the window's commands (a harness that keeps its servers
//! otherwise, a tree it cannot read), by
//! Finish alone, for a real notice ends it in Claude Code, which says no status of it
//! after and refuses its TaskStop (`cf_harness::claude::record::jobs`). So it is for
//! **any working task**, held or not,
//! at rest or not: the chief's word overrides what the reader thinks. It takes the last
//! complete answer the worker wrote after the task's latest message pasted into its window
//! (`answers_to_the_task`'s `complete_run`), records it once as the result, with a line of
//! what it knows still runs, and ends nothing. It is refused, with the reason, where there is no such answer, where
//! the task is neither working nor paused, and where no member's open window works on it;
//! cancel is the way out then.
//!
//! **Finishing ends nothing.** The host cannot prove a process a job's: what still runs
//! goes on, and the window's close ends what the host can tell is the window's, as a
//! close does. A window the human keeps open keeps it; no other task goes into that window
//! (`ownership`).
//!
//! It is done at once, holding the window as a step does, so that no pass looks at the
//! window meanwhile: the answer is the task as it is with its result, or the refusal. A
//! second request finds the task done and is refused; a turn that ends later answers no
//! working task.

use std::rc::Rc;

use cf_base::refusal::Refusal;
use cf_ledger::{ParticipantView, TaskThread, TaskView};

use super::{
    first_on_its_way, leftovers, lost_in_turn, pending_after_look, watching_of, with_leftovers,
    with_lost, words_of,
};
use crate::dispatcher::Dispatcher;
use crate::record::Record;
use crate::role_change::answers_to_the_task;
use crate::runtime::returning;
use crate::seams::EngineError;

/// A refusal of a task's move, as the ledger's are: its state, 409.
fn refused(code: &'static str, message: String) -> EngineError {
    EngineError::Refused(Refusal::with_status(code, message, 409))
}

/// A task finished, as it is now: done, with its result; and how many background jobs and
/// agents the worker's harness counted still running, none of which finishing ends.
#[derive(Debug, Clone, PartialEq)]
pub struct Finished {
    pub task: TaskView,
    pub jobs: usize,
    pub agents: usize,
}

impl Dispatcher {
    /// `by` (the chief's handle, or the human's) finishes T-`number` of `project`: what its
    /// worker said last is its result (see above). Refused for a task that is neither working
    /// nor paused, that no member's open window works on, whose worker wrote no complete answer
    /// after its latest message, or whose window has words told to it on their way.
    pub async fn finish_task(
        self: &Rc<Self>,
        project: i64,
        number: i64,
        by: &str,
    ) -> Result<Finished, EngineError> {
        let (_, _, record) = self.finishable(project, number)?;
        let (this, held, by) = (Rc::clone(self), Rc::clone(&record), by.to_owned());
        let work = async move { this.finish_held(project, number, &by, &held).await };
        self.exclusive(&record, work).await.await
    }

    /// The task, its worker and the record of the window that works on it, where it is one
    /// to finish: working or paused, a member's, its window open.
    fn finishable(
        &self,
        project: i64,
        number: i64,
    ) -> Result<(TaskThread, ParticipantView, Rc<Record>), EngineError> {
        let found = self.seams.ledger.borrow().task(project, number)?;
        let Some(thread) = found else {
            return Err(EngineError::said(
                "unknown-task",
                format!("no task T-{number} in this project"),
            ));
        };
        if !matches!(thread.task.state.as_str(), "working" | "paused") {
            return Err(refused(
                "invalid-transition",
                format!("cannot finish T-{number}: it is {}", thread.task.state),
            ));
        }
        let worker = self
            .known_project(project)?
            .participants
            .into_iter()
            .find(|participant| {
                !matches!(participant.role.as_str(), "chief" | "human")
                    && Some(&participant.handle) == thread.task.assignee.as_ref()
            });
        let Some(worker) = worker else {
            return Err(refused(
                "not-a-session",
                format!("cannot finish T-{number}: no member's window works on it"),
            ));
        };
        let record = self
            .record(worker.id)
            .filter(|record| record.window.borrow().pane.is_some());
        let Some(record) = record else {
            return Err(refused(
                "no-window",
                format!(
                    "cannot finish T-{number}: @{}'s window is not open, so nothing it said can be read; cancel it instead",
                    worker.handle
                ),
            ));
        };
        Ok((thread, worker, record))
    }

    /// The finish, holding the window: the task asked again (it may have moved while the
    /// step before held it), the window read now, and the result recorded.
    async fn finish_held(
        self: &Rc<Self>,
        project: i64,
        number: i64,
        by: &str,
        record: &Rc<Record>,
    ) -> Result<Finished, EngineError> {
        let (thread, participant, _) = self.finishable(project, number)?;
        // The record holds the turn from before a tell until the tell is pasted into the
        // window: what it says last then is not the answer to the tell (Calliope's P3 on
        // round 2).
        if self
            .seams
            .ledger
            .borrow()
            .tell_on_its_way(participant.id, thread.task.id)?
        {
            return Err(refused(
                "tell-on-its-way",
                format!(
                    "cannot finish T-{number}: the words you told @{} are on their way to its window, and its answer to them comes first; finish it once it has answered them, or cancel it",
                    participant.handle
                ),
            ));
        }
        let observed = returning(self.observe(&participant, record))
            .await
            .map_err(|why| {
                refused(
                    "unread",
                    format!(
                        "cannot finish T-{number}: @{}'s window could not be read: {why}",
                        participant.handle
                    ),
                )
            })?;
        // The task may have moved while the window was read.
        let (thread, _, _) = self.finishable(project, number)?;
        if first_on_its_way(record) {
            return Err(refused(
                "first-message-on-its-way",
                format!(
                    "cannot finish T-{number}: @{}'s window has not shown its first message yet, and its answer to it is the result; finish it once it has answered it, or cancel it",
                    participant.handle
                ),
            ));
        }
        let items = observed.items();
        let replacements = self.seams.ledger.borrow().role_notes_to(participant.id)?;
        let start = self
            .latest_message_at(&thread, &participant, items, &replacements)?
            .ok_or_else(|| {
                refused(
                    "no-delivery",
                    format!(
                        "cannot finish T-{number}: no answer can be told to follow its latest message to @{}: the message is not in its window yet, or its record holds no paste of it; wait, send it a message about the task, or cancel it",
                        participant.handle
                    ),
                )
            })?;
        // Where torn-line counts the lines its record lost from (`lost_in_turn`): the paste of
        // the latest message about the task, before any later one (3.0.6-alpha.3's integration B).
        let delivered = self.delivered_at(participant.id, &thread, items)?;
        let lost = lost_in_turn(record, &observed, delivered);
        // A line lost in the turn may have been the answer: the refusal says so (Calliope's
        // P3 (a), T-624).
        let none = || {
            let handle = &participant.handle;
            let because = match lost {
                Some(line) => format!(
                    "; a line of @{handle}'s record was lost (record {line}) in the turn, which may have been its answer: send it a message about the task, or cancel it"
                ),
                None => "; wait for one, or cancel it".to_owned(),
            };
            refused(
                "no-answer",
                format!(
                    "cannot finish T-{number}: @{handle} has written no complete answer since its latest message about it{because}"
                ),
            )
        };
        let run = answers_to_the_task(&items[start + 1..], &replacements)
            .and_then(|answers| answers.complete_run)
            .ok_or_else(none)?;
        // Words told to the window that have not reached it yet go in when it is next at
        // rest, and the answer to them is the result: finishing now would take an answer
        // that came before them (Calliope's L1 on the merge with field-306, T-370).
        let told = self
            .seams
            .ledger
            .borrow()
            .told_waiting(participant.id, thread.task.id)?;
        if told {
            return Err(refused(
                "words-on-their-way",
                format!(
                    "cannot finish T-{number}: words told to @{} are on their way to its window, and its answer to them is the result; finish it once it has answered them, or cancel it",
                    participant.handle
                ),
            ));
        }
        // What the turn left, as the note to the chief counts it: a monitor that said it ended
        // is over where a look found no command in the window.
        let pending = pending_after_look(&observed, watching_of(&record.held.borrow()));
        let words = with_lost(words_of(&run), &participant.handle, lost);
        let body = with_leftovers(
            words,
            &leftovers(&participant.handle, pending, Some(by), observed.settled),
        );
        let moved = self
            .seams
            .ledger
            .borrow_mut()
            .record_finished(project, number, &body)?;
        record.held.borrow_mut().reset();
        self.changed();
        Ok(Finished {
            task: moved.task,
            jobs: pending.jobs,
            agents: pending.agents,
        })
    }
}

//! What a worker's turn comes to when it ends: its final answer is its task's result,
//! unless its harness says the turn left work running that it wakes its agent for.
//! Turn-end's smallest rule (the chief's decision of 2026-10-21, on the advisor's answer
//! of T-8): what is uncertain never completes a task, reuses a window or ends a process.
//!
//! **The result is the final answer** (the owner's decision of 2026-10-15, after the
//! chief's reports on 3.0.4: T-14 and T-15 were given ten to twenty lines of progress
//! before their reports; dip-agentos #5): what the member wrote after its last tool call
//! or message in its last turn, after the task's latest message pasted into the window
//! ([`answers_to_the_task`]). That message is found by the record's item its arrival was
//! proved by (its receipt), never by words that quote its header. What it wrote before
//! stays in the transcript (`cf task get --transcript`), and so does all it wrote of a
//! turn that ended before the one that answers. Each harness's own record says which turn
//! that is: a harness marks the step that ends its turn complete (Claude Code's
//! `end_turn` and its boundary record, Pi's `stop`, OpenCode's `finish: stop`, Devin's
//! last reply of a request, Codex's `final_answer`), and the progress it writes on the way
//! is no complete answer. What the harness wrote itself is no answer and cuts none: the
//! summary of a compaction, a notification it keeps in its record alone, Claude Code's
//! close of a cut turn. A turn that wrote nothing after its last tool call says so
//! ([`WITHOUT_ANSWER`]).
//!
//! **Pending work keeps the task working** (A2; dip-agentos #1, poker-lab's T-442):
//! Claude Code lets a command run in the background, or a Monitor watch one, and its
//! agent end its turn, and begins a turn by itself for each event and each end, which
//! wakes the agent; it counts the background agents and workflows it wakes the agent for
//! itself. A turn that ended with any of those pending is no result: the task stays
//! working, nothing is ended, and whoever gave the task is told once that it goes on in
//! the background, with the words the turn ended with and how to end the wait
//! ([`Dispatcher::finish_task`]). Only the harness's own word that each is over clears
//! it (`cf-harness`' `claude::record::jobs`): no time that passes, and no look at the
//! window's processes that finds none, releases a task held so. A job that never ends
//! (a dev server) keeps its task working until the chief or the human finishes it.
//!
//! **A turn's end ends nothing.** What a worker's turn left running, its harness's or
//! not, goes on: a free worker's window closes after its result, as any does, and the
//! close ends what the host can tell is the window's, which it lists to whoever gave the
//! task ([`Dispatcher::tell_left_running`]); a window the human keeps open keeps it. What
//! is known of the work the record no longer waits on, and how much of it is not known
//! to have ended, is said in the result itself ([`leftovers`]).
//!
//! **A window has one task** (`ownership`): no other task's message goes into a window
//! once one task's has, whatever ended that task; so no later task's turn is woken by
//! what an earlier one left, and no later task is given an earlier task's words.

use cf_base::js;
use cf_harness::contract::Observed;
use cf_harness::records::{Item, Pending, Role};
use cf_ledger::{NewNote, ParticipantView, ProjectView, TaskThread};
use serde_json::Value;

use crate::delivery_text::marker_of;
use crate::dispatcher::Dispatcher;
use crate::record::Record;
use crate::role_change::answers_to_the_task;
use crate::seams::EngineError;

/// What a result says of a turn that wrote nothing after its last tool call.
pub(crate) const WITHOUT_ANSWER: &str = "(the agent ended its turn without a written answer)";

/// What a worker's turn came to, for its task.
pub(crate) enum Turn {
    /// Nothing is over: the turn goes on, or it answers no task, or it failed (and the
    /// task with it).
    Open,
    /// Over, and what it left running is what its harness wakes its agent for: the
    /// task goes on in the background.
    Held,
    /// Over, and its final answer is the task's result, recorded.
    Answered,
}

/// What a worker's task is held for: the task, and how many jobs and agents its turn left
/// running, which its harness wakes the agent for as each ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hold {
    pub task: i64,
    pub jobs: usize,
    pub agents: usize,
}

/// What the engine keeps of a held task: the record's part of the turn's end.
#[derive(Debug, Default)]
pub(crate) struct HeldPart {
    /// What the task is held for, while it is.
    pub(crate) holding: Option<Hold>,
    /// The task whose requester was told it goes on in the background: once for each
    /// time it is held, whatever its turns say on the way.
    told: Option<i64>,
}

impl Dispatcher {
    /// A worker's turn, looked at: whether it is over with an answer to its task's latest
    /// message, and what that comes to ([`Turn`]). A turn that failed is its task's
    /// failure, whatever it left running; one that ended with its answer and nothing
    /// pending is its result, recorded now, before anything else is asked of anyone: what
    /// follows (a close, its note) can no longer lose it.
    pub(crate) fn collect(
        &self,
        project: &ProjectView,
        participant: &ParticipantView,
        record: &Record,
        observed: &Observed,
    ) -> Result<Turn, EngineError> {
        let task = self
            .seams
            .ledger
            .borrow()
            .active_task(participant.id, false)?;
        let Some(thread) = task.filter(|thread| thread.task.state == "working") else {
            *record.held.borrow_mut() = HeldPart::default();
            return Ok(Turn::Open);
        };
        let items = observed.items();
        let Some(start) = self.delivered_at(participant.id, &thread, items)? else {
            return Ok(Turn::Open);
        };
        if observed.failed {
            let because = format!("@{}'s harness reported a failure", participant.handle);
            self.fail_task(project, &thread.task, &because)?;
            return Ok(Turn::Open);
        }
        if !observed.settled {
            return Ok(Turn::Open);
        }
        let replacements = self.seams.ledger.borrow().role_notes_to(participant.id)?;
        let Some(answers) = answers_to_the_task(&items[start + 1..], &replacements)
            .filter(|answers| answers.last.is_some_and(|last| last.complete))
        else {
            return Ok(Turn::Open);
        };
        let pending = observed.pending();
        let words = words_of(&answers.final_run);
        if pending.total() > 0 {
            let hold = Hold {
                task: thread.task.number,
                jobs: pending.jobs,
                agents: pending.agents,
            };
            let told = {
                let mut held = record.held.borrow_mut();
                held.holding = Some(hold);
                held.told == Some(hold.task)
            };
            if !told {
                self.tell_it_goes_on(project, participant, &thread, pending, &words)?;
                record.held.borrow_mut().told = Some(hold.task);
            }
            return Ok(Turn::Held);
        }
        *record.held.borrow_mut() = HeldPart::default();
        let body = with_leftovers(words, &leftovers(&participant.handle, pending, None));
        self.seams
            .ledger
            .borrow_mut()
            .record_result(project.id, thread.task.number, &body)?;
        self.changed();
        Ok(Turn::Answered)
    }

    /// The place among `items`, the window's record, of the latest message of the task
    /// `thread` pasted into the window of the participant: the item its arrival was proved
    /// by (`receipt`). A message whose receipt names no item of the record (one the record
    /// lost, a receipt of an older build) is found by its header beginning an item of the
    /// user's, never by a quotation in a tool's output or in the member's words. None
    /// where the task has no message pasted, or the record does not hold it: the window
    /// answers nothing of the task yet.
    pub(crate) fn delivered_at(
        &self,
        participant: i64,
        thread: &TaskThread,
        items: &[Item],
    ) -> Result<Option<usize>, EngineError> {
        let latest = self
            .seams
            .ledger
            .borrow()
            .last_pasted(participant, thread.task.id)?;
        let Some(latest) = latest else {
            return Ok(None);
        };
        let receipt = latest.receipt.get("item").and_then(Value::as_str);
        if let Some(found) = receipt.and_then(|item| {
            items
                .iter()
                .rposition(|said| said.role == Role::User && &*said.id == item)
        }) {
            return Ok(Some(found));
        }
        let marker = marker_of(latest.id);
        Ok(items
            .iter()
            .position(|item| item.role == Role::User && item.text.contains(&marker)))
    }

    /// What the task of the participant's window is held for, while it is.
    pub fn held_for(&self, participant: i64) -> Option<Hold> {
        self.record(participant)
            .and_then(|record| record.held.borrow().holding)
    }

    /// Whoever gave the task is told it goes on in the background, once for each time it is
    /// held ([`HeldPart`]): what the worker ended its turn with, what it left running that
    /// it is woken for, and how to take its last complete answer as the result instead
    /// ([`Dispatcher::finish_task`]). What the engine keeps goes with the window and with
    /// the daemon: a task held in a window opened again, or after a restart, is told again.
    fn tell_it_goes_on(
        &self,
        project: &ProjectView,
        participant: &ParticipantView,
        thread: &TaskThread,
        pending: Pending,
        words: &str,
    ) -> Result<(), EngineError> {
        let number = thread.task.number;
        let what = match pending.total() {
            1 => "it",
            _ => "them",
        };
        let body = format!(
            "T-{number} goes on in the background: @{} ended its turn with {} running, which its harness wakes it for as each ends, and its result comes when a turn of its ends with none left. To take what it said last as the result now: cf task finish T-{number}, which does not end {what}. What it said:\n\n{words}",
            participant.handle,
            running(pending),
        );
        self.seams.ledger.borrow_mut().note(
            project.id,
            &NewNote {
                from: None,
                to: thread.task.requester.clone(),
                task: Some(number),
                body,
            },
        )?;
        self.changed();
        Ok(())
    }
}

/// What a turn left running, in the words of a note.
pub(crate) fn running(pending: Pending) -> String {
    let jobs = |count: usize| {
        let (job, jobs) = ("background job", "background jobs");
        format!("{count} {}", if count == 1 { job } else { jobs })
    };
    let agents = |count: usize| {
        let (agent, agents) = (
            "background agent or workflow",
            "background agents or workflows",
        );
        format!("{count} {}", if count == 1 { agent } else { agents })
    };
    match (pending.jobs, pending.agents) {
        (jobs_running, 0) => jobs(jobs_running),
        (0, agents_running) => agents(agents_running),
        (jobs_running, agents_running) => {
            format!("{} and {}", jobs(jobs_running), agents(agents_running))
        }
    }
}

/// What a result says, after the worker's words, of the work its window's harness knew of
/// and no longer waits on (dip-agentos #8: these were notes of their own): what was still
/// running when someone finished the task (`finished_by`), which finishing ends none of;
/// and the jobs an earlier window of the conversation left that no word says ended
/// (`Pending::retired`). Nothing where there is nothing to say. What the host finds left
/// in the window is not here: the window's close says what it ended.
pub(crate) fn leftovers(handle: &str, pending: Pending, finished_by: Option<&str>) -> String {
    let mut said = Vec::new();
    if let Some(by) = finished_by {
        let who = if by == "human" {
            "the human".to_owned()
        } else {
            format!("@{by}")
        };
        if pending.total() > 0 {
            let what = if pending.total() == 1 { "it" } else { "them" };
            said.push(format!(
                "Finished at {who}'s word while @{handle}'s harness still counted {} running, which finishing ends none of: {what} may run on until its window closes, or past it where the host cannot tell what is the window's.",
                running(pending)
            ));
        } else {
            said.push(format!(
                "Finished at {who}'s word: what @{handle} said last is the result."
            ));
        }
    }
    if pending.retired > 0 {
        let jobs = if pending.retired == 1 {
            "1 background job".to_owned()
        } else {
            format!("{} background jobs", pending.retired)
        };
        said.push(format!(
            "no word came that the {jobs} an earlier window of @{handle}'s conversation started had ended; that window was closed, which ends what the host could tell was the window's, and may not have ended a job its shell detached."
        ));
    }
    said.join(" ")
}

/// `words`, and after them what ConsensFlow says of the leftovers (`said`), where it says
/// anything: a line apart, marked as ConsensFlow's.
pub(crate) fn with_leftovers(words: String, said: &str) -> String {
    if said.is_empty() {
        words
    } else {
        format!("{words}\n\n(ConsensFlow: {said})")
    }
}

/// The words of a final answer, in the items it was written in: each trimmed, a blank
/// line between, what a harness marks as its progress left out. Where the member wrote
/// none, what says so.
pub(crate) fn words_of(final_run: &[&Item]) -> String {
    let words: Vec<&str> = final_run
        .iter()
        .filter(|item| !item.commentary)
        .map(|item| js::trim(&item.text))
        .filter(|text| !text.is_empty())
        .collect();
    if words.is_empty() {
        WITHOUT_ANSWER.to_owned()
    } else {
        words.join("\n\n")
    }
}

mod finish;

pub use finish::Finished;

#[cfg(test)]
mod tests;

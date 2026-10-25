//! The answer a tell's turn ends with while its task is paused (T-442; T-429 on 3.0.5,
//! T-534's live runs on 3.0.6-alpha.2).
//!
//! `cf tell` pauses a working task and puts the chief's words to its window as a question;
//! the window answers it with `cf answer`, and the answer reaches the chief as a message.
//! A member that has answered may go straight on, in the same turn, with the work the
//! pause stopped, and end that turn with its complete answer, or answer in plain text what
//! it should have sent with `cf answer` (OpenCode did). The task is paused, so
//! [`Dispatcher::collect`] takes no result of it, and nothing else carried those words to
//! anyone: the resume that followed began a turn whose short follow-up (the member said it
//! had finished) became the result, and the full answer had to be asked for again.
//!
//! **A pause is the chief's or the human's to end, and what a tell's turn ends with is as
//! likely to be the end of its answer as a result** ("Answered m-3."), so nothing is
//! completed for them. A turn that ends with a complete answer after the tell's paste
//! (the latest message of the window's record is a question the chief put to it, `tells`),
//! while its task is paused and nothing is pending that its harness wakes it for, puts
//! that answer to whoever gave the task, once, in a note about the task: the words, and
//! the two ways on. Finish takes the words as the task's result (a paused task is finished
//! as a working one is, `finish`), and a resume sends the window on. The ledger writes the
//! note and keeps its words true as the task moves (`cf-ledger`'s `paused_answers`): a
//! resume does not take it back unread, it says the task has moved on and that its result
//! will be the next complete answer; Finish withdraws it. What the window wrote before its
//! last tool call, or in an earlier turn, stays in the transcript, as everywhere else.
//!
//! **The chief may resume at once**, on the answer it was given, while the window is still
//! at work on the rest of its turn: the resume waits behind the turn, the task is queued
//! when the turn ends, and its answer is the follow-up to the resume's words, not the turn
//! on the tell. That turn's end is told all the same, in the words of a task that has
//! moved on.
//!
//! Told once for each tell: the ledger writes none where the task's thread has a note of
//! this tell, whatever became of it, so a daemon that restarts says nothing again.

use cf_harness::contract::Observed;
use cf_ledger::{ParticipantView, ProjectView};

use super::{words_of, WITHOUT_ANSWER};
use crate::delivery_text::header_id;
use crate::dispatcher::Dispatcher;
use crate::role_change::answers_to_the_task;
use crate::seams::EngineError;

impl Dispatcher {
    /// The turn of the window of a paused task (or of one resumed, whose resume waits behind
    /// the turn), looked at: if it ended with a complete answer after the chief's tell,
    /// whoever gave the task is told it (see above). Nothing where the task is neither, no
    /// tell is the latest message the window was pasted, the turn goes on, left work pending,
    /// or was told already.
    pub(super) fn tell_paused_answer(
        &self,
        project: &ProjectView,
        participant: &ParticipantView,
        observed: &Observed,
    ) -> Result<(), EngineError> {
        if !observed.settled || observed.failed || observed.pending().total() > 0 {
            return Ok(());
        }
        // Paused, or resumed already with the resume queued behind the turn that just ended.
        let found = self.seams.ledger.borrow().task_in_hand(participant.id)?;
        let Some(thread) =
            found.filter(|thread| matches!(thread.task.state.as_str(), "paused" | "queued"))
        else {
            return Ok(());
        };
        let tells: Vec<i64> = thread
            .messages
            .iter()
            .filter(|message| {
                message.kind == "question"
                    && message.urgent
                    && message.recipient == participant.handle
            })
            .map(|message| message.id)
            .collect();
        if tells.is_empty() {
            return Ok(());
        }
        let (items, replacements) = (
            observed.items(),
            self.seams.ledger.borrow().role_notes_to(participant.id)?,
        );
        let Some(start) = self.latest_message_at(&thread, participant, items, &replacements)?
        else {
            return Ok(());
        };
        let Some(tell) = header_id(&items[start].text).filter(|id| tells.contains(id)) else {
            return Ok(());
        };
        let Some(answers) = answers_to_the_task(&items[start + 1..], &replacements)
            .filter(|answers| answers.last.is_some_and(|last| last.complete))
        else {
            return Ok(());
        };
        let words = words_of(&answers.final_run);
        if words == WITHOUT_ANSWER {
            return Ok(());
        }
        let written = self.seams.ledger.borrow_mut().note_paused_answer(
            project.id,
            thread.task.number,
            &participant.handle,
            tell,
            &words,
        )?;
        if written.is_none() {
            return Ok(());
        }
        self.changed();
        Ok(())
    }
}

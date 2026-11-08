//! The answer a tell's turn ends with while its task is paused (T-442; T-429 on 3.0.5,
//! T-534's live runs on 3.0.6-alpha.2).
//!
//! `cf tell` pauses a working task and puts the chief's words to its window as a question;
//! the window answers it with `cf answer`, and the answer reaches the chief as a message.
//! A member that has answered may go straight on, in the same turn, with the work the
//! pause stopped, and end that turn with its complete answer, or answer in plain text what
//! it should have sent with `cf answer` (T-534). The task is paused, so
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
//! will be the next complete answer; Finish withdraws it.
//!
//! **What the window wrote first** (T-1151): a window answers a question before it works, so
//! the note also holds what it wrote after the tell and before its first tool call, where it
//! went on to a tool and then to other last words (`Answers::reply`). `live tell devin` lost
//! it: Devin wrote the line asked for as text, ran its command, and ended with the last
//! line, and the note had that alone; Claude Code, Codex and Muse write the same record, and
//! theirs reached the chief because the model also put the line into `cf answer`, or wrote
//! it again. It is not told again when it is the last words, or when the window sent it with
//! `cf answer` (the chief has it as a message of its own). Finish takes the last words only.
//! What it wrote between its tools, or in an earlier turn, stays in the transcript, as
//! everywhere else.
//!
//! **Only words after the last tool call are told** (T-878, Calliope's P2 on T-262). A result
//! with none there takes the last words written before it (`Answers::final_run`), but a
//! tell's turn that wrote words, answered with `cf answer` and ended on a step with no words
//! said what it had to in the answer: it is told nothing more, as one that ran `cf answer`
//! alone is not.
//!
//! A harness that marks the text of a turn it was stopped in complete would have a note with
//! those cut words, after a tell as after any pause; Claude Code marks it incomplete, and
//! [`answers_to_the_task`] takes only a complete answer.
//!
//! **The chief may resume at once**, on the answer it was given, while the window is still
//! at work on the rest of its turn: the resume's words wait behind the turn, and the task
//! reads queued until the window's record shows it acting after the resume, then working
//! (`resumed_at_work`, T-1139: before, it read queued for as long as the turn ran, with the
//! window at work on it). The turn's end is no result either way: the answer to the resume's
//! words is the follow-up, not the turn on the tell ([`Dispatcher::collect`] leaves a task
//! whose resume words wait to this note). That turn's end is told all the same, in the
//! words of a task that has moved on.
//!
//! **A turn that left a Monitor** that said it ended is told as a working task's result is:
//! once a look at the window's commands found none of the monitor's running
//! ([`pending_after_look`]); until then it is pending, and the look state is kept across the
//! passes of a task that is not working (`HeldPart::drop_hold`).
//!
//! Told once for each tell: the ledger writes none where the task's thread has a note of
//! this tell, whatever became of it, so a daemon that restarts says nothing again.

use cf_harness::contract::Observed;
use cf_ledger::{ParticipantView, ProjectView};

use super::{pending_after_look, reply_of, words_of, Watching, WITHOUT_ANSWER};
use crate::delivery_text::header_id;
use crate::dispatcher::Dispatcher;
use crate::role_change::answers_to_the_task;
use crate::seams::EngineError;

impl Dispatcher {
    /// The turn of the window of a paused task (or of one resumed, whose resume waits behind
    /// the turn, whether it reads queued or working meanwhile), looked at: if it ended with a complete answer after the chief's tell,
    /// whoever gave the task is told it (see above). Nothing where the task is neither, no
    /// tell is the latest message the window was pasted, the turn goes on, left work pending
    /// (the monitors that said they ended are over once a look found no command, as for a
    /// working task's result), or was told already.
    pub(super) fn tell_paused_answer(
        &self,
        project: &ProjectView,
        participant: &ParticipantView,
        observed: &Observed,
        watching: Watching,
    ) -> Result<(), EngineError> {
        if !observed.settled
            || observed.failed
            || pending_after_look(observed, watching).total() > 0
        {
            return Ok(());
        }
        // Paused, or resumed already with the resume queued behind the turn that just ended.
        let found = self.seams.ledger.borrow().task_in_hand(participant.id)?;
        // Or at work on the turn it was resumed in, the words that resumed it still waiting
        // (`resumed_at_work`).
        let Some(thread) = found else {
            return Ok(());
        };
        let waiting = thread.task.state == "working"
            && self.resume_words_wait(project.id, thread.task.number)?;
        if !waiting && !matches!(thread.task.state.as_str(), "paused" | "queued") {
            return Ok(());
        }
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
        // The words after its last tool call alone: a turn that wrote none there answered
        // with `cf answer`, which the chief has as a message, and the words it wrote before
        // that call (which a result takes where there are no others, T-262) are no more of
        // an answer (Calliope's P2 on T-262, T-878).
        let words = words_of(&answers.after_the_last_tool);
        if words == WITHOUT_ANSWER {
            return Ok(());
        }
        // What it wrote first, before its first tool call, is its reply to the tell: told
        // beside its last words, unless those are the same words or it sent them with `cf
        // answer`, which reached the chief as a message of its own.
        let reply = reply_of(&answers.reply).filter(|reply| {
            *reply != words
                && !thread.messages.iter().any(|message| {
                    message.kind == "answer"
                        && message.reply_to == Some(tell)
                        && message.body.contains(reply.as_str())
                })
        });
        let written = self.seams.ledger.borrow_mut().note_paused_answer(
            project.id,
            thread.task.number,
            &participant.handle,
            tell,
            reply.as_deref(),
            &words,
        )?;
        if written.is_none() {
            return Ok(());
        }
        self.changed();
        Ok(())
    }
}

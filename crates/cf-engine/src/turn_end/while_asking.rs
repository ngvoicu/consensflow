//! The words a worker's turn ended with while its own question was out (T-1204; the field
//! board's T-837 of 2026-10-28, found again by the triage T-1201 as T-846).
//!
//! A member asks with `cf ask` and, as often as not, goes on in the same turn (a notice
//! that needs no answer: "disk notice, no answer needed"). Its task reads `waiting` from the
//! question until the answer is *delivered*, and an answer is typed in only when the window
//! is at rest. T-837's turn ended at 01:01 with a 5.8 KB write-up, the chief had answered at
//! 00:43, and the answer went in after the write-up: so the turn ended on a task that was
//! not working, [`Dispatcher::collect`] took no result of it, and the turn that the answer
//! began ended with five lines ("I'd already finished… My result above still stands"),
//! which became the result. The write-up was in the window's record and nowhere on the
//! board; the chief took it out of the record by hand.
//!
//! **Nothing completes at that turn's end.** A member that needs the answer asks and ends its
//! turn on a line about it, and that line is no result: what is uncertain never completes a
//! task (T-8), and T-1119 undid the done tasks that a question made. The same turn, with a
//! write-up for its words, cannot be told from it by its words ("Asked.", "I still need the
//! format"). Nor is the chief told: the question is its own, and the common case, a question
//! asked and a turn ended on it, would send it a note for each. The turn is read as it was
//! (`Turn::Open`).
//!
//! **The words are kept, and given with the result.** The record holds them, and a result is
//! recorded when the turn that the answer began ends ([`Dispatcher::collect`]) or the chief
//! finishes the task ([`Dispatcher::finish_task`]): both give the turns that ended with a
//! question of the worker's out before that last turn's words, in order, each saying what it
//! was written before: "Written before the answer to its question m-N went in:". Found in
//! the record at that time, so a daemon that restarts in between loses nothing, and nothing
//! is written to the ledger for it.
//!
//! **Which turns.** Those that end right before the paste of an answer to one of the
//! worker's own questions on the task (`cf ask`, or the question ConsensFlow raised from its
//! last words, T-1119), after the latest message of the task's own that was pasted (the
//! brief, a resume, a follow-up: what was written for an earlier round was decided with it).
//! An answer is typed in at rest, so the turn before its paste is a turn that ended with the
//! question out. **Not** those that
//! - wrote no complete answer (a turn cut off);
//! - ended on the question: the last output of a tool in the turn is `cf ask`'s own ("m-N
//!   asked @chief. The answer arrives as a message; end your turn now."), or there is none,
//!   and the words are short ([`WRAPPER_CHARS`]). That is the member that did what it was
//!   told, and the words are about the question. Six such lines were measured in recorded
//!   transcripts: 51 to 249 characters ("Waiting for the chief's feedback on the FAQ
//!   pages."); T-837's write-up was 5,800. A turn that went on after asking, or whose words
//!   are long, is kept, whatever it ended on: a report followed by "Shall I continue?" is a
//!   report;
//! - are the question ConsensFlow raised, whole (it is on the board with them);
//! - are the words the result is (a reply that repeats the write-up word for word), or
//!   repeat another turn's.
//!
//! **What each side sees.** The worker's window is told nothing new. The requester (the
//! chief, or the human in the task's drawer) sees the task `waiting` until the answer is
//! delivered, as before, and then a result that begins with the write-up, labelled, and ends
//! with "What it said last:" and the reply. A result of a turn with no such turn before it is
//! what it was, byte for byte.

use cf_harness::records::{Item, Role};
use cf_ledger::{ParticipantView, TaskThread};
use cf_proto::questions::ASKED_THEN_END_THE_TURN;

use super::{with_finish_line, words_of, WITHOUT_ANSWER};
use crate::delivery_text::header_id;
use crate::dispatcher::Dispatcher;
use crate::role_change::answers_to_the_task;
use crate::seams::EngineError;

/// How many characters the words of a turn that ended on its question may have to be taken
/// for what they are, a line about the question. The longest of six measured was 249.
const WRAPPER_CHARS: usize = 500;

/// What a turn wrote that ended with a question of its worker's out: the question whose
/// answer was typed in after it, and the turn's final words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Earlier {
    pub(crate) question: i64,
    pub(crate) words: String,
}

/// Whether `turn`, the items a window's turn wrote, ended on its question: the last output
/// of a tool is `cf ask`'s ([`ASKED_THEN_END_THE_TURN`]), or it ran no tool.
fn ended_on_asking(turn: &[Item]) -> bool {
    turn.iter()
        .rfind(|item| item.role == Role::Tool)
        .is_none_or(|last| last.text.contains(ASKED_THEN_END_THE_TURN))
}

impl Dispatcher {
    /// The turns of `participant`'s window since the latest message of `thread`'s task
    /// pasted into it that ended with one of its own questions out, in order, as the record
    /// `items` shows them: the words each ended with (see above). `last` are the words the
    /// result is, which no turn of these repeats.
    pub(crate) fn written_while_asking(
        &self,
        thread: &TaskThread,
        participant: &ParticipantView,
        items: &[Item],
        replacements: &[i64],
        last: &str,
    ) -> Result<Vec<Earlier>, EngineError> {
        let own: Vec<i64> = thread
            .messages
            .iter()
            .filter(|message| {
                message.kind == "question"
                    && !message.urgent
                    && message.sender.as_deref() == Some(participant.handle.as_str())
            })
            .map(|message| message.id)
            .collect();
        if own.is_empty() {
            return Ok(Vec::new());
        }
        // The latest message of the task's own that the window was given, and where each
        // answer to one of the worker's questions was typed in after it.
        let mut floor = None;
        let mut answers = Vec::new();
        for message in &thread.messages {
            if message.recipient_id != participant.id
                || !matches!(message.state.as_str(), "delivered" | "read")
            {
                continue;
            }
            let question = message.reply_to.filter(|asked| own.contains(asked));
            match (message.kind.as_str(), question) {
                ("task", _) => {
                    if let Some(at) = self.paste_of(message, items)? {
                        floor = floor.max(Some(at));
                    }
                }
                ("answer", Some(question)) => {
                    if let Some(at) = self.paste_of(message, items)? {
                        answers.push((at, question));
                    }
                }
                _ => {}
            }
        }
        let Some(floor) = floor else {
            return Ok(Vec::new());
        };
        answers.retain(|(at, _)| *at > floor);
        answers.sort_unstable();
        let mut earlier: Vec<Earlier> = Vec::new();
        for (at, question) in answers {
            // Every message to the member ends the run of words before it, so what the
            // walk over the record from the task's message to this paste ends on is the
            // turn that ended right before the paste.
            let before = &items[floor + 1..at];
            let Some(turn) = answers_to_the_task(before, replacements)
                .filter(|turn| turn.last.is_some_and(|item| item.complete))
            else {
                continue;
            };
            let words = words_of(&turn.final_run);
            // The items of that turn: those after the last message to the member.
            let began = before
                .iter()
                .rposition(|item| {
                    item.role == Role::User
                        && !item.compaction
                        && !header_id(&item.text).is_some_and(|id| replacements.contains(&id))
                })
                .map_or(0, |at| at + 1);
            // The question raised from these words has them first, a line of a record lost in
            // the turn after them where there was one, and the line that says how to finish.
            let finish_line =
                with_finish_line(String::new(), &participant.handle, thread.task.number);
            let on_the_board = thread.messages.iter().any(|message| {
                message.kind == "question"
                    && message.sender.as_deref() == Some(participant.handle.as_str())
                    && message.body.starts_with(&words)
                    && message.body.ends_with(&finish_line)
            });
            let a_line_about_the_question =
                ended_on_asking(&before[began..]) && words.chars().count() <= WRAPPER_CHARS;
            if words == WITHOUT_ANSWER
                || words == last
                || a_line_about_the_question
                || on_the_board
                || earlier.iter().any(|kept| kept.words == words)
            {
                continue;
            }
            earlier.push(Earlier { question, words });
        }
        Ok(earlier)
    }
}

/// The words of a result with the turns that ended with a question out in front of them
/// (see above): each labelled with the answer it was written before, then the words the
/// result is. Those words alone where there are none.
pub(crate) fn with_earlier(words: String, earlier: &[Earlier]) -> String {
    if earlier.is_empty() {
        return words;
    }
    let mut said = String::new();
    for turn in earlier {
        said.push_str(&format!(
            "Written before the answer to its question m-{} went in:\n\n{}\n\n",
            turn.question, turn.words
        ));
    }
    said.push_str("What it said last:\n\n");
    said.push_str(&words);
    said
}

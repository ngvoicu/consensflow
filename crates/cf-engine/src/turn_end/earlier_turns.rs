//! The words of the turns that ended before the one whose words are the result, and that
//! no result took (T-1204; T-1219).
//!
//! A result is the final answer of a worker's last turn ([`answers_to_the_task`]): what it
//! wrote after the latest message or tool output of that turn. A turn that ended with a
//! complete answer and was not taken for the result, for whatever reason, loses its words
//! when another turn follows it: the next message to the window clears them. Two boards met
//! it:
//!
//! - **T-837** (the field board, 2026-10-28; T-1204). The worker asked the chief a question
//!   that needed no answer (`cf ask`) and went on in the same turn. Its task reads `waiting`
//!   from the question until the answer is *delivered*, and an answer is typed in only when
//!   the window is at rest: the turn ended at 01:01 with a 5.8 KB write-up on a task that
//!   takes no result ([`Dispatcher::collect`]), the answer went in after it, and the turn it
//!   began ended with five lines, which became the result.
//! - **T-1200** (2026-11-04, 02:0x; T-1219). A command the worker's Bash tool had moved to
//!   the background ended while the turn was still at work, and Claude Code queued its
//!   `<task-notification>` for the agent (a `queue-operation enqueue`, 00:08:23). The turn
//!   ended with its 8.8 KB write-up at 00:09:01.257 with that message still queued, and a
//!   turn is not over while Claude Code's queue holds one (`cf-harness`'s `claude::record`
//!   `queues`): the record read in flight, no job pending, so the end was not a hold either
//!   (`Dispatcher::collect` took the `!observed.settled` branch, before the hold's), and the
//!   notification began the next turn 0.1 s later, whose three lines ("My result … is
//!   unchanged") became the result. Taking the two queue records out of the record reads it
//!   at rest, with nothing pending. Nothing was told of the write-up.
//!
//! **Nothing completes at those turns' ends.** What is uncertain never completes a task (T-8):
//! a worker that asks and stops ends on a line about its question, a turn held for work that
//! runs ends on a line about it, and neither is a result. Nor is the chief told anything new.
//!
//! **The words are kept, and given with the result.** The record holds them, and a result is
//! recorded when the last turn ends ([`Dispatcher::collect`]) or the chief finishes the task
//! ([`Dispatcher::finish_task`]): both give the turns that ended before it, in order, each
//! saying what it was written before ([`Why`]), and then "What it said last:" and the last
//! words. They are found in the record then, so a daemon that restarts in between loses
//! nothing and nothing is written to the ledger for them. A hold's note (`tell_it_goes_on`)
//! gives the requester a held turn's words once; the result gives them again, since it is
//! what stays.
//!
//! **Which turns.** Every turn after the latest message of the task's own that was pasted
//! into the window (the brief, a resume, a follow-up: what was written for an earlier round
//! was decided with it) that ended with a complete answer and was followed by a message to the
//! window: an answer to one of the worker's own questions (`cf ask`, or the question
//! ConsensFlow raised from its last words, T-1119), a note of ConsensFlow's that went in right
//! after it (words told before the window was up), another message of ConsensFlow's, or one
//! that has no header of ConsensFlow's: the notice with which Claude Code, Muse or Codex wakes
//! its agent for work that ended, or words typed into the window. **Not** those that
//! - wrote no complete answer (a turn cut off);
//! - are a line about what the worker waits for, which is not a report ([`WRAPPER_CHARS`]):
//!   - where the next message is the answer to its question, a turn that ended on the question
//!     (the last output of a tool is `cf ask`'s own, "m-N asked @chief. The answer arrives as a
//!     message; end your turn now.", or it ran no tool) in at most that many characters.
//!     Six such lines were measured in recorded transcripts: 51 to 249 characters ("Waiting
//!     for the chief's feedback on the FAQ pages."); a turn that went on after asking, or whose
//!     words are long, is kept, whatever it ended on: a report followed by "Shall I continue?"
//!     is a report;
//!   - where a notice, a note or another message follows, words of at most that many
//!     characters: where it stands ("Started the recording run", "Still going; I'll report
//!     when it ends", "The lexer is half done" before a resume's note), which the results of
//!     held tasks never began with (the owner's decision of 2026-10-15) and the hold's note
//!     told. T-1200's was 8,786;
//! - are the question ConsensFlow raised, whole (it is on the board with them);
//! - are the words the result is (a reply that repeats the write-up word for word), or
//!   repeat another turn's.
//!
//! **What each side sees.** The worker's window is told nothing new. The requester (the chief,
//! or the human in the task's drawer) sees the task as it was (`waiting` until an answer is
//! delivered, `working` while a notice's turn goes on) and then a result that begins with each
//! such turn's words, labelled, and ends with "What it said last:" and the last words. A result
//! with no such turn before it is what it was, byte for byte.
//!
//! **Not covered.** A turn that ends and is followed, in the record, by no message at all (a
//! harness that writes its wake-ups' turns without one) is not told from one of its own, and
//! the tell of a paused task is the pause note's (`paused`).

use cf_harness::records::{Item, Role};
use cf_ledger::{ParticipantView, TaskThread};
use cf_proto::ledger::MessageView;
use cf_proto::questions::ASKED_THEN_END_THE_TURN;
use serde_json::Value;

use super::{with_finish_line, words_of, WITHOUT_ANSWER};
use crate::delivery_text::header_id;
use crate::dispatcher::Dispatcher;
use crate::seams::EngineError;

/// How many characters the words of a turn may have to be taken for a line about what the
/// worker waits for, not a report. The longest of six measured was 249.
const WRAPPER_CHARS: usize = 500;

/// What began the turn after one that ended with a complete answer, which says why those
/// words were not the result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Why {
    /// The chief's answer to a question of the worker's own, `cf ask`'s or the one raised from
    /// its words, went in: the question's id.
    Answer(i64),
    /// A note of ConsensFlow's for the window went in (words told before it was up): its id.
    Note(i64),
    /// Another message of ConsensFlow's went in (the chief's tell): its id, and what it is.
    Message(i64, String),
    /// A message with no header of ConsensFlow's went in: the harness's own notice that work
    /// ended (Claude Code's `<task-notification>`, which wakes its agent), or words typed into
    /// the window.
    Notice,
}

impl Why {
    /// The line that says it, in front of the words.
    fn label(&self) -> String {
        match self {
            Why::Answer(question) => {
                format!("Written before the answer to its question m-{question} went in:")
            }
            Why::Note(id) => format!("Written before the note m-{id} went in:"),
            Why::Message(id, kind) => {
                format!("Written before m-{id}, {}, went in:", kind_words(kind))
            }
            Why::Notice => "Written before a notice of its harness's (or words typed into its window) began its next turn:".to_owned(),
        }
    }
}

/// What a kind of message is called in a label.
fn kind_words(kind: &str) -> &str {
    match kind {
        "question" => "the chief's tell",
        "answer" => "an answer",
        "task" => "words for the task",
        "result" => "a result",
        _ => "a message",
    }
}

/// What a turn wrote that ended before the last one: why its words were not the result, and the
/// words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Earlier {
    pub(crate) why: Why,
    pub(crate) words: String,
}

/// Whether `turn`, the items a window's turn wrote, ended on its question: the last output
/// of a tool is `cf ask`'s ([`ASKED_THEN_END_THE_TURN`]), or it ran no tool.
fn ended_on_asking(turn: &[Item]) -> bool {
    turn.iter()
        .rfind(|item| item.role == Role::Tool)
        .is_none_or(|last| last.text.contains(ASKED_THEN_END_THE_TURN))
}

/// The message of `thread` that `item`, a user's item of the window's record, is the paste of:
/// the one whose receipt names the item, or whose header the item begins with.
fn message_of<'a>(thread: &'a TaskThread, item: &Item) -> Option<&'a MessageView> {
    thread
        .messages
        .iter()
        .find(|message| message.receipt.get("item").and_then(Value::as_str) == Some(&*item.id))
        .or_else(|| {
            let id = header_id(&item.text)?;
            thread.messages.iter().find(|message| message.id == id)
        })
}

/// A turn of the record that ended with a complete answer and was followed by a message to the
/// window: its final run of words, the items it wrote (since the message before it), and the place
/// of the message that followed.
struct Ended<'a> {
    words: Vec<&'a Item>,
    turn: &'a [Item],
    at: usize,
}

/// The turns of `items` from `from` that ended with a complete answer and were followed by a
/// message to the member, in order. The walk is [`answers_to_the_task`]'s: a message to the member
/// or a tool's output ends the run of words before it, a compaction's summary is none of them, and
/// a message that says the role (`replacements`), and the turn on it, are none of the task's. Words
/// after the last complete one (the step that stopped for a tool) are the next turn's.
///
/// [`answers_to_the_task`]: crate::role_change::answers_to_the_task
fn ended_turns<'a>(items: &'a [Item], from: usize, replacements: &[i64]) -> Vec<Ended<'a>> {
    let mut ended = Vec::new();
    let mut of_the_role = false;
    let mut run: Vec<&Item> = Vec::new();
    let mut complete = 0;
    let mut began = from;
    for (at, item) in items.iter().enumerate().skip(from) {
        if item.compaction {
            continue;
        }
        match item.role {
            Role::User => {
                of_the_role =
                    header_id(&item.text).is_some_and(|message| replacements.contains(&message));
                if of_the_role {
                    continue;
                }
                if complete > 0 {
                    ended.push(Ended {
                        words: run[..complete].to_vec(),
                        turn: &items[began..at],
                        at,
                    });
                }
                run.clear();
                complete = 0;
                began = at + 1;
            }
            Role::Tool if !of_the_role => {
                run.clear();
                complete = 0;
            }
            Role::Assistant if !of_the_role => {
                run.push(item);
                if item.complete {
                    complete = run.len();
                }
            }
            _ => {}
        }
    }
    ended
}

impl Dispatcher {
    /// The turns of `participant`'s window since the latest message of `thread`'s task pasted
    /// into it that ended with a complete answer which no result took, in order, as the record
    /// `items` shows them (see above). `replacements` are the ids of the notes that say the
    /// window its role, no message to the worker; `last` are the words the result is, which no
    /// turn of these repeats.
    pub(crate) fn earlier_turns(
        &self,
        thread: &TaskThread,
        participant: &ParticipantView,
        items: &[Item],
        replacements: &[i64],
        last: &str,
    ) -> Result<Vec<Earlier>, EngineError> {
        // The latest message of the task's own that the window was given: what came before it
        // was written for a round that is over.
        let mut floor = None;
        for message in &thread.messages {
            if message.kind == "task"
                && message.recipient_id == participant.id
                && matches!(message.state.as_str(), "delivered" | "read")
            {
                if let Some(at) = self.paste_of(message, items)? {
                    floor = floor.max(Some(at));
                }
            }
        }
        let Some(floor) = floor else {
            return Ok(Vec::new());
        };
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
        let mut earlier: Vec<Earlier> = Vec::new();
        for ended in ended_turns(items, floor + 1, replacements) {
            let why = match message_of(thread, &items[ended.at]) {
                Some(message) => match message.reply_to.filter(|asked| own.contains(asked)) {
                    Some(question) if message.kind == "answer" => Why::Answer(question),
                    _ if message.kind == "note" => Why::Note(message.id),
                    _ => Why::Message(message.id, message.kind.clone()),
                },
                None => match header_id(&items[ended.at].text) {
                    Some(id) => Why::Message(id, String::new()),
                    None => Why::Notice,
                },
            };
            self.keep_earlier(thread, participant, &ended, why, last, &mut earlier);
        }
        Ok(earlier)
    }

    /// The words of the turn `ended`, which the message of `why` followed, added to `earlier`
    /// unless they are not to be kept (see above).
    fn keep_earlier(
        &self,
        thread: &TaskThread,
        participant: &ParticipantView,
        ended: &Ended<'_>,
        why: Why,
        last: &str,
        earlier: &mut Vec<Earlier>,
    ) {
        let words = words_of(&ended.words);
        // The question ConsensFlow raised from these words has them first, a line of a record
        // lost in the turn after them where there was one, and the line that says how to finish.
        let finish_line = with_finish_line(String::new(), &participant.handle, thread.task.number);
        let on_the_board = thread.messages.iter().any(|message| {
            message.kind == "question"
                && message.sender.as_deref() == Some(participant.handle.as_str())
                && message.body.starts_with(&words)
                && message.body.ends_with(&finish_line)
        });
        let short = words.chars().count() <= WRAPPER_CHARS;
        let a_line_about_the_wait = short
            && match &why {
                Why::Answer(_) => ended_on_asking(ended.turn),
                Why::Note(_) | Why::Message(..) | Why::Notice => true,
            };
        if words == WITHOUT_ANSWER
            || words == last
            || a_line_about_the_wait
            || on_the_board
            || earlier.iter().any(|kept| kept.words == words)
        {
            return;
        }
        earlier.push(Earlier { why, words });
    }
}

/// The words of a result with the turns that ended before it in front of them (see above):
/// each labelled with what it was written before, then the words the result is. Those words
/// alone where there are none.
pub(crate) fn with_earlier(words: String, earlier: &[Earlier]) -> String {
    if earlier.is_empty() {
        return words;
    }
    let mut said = String::new();
    for turn in earlier {
        said.push_str(&format!("{}\n\n{}\n\n", turn.why.label(), turn.words));
    }
    said.push_str("What it said last:\n\n");
    said.push_str(&words);
    said
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What each label says, and that a result with none in front of it is its words.
    #[test]
    fn each_why_is_said_in_a_line_of_its_own_and_no_turn_before_leaves_the_words_alone() {
        assert_eq!(
            Why::Answer(5).label(),
            "Written before the answer to its question m-5 went in:"
        );
        assert_eq!(Why::Note(7).label(), "Written before the note m-7 went in:");
        assert_eq!(
            Why::Message(9, "question".to_owned()).label(),
            "Written before m-9, the chief's tell, went in:"
        );
        assert_eq!(
            Why::Message(9, String::new()).label(),
            "Written before m-9, a message, went in:"
        );
        assert_eq!(
            Why::Notice.label(),
            "Written before a notice of its harness's (or words typed into its window) began its next turn:"
        );
        assert_eq!(with_earlier("Done".to_owned(), &[]), "Done");
        let earlier = [
            Earlier {
                why: Why::Note(7),
                words: "First".to_owned(),
            },
            Earlier {
                why: Why::Notice,
                words: "Second".to_owned(),
            },
        ];
        assert_eq!(
            with_earlier("Last".to_owned(), &earlier),
            format!(
                "{}\n\nFirst\n\n{}\n\nSecond\n\nWhat it said last:\n\nLast",
                Why::Note(7).label(),
                Why::Notice.label()
            )
        );
    }

    /// An item of a record, numbered by its text.
    fn item(role: Role, text: &str, complete: bool) -> Item {
        Item {
            id: text.into(),
            role,
            text: text.into(),
            complete,
            at: None,
            commentary: false,
            synthetic: false,
            compaction: false,
        }
    }

    /// A turn ended on its question when its last tool output is `cf ask`'s, or it ran no tool.
    #[test]
    fn a_turn_ended_on_its_question_when_the_last_output_of_a_tool_is_cf_asks_or_there_is_none() {
        let asked = item(
            Role::Tool,
            "m-4 asked @chief. The answer arrives as a message; end your turn now.",
            true,
        );
        let other = item(Role::Tool, "412 GiB in 9 trees", true);
        let words = item(Role::Assistant, "Asked.", true);
        assert!(ended_on_asking(&[words.clone()]), "no tool ran");
        assert!(ended_on_asking(&[
            other.clone(),
            asked.clone(),
            words.clone()
        ]));
        assert!(
            !ended_on_asking(&[asked, other, words]),
            "it went on after asking"
        );
    }

    /// The words of each turn the walk found, and the place of the message that followed it.
    fn found(items: &[Item], replacements: &[i64]) -> Vec<(String, usize)> {
        ended_turns(items, 0, replacements)
            .iter()
            .map(|ended| (words_of(&ended.words), ended.at))
            .collect()
    }

    /// A message to the member ends the turn before it, a tool's output the run of words, a role's
    /// note and the turn on it are none of the task's, a compaction's summary is no message, and a
    /// turn with no complete answer, or only steps after it, has none or just the complete ones.
    #[test]
    fn the_turns_that_ended_are_the_complete_runs_before_a_message_to_the_member() {
        let user = |text: &str| item(Role::User, text, true);
        let said = |text: &str| item(Role::Assistant, text, true);
        let step = |text: &str| item(Role::Assistant, text, false);
        let tool = || item(Role::Tool, "output", true);
        let role = "[ConsensFlow m-9 · T-1 · note from ConsensFlow]\nYou are the worker";
        let mut summary = user("A summary of the conversation");
        summary.compaction = true;
        let items = [
            user("[ConsensFlow m-1 · T-1 · task from @chief]\nBrief"),
            step("Reading"),
            tool(),
            said("First write-up"),
            summary,
            step("Going on"),
            tool(),
            said("Second write-up"),
            step("a step that stopped for a tool"),
            user("<task-notification>b1 ended</task-notification>"),
            said("Cut"),
            user(role),
            said("The turn on the role"),
            user("[ConsensFlow m-5 · T-1 · note from @chief]\nAlso"),
            step("Half a thought"),
            user("typed"),
            said("Last"),
        ];
        assert_eq!(
            found(&items, &[9]),
            [
                ("Second write-up".to_owned(), 9),
                ("Cut".to_owned(), 13),
            ],
            "the tool's output ended the first write-up's run; the role's note and its turn are none; \
             a turn with only a step has no end; the last turn is the result's, not found"
        );
        // Without the role's id the note is a message, and the turn on it a turn.
        assert_eq!(
            found(&items, &[])
                .iter()
                .map(|(_, at)| *at)
                .collect::<Vec<_>>(),
            [9, 11, 13]
        );
    }
}

//! A role that changed under a conversation whose harness keeps its first
//! instructions.
//!
//! A launch gives its window the role text (`Launch::instructions`). Most
//! harnesses take it again at every launch, so a role that changed since a
//! conversation began reaches it when the conversation is resumed. A harness
//! that keeps the instructions a conversation began with
//! (`Adapter::keeps_first_instructions`, Codex) reads the new text at a resume
//! and does not use it. Probed on Codex 0.162.1 through `codex app-server`,
//! 2026-10-13: a thread started with the developer instructions "Your code word
//! is ROLEONE." answered ROLEONE to a turn asking for the code word; a second
//! app-server resumed that thread (`thread/resume`) with the developer
//! instructions "Your code word is now ROLETWO; this replaces any earlier code
//! word.", and the same turn was answered ROLEONE again. Neither the
//! `developer_instructions` setting of a resumed window's command line nor that
//! field of the resume request replaces what the thread began with, so a chief's
//! specs turned on, a member added to the staff or a release that rewrote a text
//! would never reach a conversation that is resumed. The engine says it itself.
//!
//! The ledger records the role text each such conversation was last told. A
//! launch that resumes one whose text now differs has a note from ConsensFlow
//! written: the new text in full, said plainly to replace the earlier one. The
//! note is not on the window's command line. The window opens with no first
//! message, and once it is up the note is the first delivery of its queue, by the
//! way every message goes in (`next_in_line`): then the message the launch would
//! have given first (a handoff, a brief, the note that its turn was cut), then
//! the rest of the queue in its order. The command line carries the role once, as
//! the developer instructions a window always had.
//!
//! The text is recorded when the note is delivered, not when it is written, and
//! in the very write that confirms the delivery (`confirm_delivered`, at the two
//! places a delivery is confirmed), so that what the ledger holds is what the
//! conversation has really been told, and neither stands without the other. A note
//! that never comes in (its window did not open, went before the harness's record
//! showed it, or its delivery failed) leaves the record as it was, and the launch
//! after says the role again. One that was given back to the queue, its window
//! closed before the daemon saw it arrive, may have come in all the same: a launch
//! looks for it in the conversation's own record before it withdraws anything
//! (`settle_replacements`), and one that is there is confirmed and recorded, so
//! that a role the thread was told is never taken for one it was not: a human who
//! went back to the role the conversation began with is not left with a thread
//! that holds the other. One whose delivery was given up (its attempts spent)
//! after its window took it is not looked for, and the record then holds a role
//! the ledger does not: that takes as many windows closed before a note was seen
//! as a message is given attempts.
//!
//! A note that still waits when a launch begins is withdrawn, whatever the launch
//! is: it was written for a window that never took it, and a conversation that
//! begins now, or one on another harness, is not the one it says a role to. The
//! role is said anew only where the resumed conversation was told something else,
//! so that a conversation is not told a role twice over, nor one that is no
//! longer its role.
//!
//! A member whose launch fails has its first message fail, and a brief that fails
//! fails its task, which whoever gave it hears of. A launch that is to say a role
//! first has no message on its command line, but it was for the message the note
//! goes ahead of: where the launch fails, whether its window did not open or
//! could not start, that message fails with it as it always did
//! (`begin_displaced`), or the launch would be tried again at every pass, a new
//! note written each time, and no end to it. The note written for the launch is
//! withdrawn at the failure: no queue of a member's gives a note about no task to
//! a window, so it would wait for ever. A chief's stays: it is the head of the
//! chief's queue, which keeps the chief tried again at the backoff, and the
//! launch that comes withdraws it before it says the role.
//!
//! A role too long for one message (more than 16,000 units: a staff of more than
//! 43 rows with specs on, 72 without, two roles to a row, on the real text) is
//! delivered cut to its opening, with the way to read the rest (`cf inbox read`).
//! That is a role told in part: it is not recorded as told, and the human has a
//! line on the board that says so. The role is said again at every Resume, and
//! the line written again, until the staff is smaller. The role is not given in
//! several messages.
//!
//! It is the text as the window would be given it that is compared. For a chief
//! given specs that text names the Specmint skill by its path, and the path is
//! the same at every launch (the harness crate keeps one copy of ConsensFlow's
//! for each harness and variant, outside every launch's folder), so a Resume says
//! nothing of the role unless something changed: the project's specs, their
//! variant or their folder, the staff, a text a release rewrote, a release that
//! vendors another commit of the skill, or the harness's own copy of the skill
//! turning up or going. A conversation whose text is not known (one from before
//! the record, or one the human switched their window to) counts as changed, and
//! is told once.
//!
//! A harness that takes its role at every launch is never asked, and its launch
//! is what it always was.
//!
//! What it asks of a Windows launch is nothing it did not ask before. The command
//! line carries the role once, as the developer instructions of the window
//! (`-c developer_instructions=<JSON>`, which a resumed thread reads and does not
//! use), and the note goes in by paste once the window is up, which has no such
//! limit. Windows' `CreateProcess` takes 32,767 UTF-16 units of command line, the
//! last for its end. Measured on the real text of a chief with specs on (tdd-html,
//! a staff whose rows hold two roles each, Windows paths for the skill and for
//! `cf`, 190 units before the role for what the harness says of it), the word as
//! Windows quotes it (each of the role's 48 quotes is two more than JSON has it)
//! comes to 14,122 units for 1 staff row, 15,313 for 25 and 16,313 for 45, about
//! 50 a row. With some 400 units for the rest of the line (the two programs'
//! paths, the flags, the thread's id, the model) a staff of 360 rows is the most
//! it holds. What Codex's own instructions hold (the human's own, which begin the
//! developer instructions) is in none of that: 2,500 units of them are some 2,700
//! on the line, and the most is then 307 rows. Without specs the role is 1,422
//! units shorter at 25 rows, and the most is 388. (When the note was the
//! launch's first message the role was on the line twice, and a staff of 25 with
//! 2,500 units of Codex's own instructions was over the limit.) A test holds
//! these numbers to the real text.

use cf_harness::records::{Item, Role};
use cf_ledger::{ConversationView, MessageView, NewNote, ParticipantView, ProjectView};
use serde_json::{json, Value};

use crate::deliveries::Delivering;
use crate::delivery_text::{goes_whole, marker_of};
use crate::dispatcher::Dispatcher;
use crate::record::Record;
use crate::seams::EngineError;
use crate::specs::RoleText;

/// What a replacement says before the role text, and how one is told from any
/// other note of ConsensFlow's.
const OPENING: &str = "Your ConsensFlow role instructions have changed since this conversation began. The instructions below replace the ones you were given then: follow these from now on, and where they differ from what you were told before, these are right. This is context, not a task: say in one short line that you have them, and wait for the next message.";

/// Why a replacement that waited is withdrawn.
const WITHDRAWN: &str = "the window it was written for never took it";

/// What a launch's first message and role text come to, once the role was
/// compared with the one the conversation was last told.
pub(crate) struct Compared {
    /// The launch's first message, on its command line: the message the launch
    /// was given, but for a replacement that waited and was withdrawn; and none
    /// where the role is to be said first, since the window then opens with none.
    pub(crate) first: Option<MessageView>,
    /// What goes into the window before anything else of its queue once it is up,
    /// in this order: the replacement of the role, and the message it displaced,
    /// if there was one. Empty where the role is not to be said.
    pub(crate) due: Vec<i64>,
    /// The role text the comparison was made with, for the launch to use and not
    /// ask for again; none where there was nothing to compare.
    pub(crate) role: Option<RoleText>,
}

impl Dispatcher {
    /// What a launch of `participant` finds in the conversation's own record
    /// before it begins: a replacement given back to the queue (its window closed
    /// before the daemon saw it arrive) that the record shows the window took. It
    /// was told, whatever the ledger says: it is confirmed and the role recorded,
    /// and not withdrawn as a note that never came in. The ones it confirmed.
    pub(crate) async fn settle_replacements(
        &self,
        participant: &ParticipantView,
    ) -> Result<Vec<i64>, EngineError> {
        let waiting: Vec<MessageView> = self
            .seams
            .ledger
            .borrow()
            .pending(participant.id)?
            .into_iter()
            .filter(|message| message.state == "queued" && is_replacement(message))
            .collect();
        let mut told = Vec::new();
        for note in waiting {
            let shown = self
                .recorded_item_with(participant.id, &marker_of(note.id))
                .await?;
            if let Some(item) = shown {
                let begun = self.seams.ledger.borrow_mut().begin_delivery(note.id)?;
                self.confirm_delivered(&begun.message, &json!({ "item": item }))?;
                told.push(note.id);
            }
        }
        Ok(told)
    }

    /// The launch of `participant`'s window in `project`, `first` its first
    /// message if it has one: a conversation that is resumed, on a harness that
    /// keeps its first instructions, whose recorded text is not the one its
    /// window would be given now has the replacement of its role written, to go
    /// in first once the window is up; the window opens with no first message. A
    /// replacement that still waits is withdrawn before, whatever this launch is.
    pub(crate) fn compare_role(
        &self,
        project: &ProjectView,
        participant: &ParticipantView,
        resumed: Option<&ConversationView>,
        first: Option<MessageView>,
    ) -> Result<Compared, EngineError> {
        let withdrawn = self.withdraw_replacements(participant)?;
        let first = first.filter(|message| !withdrawn.contains(&message.id));
        let apart = |first| {
            Ok(Compared {
                first,
                due: Vec::new(),
                role: None,
            })
        };
        let Some(conversation) = resumed else {
            return apart(first);
        };
        let harness = participant.harness.as_deref().unwrap_or_default();
        let kept = self
            .seams
            .adapters
            .adapter(harness)
            .filter(|adapter| adapter.keeps_first_instructions());
        // A launch that cannot go (no adapter, an agent the human deleted) says
        // why in its own step, which asks for the text again where this could not.
        let Some(adapter) = kept.filter(|_| !self.agent_gone(participant)) else {
            return apart(first);
        };
        let Ok(role) = self.role_text(project, participant, &*adapter) else {
            return apart(first);
        };
        let told = self
            .seams
            .ledger
            .borrow()
            .conversation_role_text(conversation.id)?;
        if told.as_deref() == Some(role.instructions.as_str()) {
            return Ok(Compared {
                first,
                due: Vec::new(),
                role: Some(role),
            });
        }
        let replacement = self.replace_role(project, participant, &role)?;
        let due = std::iter::once(replacement.id)
            .chain(first.as_ref().map(|message| message.id))
            .collect();
        Ok(Compared {
            first: None,
            due,
            role: Some(role),
        })
    }

    /// The replacements of `participant`'s role that still wait, withdrawn: their
    /// window went before they landed. They say what may not be the role now, or
    /// what a replacement of this launch will say again.
    pub(crate) fn withdraw_replacements(
        &self,
        participant: &ParticipantView,
    ) -> Result<Vec<i64>, EngineError> {
        let mut ledger = self.seams.ledger.borrow_mut();
        let mut withdrawn = Vec::new();
        for waiting in ledger.pending(participant.id)? {
            if waiting.state == "queued" && is_replacement(&waiting) {
                ledger.cancel_message(waiting.id, WITHDRAWN)?;
                withdrawn.push(waiting.id);
            }
        }
        Ok(withdrawn)
    }

    /// The note that says `role` to `participant`, written. What its
    /// conversation was told is recorded when it is delivered.
    fn replace_role(
        &self,
        project: &ProjectView,
        participant: &ParticipantView,
        role: &RoleText,
    ) -> Result<MessageView, EngineError> {
        Ok(self.seams.ledger.borrow_mut().note(
            project.id,
            &NewNote {
                from: None,
                to: participant.handle.clone(),
                task: None,
                body: body_of(&role.instructions),
            },
        )?)
    }

    /// The next message to go into `record`'s window: what the launch put ahead
    /// of its queue, in its order, while it waits and may go, else the head of the
    /// queue. The replacement of a role goes whatever its recipient holds (a
    /// note about no task would not be the head of a member's queue); the message
    /// it displaced goes by the rules the head goes by. Nothing goes while a
    /// delivery is on its way. What the launch put ahead of its queue and has been
    /// settled since (delivered, withdrawn or given up) is passed over.
    pub(crate) fn next_in_line(&self, record: &Record) -> Result<Option<MessageView>, EngineError> {
        let ledger = self.seams.ledger.borrow();
        let mut waiting = Vec::new();
        for id in record.delivery.borrow().due.iter().copied() {
            let found = ledger.message(id)?;
            waiting.extend(
                found.filter(|message| matches!(message.state.as_str(), "queued" | "delivering")),
            );
        }
        let busy = !waiting.is_empty()
            && ledger
                .pending(record.id)?
                .iter()
                .any(|other| other.state == "delivering");
        if !busy {
            for message in waiting {
                let goes = message.state == "queued"
                    && (is_replacement(&message)
                        || ledger.next_delivery_of(record.id, message.id)?.is_some());
                if goes {
                    return Ok(Some(message));
                }
            }
        }
        Ok(ledger.next_delivery(record.id)?)
    }

    /// The message a member's launch was for, which the replacement of a role
    /// went ahead of: what the launch's failure settles, as it would have as the
    /// launch's first message. None for a chief's, which waits in its queue for
    /// its next window, and none where no message was displaced.
    pub(crate) fn displaced(
        &self,
        participant: &ParticipantView,
        due: &[i64],
    ) -> Option<Delivering> {
        let message = *due.get(1)?;
        (participant.role != "chief").then(|| Delivering {
            message,
            marker: marker_of(message),
            since: self.now(),
            launch: true,
            chief: false,
            queued: false,
            entered_again: false,
        })
    }

    /// The message a launch was for, which the replacement of a role went ahead
    /// of, is begun where it waits still, so that the launch's failure settles it
    /// as it would have as the launch's first message.
    pub(crate) fn begin_displaced(&self, delivering: &Delivering) -> Result<(), EngineError> {
        let queued = self
            .seams
            .ledger
            .borrow()
            .message(delivering.message)?
            .is_some_and(|message| message.state == "queued");
        if queued {
            self.seams
                .ledger
                .borrow_mut()
                .begin_delivery(delivering.message)?;
        }
        Ok(())
    }

    /// The harness's own record proves `message` arrived: it is confirmed. A
    /// replacement of a role that went in whole also records the conversation as
    /// told it, in the same write. One that was cut (more than a message goes in
    /// whole at) tells the conversation its role in part: it is not recorded, and
    /// the human is told so on the board. One whose recipient has no current
    /// conversation any more (it ended while the note was on its way) is
    /// confirmed and records nothing: there is no conversation to have told.
    pub(crate) fn confirm_delivered(
        &self,
        message: &MessageView,
        receipt: &Value,
    ) -> Result<(), EngineError> {
        let Some(instructions) = instructions_of(message) else {
            self.seams
                .ledger
                .borrow_mut()
                .confirm_delivery(message.id, Some(receipt))?;
            return Ok(());
        };
        let conversation = self
            .seams
            .ledger
            .borrow()
            .current_conversation(message.recipient_id)?;
        let mut ledger = self.seams.ledger.borrow_mut();
        match conversation.filter(|_| goes_whole(message, &[])) {
            Some(conversation) => {
                ledger.confirm_delivery_telling(
                    message.id,
                    Some(receipt),
                    conversation.id,
                    instructions,
                )?;
            }
            None => {
                ledger.confirm_delivery(message.id, Some(receipt))?;
                if !goes_whole(message, &[]) {
                    ledger.note(
                        message.project_id,
                        &NewNote {
                            from: None,
                            to: "human".to_owned(),
                            task: message.task_number,
                            body: format!(
                                "@{} was told its new role in part: the staff is too large for one message (m-{} was cut at 16,000 characters). It is told again at every Resume until the staff is smaller.",
                                message.recipient, message.id
                            ),
                        },
                    )?;
                }
            }
        }
        Ok(())
    }
}

/// What a note that replaces the role with `instructions` says: the words that
/// say so, and the text in full.
fn body_of(instructions: &str) -> String {
    format!("{OPENING}\n\n{instructions}")
}

/// Whether `message` is a replacement of a role: a note from ConsensFlow that
/// opens as one does.
fn is_replacement(message: &MessageView) -> bool {
    message.sender.is_none() && message.kind == "note" && message.body.starts_with(OPENING)
}

/// The role text a replacement says: what follows its opening.
fn instructions_of(message: &MessageView) -> Option<&str> {
    message
        .body
        .strip_prefix(OPENING)
        .and_then(|rest| rest.strip_prefix("\n\n"))
        .filter(|_| is_replacement(message))
}

/// Whether `text`, a message of a conversation's record, is a replacement of a
/// role as its window was given it: it holds the opening of one.
pub(crate) fn says_a_replacement(text: &str) -> bool {
    text.contains(OPENING)
}

/// The assistant's items among `items`, a member's record after its brief, that
/// answer the task: not those of a turn that answers a replacement of the role.
/// A user's message that says a role begins a turn whose words are the window's
/// word of its role and none of its task's, and the next user's message ends it.
pub(crate) fn answers_to_the_task(items: &[Item]) -> Vec<&Item> {
    let mut of_the_role = false;
    let mut answers = Vec::new();
    for item in items {
        match item.role {
            Role::User => of_the_role = says_a_replacement(&item.text),
            Role::Assistant if !of_the_role => answers.push(item),
            _ => {}
        }
    }
    answers
}

#[cfg(test)]
mod tests;

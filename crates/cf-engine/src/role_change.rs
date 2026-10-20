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
//! the developer instructions a window always had. What the launch owes the note
//! until it is told, and what comes of a launch that cannot tell it, is in
//! [`prelude`].
//!
//! The ledger knows a replacement by the role text it holds
//! (`Ledger::note_role`), and by nothing in its words. A note of ConsensFlow's
//! about no task is not enough: the engine writes others to a chief (its handoff,
//! the note that its turn was cut), and a message that quotes the words of a
//! replacement is none. So every place that tells one apart (the replacements that
//! wait, the one that is confirmed, the turn of a member's record that answers
//! one) asks the ledger for its id, and a turn of a record is a replacement's by
//! the id in its header (`header_id`).
//!
//! The text is recorded when the note is delivered, not when it is written, and
//! in the very write that confirms the delivery (`confirm_delivered`, at the
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
//! that holds the other.
//!
//! A note that still waits when a launch begins is withdrawn, whatever the launch
//! is: it was written for a window that never took it, and a conversation that
//! begins now, or one on another harness, is not the one it says a role to. The
//! role is said anew only where the resumed conversation was told something else,
//! so that a conversation is not told a role twice over, nor one that is no
//! longer its role.
//!
//! Whether the window goes on with the conversation at all is decided before the
//! role is compared (`conversation_to_go_on_with`): a conversation its harness
//! holds nothing of (a Codex thread that never had a turn has no record for a
//! resume to find) is not resumed, its window opens afresh with the role as it is
//! on its command line, as the window of any conversation that begins, and there
//! is nothing to tell it: no note is written for it, and the text it begins with
//! is what is recorded of it. A chief's launch asks its harness first, its handoff
//! depending on the answer. A member's first message is begun, and the stop its
//! window owes read, before its harness is asked, so that a pause that comes
//! during the ask is seen; its role is compared once the ask is answered, in the
//! plan of its launch, and where it is then to be said first the message that was
//! begun goes back to its queue with its attempt, to go in after the note
//! ([`prelude`]).
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
//!
//! What is written down and not done (the limits of this):
//!
//! - A launch that fails and is tried again by itself (a chief, at the backoff)
//!   is tried with the head of its queue as the message it would have begun
//!   with, and a handoff keeps its place (`chief_first`). The note that its turn
//!   was cut, which a Resume wrote and put ahead of older work, does not: it
//!   goes behind what it was ahead of. The first launch would have said the
//!   role, the note and then the older work; the next says the role, the older
//!   work and then the note. The priority of the continuation is lost with a
//!   failed launch.
//! - What a launch finds in the conversation's own record is taken as it is
//!   read. Where the record cannot be read (a file that is not there for a
//!   moment, an adapter that fails) a replacement that waited is taken as not
//!   received: it is withdrawn, and the role is said again at the next launch. A
//!   thread that took it meanwhile then holds the role twice, and a human who
//!   had gone back to the role the conversation began with is not told that the
//!   thread holds another. A replacement whose delivery was given up is not
//!   looked for at all: it failed, and nothing reconciles it with what the
//!   thread may hold. Unreadable evidence is not told from none.
//! - A role told in part (cut at 16,000 units) is not recorded, so the role the
//!   conversation was told before stays recorded: where the human goes back to
//!   that one, the launch says nothing, though the thread holds the cut words
//!   after it. A staff that shrinks, or a role that changes again, tells the
//!   thread a whole one.
//! - A resumed Codex window that is given no first message has the turn its
//!   record ended in read as the old process's, not as its own, until it begins
//!   one (`LeftOpen`), whether or not a replacement is due. That is a different
//!   reading from the one `main` made of every resume with no message, whatever
//!   the role.
//! - Attempts are spent within one window as much as across windows. A paste the
//!   harness refuses or never shows is given again, and given up after as many
//!   attempts as any message is given, so a paste that was taken and did not show
//!   can arrive twice. A replacement the harness's own queue took (a Codex
//!   broker's admission) is waited for in the record as any message that was
//!   queued is, with no time it is given up at.
//! - What goes into a chief's window after the replacement is given up as any
//!   message is where the harness refuses it as many times as a message is
//!   given attempts: the handoff or the note that its turn was cut is then lost
//!   as it would be at any other moment. It is never given up for being slow, and
//!   goes back to its queue with its window.
//! - A chief whose window went before it was told its role is tried again at the
//!   backoff where something waits for it, as a chief that could not start is;
//!   with nothing queued it waits for what comes. A member's message that the
//!   role went ahead of, and that rides in the paste of another by the time its
//!   launch is given up (its task was paused and resumed while the window
//!   waited), cannot fail with the launch: it goes with its carrier, which is
//!   the message the launch that follows is for.
//! - A pause that comes while the window of a member's launch that says the role
//!   first is prepared does not stop it from opening, as one does a launch whose
//!   message is begun and held (`launch_over`): nothing is begun for it to hold, so
//!   the window opens with the note, and the message waits on its paused task.

mod prelude;
#[cfg(test)]
mod tests;

use cf_harness::records::{Item, Role};
use cf_ledger::{ConversationView, MessageView, NewNote, ParticipantView, ProjectView};
use serde_json::{json, Value};

use crate::delivery_text::{goes_whole, header_id};
use crate::dispatcher::Dispatcher;
use crate::seams::EngineError;
use crate::specs::RoleText;

pub(crate) use prelude::Prelude;

/// What a replacement says before the role text.
pub(crate) const OPENING: &str = "Your ConsensFlow role instructions have changed since this conversation began. The instructions below replace the ones you were given then: follow these from now on, and where they differ from what you were told before, these are right. This is context, not a task: say in one short line that you have them, and wait for the next message.";

/// Why a replacement that waited is withdrawn.
const WITHDRAWN: &str = "the window it was written for never took it";

/// What a launch's first message and role text come to, once the role was
/// compared with the one the conversation was last told.
pub(crate) struct Compared {
    /// The launch's first message, on its command line: the message the launch
    /// was given, but for a replacement that waited and was withdrawn; and none
    /// where the role is to be said first, since the window then opens with none.
    pub(crate) first: Option<MessageView>,
    /// What goes into the window before anything else of its queue once it is up:
    /// the replacement of the role, and the message it displaced, if there was
    /// one. None where the role is not to be said.
    pub(crate) prelude: Option<Prelude>,
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
        let waiting = self.waiting_replacements(participant)?;
        let mut told = Vec::new();
        for note in waiting {
            let shown = self
                .recorded_paste_of(participant.id, note.id, None)
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
                prelude: None,
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
                prelude: None,
                role: Some(role),
            });
        }
        let replacement = self.replace_role(project, participant, &role)?;
        let prelude = Prelude {
            replacement: replacement.id,
            displaced: first.as_ref().map(|message| message.id),
            chief: participant.role == "chief",
            since: self.now(),
            begun: false,
        };
        Ok(Compared {
            first: None,
            prelude: Some(prelude),
            role: Some(role),
        })
    }

    /// The replacements of `participant`'s role that still wait, as the ledger
    /// knows them.
    fn waiting_replacements(
        &self,
        participant: &ParticipantView,
    ) -> Result<Vec<MessageView>, EngineError> {
        let ledger = self.seams.ledger.borrow();
        let notes = ledger.role_notes_to(participant.id)?;
        Ok(ledger
            .pending(participant.id)?
            .into_iter()
            .filter(|message| message.state == "queued" && notes.contains(&message.id))
            .collect())
    }

    /// The replacements of `participant`'s role that still wait, withdrawn: their
    /// window went before they landed. They say what may not be the role now, or
    /// what a replacement of this launch will say again.
    pub(crate) fn withdraw_replacements(
        &self,
        participant: &ParticipantView,
    ) -> Result<Vec<i64>, EngineError> {
        let waiting = self.waiting_replacements(participant)?;
        let mut ledger = self.seams.ledger.borrow_mut();
        let mut withdrawn = Vec::new();
        for note in waiting {
            ledger.cancel_message(note.id, WITHDRAWN)?;
            withdrawn.push(note.id);
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
        Ok(self.seams.ledger.borrow_mut().note_role(
            project.id,
            &participant.handle,
            &body_of(&role.instructions),
            &role.instructions,
        )?)
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
        let says_a_role = self
            .seams
            .ledger
            .borrow()
            .role_notes_to(message.recipient_id)?
            .contains(&message.id);
        let mut ledger = self.seams.ledger.borrow_mut();
        if !says_a_role {
            ledger.confirm_delivery(message.id, Some(receipt))?;
        } else if goes_whole(message, &[]) {
            ledger.confirm_delivery_telling(message.id, Some(receipt))?;
        } else {
            ledger.confirm_delivery(message.id, Some(receipt))?;
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
        Ok(())
    }
}

/// What a note that replaces the role with `instructions` says: the words that
/// say so, and the text in full.
fn body_of(instructions: &str) -> String {
    format!("{OPENING}\n\n{instructions}")
}

/// The assistant's items among `items`, a member's record after its brief, that
/// answer the task: not those of a turn that answers a replacement of the role.
/// A user's message that says a role begins a turn whose words are the window's
/// word of its role and none of its task's, and the next user's message ends it.
/// `replacements` are the ids of the replacements the ledger knows were written
/// to the member; a message is one by the id of its header, and not by any words.
pub(crate) fn answers_to_the_task<'a>(items: &'a [Item], replacements: &[i64]) -> Vec<&'a Item> {
    let mut of_the_role = false;
    let mut answers = Vec::new();
    for item in items {
        match item.role {
            Role::User => {
                of_the_role =
                    header_id(&item.text).is_some_and(|message| replacements.contains(&message));
            }
            Role::Assistant if !of_the_role => answers.push(item),
            _ => {}
        }
    }
    answers
}

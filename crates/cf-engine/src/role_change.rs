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
//! launch that resumes one whose text now differs puts a note from ConsensFlow
//! first: the new text in full, said plainly to replace the earlier one, ahead
//! of whatever else waits for the window, which is delivered after it by the
//! rules of every message.
//!
//! The text is recorded when the note is delivered, not when it is written
//! (`role_delivered`, at the two places a delivery is confirmed), so that what
//! the ledger holds is what the conversation has really been told. A note that
//! never comes in (its window did not open, went before the harness's record
//! showed it, or its delivery failed) leaves the record as it was, and the launch
//! after says the role again. A note that still waits when a launch begins is
//! withdrawn first, whatever the launch is: it was written for a window that never
//! took it, and a conversation that begins now, or one on another harness, is not
//! the one it says a role to. The role is said anew only where the resumed
//! conversation was told something else, so that a conversation is not told a
//! role twice over, nor one that is no longer its role. What is recorded is the
//! text the note said: a note longer than a message goes in whole at (16,000
//! units) is cut, with the way to read the rest (`cf inbox read`), and the
//! conversation is taken to have been told all of it.
//!
//! A member whose launch fails has its first message fail, and a brief that fails
//! fails its task, which whoever gave it hears of. So a replacement of a member's
//! role that fails takes the message it went ahead of with it (`fail_behind`):
//! left waiting, that message would have the launch tried again at every pass,
//! with a new replacement written ahead of it each time, and no end to it. A
//! chief's launch is tried again anyway, ever more slowly, and what waits for the
//! chief waits.
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
//! What it asks of a Windows launch. The note is the launch's first message,
//! which Codex takes as the last word of its command line, and that command line
//! has the role already, as the developer instructions of the window
//! (`-c developer_instructions=<JSON>`, read by the resumed thread and not used).
//! So at a Resume that says a changed role the command line holds the role
//! twice, and Windows' `CreateProcess` takes 32,767 UTF-16 units of it, the last
//! for its end. Measured on the real text of a chief with specs on (tdd-html, a
//! staff whose rows hold two roles each, Windows paths for the skill and for
//! `cf`), each word as Windows quotes it (a `"` is backslashed: the role has 48,
//! which is 100 units over the two words):
//!
//! | staff rows | developer instructions | first message | with about 400 for the rest |
//! |-----------:|-----------------------:|--------------:|----------------------------:|
//! |          1 |                 14,115 |        13,975 |                      28,490 |
//! |         25 |                 15,306 |        15,142 |                      30,848 |
//! |         44 |                 16,256 |        16,073 |                      32,729 |
//! |         45 |                 16,306 |  15,159 (cut) |                      31,865 |
//!
//! The developer instructions grow 50 units a row; the message does too, until
//! it is cut at what a message goes in whole at (from 45 rows), and the sum then
//! goes on with the developer instructions alone: it passes the limit at 64 rows.
//! The least margin is at 44 rows, 37 units, the last before the cut. The
//! "rest" is the two programs' paths, the flags, the thread's id and the model;
//! what Codex's own instructions hold (the human's own, which begin the developer
//! instructions) is in none of these, and 100 units of it are two rows less.
//! Without specs the role is some 1,400 units shorter: the least margin is about
//! 100 units at 72 rows, and the limit is passed at 92.
//!
//! No code handles it. Past it the launch does not open (`the window did not
//! open: …`, the system's words, as for any launch that cannot): the chief waits,
//! ConsensFlow tries again, and so does the Resume after, for the record still
//! says the old role. Nothing is lost, and what ends it is a smaller staff, or
//! another chief. A later message, a paste, has no such limit.

use cf_ledger::{ConversationView, MessageView, NewNote, ParticipantView, ProjectView};

use crate::deliveries::Delivering;
use crate::delivery_text::marker_of;
use crate::dispatcher::Dispatcher;
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
    /// The launch's first message: the replacement of the role where there is one,
    /// else the message the launch was given, but for a replacement that waited
    /// and was withdrawn. What the replacement put second stays queued for the
    /// window.
    pub(crate) first: Option<MessageView>,
    /// The role text the comparison was made with, for the launch to use and not
    /// ask for again; none where there was nothing to compare.
    pub(crate) role: Option<RoleText>,
}

impl Dispatcher {
    /// The launch of `participant`'s window in `project`, `first` its first
    /// message if it has one: a conversation that is resumed, on a harness that
    /// keeps its first instructions, whose recorded text is not the one its
    /// window would be given now has the replacement of its role put first. A
    /// replacement that still waits is withdrawn before, whatever this launch is:
    /// it was written for a window that never took it, and a conversation that
    /// begins now, or one on another harness, is not the one it says a role to.
    pub(crate) fn compare_role(
        &self,
        project: &ProjectView,
        participant: &ParticipantView,
        resumed: Option<&ConversationView>,
        first: Option<MessageView>,
    ) -> Result<Compared, EngineError> {
        let withdrawn = self.withdraw_replacements(participant)?;
        let first = first.filter(|message| !withdrawn.contains(&message.id));
        let apart = |first| Ok(Compared { first, role: None });
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
                role: Some(role),
            });
        }
        let replacement = self.replace_role(project, participant, &role)?;
        Ok(Compared {
            first: Some(replacement),
            role: Some(role),
        })
    }

    /// The replacements of `participant`'s role that still wait, withdrawn: their
    /// window went before they landed. They say what may not be the role now, or
    /// what a replacement of this launch will say again.
    fn withdraw_replacements(
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

    /// A replacement of a member's role that will not be delivered takes with it
    /// the message it went ahead of, as the launch it was the first message of
    /// would have: a member whose launch fails fails its first message, and a
    /// brief that fails fails its task, which whoever gave it hears of. Left
    /// waiting, the brief would have the launch tried again at every pass, a new
    /// replacement written ahead of it each time (the record says the role was not
    /// told), and no end to it. A chief's launch is tried again anyway, ever more
    /// slowly, and what waits for the chief waits.
    pub(crate) fn fail_behind(
        &self,
        replacement: &MessageView,
        because: &str,
    ) -> Result<(), EngineError> {
        if !is_replacement(replacement) || replacement.recipient_role == "chief" {
            return Ok(());
        }
        let next = self
            .seams
            .ledger
            .borrow()
            .next_delivery(replacement.recipient_id)?;
        let Some(next) = next else {
            return Ok(());
        };
        self.seams.ledger.borrow_mut().begin_delivery(next.id)?;
        self.settle_failure(
            Delivering {
                message: next.id,
                marker: marker_of(next.id),
                since: self.now(),
                launch: true,
                chief: false,
                queued: false,
                entered_again: false,
            },
            because,
            false,
        )
    }

    /// A message came into its window, and the harness's record shows it. A
    /// replacement of a role has told the conversation that text: it is what the
    /// conversation was last told from now on. Any other message says nothing of it.
    pub(crate) fn role_delivered(&self, message: &MessageView) -> Result<(), EngineError> {
        let Some(instructions) = instructions_of(message) else {
            return Ok(());
        };
        let mut ledger = self.seams.ledger.borrow_mut();
        if let Some(conversation) = ledger.current_conversation(message.recipient_id)? {
            ledger.set_conversation_role_text(conversation.id, instructions)?;
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

#[cfg(test)]
mod tests;

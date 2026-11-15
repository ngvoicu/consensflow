//! Compare a resumed conversation's role with the one its window would receive now.

use cf_ledger::{ConversationView, MessageView, NewNote, ParticipantView, ProjectView};

use super::Prelude;
use crate::delivery_text::goes_whole;
use crate::dispatcher::Dispatcher;
use crate::seams::EngineError;
use crate::specs::RoleText;

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
        // A launch that cannot go (no adapter) says why in its own step, which asks for
        // the text again where this could not.
        let Some(adapter) = kept else {
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
        let unread = self.unread_role(participant, &role.instructions)?;
        let replacement = self.replace_role(project, participant, &role)?;
        if let Some(unread) = unread {
            self.not_read(&unread, replacement.id)?;
        }
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

    /// The last note that told `participant` the role `instructions` and that
    /// its window has and has not read: a note that reached the window (the last
    /// of those that did), cut to fit in one message, delivered and not read since.
    /// None where the last one to reach it went in whole, was read, or said
    /// another role.
    fn unread_role(
        &self,
        participant: &ParticipantView,
        instructions: &str,
    ) -> Result<Option<MessageView>, EngineError> {
        let cf = self.seams.roles.cf(participant);
        let ledger = self.seams.ledger.borrow();
        for id in ledger.role_notes_to(participant.id)?.into_iter().rev() {
            let Some(note) = ledger.message(id)? else {
                continue;
            };
            if !matches!(note.state.as_str(), "delivered" | "read") {
                continue;
            }
            let unread = note.state == "delivered"
                && !goes_whole(&note, &[], &cf)
                && ledger.role_of_note(id)?.as_deref() == Some(instructions);
            return Ok(unread.then_some(note));
        }
        Ok(None)
    }

    /// The human is told on the board that `unread`, a role cut to fit in one
    /// message, was not read by its window before this Resume, which tells it the
    /// role again in the note `again`. The cut is at 16,000 UTF-16 units, which a
    /// person counts as characters but for the few that take two (an emoji), so the
    /// line says "about 16,000 characters".
    pub(crate) fn not_read(&self, unread: &MessageView, again: i64) -> Result<(), EngineError> {
        self.seams.ledger.borrow_mut().note(
            unread.project_id,
            &NewNote {
                from: None,
                to: "human".to_owned(),
                task: unread.task_number,
                body: format!(
                    "@{} did not read its new role before this Resume: it is too long for one message, and m-{} went in cut at about 16,000 characters with `cf inbox read m-{}` for the rest. It is told again now, in m-{again}.",
                    unread.recipient, unread.id, unread.id
                ),
            },
        )?;
        Ok(())
    }
}

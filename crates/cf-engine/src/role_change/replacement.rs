//! Settle, withdraw and write replacements of a conversation's role.

use cf_ledger::{MessageView, ParticipantView, ProjectView};
use serde_json::json;

use super::body_of;
use crate::dispatcher::Dispatcher;
use crate::seams::EngineError;
use crate::specs::RoleText;

/// Why a replacement that waited is withdrawn.
const WITHDRAWN: &str = "the window it was written for never took it";

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
                .recorded_paste_of(participant.id, note.id, None, None)
                .await?;
            if let Some(item) = shown {
                let begun = self.seams.ledger.borrow_mut().begin_delivery(note.id)?;
                self.confirm_delivered(&begun.message, &json!({ "item": item }))?;
                told.push(note.id);
            }
        }
        Ok(told)
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
    pub(super) fn replace_role(
        &self,
        project: &ProjectView,
        participant: &ParticipantView,
        role: &RoleText,
    ) -> Result<MessageView, EngineError> {
        Ok(self.seams.ledger.borrow_mut().note_role(
            project.id,
            &participant.handle,
            &body_of(&role.instructions, &self.seams.roles.cf(participant)),
            &role.instructions,
        )?)
    }
}

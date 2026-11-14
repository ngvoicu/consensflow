//! Greet a switched conversation whose harness gave it no role.

use cf_ledger::{MessageView, ParticipantView, ProjectView};

use super::{note_of, Telling};
use crate::dispatcher::Dispatcher;
use crate::record::Record;
use crate::seams::EngineError;

/// Why a greeting that waited is withdrawn: the session was given its role since.
const GREETED_SINCE: &str = "a hook of the harness gave the session its role since";

impl Dispatcher {
    /// A window its human switched to `session`, a session no hook of its harness
    /// greeted with the role text as it began (`Window::greeted`: Muse Code's `/clear`
    /// runs no SessionStart hook), began that conversation with none. On a harness that
    /// takes its role at every launch (no `keeps_first_instructions`: one that keeps them
    /// is told at its launches, `compare_role`), the role is written for it and goes in
    /// first (`DeliveryPart::greeting`), and is recorded told once delivered. It is no
    /// launch's prelude: the window is up, and is given the note as it is given any
    /// message, its failure any message's (T-931 P1-1). A session a hook greeted, one
    /// already told the role as it is now, a window whose launch prelude is not done
    /// with, and one whose greeting still waits (it goes into the newer session) are
    /// given nothing.
    pub(crate) fn greet_switched(
        &self,
        project: &ProjectView,
        participant: &ParticipantView,
        record: &Record,
        session: &str,
    ) -> Result<(), EngineError> {
        let window = record.window.borrow().window.clone();
        if window.is_none_or(|window| window.greeted(session))
            || self.prelude_waits(record)?
            || self.greeting(record)?.is_some()
        {
            return Ok(());
        }
        let harness = participant.harness.as_deref().unwrap_or_default();
        let Some(adapter) = self
            .seams
            .adapters
            .adapter(harness)
            .filter(|adapter| !adapter.keeps_first_instructions())
        else {
            return Ok(());
        };
        let Ok(role) = self.role_text(project, participant, &*adapter) else {
            return Ok(());
        };
        let conversation = self.seams.ledger.borrow().current_conversation(record.id)?;
        if let Some(conversation) = conversation {
            let told = self
                .seams
                .ledger
                .borrow()
                .conversation_role_text(conversation.id)?;
            if told.is_some_and(|told| told == role.instructions) {
                return Ok(());
            }
        }
        let note = self.seams.ledger.borrow_mut().note_role(
            project.id,
            &participant.handle,
            &note_of(
                Telling::BeganWithout,
                &role.instructions,
                &self.seams.roles.cf(participant),
            ),
            &role.instructions,
        )?;
        record.delivery.borrow_mut().greeting = Some(note.id);
        // It says the role as it is now: what a note of a setting's change would say
        // is said already (`role_news`).
        self.told_by_greeting(project, participant, record, &role);
        self.changed();
        Ok(())
    }

    /// The greeting of `record`'s window while it waits to go in (`greet_switched`), as
    /// the ledger has it now. One that went in, was given up or withdrawn is let go. One
    /// whose session a hook of the harness greeted since (a later harness that runs it on
    /// a `/clear` at its first prompt) is withdrawn: the session has its role (T-931 P3-4).
    pub(crate) fn greeting(&self, record: &Record) -> Result<Option<MessageView>, EngineError> {
        let Some(id) = record.delivery.borrow().greeting else {
            return Ok(None);
        };
        let note = self.seams.ledger.borrow().message(id)?;
        let Some(note) = note.filter(|note| matches!(note.state.as_str(), "queued" | "delivering"))
        else {
            record.delivery.borrow_mut().greeting = None;
            return Ok(None);
        };
        if note.state == "queued" && self.greeted_since(record)? {
            self.seams
                .ledger
                .borrow_mut()
                .cancel_message(note.id, GREETED_SINCE)?;
            record.delivery.borrow_mut().greeting = None;
            self.changed();
            return Ok(None);
        }
        Ok(Some(note))
    }

    /// Whether a hook of the harness greeted the session `record`'s window shows now.
    fn greeted_since(&self, record: &Record) -> Result<bool, EngineError> {
        let window = record.window.borrow().window.clone();
        let conversation = self.seams.ledger.borrow().current_conversation(record.id)?;
        let session = conversation.and_then(|conversation| conversation.native_session);
        Ok(window
            .zip(session)
            .is_some_and(|(window, session)| window.greeted(&session)))
    }

    /// Whether what a launch or a switch put ahead of `record`'s queue (its prelude) is
    /// not done with: its replacement still owed ([`Dispatcher::owed`], a failed one
    /// included, which the next look settles), or the message it displaced still to go.
    pub(crate) fn prelude_waits(&self, record: &Record) -> Result<bool, EngineError> {
        if self.owed(record)?.is_some() {
            return Ok(true);
        }
        let displaced = record
            .delivery
            .borrow()
            .prelude
            .as_ref()
            .and_then(|prelude| prelude.displaced);
        let Some(displaced) = displaced else {
            return Ok(false);
        };
        Ok(self
            .seams
            .ledger
            .borrow()
            .message(displaced)?
            .is_some_and(|message| matches!(message.state.as_str(), "queued" | "delivering")))
    }
}

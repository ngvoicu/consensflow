//! The ledger's messages, their delivery and the human's gate, as its callers hold them (`index.js`, messages).

use serde_json::Value;

use super::Ledger;
use crate::{messages, LedgerError, MessageView, NewNote, NewQuestion};

impl Ledger {
    /// One message, or none.
    pub fn message(&self, id: i64) -> Result<Option<MessageView>, LedgerError> {
        self.store.message(id)
    }

    /// A note from a participant, or from ConsensFlow itself.
    pub fn note(&mut self, project_id: i64, note: &NewNote) -> Result<MessageView, LedgerError> {
        messages::note(&mut self.store, project_id, note)
    }

    /// A question for a coordinator; the asker's task waits for the answer.
    pub fn ask(
        &mut self,
        project_id: i64,
        question: &NewQuestion,
    ) -> Result<MessageView, LedgerError> {
        messages::ask(&mut self.store, project_id, question)
    }

    /// The answer to a question, from participant `from`: in words (`body`) or by `choices`.
    pub fn answer(
        &mut self,
        question_id: i64,
        from: i64,
        body: Option<&Value>,
        choices: Option<&Value>,
    ) -> Result<MessageView, LedgerError> {
        messages::answer(&mut self.store, question_id, from, body, choices)
    }

    /// The answer to a question, or none while it waits.
    pub fn answer_to(&self, question_id: i64) -> Result<Option<MessageView>, LedgerError> {
        messages::answer_to(&self.store, question_id)
    }

    /// The participants something waits on, each once.
    pub fn with_work(&self, project_id: i64) -> Result<Vec<i64>, LedgerError> {
        messages::with_work(&self.store, project_id)
    }

    /// The head of a participant's queue that may go now, or none.
    pub fn next_delivery(&self, participant_id: i64) -> Result<Option<MessageView>, LedgerError> {
        #[cfg(feature = "test-support")]
        self.watched("next_delivery", Some(participant_id))?;
        messages::next_delivery(&self.store, participant_id)
    }

    /// A message's delivery into its window begins.
    pub fn begin_delivery(&mut self, message_id: i64) -> Result<MessageView, LedgerError> {
        messages::begin_delivery(&mut self.store, message_id)
    }

    /// The harness's own record proves the message arrived.
    pub fn confirm_delivery(
        &mut self,
        message_id: i64,
        receipt: Option<&Value>,
    ) -> Result<MessageView, LedgerError> {
        messages::confirm_delivery(&mut self.store, message_id, receipt)
    }

    /// A message not yet delivered that no longer applies.
    pub fn cancel_message(
        &mut self,
        message_id: i64,
        reason: &str,
    ) -> Result<MessageView, LedgerError> {
        messages::cancel_message(&mut self.store, message_id, reason)
    }

    /// A delivery queued again; `refund` gives back its attempt.
    pub fn retry_delivery(
        &mut self,
        message_id: i64,
        reason: &str,
        refund: bool,
    ) -> Result<MessageView, LedgerError> {
        messages::retry_delivery(&mut self.store, message_id, reason, refund)
    }

    /// A delivery given up.
    pub fn fail_delivery(
        &mut self,
        message_id: i64,
        reason: &str,
    ) -> Result<MessageView, LedgerError> {
        messages::fail_delivery(&mut self.store, message_id, reason)
    }

    /// Every message on its way to a window, oldest first.
    pub fn in_flight(&self) -> Result<Vec<MessageView>, LedgerError> {
        messages::in_flight(&self.store)
    }

    /// What is on its way to a participant, oldest first.
    pub fn pending(&self, participant_id: i64) -> Result<Vec<MessageView>, LedgerError> {
        messages::pending(&self.store, participant_id)
    }

    /// The human read a message in the app.
    pub fn mark_read(&mut self, message_id: i64) -> Result<MessageView, LedgerError> {
        messages::mark_read(&mut self.store, message_id)
    }

    /// The human passes a gated message on.
    pub fn approve_message(
        &mut self,
        message_id: i64,
        by: &str,
    ) -> Result<MessageView, LedgerError> {
        messages::approve_message(&mut self.store, message_id, by)
    }

    /// The human declines a gated message.
    pub fn decline_message(
        &mut self,
        message_id: i64,
        by: &str,
    ) -> Result<MessageView, LedgerError> {
        messages::decline_message(&mut self.store, message_id, by)
    }

    /// A participant's messages, newest first, at most `limit` (Node's default: 100).
    pub fn inbox(&self, participant_id: i64, limit: i64) -> Result<Vec<MessageView>, LedgerError> {
        messages::inbox(&self.store, participant_id, limit)
    }
}

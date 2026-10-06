//! The ledger's messages, their delivery and the human's gate, as its callers hold them (`index.js`, messages).

use serde_json::Value;

use super::Ledger;
use crate::{messages, Begun, Claim, LedgerError, MessageView, NewNote, NewQuestion, Read};

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

    /// A door asks for the answer to its question, as the one who asked it:
    /// the answer, claimed for it; none yet; or the door is shut.
    pub fn claim_answer(&mut self, question_id: i64, asker: i64) -> Result<Claim, LedgerError> {
        messages::claim_answer(&mut self.store, question_id, asker)
    }

    /// A door says whether it handed the answer it claimed to its harness, as
    /// the one the answer was for.
    pub fn settle_claim(
        &mut self,
        answer_id: i64,
        recipient: i64,
        received: bool,
    ) -> Result<(), LedgerError> {
        messages::settle_claim(&mut self.store, answer_id, recipient, received)
    }

    /// Answers of `recipient` among `ids` that `cf` served whole are received.
    pub fn receive_read(
        &mut self,
        recipient: i64,
        ids: &[i64],
        via: Read,
    ) -> Result<(), LedgerError> {
        messages::receive_read(&mut self.store, recipient, ids, via)
    }

    /// The claims no door acknowledged on answers for a participant are voided,
    /// for `because`.
    pub fn release_claims(
        &mut self,
        participant_id: i64,
        because: &str,
    ) -> Result<(), LedgerError> {
        messages::release_claims(&mut self.store, participant_id, because)
    }

    /// Every claim of every window is voided: no door survives a start.
    pub fn release_all_claims(&mut self) -> Result<(), LedgerError> {
        messages::release_all_claims(&mut self.store)
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

    /// A message's delivery into its window begins: it, and the rows its paste carries.
    pub fn begin_delivery(&mut self, message_id: i64) -> Result<Begun, LedgerError> {
        messages::begin_delivery(&mut self.store, message_id)
    }

    /// The newest message pasted into a participant's window about a task and proved.
    pub fn last_pasted(
        &self,
        participant_id: i64,
        task_id: i64,
    ) -> Result<Option<MessageView>, LedgerError> {
        messages::last_pasted(&self.store, participant_id, task_id)
    }

    /// The first task message a participant's window received about a task on
    /// its own, with what its paste carried.
    pub fn first_received(
        &self,
        participant_id: i64,
        task_id: i64,
    ) -> Result<Option<Begun>, LedgerError> {
        messages::first_received(&self.store, participant_id, task_id)
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

    /// A delivery given up. A task message that fails takes its task with it;
    /// an answer that fails leaves its question to be answered again, and the
    /// one who was asked is told so.
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

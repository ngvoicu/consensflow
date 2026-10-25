//! The ledger's messages, their delivery and the human's gate, as its callers
//! hold them.

use serde_json::Value;

use super::Ledger;
use crate::{
    messages, Begun, Claim, GivenUp, HandOver, LedgerError, MessageView, NewNote, NewQuestion, Read,
};

impl Ledger {
    /// One message, or none.
    pub fn message(&self, id: i64) -> Result<Option<MessageView>, LedgerError> {
        self.store.message(id)
    }

    /// A note from a participant, or from ConsensFlow itself.
    pub fn note(&mut self, project_id: i64, note: &NewNote) -> Result<MessageView, LedgerError> {
        messages::note(&mut self.store, project_id, note)
    }

    /// A note from ConsensFlow to `to`, about no task, that says its
    /// conversation `role` again, in the words `body`: the ledger knows it for a
    /// replacement of a role from then on ([`Ledger::role_notes_to`]), and records
    /// the role as told when the note is confirmed
    /// ([`Ledger::confirm_delivery_telling`]).
    pub fn note_role(
        &mut self,
        project_id: i64,
        to: &str,
        body: &str,
        role: &str,
    ) -> Result<MessageView, LedgerError> {
        messages::note_role(&mut self.store, project_id, to, body, role)
    }

    /// The ids of the replacements of a role written to a participant, whatever
    /// became of them, oldest first.
    pub fn role_notes_to(&self, participant_id: i64) -> Result<Vec<i64>, LedgerError> {
        messages::role_notes_to(&self.store, participant_id)
    }

    /// ConsensFlow tells `to` that T-`number` is paused and why, when a stall
    /// paused it: a note of the task alone, which the other stalls of the
    /// pass join ([`Ledger::join_pause_note`]).
    pub fn note_pause(
        &mut self,
        project_id: i64,
        to: &str,
        number: i64,
        because: &str,
    ) -> Result<MessageView, LedgerError> {
        messages::note_pause(&mut self.store, project_id, to, number, because)
    }

    /// ConsensFlow tells `to` that T-`number` is held with `handle`, out of
    /// quota until `until`, when it holds the task. The ledger writes the
    /// words, and tells the requester again when the daemon resumes the task.
    pub fn note_hold(
        &mut self,
        project_id: i64,
        to: &str,
        number: i64,
        handle: &str,
        until: &str,
    ) -> Result<MessageView, LedgerError> {
        messages::note_hold(&mut self.store, project_id, to, number, handle, until)
    }

    /// A stall of the same pass adds T-`number` to the pause note `note`
    /// instead of a note of its own. None when the note cannot take it: its
    /// reader has it, or it was withdrawn.
    pub fn join_pause_note(
        &mut self,
        note: i64,
        number: i64,
        because: &str,
    ) -> Result<Option<MessageView>, LedgerError> {
        messages::join_pause_note(&mut self.store, note, number, because)
    }

    /// A question for a coordinator; the asker's task waits for the answer.
    pub fn ask(
        &mut self,
        project_id: i64,
        question: &NewQuestion,
    ) -> Result<MessageView, LedgerError> {
        messages::ask(&mut self.store, project_id, question)
    }

    /// `from` tells the window of T-`number`, still queued for it, `body`: kept
    /// for the window, it goes in after the brief, and nothing is paused.
    pub fn tell_queued(
        &mut self,
        project_id: i64,
        number: i64,
        from: &str,
        body: &str,
    ) -> Result<MessageView, LedgerError> {
        messages::tell_queued(&mut self.store, project_id, number, from, body)
    }

    /// Tells T-`number`'s requester what the window of `handle` ended its turn on tell `tell`
    /// with, while the task is paused: `words`, its last complete answer after the tell. A note
    /// for each tell; none where there is one.
    pub fn note_paused_answer(
        &mut self,
        project_id: i64,
        number: i64,
        handle: &str,
        tell: i64,
        words: &str,
    ) -> Result<Option<MessageView>, LedgerError> {
        messages::note_paused_answer(&mut self.store, project_id, number, handle, tell, words)
    }

    /// Whether words told to a participant's window about a task are still on
    /// their way to it: its answer is not the task's result until they are in.
    pub fn told_waiting(&self, participant_id: i64, task_id: i64) -> Result<bool, LedgerError> {
        messages::told_waiting(&self.store, participant_id, task_id)
    }

    /// A launch whose first message was `carrier` is over before its window
    /// opened: when its task was called off meanwhile, whoever told the window
    /// words that rode in it hears that the window never had them.
    pub fn launch_called_off(&mut self, carrier: i64) -> Result<(), LedgerError> {
        messages::launch_called_off(&mut self.store, carrier)
    }

    /// The words told to T-`number`'s windows that no window ever had, oldest
    /// first: what calling the task off now drops.
    pub fn told_unreceived(
        &self,
        project_id: i64,
        number: i64,
    ) -> Result<Vec<MessageView>, LedgerError> {
        messages::told_unreceived(&self.store, project_id, number)
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

    /// `message_id`, if it may go by every rule the head of the participant's
    /// queue goes by, whatever waits before it: for a message the engine puts
    /// ahead of the queue.
    pub fn next_delivery_of(
        &self,
        participant_id: i64,
        message_id: i64,
    ) -> Result<Option<MessageView>, LedgerError> {
        messages::next_delivery_of(&self.store, participant_id, message_id)
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

    /// The receipt of a delivered message is forgotten, as Node's ledger left one
    /// (`null`): for the engine's tests of a delivery whose arrival nothing names.
    #[cfg(feature = "test-support")]
    pub fn forget_receipt(&mut self, message_id: i64) -> Result<(), LedgerError> {
        messages::forget_receipt(&mut self.store, message_id)
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

    /// The harness's own record proves a message that says its recipient's
    /// conversation a role ([`Ledger::note_role`]) arrived: it is confirmed and
    /// the conversation recorded as told that role, in one write.
    pub fn confirm_delivery_telling(
        &mut self,
        message_id: i64,
        receipt: Option<&Value>,
    ) -> Result<MessageView, LedgerError> {
        messages::confirm_delivery_telling(&mut self.store, message_id, receipt)
    }

    /// The transport lets a hand-over begun go to its window now, after every wait
    /// of its own, if the project is open, the message still on its way, and each
    /// row of `carried` (what its paste was begun with) still riding in it: then
    /// they are the window's, marked handed in this write.
    pub fn admit(&mut self, message_id: i64, carried: &[i64]) -> Result<HandOver, LedgerError> {
        messages::admit(&mut self.store, message_id, carried)
    }

    /// The first row of `carried` (what the paste of `message_id`, handed over
    /// already, was begun with) that its window has since, read or delivered: a
    /// sign the window took the paste. None while it has none of them.
    pub fn received_of(
        &self,
        message_id: i64,
        carried: &[i64],
    ) -> Result<Option<i64>, LedgerError> {
        messages::received_of(&self.store, message_id, carried)
    }

    /// The window's record shows a paste whose Enter the engine withheld: the human
    /// sent what the box held. The message, waiting again or withdrawn since, is
    /// begun and confirmed in one write, `receipt` its receipt, with the rows of
    /// `held` riding in it again; with `telling`, its conversation is recorded as
    /// told the role it says.
    pub fn confirm_sent_by_hand(
        &mut self,
        message_id: i64,
        held: &[i64],
        receipt: &Value,
        telling: bool,
    ) -> Result<Option<MessageView>, LedgerError> {
        messages::confirm_sent_by_hand(&mut self.store, message_id, held, receipt, telling)
    }

    /// The messages of a project whose hand-over has begun at their window and is
    /// not over: let go by their transport, or claimed by a question's door.
    pub fn handed_over(&self, project_id: i64) -> Result<Vec<i64>, LedgerError> {
        messages::handed_over(&self.store, project_id)
    }

    /// A message not yet delivered that no longer applies.
    pub fn cancel_message(
        &mut self,
        message_id: i64,
        reason: &str,
    ) -> Result<MessageView, LedgerError> {
        messages::cancel_message(&mut self.store, message_id, reason)
    }

    /// A delivery queued again; `refund` gives back its attempt. A pause note
    /// naming a task that moved on while it was on its way leaves it, and says
    /// its warning again.
    pub fn retry_delivery(
        &mut self,
        message_id: i64,
        reason: &str,
        refund: bool,
    ) -> Result<MessageView, LedgerError> {
        #[cfg(feature = "test-support")]
        self.watched("retry_delivery", Some(message_id))?;
        messages::retry_delivery(&mut self.store, message_id, reason, refund)
    }

    /// A task message that arrived, and that its window took back out of its
    /// conversation, was not received: it is kept for the window again, and
    /// the words that resume the task carry it, with what its paste carried.
    pub fn take_back(&mut self, message_id: i64, reason: &str) -> Result<MessageView, LedgerError> {
        #[cfg(feature = "test-support")]
        self.watched("take_back", Some(message_id))?;
        messages::take_back(&mut self.store, message_id, reason)
    }

    /// A delivery given up. A task message that fails takes its task with it;
    /// an answer that fails leaves its question to be answered again, and the
    /// one who was asked is told so. A pause note naming a task that moved on
    /// while it was on its way leaves it, and says its warning again; one with
    /// no task left is withdrawn instead.
    pub fn fail_delivery(
        &mut self,
        message_id: i64,
        reason: &str,
    ) -> Result<MessageView, LedgerError> {
        messages::fail_delivery(&mut self.store, message_id, reason)
    }

    /// [`Ledger::fail_delivery`], saying what it did: failed, kept for the task's
    /// resume, or withdrawn ([`GivenUp`]).
    pub fn give_up_delivery(
        &mut self,
        message_id: i64,
        reason: &str,
    ) -> Result<(MessageView, GivenUp), LedgerError> {
        messages::give_up_delivery(&mut self.store, message_id, reason)
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

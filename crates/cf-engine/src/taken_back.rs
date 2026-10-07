//! A window that stopped a turn before a word of its answer, and took the
//! turn's message back out of its conversation, was not given that message
//! after all. Claude does so: interrupted that early, it puts the message in
//! its input box again and answers from what came before it, though its
//! transcript still shows the message. The ledger counted the message as
//! received, because its marker showed there; so it is told, in the step that
//! pays the stop: the message is kept for the window again, as a task paused
//! before its brief arrived keeps the brief, and the words that resume the
//! task carry it.

use cf_harness::contract::Observed;
use cf_harness::records::Role;
use cf_ledger::{ParticipantView, Stop};

use crate::delivery_text::marker_of;
use crate::dispatcher::Dispatcher;
use crate::seams::EngineError;

impl Dispatcher {
    /// The look that pays `stop` at rest found the window had taken its
    /// message back: the task message the stopped turn began with, if there is
    /// one, goes back to the ledger's keeping. That is the last one pasted
    /// into the window about the task, when the turn's own message shows its
    /// marker: a turn that began with a tell, an answer or the human's own
    /// words has none, and the task's brief stays in the conversation.
    pub(crate) fn take_back(
        &self,
        participant: &ParticipantView,
        stop: &Stop,
        observed: &Observed,
    ) -> Result<(), EngineError> {
        let turn = observed
            .items()
            .iter()
            .rev()
            .find(|item| item.role == Role::User);
        let latest = self
            .seams
            .ledger
            .borrow()
            .last_pasted(participant.id, stop.task_id)?;
        let Some(message) = latest.filter(|message| {
            message.kind == "task"
                && turn.is_some_and(|turn| turn.text.contains(&marker_of(message.id)))
        }) else {
            return Ok(());
        };
        let reason = format!(
            "@{} stopped before a word of its answer and took it back out of its conversation",
            participant.handle
        );
        self.seams
            .ledger
            .borrow_mut()
            .take_back(message.id, &reason)?;
        self.changed();
        Ok(())
    }
}

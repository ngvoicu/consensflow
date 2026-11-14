//! Confirm a delivery and, when it told a whole role, record that together.

use cf_ledger::MessageView;
use serde_json::Value;

use crate::delivery_text::goes_whole;
use crate::dispatcher::Dispatcher;
use crate::seams::EngineError;

impl Dispatcher {
    /// The harness's own record proves `message` arrived: it is confirmed. A
    /// replacement of a role that went in whole also records the conversation as
    /// told it, in the same write. One that was cut (more than a message goes in
    /// whole at) tells the conversation its role in part: it is not recorded until
    /// the window reads it whole (`cf inbox read`, which the ledger records). One
    /// whose recipient has no current conversation any more (it ended while the
    /// note was on its way) is confirmed and records nothing: there is no
    /// conversation to have told.
    pub(crate) fn confirm_delivered(
        &self,
        message: &MessageView,
        receipt: &Value,
    ) -> Result<(), EngineError> {
        let tells = self.tells_a_role(message)?;
        let mut ledger = self.seams.ledger.borrow_mut();
        if tells == Some(true) {
            ledger.confirm_delivery_telling(message.id, Some(receipt))?;
        } else {
            ledger.confirm_delivery(message.id, Some(receipt))?;
        }
        // Its window took work: it came up, whatever its record showed before (CHF-06).
        if let Some(record) = self.record(message.recipient_id) {
            record.window.borrow_mut().took = true;
        }
        Ok(())
    }

    /// Whether `message` says its recipient's conversation a role: whole (`true`),
    /// which its confirmation records as told, or cut (`false`), which its read
    /// records. None for a message that says no role.
    pub(crate) fn tells_a_role(&self, message: &MessageView) -> Result<Option<bool>, EngineError> {
        let says_a_role = self
            .seams
            .ledger
            .borrow()
            .role_notes_to(message.recipient_id)?
            .contains(&message.id);
        let cf = self.cf_for(message)?;
        Ok(says_a_role.then(|| goes_whole(message, &[], &cf)))
    }
}

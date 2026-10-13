//! The ledger's wake-up calls, as its callers hold them.

use super::Ledger;
use crate::{wake_ups, LedgerError, MessageView, NewWakeUp, WakeUpSetting};

impl Ledger {
    /// A project's wake-up calls: off, an hour apart and the default words
    /// until the human says otherwise.
    pub fn wake_up(&self, project_id: i64) -> Result<WakeUpSetting, LedgerError> {
        wake_ups::setting(&self.store, project_id)
    }

    /// The human sets a project's wake-up calls, which apply at once.
    pub fn set_wake_up(
        &mut self,
        project_id: i64,
        request: &NewWakeUp,
    ) -> Result<WakeUpSetting, LedgerError> {
        wake_ups::set_wake_up(&mut self.store, project_id, request)
    }

    /// The project's wake-up, queued for its chief when it is due at `now_ms`
    /// (a note from the human, delivered as any note is): the note, or none.
    /// Whether the chief is at rest is for the caller to say first.
    pub fn queue_wake_up_if_due(
        &mut self,
        project_id: i64,
        now_ms: i64,
    ) -> Result<Option<MessageView>, LedgerError> {
        wake_ups::queue_if_due(&mut self.store, project_id, now_ms)
    }

    /// A wake-up that waits for its turn is withdrawn, for `reason`: it is
    /// never delivered.
    pub fn withdraw_wake_up(&mut self, project_id: i64, reason: &str) -> Result<(), LedgerError> {
        wake_ups::withdraw_queued(&mut self.store, project_id, reason)
    }
}

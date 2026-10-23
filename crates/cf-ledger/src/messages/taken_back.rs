//! A message a window took back out of its conversation after it arrived: it
//! was not given after all, and is kept for the window again.

use cf_proto::ledger::MessageView;
use rusqlite::params;
use serde_json::json;

use super::carrying::adopt;
use super::told::{assignee_of, is_told};
use super::{known_message, message_task, require_message};
use crate::model::{self, LedgerError};
use crate::store::Store;

/// A task message that arrived, and that its window then took back out of its
/// conversation (Claude, stopped before a word of its answer, puts the message
/// in its input box again and answers from what came before it), was not
/// received after all. It is `queued` again, with `reason`: kept for the
/// window as a task paused before its brief arrived keeps the brief, so the
/// words that resume the task carry it. They do, whichever came first:
/// - the resume not yet made: it takes the message with what is kept;
/// - the resume made while the stop was being paid: the message joins it, as a
///   delivery tried again does (`adopt`).
///
/// What its paste carried was in the window's conversation no longer than it
/// was, so each row it carried is kept again too, and rides with it. The
/// receipt it was given is no receipt: the carrier's will be. The task's state
/// is not moved: it is paused, or queued behind the words that resume it.
/// Words told to the window (`told`), pasted on their own, are taken back the
/// same way, and the words that resume the task carry them.
pub(crate) fn take_back(
    store: &mut Store,
    message_id: i64,
    reason: &str,
) -> Result<MessageView, LedgerError> {
    model::require_text(reason, "reason", 1000)?;
    store.write(|store| {
        let message = require_message(store, message_id, "delivered")?;
        let carried_by: Option<i64> = store.db.query_row(
            "SELECT carried_by FROM message WHERE id = ?",
            [message_id],
            |row| row.get(0),
        )?;
        let assignee = match message_task(store, message_id)? {
            Some(task) => assignee_of(store, &task)?,
            None => None,
        };
        if !(message.kind == "task" || is_told(&message, assignee.as_deref()))
            || carried_by.is_some()
        {
            return Err(LedgerError::refused_with(
                "invalid-transition",
                format!("message {message_id} is not a task message of its own"),
                409,
            ));
        }
        store.db.execute(
            "UPDATE message SET state = 'queued', reason = ?, delivered_at = NULL, receipt = NULL
         WHERE id = ?",
            params![reason, message_id],
        )?;
        let kept: Vec<i64> = store
            .db
            .prepare(
                "SELECT id FROM message WHERE carried_by = ? AND state = 'delivered' ORDER BY id",
            )?
            .query_map([message_id], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        store.db.execute(
            "UPDATE message SET state = 'queued', delivered_at = NULL, receipt = NULL
         WHERE carried_by = ? AND state = 'delivered'",
            [message_id],
        )?;
        store.log(
            message.project_id,
            "delivery.taken-back",
            json!({ "message": message_id, "reason": reason, "kept": kept }),
        )?;
        adopt(store, message_id)?;
        known_message(store, message_id)
    })
}

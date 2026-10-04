//! A message on its way, and off it (`src/ledger/queue.js`). `queue` writes
//! one by participant id: queued for its recipient, or held for the human
//! when the project's gate holds it. `send` does the same by handle, and
//! logs it. A message that no longer applies is dropped, and never delivered.

use cf_proto::ledger::MessageView;
use rusqlite::params;
use serde_json::json;

use crate::model::{self, LedgerError, MAX_BODY};
use crate::store::Store;

/// A message to queue: its recipient and sender by id, what kind it is, the
/// task it is about, and what it says.
pub(crate) struct Queued<'a> {
    pub(crate) to: i64,
    pub(crate) from: Option<i64>,
    pub(crate) kind: &'a str,
    pub(crate) task_id: Option<i64>,
    pub(crate) body: &'a str,
}

/// Writes a message on its way: queued for its recipient, or waiting for
/// the human instead when the project gates it. Its id.
pub(crate) fn queue(
    store: &mut Store,
    project_id: i64,
    message: &Queued<'_>,
) -> Result<i64, LedgerError> {
    let landing = if gate_holds(store, project_id, message.from, message.to)? {
        "gated"
    } else {
        "queued"
    };
    let at = store.at();
    store.db.execute(
        "INSERT INTO message (project_id, recipient_id, sender_id, kind, task_id, reply_to, body,
                            state, questions, choices, urgent, created_at)
       VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        params![
            project_id,
            message.to,
            message.from,
            message.kind,
            message.task_id,
            None::<i64>,
            message.body,
            landing,
            None::<String>,
            None::<String>,
            0,
            at,
        ],
    )?;
    Ok(store.db.last_insert_rowid())
}

/// Whether the project's gate holds a message: one agent's word to another,
/// when the human approves every hand-off. What the human sends or
/// receives, what ConsensFlow itself notes, and what an agent tells itself pass.
fn gate_holds(
    store: &Store,
    project_id: i64,
    from: Option<i64>,
    to: i64,
) -> Result<bool, LedgerError> {
    let Some(from) = from.filter(|from| *from != to) else {
        return Ok(false);
    };
    if store.project_row(project_id)?.gate != 1 {
        return Ok(false);
    }
    Ok(store.participant_row(from)?.role != "human" && store.participant_row(to)?.role != "human")
}

/// A message to send by handle: from whom (ConsensFlow itself when no one),
/// to whom, about which task, of what kind, and what it says.
pub(crate) struct Sent<'a> {
    pub(crate) from: Option<&'a str>,
    pub(crate) to: &'a str,
    pub(crate) body: &'a str,
    pub(crate) task: Option<i64>,
    pub(crate) kind: &'a str,
}

/// Queues a message by handle, and logs it.
pub(crate) fn send(
    store: &mut Store,
    project_id: i64,
    message: &Sent<'_>,
) -> Result<MessageView, LedgerError> {
    model::require_text(message.body, "body", MAX_BODY)?;
    store.write(|store| {
        let sender = message
            .from
            .map(|from| store.participant_by_handle(project_id, from))
            .transpose()?;
        let recipient = store.participant_by_handle(project_id, message.to)?;
        let task_id = message
            .task
            .map(|task| store.task_row(project_id, task).map(|row| row.id))
            .transpose()?;
        let id = queue(
            store,
            project_id,
            &Queued {
                to: recipient.id,
                from: sender.as_ref().map(|sender| sender.id),
                kind: message.kind,
                task_id,
                body: message.body,
            },
        )?;
        store.log(
            project_id,
            "message.sent",
            json!({
                "message": id,
                "kind": message.kind,
                "from": sender.map(|sender| sender.handle),
                "to": recipient.handle,
            }),
        )?;
        store.message(id)?.ok_or_else(|| {
            LedgerError::refused_with("unknown-message", format!("no message {id}"), 404)
        })
    })
}

/// A cancelled or failed task's queued messages are never delivered.
pub(crate) fn drop_queued(store: &Store, task_id: i64) -> Result<(), LedgerError> {
    store.db.execute(
        "UPDATE message SET state = 'cancelled' WHERE task_id = ? AND state IN ('queued', 'gated')",
        [task_id],
    )?;
    Ok(())
}

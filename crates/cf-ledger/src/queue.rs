//! A message on its way, and off it (`src/ledger/queue.js`). `queue` writes
//! one by participant id: queued for its recipient, read at once when it is
//! the asker's own window that gave the answer, or held for the human when the
//! project's gate holds it. `send` does the same by handle, and logs it. A
//! message that no longer applies is withdrawn or dropped, and never delivered.

use cf_proto::ledger::{MessageView, Question};
use rusqlite::params;
use serde_json::json;

use crate::messages::{adopt, release_carried};
use crate::model::{self, LedgerError, MAX_BODY};
use crate::store::Store;
use crate::views::TaskRow;

/// A message to queue: its recipient and sender by id, what kind it is, the
/// task it is about, the question it answers, and what it says; whether it
/// is an answer the asker's own window gave, which is received as it is
/// written; whether a question's door is shut from the start; a question's
/// options, an answer's choices, and whether a tell is urgent.
#[derive(Default)]
pub(crate) struct Queued<'a> {
    pub(crate) to: i64,
    pub(crate) from: Option<i64>,
    pub(crate) kind: &'a str,
    pub(crate) task_id: Option<i64>,
    pub(crate) reply_to: Option<i64>,
    pub(crate) body: &'a str,
    pub(crate) from_window: bool,
    pub(crate) door_closed: bool,
    pub(crate) questions: Option<&'a [Question]>,
    pub(crate) choices: Option<&'a [Vec<String>]>,
    pub(crate) urgent: bool,
}

/// Writes a message on its way: queued for its recipient, read at once when
/// the asker's own window gave it (its receipt says so), or waiting for the
/// human instead when the project gates it. A row it keeps for a window joins
/// the task message that waits for it. Its id.
pub(crate) fn queue(
    store: &mut Store,
    project_id: i64,
    message: &Queued<'_>,
) -> Result<i64, LedgerError> {
    let landing = if gate_holds(store, project_id, message.from, message.to)? {
        "gated"
    } else if message.from_window {
        "read"
    } else {
        "queued"
    };
    let questions = message.questions.map(serde_json::to_string).transpose()?;
    let choices = message.choices.map(serde_json::to_string).transpose()?;
    let at = store.at();
    let received = (landing == "read").then(|| (at.clone(), r#"{"window":true}"#));
    store.db.execute(
        "INSERT INTO message (project_id, recipient_id, sender_id, kind, task_id, reply_to, body,
                            state, questions, choices, urgent, created_at, delivered_at, receipt,
                            door_closed_at)
       VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        params![
            project_id,
            message.to,
            message.from,
            message.kind,
            message.task_id,
            message.reply_to,
            message.body,
            landing,
            questions,
            choices,
            i64::from(message.urgent),
            at,
            received.as_ref().map(|(at, _)| at),
            received.as_ref().map(|(_, receipt)| receipt),
            message.door_closed.then_some(&at),
        ],
    )?;
    let id = store.db.last_insert_rowid();
    if landing == "queued" && matches!(message.kind, "answer" | "note") {
        adopt(store, id)?;
    }
    Ok(id)
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
/// to whom, about which task, of what kind, and what it says; a question's
/// options, and whether a tell is urgent.
#[derive(Default)]
pub(crate) struct Sent<'a> {
    pub(crate) from: Option<&'a str>,
    pub(crate) to: &'a str,
    pub(crate) body: &'a str,
    pub(crate) task: Option<i64>,
    pub(crate) kind: &'a str,
    pub(crate) questions: Option<&'a [Question]>,
    pub(crate) urgent: bool,
    pub(crate) door_closed: bool,
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
                questions: message.questions,
                urgent: message.urgent,
                door_closed: message.door_closed,
                ..Queued::default()
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

/// A gated message the human never passed on: declined, answered, or overtaken.
pub(crate) fn withdraw(store: &Store, message_id: i64, reason: &str) -> Result<(), LedgerError> {
    store.db.execute(
        "UPDATE message SET state = 'cancelled', reason = ? WHERE id = ?",
        params![reason, message_id],
    )?;
    Ok(())
}

/// A task's messages still held at the gate, withdrawn; what a withdrawn
/// carrier carried is its own again.
pub(crate) fn withdraw_gated(store: &Store, task_id: i64, reason: &str) -> Result<(), LedgerError> {
    store.db.execute(
        "UPDATE message SET state = 'cancelled', reason = ? WHERE task_id = ? AND state = 'gated'",
        params![reason, task_id],
    )?;
    release_carried(store, task_id)
}

/// The result of a task that was decided (`why`: "was accepted", "was sent
/// back") before its requester was given it: still `queued`, it is withdrawn,
/// with the reason `T-<number> <why>`, and never pasted, so the decision it
/// asks for is not put again to the one who took it. A result being pasted or
/// already given stays, as the requester has it; one held at the gate is
/// [`withdraw_gated`]'s.
pub(crate) fn withdraw_result(store: &Store, task: &TaskRow, why: &str) -> Result<(), LedgerError> {
    store.db.execute(
        "UPDATE message SET state = 'cancelled', reason = ?
       WHERE task_id = ? AND kind = 'result' AND state = 'queued'",
        params![format!("T-{} {why}", task.number), task.id],
    )?;
    Ok(())
}

/// A cancelled or failed task's queued messages are never delivered.
pub(crate) fn drop_queued(store: &Store, task_id: i64) -> Result<(), LedgerError> {
    store.db.execute(
        "UPDATE message SET state = 'cancelled' WHERE task_id = ? AND state IN ('queued', 'gated')",
        [task_id],
    )?;
    Ok(())
}

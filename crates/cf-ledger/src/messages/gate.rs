//! What waits for the human: their own messages, read in the app, and one
//! agent's word to another held at the gate until they pass it on or
//! decline it.

use cf_proto::ledger::MessageView;
use rusqlite::params;
use serde_json::json;

use super::again::answer_again;
use super::{adopt, known_message, message_task, require_message};
use crate::model::LedgerError;
use crate::queue::{send, withdraw, Sent};
use crate::store::Store;
use crate::tasks::call_off;

/// The human read a message in the app.
pub(crate) fn mark_read(store: &mut Store, message_id: i64) -> Result<MessageView, LedgerError> {
    store.write(|store| {
        let message = known_message(store, message_id)?;
        if message.recipient_role != "human" {
            return Err(LedgerError::refused_with(
                "not-for-the-human",
                format!("message {message_id} is delivered to a pane"),
                409,
            ));
        }
        if message.state == "read" {
            return Ok(message);
        }
        require_message(store, message_id, "queued")?;
        let at = store.at();
        store.db.execute(
            "UPDATE message SET state = 'read', delivered_at = ? WHERE id = ?",
            params![at, message_id],
        )?;
        store.log(
            message.project_id,
            "message.read",
            json!({ "message": message_id }),
        )?;
        known_message(store, message_id)
    })
}

/// The human passes a gated message on: it is queued for its recipient, as it
/// would have been without the gate, and never read: only the one it is for
/// receives it. What it becomes then, a row its window keeps, joins the task
/// message that waits for that window.
pub(crate) fn approve_message(
    store: &mut Store,
    message_id: i64,
    by: &str,
) -> Result<MessageView, LedgerError> {
    store.write(|store| {
        let message = require_message(store, message_id, "gated")?;
        store.participant_by_handle(message.project_id, by)?;
        store.db.execute(
            "UPDATE message SET state = 'queued' WHERE id = ?",
            [message_id],
        )?;
        store.log(
            message.project_id,
            "message.approved",
            json!({ "message": message_id, "by": by }),
        )?;
        adopt(store, message_id)?;
        known_message(store, message_id)
    })
}

/// The human declines a gated message, and whoever sent it is told. A
/// declined task is cancelled; a declined answer leaves its question open
/// for another, which is held for approval again if the answer had taken it
/// off the gate. A result or a question is passed on, never declined.
pub(crate) fn decline_message(
    store: &mut Store,
    message_id: i64,
    by: &str,
) -> Result<MessageView, LedgerError> {
    store.write(|store| {
        let message = require_message(store, message_id, "gated")?;
        store.participant_by_handle(message.project_id, by)?;
        if message.kind != "task" && message.kind != "answer" {
            return Err(LedgerError::refused_with(
                "not-declinable",
                format!("a {} is passed on, not declined", message.kind),
                409,
            ));
        }
        withdraw(store, message_id, &format!("declined by @{by}"))?;
        store.log(
            message.project_id,
            "message.declined",
            json!({ "message": message_id, "by": by }),
        )?;
        if let (Some(question), "answer") = (message.reply_to, message.kind.as_str()) {
            reopen_question(store, question)?;
        }
        let task = message_task(store, message_id)?;
        let reply_to = message
            .reply_to
            .map_or_else(|| "null".to_string(), |id| id.to_string());
        let mut told = message.sender.clone();
        let mut word = format!(
            "@{by} declined your answer to m-{reply_to}. {}",
            answer_again(&reply_to)
        );
        if message.kind == "task" {
            if let Some(task) = &task {
                told = Some(store.participant_row(task.requester_id)?.handle);
                call_off(store, message.project_id, task.number, by)?;
                word = format!(
                    "@{by} declined T-{} ({}). It is cancelled.",
                    task.number, task.title
                );
            }
        }
        if told.as_deref() != Some(by) {
            send(
                store,
                message.project_id,
                &Sent {
                    from: Some(by),
                    to: told.as_deref().unwrap_or("null"),
                    task: task.as_ref().map(|task| task.number),
                    body: &word,
                    kind: "note",
                    ..Sent::default()
                },
            )?;
        }
        known_message(store, message_id)
    })
}

/// A question that was withdrawn from the gate because it was answered there
/// is held for the human again once that answer is declined: nobody received
/// an answer, so the question still obliges, and is still the human's to pass on.
fn reopen_question(store: &Store, question: i64) -> Result<(), LedgerError> {
    store.db.execute(
        "UPDATE message SET state = 'gated', reason = NULL
         WHERE id = ? AND kind = 'question' AND state = 'cancelled'
           AND reason LIKE 'answered by @%'",
        [question],
    )?;
    Ok(())
}

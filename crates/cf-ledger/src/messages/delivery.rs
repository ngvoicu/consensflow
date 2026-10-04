//! A message's way into a window: the head of a participant's queue that
//! may go now, its delivery begun, confirmed by the harness's own record,
//! cancelled, retried or failed, and what is still on its way.

use cf_base::json::js_order;
use cf_proto::ledger::MessageView;
use rusqlite::params;
use serde_json::{json, Value};

use super::{known_message, message_task, messages, require_message, unanswered};
use crate::model::{self, sql_list, LedgerError, HELD_TASK_STATES, MEMBER_ROLES};
use crate::store::Store;
use crate::views::MESSAGE_SELECT;

/// The participants something waits on: a message on its way to them, or a
/// task in their hands. A session with neither and no window has nothing a
/// pass could do for it.
pub(crate) fn with_work(store: &Store, project_id: i64) -> Result<Vec<i64>, LedgerError> {
    let ids = store
        .db
        .prepare(
            "SELECT recipient_id AS id FROM message
         WHERE project_id = ? AND state IN ('queued', 'delivering')
         UNION
         SELECT assignee_id FROM task
         WHERE project_id = ? AND assignee_id IS NOT NULL AND state IN ('queued', 'working', 'waiting')",
        )?
        .query_map(params![project_id, project_id], |row| row.get::<_, i64>("id"))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut distinct = Vec::with_capacity(ids.len());
    for id in ids {
        if !distinct.contains(&id) {
            distinct.push(id);
        }
    }
    Ok(distinct)
}

/// The head of a participant's queue that may go now, or none. A member's
/// session is its task's: a task message waits while it holds one, and a
/// message about no task of its own (a stray note) never opens a window.
pub(crate) fn next_delivery(
    store: &Store,
    participant_id: i64,
) -> Result<Option<MessageView>, LedgerError> {
    let participant = store.participant_row(participant_id)?;
    if participant.role == "human" {
        return Ok(None);
    }
    let busy = store
        .db
        .prepare("SELECT 1 FROM message WHERE recipient_id = ? AND state = 'delivering'")?
        .exists([participant_id])?;
    if busy {
        return Ok(None);
    }
    let serial = i64::from(MEMBER_ROLES.contains(&participant.role.as_str()));
    let next = store
        .db
        .prepare(&format!(
            "SELECT id FROM message
       WHERE recipient_id = ? AND state = 'queued'
         AND (kind != 'task' OR ? = 0 OR NOT EXISTS (
           SELECT 1 FROM task WHERE assignee_id = ? AND state IN ('working', 'waiting')
         ))
         AND (? = 0 OR kind = 'task' OR urgent = 1 OR task_id IN (
           SELECT id FROM task WHERE assignee_id = ? AND state IN ({})
         ))
       ORDER BY id LIMIT 1",
            sql_list(&HELD_TASK_STATES)
        ))?
        .query_map(
            params![
                participant_id,
                serial,
                participant_id,
                serial,
                participant_id
            ],
            |row| row.get::<_, i64>("id"),
        )?
        .next()
        .transpose()?;
    next.map_or(Ok(None), |id| store.message(id))
}

pub(crate) fn begin_delivery(
    store: &mut Store,
    message_id: i64,
) -> Result<MessageView, LedgerError> {
    store.write(|store| {
        let message = require_message(store, message_id, "queued")?;
        if message.recipient_role == "human" {
            return Err(LedgerError::refused_with(
                "human-reads-in-app",
                "the human reads messages in the app",
                409,
            ));
        }
        let busy = store
            .db
            .prepare("SELECT id FROM message WHERE recipient_id = ? AND state = 'delivering'")?
            .exists([message.recipient_id])?;
        let working = message.kind == "task"
            && MEMBER_ROLES.contains(&message.recipient_role.as_str())
            && store
                .db
                .prepare(
                    "SELECT 1 FROM task WHERE assignee_id = ? AND state IN ('working', 'waiting')",
                )?
                .exists([message.recipient_id])?;
        if busy || working {
            return Err(LedgerError::refused_with(
                "recipient-busy",
                format!(
                    "{} is still receiving or working; message {message_id} waits",
                    message.recipient
                ),
                409,
            ));
        }
        store.db.execute(
            "UPDATE message SET state = 'delivering', attempts = attempts + 1, reason = NULL
         WHERE id = ?",
            [message_id],
        )?;
        store.log(
            message.project_id,
            "delivery.begun",
            json!({ "message": message_id, "attempt": message.attempts + 1 }),
        )?;
        known_message(store, message_id)
    })
}

/// The harness's own record proves the message arrived: `receipt` is what it showed.
pub(crate) fn confirm_delivery(
    store: &mut Store,
    message_id: i64,
    receipt: Option<&Value>,
) -> Result<MessageView, LedgerError> {
    store.write(|store| {
        let message = require_message(store, message_id, "delivering")?;
        let at = store.at();
        let receipt = js_order(receipt.cloned().unwrap_or(Value::Null)).to_string();
        store.db.execute(
            "UPDATE message SET state = 'delivered', delivered_at = ?, receipt = ? WHERE id = ?",
            params![at, receipt, message_id],
        )?;
        store.log(
            message.project_id,
            "delivery.confirmed",
            json!({ "message": message_id }),
        )?;
        if let Some(task) = message_task(store, message_id)? {
            // A task whose window already asked a question arrives waiting, not working.
            if message.kind == "task" && task.state == "queued" {
                let to = if unanswered(store, task.id)? {
                    "waiting"
                } else {
                    "working"
                };
                store.move_task(&task, to, json!({}))?;
            }
            if message.kind == "answer" && task.state == "waiting" {
                store.move_task(&task, "working", json!({}))?;
            }
        }
        known_message(store, message_id)
    })
}

/// A message not yet delivered that no longer applies: it will not be delivered.
pub(crate) fn cancel_message(
    store: &mut Store,
    message_id: i64,
    reason: &str,
) -> Result<MessageView, LedgerError> {
    model::require_text(reason, "reason", 1000)?;
    store.write(|store| {
        let message = store
            .message(message_id)?
            .filter(|message| message.state == "queued" || message.state == "delivering")
            .ok_or_else(|| {
                LedgerError::refused_with(
                    "not-pending",
                    format!("message {message_id} is not waiting to be delivered"),
                    409,
                )
            })?;
        store.db.execute(
            "UPDATE message SET state = 'cancelled', reason = ? WHERE id = ?",
            params![reason, message_id],
        )?;
        store.log(
            message.project_id,
            "delivery.cancelled",
            json!({ "message": message_id, "reason": reason }),
        )?;
        known_message(store, message_id)
    })
}

/// Queued again; `refund` gives back the attempt when the window went before it could land.
pub(crate) fn retry_delivery(
    store: &mut Store,
    message_id: i64,
    reason: &str,
    refund: bool,
) -> Result<MessageView, LedgerError> {
    model::require_text(reason, "reason", 1000)?;
    store.write(|store| {
        let message = require_message(store, message_id, "delivering")?;
        store.db.execute(
            "UPDATE message SET state = 'queued', reason = ?, attempts = MAX(attempts - ?, 0)
         WHERE id = ?",
            params![reason, i64::from(refund), message_id],
        )?;
        store.log(
            message.project_id,
            "delivery.retried",
            json!({ "message": message_id, "reason": reason }),
        )?;
        known_message(store, message_id)
    })
}

/// A delivery given up: a task it carried, still queued, fails with it.
pub(crate) fn fail_delivery(
    store: &mut Store,
    message_id: i64,
    reason: &str,
) -> Result<MessageView, LedgerError> {
    model::require_text(reason, "reason", 1000)?;
    store.write(|store| {
        let message = require_message(store, message_id, "delivering")?;
        store.db.execute(
            "UPDATE message SET state = 'failed', reason = ? WHERE id = ?",
            params![reason, message_id],
        )?;
        store.log(
            message.project_id,
            "delivery.failed",
            json!({ "message": message_id, "reason": reason }),
        )?;
        if let Some(task) = message_task(store, message_id)? {
            if message.kind == "task" && task.state == "queued" {
                store.move_task(&task, "failed", json!({}))?;
            }
        }
        known_message(store, message_id)
    })
}

/// Every message on its way to a window, oldest first. At start none of
/// those windows is left (they died with the previous process), so each is
/// settled before anything else is delivered.
pub(crate) fn in_flight(store: &Store) -> Result<Vec<MessageView>, LedgerError> {
    messages(
        store,
        &format!("{MESSAGE_SELECT} WHERE m.state = 'delivering' ORDER BY m.id"),
        [],
    )
}

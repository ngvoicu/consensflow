//! A message's way into a window: the head of a participant's queue that
//! may go now, its delivery begun, confirmed by the harness's own record,
//! cancelled, retried or failed, and what is still on its way. What a
//! window keeps rides in the paste of the message that sends it on
//! (`carrying`), and what the paste proves arrived is each of them.

use cf_base::json::js_order;
use cf_proto::ledger::{Begun, MessageView};
use rusqlite::params;
use serde_json::{json, Value};

use super::carrying::{adopt, carried, release_carried};
use super::receipt::reconcile;
use super::{known_message, message_task, messages, require_message};
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
/// session is its task's: a row of its goes only for a task it holds (queued,
/// working or waiting), a tell for the window goes whatever its task, and a
/// task message waits while it holds one at work. A row that rides in another
/// message's paste, or that a door took for its harness, is not the head; and
/// while a task message for the window waits for the human's approval, nothing
/// else about its task goes, whoever wrote it and in whatever order they are
/// approved: the words that came first are not passed over.
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
            "SELECT m.id FROM message m
       WHERE m.recipient_id = ?1 AND m.state = 'queued'
         AND m.carried_by IS NULL AND m.claimed_at IS NULL
         AND (m.kind != 'task' OR ?2 = 0 OR NOT EXISTS (
           SELECT 1 FROM task WHERE assignee_id = ?1 AND state IN ('working', 'waiting')
         ))
         AND (?2 = 0 OR m.urgent = 1 OR m.task_id IN (
           SELECT id FROM task WHERE assignee_id = ?1 AND state IN ({})
         ))
         AND (m.urgent = 1 OR m.task_id IS NULL OR NOT EXISTS (
           SELECT 1 FROM message words WHERE words.task_id = m.task_id
             AND words.recipient_id = m.recipient_id AND words.kind = 'task'
             AND words.state = 'gated'
         ))
       ORDER BY m.id LIMIT 1",
            sql_list(&HELD_TASK_STATES)
        ))?
        .query_map(params![participant_id, serial], |row| row.get::<_, i64>(0))?
        .next()
        .transpose()?;
    next.map_or(Ok(None), |id| store.message(id))
}

/// The newest message pasted into a participant's window about a task, and
/// proved by what the window's record showed: a task message or an answer
/// that is `delivered` and rides in no other's paste. What the window wrote
/// after its marker is the task's result.
pub(crate) fn last_pasted(
    store: &Store,
    participant_id: i64,
    task_id: i64,
) -> Result<Option<MessageView>, LedgerError> {
    Ok(messages(
        store,
        &format!(
            "{MESSAGE_SELECT} WHERE m.task_id = ? AND m.recipient_id = ?
         AND m.kind IN ('task', 'answer') AND m.state = 'delivered' AND m.carried_by IS NULL
       ORDER BY m.id DESC LIMIT 1"
        ),
        params![task_id, participant_id],
    )?
    .into_iter()
    .next())
}

/// The first task message a participant's window received about a task that
/// was pasted on its own (the brief, most often), with what its paste
/// carried. A window that starts afresh on the task begins from it.
pub(crate) fn first_received(
    store: &Store,
    participant_id: i64,
    task_id: i64,
) -> Result<Option<Begun>, LedgerError> {
    let first = messages(
        store,
        &format!(
            "{MESSAGE_SELECT} WHERE m.task_id = ? AND m.recipient_id = ?
         AND m.kind = 'task' AND m.state IN ('delivered', 'read') AND m.carried_by IS NULL
       ORDER BY m.id LIMIT 1"
        ),
        params![task_id, participant_id],
    )?
    .into_iter()
    .next();
    first
        .map(|message| {
            let carried = messages(
                store,
                &format!(
                    "{MESSAGE_SELECT} WHERE m.carried_by = ? AND m.state = 'delivered' ORDER BY m.id"
                ),
                params![message.id],
            )?;
            Ok(Begun { message, carried })
        })
        .transpose()
}

/// A message's delivery begins: it is what the paste is told by, with the
/// rows it carries, which are the ones still waiting now and no others.
pub(crate) fn begin_delivery(store: &mut Store, message_id: i64) -> Result<Begun, LedgerError> {
    store.write(|store| {
        let message = require_message(store, message_id, "queued")?;
        if message.recipient_role == "human" {
            return Err(LedgerError::refused_with(
                "human-reads-in-app",
                "the human reads messages in the app",
                409,
            ));
        }
        let (carrier, claimed): (Option<i64>, Option<String>) = store.db.query_row(
            "SELECT carried_by, claimed_at FROM message WHERE id = ?",
            [message_id],
            |row| Ok((row.get("carried_by")?, row.get("claimed_at")?)),
        )?;
        if let Some(carrier) = carrier {
            return Err(LedgerError::refused_with(
                "invalid-transition",
                format!("message {message_id} rides in the paste of m-{carrier}"),
                409,
            ));
        }
        if claimed.is_some() {
            return Err(LedgerError::refused_with(
                "invalid-transition",
                format!("message {message_id} is claimed by the door that asked for it"),
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
        Ok(Begun {
            message: known_message(store, message_id)?,
            carried: carried(store, message_id)?,
        })
    })
}

/// The harness's own record proves the message arrived: `receipt` is what it
/// showed. What its paste carried arrived with it, in the same step; then
/// the task it is about is reconciled once.
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
        let rode: Vec<i64> = carried(store, message_id)?
            .into_iter()
            .map(|row| row.id)
            .collect();
        store.db.execute(
            "UPDATE message SET state = 'delivered', delivered_at = ?, receipt = ?
         WHERE carried_by = ? AND state = 'queued'",
            params![at, json!({ "carrier": message_id }).to_string(), message_id],
        )?;
        let mut data = json!({ "message": message_id });
        if !rode.is_empty() {
            data["carried"] = json!(rode);
        }
        store.log(message.project_id, "delivery.confirmed", data)?;
        if let Some(task) = message_task(store, message_id)? {
            reconcile(store, task.id)?;
        }
        known_message(store, message_id)
    })
}

/// A message not yet delivered that no longer applies: it will not be
/// delivered, and what it carried is its own again.
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
        if let Some(task) = message_task(store, message_id)? {
            release_carried(store, task.id)?;
        }
        known_message(store, message_id)
    })
}

/// Queued again; `refund` gives back the attempt when the window went before
/// it could land. It joins the task message that waits for its window, if one
/// does: a resume that came while it was on its way.
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
        adopt(store, message_id)?;
        known_message(store, message_id)
    })
}

/// A delivery given up: a task it carried, still queued, fails with it, and
/// what it carried is its own again, for the reopening to take.
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
            release_carried(store, task.id)?;
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

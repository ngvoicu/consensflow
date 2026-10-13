//! When a wake-up is due, and the queuing of it. Nothing of this is kept
//! apart: it is read from the log, so a restart forgets nothing and a late
//! pass sends one wake-up, never a burst.
//!
//! A wake-up is due once the interval has passed since the latest of the last
//! wake-up (its delivery, when it was delivered), the last Resume and the last
//! change of the setting, and only if
//!
//! - the project is open and its calls are on;
//! - none is still on its way (a wake-up queued or being delivered is the
//!   only one: they never stack, whatever the clock says);
//! - and, unless it is the first since the calls were turned on or the project
//!   resumed, something happened since the last one: a task moved, a message
//!   was sent, or the human typed to the chief in its own window. A quiet
//!   board alone never skips a wake-up: the first after an enable or a Resume
//!   always goes.

use cf_base::time;
use cf_proto::ledger::MessageView;
use rusqlite::{params, OptionalExtension};
use serde_json::json;

use super::setting;
use crate::conversations::typed_since;
use crate::messages::cancel_message;
use crate::model::LedgerError;
use crate::queue::{send, Sent};
use crate::store::Store;

/// The wake-up queued last, as the log has it.
struct Last {
    /// Its `project.wake_up` event.
    event: i64,
    /// When it was queued.
    at: String,
    /// The note it queued.
    message: i64,
}

/// The newest wake-up queued for the project; none before the first.
fn newest(store: &Store, project_id: i64) -> Result<Option<Last>, LedgerError> {
    Ok(store
        .db
        .query_row(
            "SELECT id, at, json_extract(data, '$.message') AS message FROM event
             WHERE project_id = ? AND kind = 'project.wake_up' ORDER BY id DESC LIMIT 1",
            [project_id],
            |row| {
                Ok(Last {
                    event: row.get("id")?,
                    at: row.get("at")?,
                    message: row.get("message")?,
                })
            },
        )
        .optional()?)
}

/// The state of the note a wake-up queued.
fn state_of(store: &Store, last: &Last) -> Result<Option<String>, LedgerError> {
    Ok(store
        .db
        .query_row(
            "SELECT state FROM message WHERE id = ?",
            [last.message],
            |row| row.get(0),
        )
        .optional()?)
}

/// Whether a wake-up is on its way: queued, or being delivered.
fn on_its_way(store: &Store, last: &Last) -> Result<bool, LedgerError> {
    Ok(matches!(
        state_of(store, last)?.as_deref(),
        Some("queued" | "delivering")
    ))
}

/// What the interval counts from, in milliseconds: the latest of the last
/// wake-up's queuing and delivery, the last Resume and the last change of the
/// setting; the project's creation where the log holds none of them.
fn origin_ms(
    store: &Store,
    project_id: i64,
    last: Option<&Last>,
    created_at: &str,
) -> Result<i64, LedgerError> {
    let logged: Option<String> = store.db.query_row(
        "SELECT MAX(at) FROM event WHERE project_id = ?1
           AND (kind IN ('project.wake_up', 'project.wake_up_setting')
                OR (kind = 'project.state' AND json_extract(data, '$.to') = 'open'))",
        [project_id],
        |row| row.get(0),
    )?;
    let delivered: Option<String> = match last {
        Some(last) => store
            .db
            .query_row(
                "SELECT delivered_at FROM message WHERE id = ?",
                [last.message],
                |row| row.get(0),
            )
            .optional()?
            .flatten(),
        None => None,
    };
    Ok([logged, delivered]
        .into_iter()
        .flatten()
        .filter_map(|at| time::parse(&at))
        .max()
        .or_else(|| time::parse(created_at))
        .unwrap_or(0))
}

/// Whether the calls were turned on, or the project resumed, since `last`.
fn begun_since(store: &Store, project_id: i64, last: &Last) -> Result<bool, LedgerError> {
    Ok(store
        .db
        .prepare(
            "SELECT 1 FROM event WHERE project_id = ?1 AND id > ?2
               AND ((kind = 'project.state' AND json_extract(data, '$.to') = 'open')
                    OR (kind = 'project.wake_up_setting'
                        AND json_extract(data, '$.from.on') = 0
                        AND json_extract(data, '$.to.on') = 1))",
        )?
        .exists(params![project_id, last.event])?)
}

/// Whether something happened since `last` that the chief may not have seen:
/// a task moved or someone else sent a message (the wake-up's own note was
/// sent before it was logged, and its delivery is another kind of event; what
/// the chief sent is its own answer, as a "nothing left" to the human would
/// be), or the human typed to the chief in its window.
fn changed_since(store: &Store, project_id: i64, last: &Last) -> Result<bool, LedgerError> {
    let moved = store
        .db
        .prepare(
            "SELECT 1 FROM event WHERE project_id = ?1 AND id > ?2
               AND (kind LIKE 'task.%'
                    OR (kind = 'message.sent'
                        AND json_extract(data, '$.from') IS NOT 'chief'))",
        )?
        .exists(params![project_id, last.event])?;
    if moved {
        return Ok(true);
    }
    let chief = store.participant_by_handle(project_id, "chief")?;
    typed_since(store, chief.id, &last.at, last.message)
}

/// Queues the project's wake-up if it is due at `now_ms`, and logs it: the
/// note, or none when it is not due. Whether the chief is at rest is the
/// engine's to say before this is asked.
pub(crate) fn queue_if_due(
    store: &mut Store,
    project_id: i64,
    now_ms: i64,
) -> Result<Option<MessageView>, LedgerError> {
    let project = store.project_row(project_id)?;
    let wake_up = setting(store, project_id)?;
    if project.state != "open" || !wake_up.on {
        return Ok(None);
    }
    let last = newest(store, project_id)?;
    if let Some(last) = &last {
        if on_its_way(store, last)? {
            return Ok(None);
        }
    }
    let origin = origin_ms(store, project_id, last.as_ref(), &project.created_at)?;
    if now_ms < origin.saturating_add(wake_up.every_ms) {
        return Ok(None);
    }
    if let Some(last) = &last {
        if !begun_since(store, project_id, last)? && !changed_since(store, project_id, last)? {
            return Ok(None);
        }
    }
    store
        .write(|store| {
            let note = send(
                store,
                project_id,
                &Sent {
                    from: Some("human"),
                    to: "chief",
                    body: wake_up.words(),
                    kind: "note",
                    ..Sent::default()
                },
            )?;
            store.log(project_id, "project.wake_up", json!({ "message": note.id }))?;
            Ok(note)
        })
        .map(Some)
}

/// The wake-up that waits for its turn is withdrawn: it is never delivered. One
/// already being delivered is the window's to settle.
pub(crate) fn withdraw_queued(
    store: &mut Store,
    project_id: i64,
    reason: &str,
) -> Result<(), LedgerError> {
    let Some(last) = newest(store, project_id)? else {
        return Ok(());
    };
    if state_of(store, &last)?.as_deref() == Some("queued") {
        cancel_message(store, last.message, reason)?;
    }
    Ok(())
}

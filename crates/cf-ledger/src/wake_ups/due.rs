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
//! - the chief has nothing else on its way (`work_on_its_way`): no task is
//!   queued for its window or working; none waits for an answer that is on its
//!   way to the window; none is held for its member's quota, which it goes on
//!   from by itself; and none is open behind one of these, however long the
//!   chain of open tasks. What waits for the chief is the chief's own work and
//!   a wake-up goes for it: a task done and waiting for its decision, a
//!   question it has not answered (every question on the board is put to the
//!   chief), a task it holds itself (`cf task add --self`), a task someone
//!   paused or whose hold the daemon ended, a task open behind one of these or
//!   behind one that failed or was called off, and a board with nothing on it. One that is held back for work is skipped, not
//!   owed: the skip is logged (`project.wake_up_skipped`) and the interval
//!   counts again from it, so the next comes an interval later, on the same
//!   rule, and not the moment the board clears;
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
use crate::model::{sql_list, LedgerError};
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
/// wake-up's queuing and delivery or skip, the last Resume and the last change
/// of the setting; the project's creation where the log holds none of them.
fn origin_ms(
    store: &Store,
    project_id: i64,
    last: Option<&Last>,
    created_at: &str,
) -> Result<i64, LedgerError> {
    let logged: Option<String> = store.db.query_row(
        "SELECT MAX(at) FROM event WHERE project_id = ?1
           AND (kind IN ('project.wake_up', 'project.wake_up_skipped', 'project.wake_up_setting')
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

/// The states of a task that is with its window: handed to it, or at work.
const WITH_ITS_WINDOW: [&str; 2] = ["queued", "working"];

/// The numbers of the tasks the chief has on their way, oldest first: those with
/// their window (queued or working); those waiting for an answer that is on its
/// way to the window, since a task reads `waiting` until the answer is received
/// and, with no answer given, the question is the chief's to answer; those held
/// for their member's quota (`paused` with a time it goes on by itself, which
/// the daemon clears when it cannot; not one held until a time in the year 9999,
/// which an account unavailable or a model moved is held until: it waits for the
/// human, and the chief is woken for it, T-816); and, down any chain, those open behind a
/// task that is itself on its way (a task open behind one that is done and
/// undecided, failed or called off waits for the chief, and the recursion ends
/// where the tasks do: nothing is circular). A task the chief holds itself is
/// none of these, whatever its state: the chief's turn can end in the middle of
/// it, and nobody else has it to go on with, so it is the chief's own work.
fn work_on_its_way(store: &Store, project_id: i64) -> Result<Vec<i64>, LedgerError> {
    let with_its_window = sql_list(&WITH_ITS_WINDOW);
    Ok(store
        .db
        .prepare(&format!(
            "WITH RECURSIVE on_its_way(id) AS (
               SELECT t.id FROM task t WHERE t.project_id = ?1
                 AND (t.state IN ({with_its_window})
                      OR (t.state = 'waiting'
                          AND EXISTS (SELECT 1 FROM message a JOIN message q ON q.id = a.reply_to
                                      WHERE q.task_id = t.id AND q.kind = 'question'
                                        AND a.kind = 'answer'
                                        AND a.state IN ('gated', 'queued', 'delivering')))
                      OR (t.state = 'paused' AND t.held_until IS NOT NULL
                          AND t.held_until < '9999'))
                 AND NOT EXISTS (SELECT 1 FROM participant c
                                 WHERE c.id = t.assignee_id AND c.role = 'chief')
               UNION
               SELECT w.id FROM task w
                 JOIN task_need n ON n.task_id = w.id
                 JOIN on_its_way o ON o.id = n.needs_id
                 WHERE w.project_id = ?1 AND w.state = 'open'
             )
             SELECT t.number FROM task t JOIN on_its_way o ON o.id = t.id ORDER BY t.number"
        ))?
        .query_map([project_id], |row| row.get(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?)
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
    // A chief with work on its way is not asked whether it has any, the first
    // wake-up or the hundredth: the skip is logged and the interval counts from it.
    let tasks = work_on_its_way(store, project_id)?;
    if !tasks.is_empty() {
        store.write(|store| {
            store.log(
                project_id,
                "project.wake_up_skipped",
                json!({ "tasks": tasks }),
            )
        })?;
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

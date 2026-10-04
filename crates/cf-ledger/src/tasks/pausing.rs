//! A task stopped without ending: paused by the chief or the human, held by
//! the daemon while its member is out of quota, and resumed with the words
//! that send it on.

use cf_base::time;
use cf_proto::ledger::{HeldTask, TaskMoved, TaskThread, TaskView};
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Map, Value};

use super::{delivery_body, require_task_state, task, task_by_id};
use crate::model::{self, LedgerError, ACTIVE_TASK_STATES, MAX_BODY};
use crate::queue::{drop_queued, queue, Queued};
use crate::store::Store;
use crate::views::TaskRow;

/// The chief (or the human) stops a worker's task without ending it: its
/// agent is interrupted on the daemon's next look and its window stays for
/// the resumption, whatever was on its way to it is withdrawn, and the task
/// keeps its member, its conversation and its place until it is resumed or
/// cancelled. The chief's own work is not paused. `by` is none when
/// ConsensFlow pauses it, `because` says why when it is given.
pub(crate) fn pause_task(
    store: &mut Store,
    project_id: i64,
    number: i64,
    by: Option<&str>,
    because: Option<&str>,
) -> Result<TaskView, LedgerError> {
    if let Some(because) = because {
        model::require_text(because, "because", 1000)?;
    }
    store.write(|store| {
        if let Some(by) = by {
            store.participant_by_handle(project_id, by)?;
        }
        let task = store.task_row(project_id, number)?;
        let pausable = [&["open", "queued"], &ACTIVE_TASK_STATES[..]].concat();
        require_task_state(&task, &pausable, "pause")?;
        if let Some(assignee) = task.assignee_id {
            if store.participant_row(assignee)?.role == "chief" {
                return Err(LedgerError::refused_with(
                    "own-work",
                    format!("T-{number} is the chief's own: finish or cancel it"),
                    409,
                ));
            }
        }
        drop_queued(store, task.id)?;
        let mut detail = Map::new();
        detail.insert("by".into(), json!(by));
        if let Some(because) = because {
            detail.insert("because".into(), json!(because));
        }
        pause(store, &task, Value::Object(detail))?;
        task_by_id(store, task.id)
    })
}

/// The move to paused, its time kept as the task's last pause: a tell that
/// reaches the window from then on counts (`told_since_paused`).
fn pause(store: &mut Store, task: &TaskRow, detail: Value) -> Result<(), LedgerError> {
    store.move_task(task, "paused", detail)?;
    store.db.execute(
        "UPDATE task SET paused_at = updated_at WHERE id = ?",
        [task.id],
    )?;
    Ok(())
}

/// The daemon holds a task with its window while its member is out of
/// quota: paused, with the time it goes on by itself (`until`, an ISO
/// time). Its agent stops as any paused task's does, and its window waits
/// to go on at the reset.
pub(crate) fn hold_task(
    store: &mut Store,
    project_id: i64,
    number: i64,
    until: &str,
    because: &str,
) -> Result<TaskView, LedgerError> {
    model::require_text(because, "because", 1000)?;
    if time::parse(until).is_none() {
        return Err(LedgerError::refused(
            "invalid-until",
            "a hold names when it ends",
        ));
    }
    store.write(|store| {
        let task = store.task_row(project_id, number)?;
        let holdable = [&["queued"], &ACTIVE_TASK_STATES[..]].concat();
        require_task_state(&task, &holdable, "hold")?;
        drop_queued(store, task.id)?;
        pause(
            store,
            &task,
            json!({ "by": null, "because": because, "until": until }),
        )?;
        store.db.execute(
            "UPDATE task SET held_until = ? WHERE id = ?",
            params![until, task.id],
        )?;
        task_by_id(store, task.id)
    })
}

/// The held tasks whose time has come at `now` (an ISO time), oldest first.
pub(crate) fn held_tasks_due(store: &Store, now: &str) -> Result<Vec<HeldTask>, LedgerError> {
    Ok(store
        .db
        .prepare(
            "SELECT project_id, number, assignee_id FROM task
       WHERE state = 'paused' AND held_until IS NOT NULL AND held_until <= ? ORDER BY id",
        )?
        .query_map([now], |row| {
            Ok(HeldTask {
                project_id: row.get("project_id")?,
                number: row.get("number")?,
                assignee_id: row.get("assignee_id")?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?)
}

/// The paused task a participant still holds, or none.
pub(crate) fn paused_task(
    store: &Store,
    participant_id: i64,
) -> Result<Option<TaskThread>, LedgerError> {
    let found = store
        .db
        .query_row(
            "SELECT project_id, number FROM task WHERE assignee_id = ? AND state = 'paused'
       ORDER BY id LIMIT 1",
            [participant_id],
            |row| {
                Ok((
                    row.get::<_, i64>("project_id")?,
                    row.get::<_, i64>("number")?,
                ))
            },
        )
        .optional()?;
    match found {
        Some((project_id, number)) => task(store, project_id, number),
        None => Ok(None),
    }
}

/// Whether a tell for this task has reached the participant's window since
/// the task was last paused.
pub(crate) fn told_since_paused(
    store: &Store,
    participant_id: i64,
    task_id: i64,
) -> Result<bool, LedgerError> {
    Ok(store
        .db
        .prepare(
            "SELECT 1 FROM message q JOIN task t ON t.id = q.task_id
           WHERE q.recipient_id = ? AND q.task_id = ?
             AND q.kind = 'question' AND q.urgent = 1
             AND q.state IN ('delivering', 'delivered', 'read')
             AND q.created_at >= t.paused_at",
        )?
        .exists(params![participant_id, task_id])?)
}

/// A paused task goes on with the words that resume it: into the same
/// window when its session is still there (a brief never delivered goes in
/// first), or back on the board for its tier when the session has ended.
/// `by` is none when the daemon resumes a held task in its own name.
pub(crate) fn resume_task(
    store: &mut Store,
    project_id: i64,
    number: i64,
    by: Option<&str>,
    body: &str,
) -> Result<TaskMoved, LedgerError> {
    model::require_text(body, "body", MAX_BODY)?;
    store.write(|store| {
        let author = by
            .map(|by| store.participant_by_handle(project_id, by))
            .transpose()?;
        let task = store.task_row(project_id, number)?;
        require_task_state(&task, &["paused"], "resume")?;
        let assignee = task
            .assignee_id
            .map(|id| store.participant_row(id))
            .transpose()?;
        let Some(assignee) = assignee else {
            let at = store.at();
            store.db.execute(
                "UPDATE task SET body = ?, updated_at = ? WHERE id = ?",
                params![format!("{}\n\nResumed: {body}", task.body), at, task.id],
            )?;
            store.move_task(&task, "open", json!({ "by": by }))?;
            return Ok(TaskMoved {
                task: task_by_id(store, task.id)?,
                message: None,
            });
        };
        if assignee.left_at.is_some() {
            if task.pool.is_none() {
                return Err(LedgerError::refused_with(
                    "session-ended",
                    format!(
                        "the window that had T-{number} has ended: cancel it and open the work for its tier"
                    ),
                    409,
                ));
            }
            let at = store.at();
            store.db.execute(
                "UPDATE task SET assignee_id = NULL, state = 'open', body = ?, updated_at = ?
           WHERE id = ?",
                params![
                    format!(
                        "{}\n\nResumed after a pause, in a fresh window (the one that had it ended; check the working tree for partial changes): {body}",
                        task.body
                    ),
                    at,
                    task.id
                ],
            )?;
            store.log(
                project_id,
                "task.state",
                json!({ "task": number, "from": "paused", "to": "open", "by": by }),
            )?;
            return Ok(TaskMoved {
                task: task_by_id(store, task.id)?,
                message: None,
            });
        }
        let delivered = store
            .db
            .prepare(
                "SELECT 1 FROM message WHERE task_id = ? AND kind = 'task' AND recipient_id = ?
           AND state IN ('delivered', 'read')",
            )?
            .exists(params![task.id, assignee.id])?;
        let words = if delivered {
            format!("Resumed: {body}")
        } else {
            format!("{}\n\nResumed: {body}", delivery_body(&task))
        };
        let message_id = queue(
            store,
            project_id,
            &Queued {
                to: assignee.id,
                from: author.as_ref().map(|author| author.id),
                kind: "task",
                task_id: Some(task.id),
                body: &words,
                ..Queued::default()
            },
        )?;
        store.move_task(
            &task,
            "queued",
            json!({ "by": by, "message": message_id }),
        )?;
        Ok(TaskMoved {
            task: task_by_id(store, task.id)?,
            message: store.message(message_id)?,
        })
    })
}

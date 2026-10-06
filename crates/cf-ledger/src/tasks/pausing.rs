//! A task stopped without ending: paused by the chief or the human, held by
//! the daemon while its member is out of quota, and resumed with the words
//! that send it on. A pause stops the window and keeps what it was kept
//! for it: nothing is cancelled for it but what the chief wrote and now
//! takes back. What the window keeps goes in with the words that resume it.

use cf_base::time;
use cf_proto::ledger::{HeldTask, Stop, TaskMoved, TaskThread, TaskView};
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Map, Value};

use super::{
    carrier_body, delivery_body, m_list, require_task_state, task, task_by_id, task_found,
};
use crate::messages::{fold, release_carried, transfer};
use crate::model::{self, LedgerError, ACTIVE_TASK_STATES, MAX_BODY};
use crate::queue::{queue, send, Queued, Sent};
use crate::store::Store;
use crate::views::TaskRow;

/// The chief (or the human) stops a worker's task without ending it: its
/// agent is interrupted on the daemon's next look and its window stays for
/// the resumption, and the task keeps its member, its conversation, its place
/// and what was kept for it until it is resumed or cancelled. What the
/// chief wrote for the window (its words and notes, not its answers) and had
/// not yet reached it is taken back, for the chief's resume words carry what
/// is new. The chief's own work is not paused. `by` is none when ConsensFlow
/// pauses it, `because` says why when it is given.
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
        let author = by
            .map(|by| store.participant_by_handle(project_id, by))
            .transpose()?;
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
        if let (Some(author), Some(window)) = (
            author.filter(|author| author.role == "chief"),
            task.assignee_id,
        ) {
            store.db.execute(
                "UPDATE message SET state = 'cancelled', reason = ?
                 WHERE task_id = ? AND recipient_id = ? AND sender_id = ?
                   AND kind IN ('task', 'note') AND state IN ('queued', 'gated')",
                params![
                    format!("withdrawn by @{}'s pause", author.handle),
                    task.id,
                    window,
                    author.id
                ],
            )?;
            release_carried(store, task.id)?;
        }
        let mut detail = Map::new();
        detail.insert("by".into(), json!(by));
        if let Some(because) = because {
            detail.insert("because".into(), json!(because));
        }
        pause(store, &task, detail)?;
        task_by_id(store, task.id)
    })
}

/// The move to paused, which is a stop of its own: the task counts it, so two
/// pauses in one millisecond are two. Its time is kept as the task's last
/// pause (a tell that reaches the window from then on counts,
/// `told_since_paused`). Its window's questions have their doors shut before
/// anything presses a key, and what a door claimed and nobody acknowledged
/// is the ledger's again.
fn pause(
    store: &mut Store,
    task: &TaskRow,
    mut detail: Map<String, Value>,
) -> Result<(), LedgerError> {
    detail.insert("stop".into(), json!(task.stop_seq + 1));
    store.move_task(task, "paused", Value::Object(detail))?;
    store.db.execute(
        "UPDATE task SET paused_at = updated_at, stop_seq = stop_seq + 1 WHERE id = ?",
        [task.id],
    )?;
    let Some(window) = task.assignee_id else {
        return Ok(());
    };
    store.db.execute(
        "UPDATE message SET door_closed_at = (SELECT updated_at FROM task WHERE id = ?1)
         WHERE task_id = ?1 AND sender_id = ?2 AND kind = 'question' AND door_closed_at IS NULL",
        params![task.id, window],
    )?;
    let claimed: Vec<i64> = store
        .db
        .prepare(
            "SELECT id FROM message WHERE task_id = ? AND recipient_id = ? AND kind = 'answer'
               AND claimed_at IS NOT NULL ORDER BY id",
        )?
        .query_map(params![task.id, window], |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    for answer in claimed {
        store.db.execute(
            "UPDATE message SET claimed_at = NULL WHERE id = ?",
            [answer],
        )?;
        store.log(
            task.project_id,
            "delivery.unclaimed",
            json!({ "message": answer, "because": format!("T-{} was paused", task.number) }),
        )?;
    }
    Ok(())
}

/// The daemon holds a task with its window while its member is out of
/// quota: paused, with the time it goes on by itself (`until`, an ISO
/// time). Its agent stops as any paused task's does, its window waits to go
/// on at the reset, and what was on its way to it waits too: the daemon
/// resumes it with fixed words, and the chief's own are what is kept.
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
        let mut detail = Map::new();
        detail.insert("by".into(), Value::Null);
        detail.insert("because".into(), json!(because));
        detail.insert("until".into(), json!(until));
        pause(store, &task, detail)?;
        store.db.execute(
            "UPDATE task SET held_until = ? WHERE id = ?",
            params![until, task.id],
        )?;
        task_by_id(store, task.id)
    })
}

/// The daemon ends a task's hold without resuming it, when it cannot go on at
/// its time: the task stays paused, as the chief's or the human's pause leaves
/// it, and is not due again. It goes on only when one of them resumes it, or
/// is called off.
pub(crate) fn clear_hold(
    store: &mut Store,
    project_id: i64,
    number: i64,
    because: &str,
) -> Result<TaskView, LedgerError> {
    model::require_text(because, "because", 1000)?;
    store.write(|store| {
        let task = store.task_row(project_id, number)?;
        require_task_state(&task, &["paused"], "clear the hold of")?;
        let at = store.at();
        store.db.execute(
            "UPDATE task SET held_until = NULL, updated_at = ? WHERE id = ?",
            params![at, task.id],
        )?;
        store.log(
            project_id,
            "task.hold-cleared",
            json!({ "task": number, "because": because }),
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

/// The stops asked of a window's task: its task, and how many stops its
/// pauses asked of it. The task is the one it holds, a paused one first; a
/// window that holds none is asked for none. What a window owes is what this
/// counts past the last stop it paid for that task.
pub(crate) fn stop_of(store: &Store, participant_id: i64) -> Result<Option<Stop>, LedgerError> {
    Ok(store
        .db
        .query_row(
            "SELECT id, number, stop_seq FROM task
             WHERE assignee_id = ? AND state IN ('queued', 'working', 'waiting', 'paused')
             ORDER BY state = 'paused' DESC, stop_seq DESC, id LIMIT 1",
            [participant_id],
            |row| {
                Ok(Stop {
                    task_id: row.get("id")?,
                    number: row.get("number")?,
                    seq: row.get("stop_seq")?,
                })
            },
        )
        .optional()?)
}

/// The task a participant's window is on as far as a question or a note it
/// sends goes: the one it holds (queued, working or waiting), else the paused
/// one. A question asked in a window that the pause has not yet stopped is
/// still about its task.
pub(crate) fn task_in_hand(
    store: &Store,
    participant_id: i64,
) -> Result<Option<TaskThread>, LedgerError> {
    task_found(
        store,
        "SELECT project_id, number FROM task
       WHERE assignee_id = ? AND state IN ('queued', 'working', 'waiting', 'paused')
       ORDER BY state = 'paused', id LIMIT 1",
        participant_id,
    )
}

/// The paused task a participant still holds, or none. Node's ledger asks it
/// of each look at a window; this engine asks the task's stops (`stop_of`)
/// instead, and it stays for the recordings that call it.
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
/// the task was last paused. Kept as `paused_task` is.
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

/// A paused task goes on with the words that resume it:
/// - into the same window when its session is still there: the words are a
///   task message of their own, which carries what the window kept, and the
///   task's brief first when none is received or on its way;
/// - back on the board for its tier when the session has ended, with what
///   the window kept in its brief, once; a task given by name has no other
///   session to go to, and is refused.
///
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
            let carried = transfer(store, &task, &assignee, &delivery_body(&task))?;
            let at = store.at();
            store.db.execute(
                "UPDATE task SET assignee_id = NULL, state = 'open', body = ?, updated_at = ?
           WHERE id = ?",
                params![
                    format!(
                        "{}{}\n\nResumed after a pause, in a fresh window (the one that had it ended; check the working tree for partial changes): {body}",
                        task.body, carried.kept
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
            if !carried.gated.is_empty() {
                let to = match &author {
                    Some(author) => author.handle.clone(),
                    None => store.participant_row(task.requester_id)?.handle,
                };
                send(
                    store,
                    project_id,
                    &Sent {
                        to: &to,
                        task: Some(number),
                        kind: "note",
                        body: &format!(
                            "Withdrawn with it, still waiting for the human: {}.",
                            m_list(&carried.gated)
                        ),
                        ..Sent::default()
                    },
                )?;
            }
            return Ok(TaskMoved {
                task: task_by_id(store, task.id)?,
                message: None,
            });
        }
        let words = carrier_body(store, &task, assignee.id, &format!("Resumed: {body}"))?;
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
        fold(store, &task, assignee.id, message_id)?;
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

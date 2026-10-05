//! A task finished: its result recorded and accepted, or sent back; called
//! off by a coordinator, or given up by the daemon; and taken off the board
//! for good by the human once finished.

use cf_proto::ledger::{TaskMoved, TaskView};
use rusqlite::params;
use serde_json::json;

use super::{delivery_body, require_on_board, require_task_state, task_by_id};
use crate::model::{
    self, require_active, sql_list, LedgerError, ACTIVE_TASK_STATES, FINISHED_TASK_STATES,
    MAX_BODY, MEMBER_ROLES,
};
use crate::queue::{drop_queued, queue, send, withdraw_gated, Queued, Sent};
use crate::staff::{bring_back, can_continue};
use crate::store::Store;
use crate::views::{ParticipantRow, TaskRow};

/// The assignee's answer finishes the task and is queued for whoever asked for it.
pub(crate) fn record_result(
    store: &mut Store,
    project_id: i64,
    number: i64,
    body: &str,
) -> Result<TaskMoved, LedgerError> {
    model::require_text(body, "body", MAX_BODY)?;
    store.write(|store| {
        let task = store.task_row(project_id, number)?;
        require_task_state(&task, &ACTIVE_TASK_STATES, "record a result for")?;
        let message_id = queue(
            store,
            project_id,
            &Queued {
                to: task.requester_id,
                from: task.assignee_id,
                kind: "result",
                task_id: Some(task.id),
                body,
                ..Queued::default()
            },
        )?;
        store.move_task(&task, "done", json!({ "result": message_id }))?;
        Ok(TaskMoved {
            task: task_by_id(store, task.id)?,
            message: store.message(message_id)?,
        })
    })
}

/// A coordinator accepts a task's result: the task is finished, and the
/// tasks given by name that waited for it go to their windows.
pub(crate) fn accept_task(
    store: &mut Store,
    project_id: i64,
    number: i64,
    by: &str,
) -> Result<TaskView, LedgerError> {
    store.write(|store| {
        store.participant_by_handle(project_id, by)?;
        let task = store.task_row(project_id, number)?;
        require_task_state(&task, &["done"], "accept")?;
        require_result_received(store, &task, by)?;
        withdraw_gated(store, task.id, &format!("accepted by @{by}"))?;
        // What is still on its way to the member (an answer that came after its
        // result, say) has no window left to take it.
        if let Some(assignee) = task.assignee_id {
            let member = store.participant_row(assignee)?;
            store.db.execute(
                "UPDATE message SET state = 'cancelled', reason = ?
           WHERE task_id = ? AND recipient_id = ? AND state = 'queued'",
                params![
                    format!(
                        "T-{} was accepted before it reached @{}",
                        task.number, member.handle
                    ),
                    task.id,
                    member.id
                ],
            )?;
        }
        store.move_task(&task, "accepted", json!({ "by": by }))?;
        release_waiting(store, project_id, task.id)?;
        task_by_id(store, task.id)
    })
}

/// A result still waiting for the human's approval has not reached the
/// chief: deciding it is the human's alone, who may accept it or send it back.
fn require_result_received(store: &Store, task: &TaskRow, by: &str) -> Result<(), LedgerError> {
    if by == "human" {
        return Ok(());
    }
    let held = store
        .db
        .prepare("SELECT 1 FROM message WHERE task_id = ? AND kind = 'result' AND state = 'gated'")?
        .exists([task.id])?;
    if held {
        return Err(LedgerError::refused_with(
            "result-gated",
            format!(
                "T-{}'s result waits for the human's approval: it reaches you once they pass it on",
                task.number
            ),
            409,
        ));
    }
    Ok(())
}

/// The tasks given by name that waited on the board for the one just
/// accepted: each with nothing left to wait for goes to its window now.
fn release_waiting(
    store: &mut Store,
    project_id: i64,
    accepted_id: i64,
) -> Result<(), LedgerError> {
    let waiting = store
        .db
        .prepare(
            "SELECT t.* FROM task t JOIN task_need n ON n.task_id = t.id
       WHERE n.needs_id = ? AND t.state = 'open' AND t.assignee_id IS NOT NULL ORDER BY t.id",
        )?
        .query_map([accepted_id], TaskRow::read)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for task in waiting {
        let still = store
            .db
            .prepare(
                "SELECT 1 FROM task_need n JOIN task d ON d.id = n.needs_id
         WHERE n.task_id = ? AND d.state != 'accepted'",
            )?
            .exists([task.id])?;
        if still {
            continue;
        }
        let body = delivery_body(&task);
        let message_id = queue(
            store,
            project_id,
            &Queued {
                to: task.assignee_id.unwrap_or_default(),
                from: Some(task.requester_id),
                kind: "task",
                task_id: Some(task.id),
                body: &body,
                ..Queued::default()
            },
        )?;
        store.move_task(&task, "queued", json!({ "message": message_id }))?;
    }
    Ok(())
}

/// A follow-up on a finished or failed task: it goes back to its assignee's
/// queue, and a session the human deleted is brought back to the board for it.
pub(crate) fn reopen_task(
    store: &mut Store,
    project_id: i64,
    number: i64,
    by: &str,
    body: &str,
) -> Result<TaskMoved, LedgerError> {
    model::require_text(body, "body", MAX_BODY)?;
    store.write(|store| {
        let author = store.participant_by_handle(project_id, by)?;
        let task = store.task_row(project_id, number)?;
        require_task_state(&task, &["done", "failed"], "reopen")?;
        require_result_received(store, &task, by)?;
        let found = store.participant_of(task.assignee_id)?;
        if found.member_id.is_some() && !can_continue(store, &found)? {
            return Err(LedgerError::refused_with(
                "session-ended",
                format!(
                    "@{} has ended: open the task for its tier instead",
                    found.handle
                ),
                409,
            ));
        }
        let assignee = match found.member_id {
            Some(_) => bring_back(store, found)?,
            None => found,
        };
        require_active(&assignee)?;
        withdraw_gated(store, task.id, &format!("sent back by @{by}"))?;
        let message_id = queue(
            store,
            project_id,
            &Queued {
                to: assignee.id,
                from: Some(author.id),
                kind: "task",
                task_id: Some(task.id),
                body,
                ..Queued::default()
            },
        )?;
        store.move_task(&task, "queued", json!({ "by": by, "message": message_id }))?;
        Ok(TaskMoved {
            task: task_by_id(store, task.id)?,
            message: store.message(message_id)?,
        })
    })
}

/// A task is called off, and whoever asked for it hears so in the same
/// step, unless it cancelled the task itself: what was cancelled, and the
/// window that was stopped when a member's had begun on it (the daemon
/// stops that window on its next look).
pub(crate) fn cancel_task(
    store: &mut Store,
    project_id: i64,
    number: i64,
    by: &str,
) -> Result<TaskView, LedgerError> {
    store.write(|store| {
        let (task, window) = call_off(store, project_id, number, by)?;
        let requester = store.participant_row(task.requester_id)?;
        if requester.handle != by {
            let stopped = window.map_or_else(String::new, |window| {
                format!(": @{}'s window was stopped", window.handle)
            });
            send(
                store,
                project_id,
                &Sent {
                    to: &requester.handle,
                    task: Some(number),
                    kind: "note",
                    body: &format!("@{by} cancelled T-{number} ({}){stopped}.", task.title),
                    ..Sent::default()
                },
            )?;
        }
        task_by_id(store, task.id)
    })
}

/// The cancel itself, for an operation that tells the requester in its own
/// words (the human's decline of a brief). Whatever of the task is still on
/// its way, a delivery into a window included, is withdrawn, so nothing of
/// it is delivered or tried again. The task as it was, and the member's
/// window that had begun on it, if one had: its brief or the words that
/// resume it went in.
pub(crate) fn call_off(
    store: &mut Store,
    project_id: i64,
    number: i64,
    by: &str,
) -> Result<(TaskRow, Option<ParticipantRow>), LedgerError> {
    store.participant_by_handle(project_id, by)?;
    let task = store.task_row(project_id, number)?;
    let cancellable = [&["open", "queued"], &ACTIVE_TASK_STATES[..], &["paused"]].concat();
    require_task_state(&task, &cancellable, "cancel")?;
    let assignee = task
        .assignee_id
        .map(|id| store.participant_row(id))
        .transpose()?;
    let began = match &assignee {
        Some(assignee) if MEMBER_ROLES.contains(&assignee.role.as_str()) => store
            .db
            .prepare(
                "SELECT 1 FROM message WHERE task_id = ? AND recipient_id = ? AND kind = 'task'
           AND state IN ('delivering', 'delivered')",
            )?
            .exists(params![task.id, assignee.id])?,
        _ => false,
    };
    store.db.execute(
        "UPDATE message SET state = 'cancelled', reason = ?
       WHERE task_id = ? AND state IN ('queued', 'delivering', 'gated')",
        params![format!("cancelled by @{by}"), task.id],
    )?;
    store.move_task(&task, "cancelled", json!({ "by": by }))?;
    Ok((task, assignee.filter(|_| began)))
}

/// The daemon gives up on a task: its pane died, or its launch never came up.
pub(crate) fn fail_task(
    store: &mut Store,
    project_id: i64,
    number: i64,
    reason: &str,
) -> Result<TaskView, LedgerError> {
    model::require_text(reason, "reason", 1000)?;
    store.write(|store| {
        let task = store.task_row(project_id, number)?;
        let failable = [&["open", "queued"], &ACTIVE_TASK_STATES[..]].concat();
        require_task_state(&task, &failable, "fail")?;
        drop_queued(store, task.id)?;
        store.move_task(&task, "failed", json!({ "reason": reason }))?;
        task_by_id(store, task.id)
    })
}

/// The human takes finished tasks off the board for good: each leaves the
/// board and `cf task list`, and keeps its row, so its number is never
/// given again, the threads that name it stay whole and `cf task get` still
/// reads it. Only a finished task goes, and none a task not yet finished
/// still needs; all of them go, or none. Nobody is told: the human tidies
/// the board.
pub(crate) fn delete_tasks(
    store: &mut Store,
    project_id: i64,
    numbers: &[i64],
) -> Result<Vec<TaskView>, LedgerError> {
    let finished = sql_list(&FINISHED_TASK_STATES);
    let mut distinct: Vec<i64> = Vec::new();
    for number in numbers {
        if !distinct.contains(number) {
            distinct.push(*number);
        }
    }
    store.write(|store| {
        distinct
            .into_iter()
            .map(|number| {
                let task = store.task_row(project_id, number)?;
                require_on_board(&task, "delete")?;
                if !FINISHED_TASK_STATES.contains(&task.state.as_str()) {
                    let next = if task.state == "done" {
                        "the chief accepts it or sends it back first"
                    } else {
                        "cancel it first"
                    };
                    return Err(LedgerError::refused_with(
                        "not-finished",
                        format!(
                            "T-{number} is {}: only a finished task leaves the board; {next}",
                            task.state
                        ),
                        409,
                    ));
                }
                let needing = store
                    .db
                    .prepare(&format!(
                        "SELECT w.number FROM task_need n JOIN task w ON w.id = n.task_id
           WHERE n.needs_id = ? AND w.state NOT IN ({finished}) ORDER BY w.number"
                    ))?
                    .query_map([task.id], |row| {
                        Ok(format!("T-{}", row.get::<_, i64>("number")?))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                if let [needed_by] = needing.as_slice() {
                    return Err(LedgerError::refused_with(
                        "task-needed",
                        format!(
                            "{needed_by} still needs T-{number}: it stays on the board until {needed_by} is finished"
                        ),
                        409,
                    ));
                }
                if !needing.is_empty() {
                    return Err(LedgerError::refused_with(
                        "task-needed",
                        format!(
                            "{} still need T-{number}: it stays on the board until they are finished",
                            needing.join(", ")
                        ),
                        409,
                    ));
                }
                let at = store.at();
                store.db.execute(
                    "UPDATE task SET deleted_at = ? WHERE id = ?",
                    params![at, task.id],
                )?;
                store.log(
                    project_id,
                    "task.deleted",
                    json!({ "task": number, "state": task.state }),
                )?;
                task_by_id(store, task.id)
            })
            .collect()
    })
}

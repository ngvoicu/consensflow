//! A task finished: its result recorded and accepted, or sent back; called
//! off by a coordinator, or given up by the daemon; and taken off the board
//! for good by the human once finished. A result is put to its requester to
//! decide on, so a decision taken before the requester was given it withdraws
//! it, in the decision's own transaction: accepting and sending back do
//! (`withdraw_result`). A cancel has no such step to take: a task is called
//! off only before it is done, and `call_off` withdraws every message of it
//! still on its way, a result an older build left among them.

use cf_proto::ledger::{TaskMoved, TaskView};
use rusqlite::params;
use serde_json::json;

use super::{carrier_body, delivery_body, require_on_board, require_task_state, task_by_id};
use crate::messages::{fold, leave_pause_notes, tell_sent_back};
use crate::model::{
    self, require_active, sql_list, LedgerError, ACTIVE_TASK_STATES, FINISHED_TASK_STATES,
    MAX_BODY, MEMBER_ROLES,
};
use crate::queue::{drop_queued, queue, send, withdraw_gated, withdraw_result, Queued, Sent};
use crate::staff::{bring_back, can_continue, has_task_in_hand, require_free, Giving};
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
        release_ready(store, project_id)?;
        Ok(TaskMoved {
            task: task_by_id(store, task.id)?,
            message: store.message(message_id)?,
        })
    })
}

/// A coordinator accepts a task's result: the task is finished, and the
/// tasks given by name that waited for it go to their windows. The result is
/// withdrawn if its requester has not been given it yet: it asks for the
/// decision just taken.
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
        withdraw_result(store, &task, "was accepted")?;
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
        release_ready(store, project_id)?;
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

/// The tasks given to a participant (by name, or as a follow-up) that wait on
/// the board for what they need go to their windows now: each with every task
/// it needs accepted, oldest first, unless its assignee is a member session
/// with a task in hand. That one waits for the session to be free, as it
/// waited for what it needs, and goes when the task in hand is over; of two
/// waiting for one session the first goes and the second waits behind it.
/// Called in the step of whatever made a release possible: an acceptance, or
/// the end of a session's task in hand: its result, its calling off, its
/// failing (`fail_task`, and a delivery that fails for good), and its being
/// taken back for its tier. It refuses nothing of its own, which would fail
/// that step for something else's sake. Every door gives a session one waiting
/// task at a time (`holds_work`), so a session is busy here only in a ledger
/// written before they did.
pub(crate) fn release_ready(store: &mut Store, project_id: i64) -> Result<(), LedgerError> {
    let waiting = store
        .db
        .prepare(
            "SELECT t.* FROM task t
       WHERE t.project_id = ? AND t.state = 'open' AND t.assignee_id IS NOT NULL
         AND EXISTS (SELECT 1 FROM task_need n WHERE n.task_id = t.id)
         AND NOT EXISTS (SELECT 1 FROM task_need n JOIN task d ON d.id = n.needs_id
                         WHERE n.task_id = t.id AND d.state != 'accepted')
       ORDER BY t.id",
        )?
        .query_map([project_id], TaskRow::read)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for task in waiting {
        let assignee = store.participant_of(task.assignee_id)?;
        if assignee.member_id.is_some() && has_task_in_hand(store, assignee.id)? {
            continue;
        }
        let body = delivery_body(&task);
        let message_id = queue(
            store,
            project_id,
            &Queued {
                to: assignee.id,
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
/// A session that is spoken for (`holds_work`: on another task, or with a
/// follow-up waiting for what it needs) takes none, and is refused as a
/// follow-up is, `session-busy`, in the words of a task sent back (it is not
/// opened for a tier; what it needs can be given as a new task): its window
/// has one task to work on. The follow-up is a task message that carries what
/// the window kept for the task (which is what delivers what a delivery that
/// failed let go of), and the brief before it when none is received or on its
/// way. A result its requester has not been given yet is withdrawn: it asks
/// for the decision just taken, and the work it reports is sent back.
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
        require_free(store, &found, Giving::SentBack(number))?;
        let assignee = match found.member_id {
            Some(_) => bring_back(store, found)?,
            None => found,
        };
        require_active(&assignee)?;
        withdraw_gated(store, task.id, &format!("sent back by @{by}"))?;
        withdraw_result(store, &task, "was sent back")?;
        let words = carrier_body(store, &task, assignee.id, body)?;
        let message_id = queue(
            store,
            project_id,
            &Queued {
                to: assignee.id,
                from: Some(author.id),
                kind: "task",
                task_id: Some(task.id),
                body: &words,
                ..Queued::default()
            },
        )?;
        fold(store, &task, assignee.id, message_id)?;
        store.move_task(&task, "queued", json!({ "by": by, "message": message_id }))?;
        tell_sent_back(store, &task, assignee.id, by)?;
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
    // A note of several that names it, still queued, names it no more.
    leave_pause_notes(store, &task, "cancelled")?;
    store.move_task(&task, "cancelled", json!({ "by": by }))?;
    release_ready(store, project_id)?;
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
        release_ready(store, project_id)?;
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

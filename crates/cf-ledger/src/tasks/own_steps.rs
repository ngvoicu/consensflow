//! The chief's own later step (`cf task add --self --needs T-3`): a task the
//! chief asked of itself, which waits on the board for what it needs and whose
//! brief then comes back to the chief's window. The chief finishes it by its
//! word, whenever it is at it (the field report of 2026-10-15, B4: `cf task
//! done` was refused while the brief was still on its way back, and the chief's
//! own result came back to it to accept). A task the human gives the chief is
//! no such step: its result is the human's to decide. What of a step was on its
//! way to the chief when it was finished goes no further once its paste is
//! refused or tried again ([`withdrawn_late`]).

use cf_proto::ledger::{MessageView, TaskMoved};
use rusqlite::params;
use serde_json::json;

use super::{release_ready, require_task_state, task_by_id, task_row_by_id};
use crate::messages::cancel_message;
use crate::model::{LedgerError, HELD_TASK_STATES};
use crate::queue::{queue, Queued};
use crate::store::Store;
use crate::views::TaskRow;

/// Whether `task` is the chief's own step: the chief asked for it and has it.
pub(super) fn is_own_step(store: &Store, task: &TaskRow) -> Result<bool, LedgerError> {
    if task.assignee_id != Some(task.requester_id) {
        return Ok(false);
    }
    Ok(store.participant_row(task.requester_id)?.role == "chief")
}

/// Why what of the step `task` had not reached its chief, `chief`, goes no further.
fn done_before(task: &TaskRow, chief: &str) -> String {
    format!("T-{} was done before it came back to @{chief}", task.number)
}

/// `message`, a task message for `task` whose paste was refused or is tried
/// again: when `task` is the chief's own step and was finished while the paste
/// was under way (the finish withdraws only what had not set out), it goes no
/// further, withdrawn in the finish's words, at the refusal or the retry,
/// whichever comes first (the second review, 2026-10-16: a retry queued the
/// brief again, and the chief's window was given it for a step accepted).
/// Whether it was withdrawn.
pub(crate) fn withdrawn_late(
    store: &mut Store,
    message: &MessageView,
    task: &TaskRow,
) -> Result<bool, LedgerError> {
    if message.kind != "task"
        || HELD_TASK_STATES.contains(&task.state.as_str())
        || !is_own_step(store, task)?
    {
        return Ok(false);
    }
    let chief = store.participant_row(task.requester_id)?;
    cancel_message(store, message.id, &done_before(task, &chief.handle))?;
    Ok(true)
}

/// What of the step `task` is still on its way to its chief, `chief`: the
/// messages a finish withdraws (queued for the chief or held at the gate: its
/// brief, and any answer or note carried on it) and, apart, its brief if it is
/// being typed in, which cannot be taken back (it goes no further if its paste
/// is given back, and may still arrive once). Their ids, oldest first.
fn on_their_way(
    store: &Store,
    task: &TaskRow,
    chief: i64,
) -> Result<(Vec<i64>, Vec<i64>), LedgerError> {
    let ids = |sql: &str| -> Result<Vec<i64>, LedgerError> {
        Ok(store
            .db
            .prepare(sql)?
            .query_map(params![task.id, chief], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?)
    };
    Ok((
        ids(
            "SELECT id FROM message WHERE task_id = ? AND recipient_id = ?
               AND state IN ('queued', 'gated') ORDER BY id",
        )?,
        ids(
            "SELECT id FROM message WHERE task_id = ? AND recipient_id = ?
               AND state = 'delivering' AND kind = 'task' ORDER BY id",
        )?,
    ))
}

/// The messages with these ids, as they stand.
fn views(store: &Store, ids: &[i64]) -> Result<Vec<MessageView>, LedgerError> {
    let mut found = Vec::new();
    for id in ids {
        found.extend(store.message(*id)?);
    }
    Ok(found)
}

/// The chief finishes its own step with `body`, queued for it (its brief on
/// its way back) or at work on it; one still waiting on the board for what it
/// needs is refused. In one step: what of the step had not reached the chief
/// yet (its brief, most often) is withdrawn and named, and so is its brief if
/// it is being typed in and may still land (`TaskMoved::withdrawn`, `landing`);
/// the words are its result, received as they are written, never pasted; and
/// the step is accepted, there being nobody else to decide it, so what needs it
/// goes on.
pub(super) fn finish(
    store: &mut Store,
    project_id: i64,
    task: &TaskRow,
    body: &str,
) -> Result<TaskMoved, LedgerError> {
    require_task_state(task, &HELD_TASK_STATES, "finish")?;
    let chief = store.participant_row(task.requester_id)?;
    let (withdrawn, landing) = on_their_way(store, task, chief.id)?;
    store.db.execute(
        "UPDATE message SET state = 'cancelled', reason = ?
         WHERE task_id = ? AND recipient_id = ? AND state IN ('queued', 'gated')",
        params![done_before(task, &chief.handle), task.id, chief.id],
    )?;
    let message_id = queue(
        store,
        project_id,
        &Queued {
            to: task.requester_id,
            from: task.assignee_id,
            kind: "result",
            task_id: Some(task.id),
            body,
            from_window: true,
            ..Queued::default()
        },
    )?;
    store.move_task(task, "done", json!({ "result": message_id }))?;
    let done = task_row_by_id(store, task.id)?;
    store.move_task(
        &done,
        "accepted",
        json!({ "by": chief.handle, "own": true }),
    )?;
    release_ready(store, project_id)?;
    Ok(TaskMoved {
        task: task_by_id(store, task.id)?,
        message: store.message(message_id)?,
        withdrawn: views(store, &withdrawn)?,
        landing: views(store, &landing)?,
    })
}

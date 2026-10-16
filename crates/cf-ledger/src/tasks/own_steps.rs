//! The chief's own later step (`cf task add --self --needs T-3`): a task the
//! chief asked of itself, which waits on the board for what it needs and whose
//! brief then comes back to the chief's window. The chief finishes it by its
//! word, whenever it is at it (the field report of 2026-10-15, B4: `cf task
//! done` was refused while the brief was still on its way back, and the chief's
//! own result came back to it to accept). A task the human gives the chief is
//! no such step: its result is the human's to decide.

use cf_proto::ledger::TaskMoved;
use rusqlite::params;
use serde_json::json;

use super::{release_ready, require_task_state, task_by_id, task_row_by_id};
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

/// The chief finishes its own step with `body`, queued for it (its brief on
/// its way back) or at work on it; one still waiting on the board for what it
/// needs is refused. In one step: what of the step had not reached the chief
/// yet (its brief, most often) is withdrawn; the words are its result, received
/// as they are written, never pasted; and the step is accepted, there being
/// nobody else to decide it, so what needs it goes on.
pub(super) fn finish(
    store: &mut Store,
    project_id: i64,
    task: &TaskRow,
    body: &str,
) -> Result<TaskMoved, LedgerError> {
    require_task_state(task, &HELD_TASK_STATES, "finish")?;
    let chief = store.participant_row(task.requester_id)?;
    store.db.execute(
        "UPDATE message SET state = 'cancelled', reason = ?
         WHERE task_id = ? AND recipient_id = ? AND state IN ('queued', 'gated')",
        params![
            format!(
                "T-{} was done before it came back to @{}",
                task.number, chief.handle
            ),
            task.id,
            chief.id
        ],
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
    })
}

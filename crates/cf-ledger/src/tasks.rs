//! Tasks, along the state machine in the crate's rules (`src/ledger/tasks.js`):
//! given by a coordinator to a participant by name, or opened for a pool
//! and tier of the staff and assigned by the daemon, or taken back to the
//! board (`giving`); paused, held and resumed (`pausing`); finished with a
//! result and accepted, sent back, cancelled, failed, and deleted from the
//! board by the human (`finishing`); and read with their threads.

mod finishing;
mod giving;
mod pausing;

use cf_proto::ledger::{TaskThread, TaskView};
use rusqlite::{params, OptionalExtension};

use crate::model::LedgerError;
use crate::store::Store;
use crate::views::{message_view, task_view, TaskRow, MESSAGE_SELECT, TASK_SELECT};

pub(crate) use finishing::{
    accept_task, call_off, cancel_task, delete_tasks, fail_task, record_result, reopen_task,
};
pub use giving::NewTask;
pub(crate) use giving::{assign_task, check_release, create_task, release_task};
pub(crate) use pausing::{
    clear_hold, held_tasks_due, hold_task, pause_task, paused_task, resume_task, told_since_paused,
};

/// What a paused task's window is told when it goes on: the human's Resume
/// and the daemon's alike.
pub const RESUME_WORDS: &str = "Go on where you stopped.";

const CRITICAL_RULE: &str = "No coding or implementation edits. Do not write or revise specifications. Return analysis, evidence and recommendations to your coordinator.";

/// How a task reads when it is handed over: critical work leads with its purpose.
fn delivery_body(task: &TaskRow) -> String {
    match &task.purpose {
        None => task.body.clone(),
        Some(purpose) => format!("Critical work: {purpose}. {CRITICAL_RULE}\n\n{}", task.body),
    }
}

/// "standard worker", "image designer": who an open task waits for.
fn pool_name(pool: Option<&str>, tier: Option<&str>) -> String {
    match pool {
        Some("designer") => "image designer".to_string(),
        pool => format!("{} {}", tier.unwrap_or("null"), pool.unwrap_or("null")),
    }
}

/// The same, with its article.
fn a_pool(pool: Option<&str>, tier: Option<&str>) -> String {
    let article = if pool == Some("designer") { "an" } else { "a" };
    format!("{article} {}", pool_name(pool, tier))
}

/// A task and its whole thread, oldest first; none when there is no such
/// task. `cf task get` reads it over the local API.
pub(crate) fn task(
    store: &Store,
    project_id: i64,
    number: i64,
) -> Result<Option<TaskThread>, LedgerError> {
    let Some(task) = store
        .db
        .query_row(
            &format!("{TASK_SELECT} WHERE t.project_id = ? AND t.number = ?"),
            params![project_id, number],
            task_view,
        )
        .optional()?
    else {
        return Ok(None);
    };
    let messages = store
        .db
        .prepare(&format!(
            "{MESSAGE_SELECT} WHERE m.task_id = ? ORDER BY m.id"
        ))?
        .query_map([task.id], message_view)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(Some(TaskThread { task, messages }))
}

/// The task found by `sql`, which selects a project and a number, with its thread.
fn task_found(
    store: &Store,
    sql: &str,
    participant_id: i64,
) -> Result<Option<TaskThread>, LedgerError> {
    let found = store
        .db
        .query_row(sql, [participant_id], |row| {
            Ok((
                row.get::<_, i64>("project_id")?,
                row.get::<_, i64>("number")?,
            ))
        })
        .optional()?;
    match found {
        Some((project_id, number)) => task(store, project_id, number),
        None => Ok(None),
    }
}

/// The task a participant has in progress (working or waiting on an
/// answer), or none. With `queued`, one whose delivery is still being
/// confirmed counts too: a window may ask its first question before the
/// record that confirms the task's arrival is read.
pub(crate) fn active_task(
    store: &Store,
    participant_id: i64,
    queued: bool,
) -> Result<Option<TaskThread>, LedgerError> {
    task_found(
        store,
        &format!(
            "SELECT project_id, number FROM task
       WHERE assignee_id = ? AND state IN ({}'working', 'waiting')
       ORDER BY id LIMIT 1",
            if queued { "'queued', " } else { "" }
        ),
        participant_id,
    )
}

/// The task of the newest message for a participant, whatever its state
/// now, or none; a message the human still holds at the gate does not
/// count. For a member's window: the task it is on, or was on until it was
/// cancelled.
pub(crate) fn last_task(
    store: &Store,
    participant_id: i64,
) -> Result<Option<TaskThread>, LedgerError> {
    task_found(
        store,
        "SELECT t.project_id, t.number FROM message m JOIN task t ON t.id = m.task_id
       WHERE m.recipient_id = ? AND m.state != 'gated' ORDER BY m.id DESC LIMIT 1",
        participant_id,
    )
}

fn task_row_by_id(store: &Store, id: i64) -> Result<TaskRow, LedgerError> {
    Ok(store
        .db
        .query_row("SELECT * FROM task WHERE id = ?", [id], TaskRow::read)?)
}

fn task_by_id(store: &Store, id: i64) -> Result<TaskView, LedgerError> {
    Ok(store
        .db
        .query_row(&format!("{TASK_SELECT} WHERE t.id = ?"), [id], task_view)?)
}

/// A task still on the board, in one of `states`, for `action`.
fn require_task_state(task: &TaskRow, states: &[&str], action: &str) -> Result<(), LedgerError> {
    require_on_board(task, action)?;
    if states.contains(&task.state.as_str()) {
        return Ok(());
    }
    Err(LedgerError::refused_with(
        "invalid-transition",
        format!("cannot {action} T-{}: it is {}", task.number, task.state),
        409,
    ))
}

/// A task the human deleted from the board moves no more, and nothing new waits for it.
fn require_on_board(task: &TaskRow, action: &str) -> Result<(), LedgerError> {
    if task.deleted_at.is_none() {
        return Ok(());
    }
    Err(LedgerError::refused_with(
        "task-deleted",
        format!(
            "cannot {action} T-{}: it was deleted from the board",
            task.number
        ),
        409,
    ))
}

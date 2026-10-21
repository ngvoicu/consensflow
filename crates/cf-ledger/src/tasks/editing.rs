//! A brief replaced (`cf task edit`): the chief wrote a task for a member
//! and sees, before any window has it, that it should say something else. The
//! task may be open for a member, blocked on what it needs, paused before any
//! window had it, or queued for its window without the brief having begun to
//! reach it. A task a window was
//! given is not edited: the window has the old words, and says what it did
//! with them; what the chief has to add is told to it, or the task is
//! reopened once it has a result.

use cf_proto::ledger::TaskView;
use rusqlite::params;
use serde_json::json;

use super::{delivery_body, require_on_board, task_by_id, task_row_by_id};
use crate::model::{self, title_of, LedgerError, MAX_BODY};
use crate::store::Store;
use crate::views::TaskRow;

/// The brief of task `number` is replaced by `body`, by `by`. The old brief is
/// kept in the log (`task.edited`, as `from`), and a task message for its
/// window that is still waiting (queued, or held at the human's gate) says the
/// new one, so the window never has the old words. The task keeps its state, its
/// place on the board and what it needs.
///
/// Refused while the task was given to a window (`already-given`, saying what
/// to do instead) and once it is over (`invalid-transition`).
pub(crate) fn edit_task(
    store: &mut Store,
    project_id: i64,
    number: i64,
    by: &str,
    body: &str,
) -> Result<TaskView, LedgerError> {
    model::require_text(body, "body", MAX_BODY)?;
    store.write(|store| {
        store.participant_by_handle(project_id, by)?;
        let task = store.task_row(project_id, number)?;
        require_on_board(&task, "edit")?;
        require_not_given(store, &task)?;
        let at = store.at();
        store.db.execute(
            "UPDATE task SET body = ?, title = ?, updated_at = ? WHERE id = ?",
            params![body, title_of(body), at, task.id],
        )?;
        let words = delivery_body(&task_row_by_id(store, task.id)?);
        store.db.execute(
            "UPDATE message SET body = ? WHERE task_id = ? AND kind = 'task'
               AND state IN ('queued', 'gated')",
            params![words, task.id],
        )?;
        store.log(
            project_id,
            "task.edited",
            json!({ "task": number, "by": by, "from": task.body }),
        )?;
        task_by_id(store, task.id)
    })
}

/// Whether the task's brief may still be replaced: it waits for a window (open,
/// queued, or paused before any had it), and no task message of it began to
/// reach one. The refusal says what to do instead, by where the task is.
fn require_not_given(store: &Store, task: &TaskRow) -> Result<(), LedgerError> {
    let number = task.number;
    let tell = format!("tell its window: cf tell T-{number} \"…\"");
    match task.state.as_str() {
        "working" | "waiting" => return Err(given(store, task, tell)?),
        "done" | "failed" => {
            let reopen = format!("reopen it with your words: cf task reopen T-{number} \"…\"");
            return Err(given(store, task, reopen)?);
        }
        "open" | "queued" | "paused" => {}
        state => {
            return Err(LedgerError::refused_with(
                "invalid-transition",
                format!("cannot edit T-{number}: it is {state}"),
                409,
            ))
        }
    }
    let begun = store
        .db
        .prepare(
            "SELECT 1 FROM message WHERE task_id = ? AND kind = 'task'
               AND state IN ('delivering', 'delivered', 'read')",
        )?
        .exists([task.id])?;
    if begun {
        let instead = match task.state.as_str() {
            "paused" => tell,
            "queued" => format!("tell its window once it has begun: cf tell T-{number} \"…\""),
            _ => format!("tell its next window once it has begun: cf tell T-{number} \"…\""),
        };
        return Err(given(store, task, instead)?);
    }
    Ok(())
}

/// The refusal of a task a window has: who has it, and what to do instead.
fn given(store: &Store, task: &TaskRow, instead: String) -> Result<LedgerError, LedgerError> {
    let window = task
        .assignee_id
        .map(|id| store.participant_row(id))
        .transpose()?
        .map_or_else(
            || "a window".to_owned(),
            |window| format!("@{}", window.handle),
        );
    Ok(LedgerError::refused_with(
        "already-given",
        format!(
            "cannot edit T-{}: it was given to {window}, which has the brief; {instead}",
            task.number
        ),
        409,
    ))
}

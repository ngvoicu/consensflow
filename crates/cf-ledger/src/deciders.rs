//! Who decides a task, and so who a question bound to it is put to (the leads design, section
//! 1: "a task's decider is its requester where the requester is a coordinator still in the
//! project; otherwise the chief").
//!
//! With no lead in a project the decider of every task is the chief: a task the chief gave, one
//! the human gave, and one any other giver left behind.

use crate::model::LedgerError;
use crate::store::Store;
use crate::views::{ParticipantRow, TaskRow};

/// The chief's handle, which no other participant may take (`model`'s reserved names).
const CHIEF: &str = "chief";

/// The decider of `task`: its requester while that is a lead still in the project, else the
/// project's chief, found as every message to the chief finds it, by its handle.
pub(crate) fn decider_of(store: &Store, task: &TaskRow) -> Result<ParticipantRow, LedgerError> {
    let requester = store.participant_row(task.requester_id)?;
    if requester.seat().coordinates() && !requester.seat().is_chief() && requester.left_at.is_none()
    {
        return Ok(requester);
    }
    store.participant_by_handle(task.project_id, CHIEF)
}

/// The handle a question bound to `task` is put to, or the chief's for a question bound to none.
pub(crate) fn question_recipient(
    store: &Store,
    project_id: i64,
    task: Option<i64>,
) -> Result<String, LedgerError> {
    match task {
        Some(task) => {
            let number: i64 =
                store
                    .db
                    .query_row("SELECT number FROM task WHERE id = ?", [task], |row| {
                        row.get(0)
                    })?;
            let row = store.task_row(project_id, number)?;
            Ok(decider_of(store, &row)?.handle)
        }
        None => Ok(store.participant_by_handle(project_id, CHIEF)?.handle),
    }
}

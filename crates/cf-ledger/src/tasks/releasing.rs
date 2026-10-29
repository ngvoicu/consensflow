//! Held work let go early (`cf task release`, T-798, 3.0.7; the field report of 2026-10-27).
//! The chief holds work behind a step of its own (`cf task add --self --needs T-665
//! --before …`, a "stage" that waits for a release), and with the machine idle the human said
//! to start a few of the held tasks early. A step that has not arrived cannot be finished
//! (`own_steps::finish` refuses an open one), no command dropped a need from a task, and
//! cancelling the step would have blocked every task that needs it for good; so the chief
//! cancelled eleven tasks, added them again, renumbered them and rewrote their
//! cross-references.
//!
//! A task is let go of what it waits for by the chief or the human: of one need it names, or
//! of every need not yet accepted. It then goes where it would have gone had they been
//! accepted: a task given to a window goes into it, a task for a tier waits for a member, and
//! one that is paused stays so for its resume. Only the named tasks go; the step they waited
//! for is untouched, and what else waits for it still does. One need is never let go: the
//! task a follow-up comes after, whose window the follow-up goes into (`--after`), because
//! that window has not finished it and holds no second task. The need is dropped, not the
//! task it names: nothing is cancelled, and that task's own result still decides what is
//! accepted.

use cf_proto::ledger::TaskLetGo;
use rusqlite::params;
use serde_json::json;

use super::finishing::release_waiting;
use super::{require_on_board, task_by_id};
use crate::model::LedgerError;
use crate::store::Store;

/// A need of a task not yet accepted: the task it names, its number, and who has it.
struct Need {
    id: i64,
    number: i64,
    assignee_id: Option<i64>,
}

/// Task `number` is let go of the need `from`, or of every need not yet accepted when none is
/// named, by `by`. Refused, `not-waiting`, for a task that is neither open nor paused, one that
/// waits for nothing, and a `from` it does not wait for; refused, `follows-in-window`, for the
/// need that is the task a follow-up comes after. Nothing changes on a refusal.
pub(crate) fn let_go(
    store: &mut Store,
    project_id: i64,
    number: i64,
    by: &str,
    from: Option<i64>,
) -> Result<TaskLetGo, LedgerError> {
    store.write(|store| {
        store.participant_by_handle(project_id, by)?;
        let task = store.task_row(project_id, number)?;
        require_on_board(&task, "release")?;
        if !matches!(task.state.as_str(), "open" | "paused") {
            return Err(not_waiting(format!(
                "T-{number} is {}: only an open or paused task can be let go of what it waits for",
                task.state
            )));
        }
        let waits: Vec<Need> = store
            .db
            .prepare(
                "SELECT d.id, d.number, d.assignee_id FROM task_need n JOIN task d ON d.id = n.needs_id
                  WHERE n.task_id = ? AND d.state != 'accepted' ORDER BY d.number",
            )?
            .query_map([task.id], |row| {
                Ok(Need {
                    id: row.get(0)?,
                    number: row.get(1)?,
                    assignee_id: row.get(2)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        let letting: Vec<&Need> = match from {
            None => waits.iter().collect(),
            Some(from) => waits.iter().filter(|need| need.number == from).collect(),
        };
        if letting.is_empty() {
            let waiting = waits
                .iter()
                .map(|need| format!("T-{}", need.number))
                .collect::<Vec<_>>()
                .join(", ");
            let message = if waiting.is_empty() {
                format!("T-{number} waits for no task that is not accepted")
            } else {
                format!(
                    "T-{number} does not wait for T-{}; it waits for {waiting}",
                    from.unwrap_or_default()
                )
            };
            return Err(not_waiting(message));
        }
        // The window a follow-up goes into has the task it follows: it is not let go of it.
        if let Some(assignee) = task.assignee_id {
            if store.participant_row(assignee)?.member_id.is_some() {
                if let Some(anchor) = letting
                    .iter()
                    .find(|need| need.assignee_id == Some(assignee))
                {
                    return Err(LedgerError::refused_with(
                        "follows-in-window",
                        format!(
                            "T-{number} follows T-{} in the window that has it: it goes in once T-{} is accepted, which no release hurries; cancel T-{number} and add it for a tier if it should not wait",
                            anchor.number, anchor.number
                        ),
                        409,
                    ));
                }
            }
        }
        let numbers: Vec<i64> = letting.iter().map(|need| need.number).collect();
        for need in &letting {
            store.db.execute(
                "DELETE FROM task_need WHERE task_id = ? AND needs_id = ?",
                params![task.id, need.id],
            )?;
        }
        store.log(
            project_id,
            "task.need-dropped",
            json!({ "task": number, "from": numbers, "by": by }),
        )?;
        // A task given to a window goes into it once nothing it waits for is left. The need
        // rows it has are what `release_ready` finds such a task by, and it has none now.
        let task = store.task_row(project_id, number)?;
        let waits_still = store
            .db
            .prepare(
                "SELECT 1 FROM task_need n JOIN task d ON d.id = n.needs_id
                  WHERE n.task_id = ? AND d.state != 'accepted'",
            )?
            .exists([task.id])?;
        if task.state == "open" && task.assignee_id.is_some() && !waits_still {
            release_waiting(store, project_id, &task)?;
        }
        Ok(TaskLetGo {
            task: task_by_id(store, task.id)?,
            from: numbers,
        })
    })
}

fn not_waiting(message: String) -> LedgerError {
    LedgerError::refused_with("not-waiting", message, 409)
}

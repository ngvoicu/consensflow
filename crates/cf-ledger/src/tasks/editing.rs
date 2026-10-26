//! A brief replaced (`cf task edit`): the chief wrote a task for a member
//! and sees, before any window has it, that it should say something else. The
//! task may be open for a member, blocked on what it needs, paused before any
//! window had it, or queued for its window without the brief having begun to
//! reach it. A task a window was
//! given is not edited: the window has the old words, and says what it did
//! with them; what the chief has to add is told to it, or the task is
//! reopened once it has a result. A task still open is told too (`tell_open`):
//! its words are added to the brief.

use cf_proto::ledger::TaskView;
use rusqlite::params;
use serde_json::json;

use super::own_steps::is_own_step;
use super::{delivery_body, require_on_board, require_task_state, task_by_id, task_row_by_id};
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
        let old = delivery_body(&task);
        let words = delivery_body(&task_row_by_id(store, task.id)?);
        // Only the rows that say the brief: a row that is it (the brief as it was
        // queued), or starts with it and a blank line (a resume that carries it); a
        // row of the resume's words alone, or any other, is left as it is.
        let rows: Vec<(i64, String)> = store
            .db
            .prepare(
                "SELECT id, body FROM message WHERE task_id = ? AND kind = 'task'
                   AND state IN ('queued', 'gated')",
            )?
            .query_map([task.id], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        for (id, row) in rows {
            let renewed = if row == old {
                Some(words.clone())
            } else {
                row.strip_prefix(old.as_str())
                    .and_then(|rest| rest.strip_prefix("\n\n"))
                    .map(|rest| format!("{words}\n\n{rest}"))
            };
            if let Some(renewed) = renewed {
                store.db.execute(
                    "UPDATE message SET body = ? WHERE id = ?",
                    params![renewed, id],
                )?;
            }
        }
        store.log(
            project_id,
            "task.edited",
            json!({ "task": number, "by": by, "from": task.body }),
        )?;
        task_by_id(store, task.id)
    })
}

/// `from` tells open task `number` `words`: no window has it, nor is one bound to take it
/// (it waits on the board for what it needs, or for a member, whom the daemon picks when
/// it goes), so there is no one to keep a message for, and a message is addressed to a
/// participant. The words wait where the board puts what is said to a task with no window
/// (a resume's `Resumed:`, the words kept for a window that ended, `Kept from before`): in
/// its brief, after the brief, under a label of who said them. Whichever window takes the
/// task is given them once, with the brief, in the order they were told; nothing is
/// paused, queued or waited for. The title stays the brief's first line. The words are in
/// the log (`task.told`). A later `edit` replaces the brief whole, the words with it.
///
/// Refused for a task in any other state (`invalid-transition`: a queued task keeps the
/// words for its window, one a window has is paused for them, and `cf tell` chooses by
/// the state it reads), for the chief's own step (`own-work`: its brief is the chief's to
/// change), and for words that are none or would outgrow a brief (`invalid-text`).
pub(crate) fn tell_open(
    store: &mut Store,
    project_id: i64,
    number: i64,
    from: &str,
    words: &str,
) -> Result<TaskView, LedgerError> {
    model::require_text(words, "body", MAX_BODY)?;
    store.write(|store| {
        store.participant_by_handle(project_id, from)?;
        let task = store.task_row(project_id, number)?;
        require_task_state(&task, &["open"], "tell the window of")?;
        if is_own_step(store, &task)? {
            return Err(LedgerError::refused_with(
                "own-work",
                format!(
                    "T-{number} is the chief's own step: its brief is yours to change: cf task edit T-{number} \"…\""
                ),
                409,
            ));
        }
        let told = format!(
            "{}\n\nTold by @{from} before a window had it: {words}",
            task.body
        );
        model::require_text(&told, "body", MAX_BODY)?;
        let at = store.at();
        store.db.execute(
            "UPDATE task SET body = ?, updated_at = ? WHERE id = ?",
            params![told, at, task.id],
        )?;
        store.log(
            project_id,
            "task.told",
            json!({ "task": number, "by": from, "words": words }),
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
        // A queued task's window, coming up, keeps what it is told for after its
        // brief (`tell_queued`, B3): it is told now. One taken back to the board is
        // told now too (`tell_open`): its words are added to the brief its next window
        // is given.
        let instead = match task.state.as_str() {
            "paused" | "queued" => tell,
            _ => format!(
                "tell it now, and the window that takes it has your words in its brief: cf tell T-{number} \"…\""
            ),
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

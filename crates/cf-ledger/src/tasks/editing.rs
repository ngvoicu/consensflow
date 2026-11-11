//! A brief or a tier replaced (`cf task edit`): the chief wrote a task for a member
//! and sees, before any window has it, that it should say something else, or ask
//! another tier. The
//! task may be open for a member, blocked on what it needs, paused before any
//! window had it, or queued for its window without the brief having begun to
//! reach it. A task a window was
//! given is not edited: the window has the old words, and says what it did
//! with them; what the chief has to add is told to it, or the task is
//! reopened once it has a result. A task still open is told too (`tell_open`):
//! its words are added to the brief.

use cf_proto::ledger::{Links, TaskEdit, TaskEdited, TaskView};
use rusqlite::params;
use serde_json::{json, Value};

use super::giving::upstream;
use super::own_steps::is_own_step;
use super::{
    delivery_body, release_inside, release_ready, require_on_board, require_task_state, task_by_id,
    task_row_by_id,
};
use crate::model::{self, title_of, LedgerError, MAX_BODY, PURPOSES};
use crate::roles;
use crate::staff::{has_members_of_tier, holders_of_tier, nearest_tier};
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
    let edit = TaskEdit {
        body: Some(body.to_owned()),
        ..TaskEdit::default()
    };
    Ok(edit_task_with(store, project_id, number, by, &edit)?.task)
}

/// The brief of task `number`, its tier, or both, are replaced in one step: all or
/// none. The brief as [`edit_task`] says. A new tier is read as `cf task add` reads
/// one: a tier nobody on the staff holds goes to the nearest one somebody does, the
/// next one up first (`asked` says the tier asked then, with the members of the one it
/// went to), and a role nobody holds is refused. A task still open or paused with no
/// window just asks the new tier; one queued for a member's window the brief has not
/// reached goes back to the board for the new tier, unless that member holds it. A task
/// given by name, a follow-up, the chief's own step and the image designer's have no
/// tier to change (`invalid-transition`); critical work names its purpose
/// (`purpose-required`), and a task that has none cannot become it.
pub(crate) fn edit_task_with(
    store: &mut Store,
    project_id: i64,
    number: i64,
    by: &str,
    edit: &TaskEdit,
) -> Result<TaskEdited, LedgerError> {
    let body = edit.body.as_deref();
    let links = edit.needs.is_some() || edit.before.is_some();
    match body {
        Some(body) => model::require_text(body, "body", MAX_BODY)?,
        None if edit.tier.is_none() && !links => model::require_text("", "body", MAX_BODY)?,
        None => {}
    }
    let wanted = edit
        .tier
        .as_deref()
        .map(|tier| model::parse_tier(Some(&Value::from(tier))))
        .transpose()?;
    store.write(|store| {
        store.participant_by_handle(project_id, by)?;
        let task = store.task_row(project_id, number)?;
        require_on_board(&task, "edit")?;
        require_not_given(store, &task)?;
        // The tier is checked before anything is written: a refusal changes nothing.
        let moving = match wanted {
            Some(wanted) => Some(tier_for(store, project_id, &task, wanted)?),
            None => None,
        };
        let at = store.at();
        if let Some(body) = body {
            replace_brief(store, project_id, &task, by, body, &at)?;
        }
        let mut asked = None;
        let mut holders = Vec::new();
        if let Some((asked_tier, tier)) = moving {
            if task.tier.as_deref() != Some(tier.as_str()) {
                store.db.execute(
                    "UPDATE task SET tier = ?, updated_at = ? WHERE id = ?",
                    params![tier, at, task.id],
                )?;
                store.log(
                    project_id,
                    "task.retiered",
                    json!({ "task": number, "by": by, "from": task.tier, "to": tier }),
                )?;
                // Queued for a member's window the brief has not reached: it goes back to
                // the board for the new tier, unless that member holds it. Nothing was given.
                if let Some(window) = task.assignee_id {
                    let window = store.participant_row(window)?;
                    if window.tier.as_deref() != Some(tier.as_str()) {
                        release_inside(
                            store,
                            project_id,
                            number,
                            &format!("its tier was changed to {tier} by @{by}"),
                            true,
                        )?;
                    }
                }
            }
            if asked_tier != tier {
                holders = holders_of_tier(
                    store,
                    project_id,
                    task.pool.as_deref().unwrap_or_default(),
                    &tier,
                )?;
                asked = Some(asked_tier);
            }
        }
        let waiting = if links {
            relink(store, project_id, number, by, edit)?
        } else {
            None
        };
        Ok(TaskEdited {
            task: task_by_id(store, task.id)?,
            asked,
            holders,
            waiting,
        })
    })
}

/// The tasks `task` needs and those that wait for it, changed as `edit` says: a list
/// replaced, or some added and some removed (one added already, or one removed that
/// is not there, changes nothing). Each task named is a task of the project, and
/// the plan has no circles, near or far; what waits for it is still on the board
/// (`not-on-the-board`); what it needs is not cancelled (`need-cancelled`) and not
/// deleted unless accepted. A task given a window that now waits for what is not
/// accepted goes back to the board to wait there, where it can; one given by name
/// cannot (`invalid-transition`). Says what waits for the task now, when that was
/// asked, and lets go of what no longer waits.
fn relink(
    store: &mut Store,
    project_id: i64,
    number: i64,
    by: &str,
    edit: &TaskEdit,
) -> Result<Option<Vec<i64>>, LedgerError> {
    // As the tier left it: it may have gone back to the board.
    let task = &store.task_row(project_id, number)?;
    let numbers = |sql: &str| -> Result<Vec<i64>, LedgerError> {
        Ok(store
            .db
            .prepare(sql)?
            .query_map([task.id], |row| row.get::<_, i64>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    };
    let needs = numbers(
        "SELECT d.number FROM task_need n JOIN task d ON d.id = n.needs_id
           WHERE n.task_id = ?1 ORDER BY d.number",
    )?;
    let waiters = numbers(
        "SELECT w.number FROM task_need n JOIN task w ON w.id = n.task_id
           WHERE n.needs_id = ?1 ORDER BY w.number",
    )?;
    let wanted = |links: &Option<Links>, held: &[i64]| -> Option<Vec<i64>> {
        links.as_ref().map(|links| match links {
            Links::Set(set) => sorted(set.clone()),
            Links::Change { add, remove } => sorted(
                held.iter()
                    .chain(add)
                    .copied()
                    .filter(|number| !remove.contains(number) || add.contains(number))
                    .collect(),
            ),
        })
    };
    let needs_now = wanted(&edit.needs, &needs);
    let waiters_now = wanted(&edit.before, &waiters);
    for other in needs
        .iter()
        .filter(|n| needs_now.as_ref().is_some_and(|now| !now.contains(n)))
    {
        let other = store.task_row(project_id, *other)?;
        unlink(store, task.id, other.id)?;
    }
    for other in waiters
        .iter()
        .filter(|n| waiters_now.as_ref().is_some_and(|now| !now.contains(n)))
    {
        let other = store.task_row(project_id, *other)?;
        unlink(store, other.id, task.id)?;
    }
    for added in needs_now.iter().flatten().filter(|n| !needs.contains(n)) {
        let need = store.task_row(project_id, *added)?;
        if need.id == task.id || upstream(store, need.id, task.id)? {
            return Err(circle(number, *added));
        }
        if need.state == "cancelled" {
            return Err(LedgerError::refused_with(
                "need-cancelled",
                format!("T-{added} is cancelled: nothing waits for it"),
                409,
            ));
        }
        if need.state != "accepted" {
            require_on_board(&need, "wait for")?;
        }
        link(store, task.id, need.id)?;
    }
    for added in waiters_now
        .iter()
        .flatten()
        .filter(|n| !waiters.contains(n))
    {
        let waits = store.task_row(project_id, *added)?;
        if waits.id == task.id || upstream(store, task.id, waits.id)? {
            return Err(circle(*added, number));
        }
        if waits.state != "open" {
            return Err(LedgerError::refused_with(
                "not-on-the-board",
                format!(
                    "T-{added} is {}: only a task still on the board can wait for T-{number}",
                    waits.state
                ),
                409,
            ));
        }
        link(store, waits.id, task.id)?;
    }
    // A task queued for a window that now waits for what is not accepted waits on the
    // board for it, as an open one does: its brief had not begun, so nothing is lost. One
    // paused with a window, or given by name, cannot: it says what to do instead.
    let blocked = store
        .db
        .prepare(
            "SELECT 1 FROM task_need n JOIN task d ON d.id = n.needs_id
               WHERE n.task_id = ? AND d.state != 'accepted' LIMIT 1",
        )?
        .exists([task.id])?;
    if blocked && task.assignee_id.is_some() && task.state != "open" {
        if task.state != "queued" || task.pool.is_none() {
            return Err(LedgerError::refused_with(
                "invalid-transition",
                format!(
                    "cannot make T-{number} wait for what is not accepted: it is {} with a window, {}",
                    task.state,
                    if task.pool.is_none() {
                        "given by name, not by tier"
                    } else {
                        "and the pause keeps it there: resume it, or cancel it"
                    }
                ),
                409,
            ));
        }
        release_inside(
            store,
            project_id,
            number,
            &format!("it now waits for what it needs, by @{by}"),
            true,
        )?;
    }
    store.log(
        project_id,
        "task.linked",
        json!({
            "task": number, "by": by,
            "needs": needs_now.as_ref().unwrap_or(&needs),
            "before": waiters_now.as_ref().unwrap_or(&waiters),
        }),
    )?;
    release_ready(store, project_id)?;
    Ok(waiters_now)
}

fn sorted(mut numbers: Vec<i64>) -> Vec<i64> {
    numbers.sort_unstable();
    numbers.dedup();
    numbers
}

fn link(store: &Store, task: i64, needs: i64) -> Result<(), LedgerError> {
    store.db.execute(
        "INSERT INTO task_need (task_id, needs_id) VALUES (?, ?)",
        params![task, needs],
    )?;
    Ok(())
}

fn unlink(store: &Store, task: i64, needs: i64) -> Result<(), LedgerError> {
    store.db.execute(
        "DELETE FROM task_need WHERE task_id = ? AND needs_id = ?",
        params![task, needs],
    )?;
    Ok(())
}

fn circle(waiter: i64, waited: i64) -> LedgerError {
    LedgerError::refused_with(
        "circular-needs",
        format!("T-{waiter} cannot wait for T-{waited}: that makes a circle, and a plan has none"),
        409,
    )
}

/// The tier a task asked for `wanted` goes to, as `(wanted, nearest)`: it is a task
/// given by tier, for a role with tiers, and critical work has its purpose, and the staff
/// holds the role.
fn tier_for(
    store: &Store,
    project_id: i64,
    task: &TaskRow,
    wanted: &'static str,
) -> Result<(String, String), LedgerError> {
    let number = task.number;
    let pool = match task.pool.as_deref() {
        Some(pool) if roles::untiered(pool) => {
            return Err(LedgerError::refused_with(
                "invalid-transition",
                format!(
                    "cannot change the tier of T-{number}: the {} has no tier",
                    roles::name_of(pool)
                ),
                409,
            ))
        }
        Some(pool) => pool,
        None => {
            return Err(LedgerError::refused_with(
                "invalid-transition",
                format!("cannot change the tier of T-{number}: it was given by name, not by tier"),
                409,
            ))
        }
    };
    if wanted == "critical"
        && !task
            .purpose
            .as_deref()
            .is_some_and(|purpose| PURPOSES.contains(&purpose))
    {
        return Err(LedgerError::refused(
            "purpose-required",
            format!(
                "critical work names its purpose ({}), and T-{number} has none: cancel it and add it again",
                PURPOSES.join(", ")
            ),
        ));
    }
    let tier =
        nearest_tier(store, project_id, pool, Some(wanted))?.unwrap_or_else(|| wanted.to_owned());
    if !has_members_of_tier(store, project_id, pool, Some(&tier))? {
        return Err(LedgerError::refused_with(
            "no-member-of-tier",
            format!(
                "no {} is on the staff: ask the human for one, in your terminal",
                roles::name_of(pool)
            ),
            409,
        ));
    }
    Ok((wanted.to_owned(), tier))
}

/// The brief of `task` replaced by `body`, with the task messages that say it.
fn replace_brief(
    store: &mut Store,
    project_id: i64,
    task: &TaskRow,
    by: &str,
    body: &str,
    at: &str,
) -> Result<(), LedgerError> {
    let number = task.number;
    store.db.execute(
        "UPDATE task SET body = ?, title = ?, updated_at = ? WHERE id = ?",
        params![body, title_of(body), at, task.id],
    )?;
    let old = delivery_body(task);
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
    Ok(())
}

/// The title of task `number` is `title`, by `by`, in any state: the title is a label
/// the board shows, apart from the brief a window has, and a reopen or a resume adds
/// words to a brief and keeps it. It is read as a brief's first line is
/// ([`model::checked_title`]). The task keeps its state, its brief and everything else;
/// the old title is in the log (`task.retitled`, as `from`). The same title again changes
/// and logs nothing.
pub(crate) fn retitle_task(
    store: &mut Store,
    project_id: i64,
    number: i64,
    by: &str,
    title: &str,
) -> Result<TaskView, LedgerError> {
    let title = model::checked_title(title)?;
    store.write(|store| {
        store.participant_by_handle(project_id, by)?;
        let task = store.task_row(project_id, number)?;
        if task.title != title {
            let at = store.at();
            store.db.execute(
                "UPDATE task SET title = ?, updated_at = ? WHERE id = ?",
                params![title, at, task.id],
            )?;
            store.log(
                project_id,
                "task.retitled",
                json!({ "task": number, "by": by, "from": task.title, "to": title }),
            )?;
        }
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

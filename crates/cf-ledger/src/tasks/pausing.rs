//! A task stopped without ending: paused by the chief or the human, held by
//! the daemon while its member is out of quota, and resumed with the words
//! that send it on. A pause stops the window and keeps what it was kept
//! for it: nothing is cancelled for it but what the chief wrote and now
//! takes back. What the window keeps goes in with the words that resume it.
//!
//! A task whose window closed with its project (the human's Close, or the
//! chief's window ending, suspends it) is paused with a mark of that
//! (`paused_with_project`), and goes on by itself with the human's Resume of
//! the project ([`resume_with_project`]), unless its window holds other tasks
//! too. Every other pause stays until the chief or the human resumes it, or the
//! daemon does at a hold's end: so does a pause a ledger made before the mark,
//! which has none.

use cf_base::time;
use cf_proto::ledger::{HeldTask, MessageView, Stop, TaskMoved, TaskThread, TaskView};
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Map, Value};

use super::{
    carrier_body, delivery_body, m_list, require_task_state, task, task_by_id, task_found,
    RESUME_WORDS,
};
use crate::messages::{
    cancel_message, fold, join_pause_note, leave_pause_notes, note_pause, reconcile,
    release_carried, tell_goes_on, transfer, withdraw_stale, Afterwards,
};
use crate::model::{self, sql_list, LedgerError, ACTIVE_TASK_STATES, HELD_TASK_STATES, MAX_BODY};
use crate::queue::{queue, send, Queued, Sent};
use crate::staff::{require_free, Giving};
use crate::store::Store;
use crate::views::TaskRow;

/// The chief (or the human) stops a worker's task without ending it: its
/// agent is interrupted on the daemon's next look and its window stays for
/// the resumption, and the task keeps its member, its conversation, its place
/// and what was kept for it until it is resumed or cancelled. What the
/// chief wrote for the window (its words and notes, not its answers) and had
/// not yet reached it is taken back, for the chief's resume words carry what
/// is new. The chief's own work is not paused. `by` is none when ConsensFlow
/// pauses it, `because` says why when it is given.
pub(crate) fn pause_task(
    store: &mut Store,
    project_id: i64,
    number: i64,
    by: Option<&str>,
    because: Option<&str>,
) -> Result<TaskView, LedgerError> {
    pause_task_as(store, project_id, number, by, because, false)
}

/// ConsensFlow pauses a worker's task whose window it closed itself, as it
/// pauses one whose window went away. In a suspended project the window closed
/// with the project, and the pause is marked for the human's Resume of the
/// project, which resumes the task ([`resume_with_project`]); in an open one
/// it is as any other pause of ConsensFlow's.
pub(crate) fn pause_with_project(
    store: &mut Store,
    project_id: i64,
    number: i64,
    because: &str,
) -> Result<TaskView, LedgerError> {
    pause_task_as(store, project_id, number, None, Some(because), true)
}

/// A pause by `by` (none: ConsensFlow), marked as its project's suspension's
/// when `with_project` says so and the project is suspended.
fn pause_task_as(
    store: &mut Store,
    project_id: i64,
    number: i64,
    by: Option<&str>,
    because: Option<&str>,
    with_project: bool,
) -> Result<TaskView, LedgerError> {
    if let Some(because) = because {
        model::require_text(because, "because", 1000)?;
    }
    store.write(|store| {
        let author = by
            .map(|by| store.participant_by_handle(project_id, by))
            .transpose()?;
        let task = store.task_row(project_id, number)?;
        // A pause of the chief's or the human's that lands on a task its project's
        // suspension paused (sent as the Close began) takes that pause over.
        let taken = match &author {
            Some(author) => take_over_pause(store, &task, &author.handle)?,
            None => false,
        };
        let pausable = [&["open", "queued"], &ACTIVE_TASK_STATES[..]].concat();
        if !taken {
            require_task_state(&task, &pausable, "pause")?;
        }
        if let Some(assignee) = task.assignee_id {
            if store.participant_row(assignee)?.role == "chief" {
                return Err(LedgerError::refused_with(
                    "own-work",
                    format!("T-{number} is the chief's own: finish or cancel it"),
                    409,
                ));
            }
        }
        if let (Some(author), Some(window)) = (
            author.filter(|author| author.role == "chief"),
            task.assignee_id,
        ) {
            store.db.execute(
                "UPDATE message SET state = 'cancelled', reason = ?
                 WHERE task_id = ? AND recipient_id = ? AND sender_id = ?
                   AND kind IN ('task', 'note') AND state IN ('queued', 'gated')",
                params![
                    format!("withdrawn by @{}'s pause", author.handle),
                    task.id,
                    window,
                    author.id
                ],
            )?;
            release_carried(store, task.id)?;
        }
        if !taken {
            let mut detail = Map::new();
            detail.insert("by".into(), json!(by));
            if let Some(because) = because {
                detail.insert("because".into(), json!(because));
            }
            pause(store, &task, detail, with_project)?;
        }
        task_by_id(store, task.id)
    })
}

/// The window of a task the engine paused with its project ended by itself after
/// all: its exit came while the engine's kill of it waited for its answer, and the
/// kill was refused. The task stays paused, as any whose window closed by itself,
/// and no Resume of its project resumes it.
pub(crate) fn unmark_pause(
    store: &mut Store,
    project_id: i64,
    number: i64,
) -> Result<(), LedgerError> {
    store.write(|store| {
        let task = store.task_row(project_id, number)?;
        store.db.execute(
            "UPDATE task SET paused_with_project = 0 WHERE id = ? AND state = 'paused'",
            [task.id],
        )?;
        Ok(())
    })
}

/// The chief or the human (`by`) takes over the pause of a task its project's
/// suspension paused: their tell or their pause, sent as the Close began, lands
/// after it. The task stays paused, and the pause is theirs from then on: no
/// Resume of its project resumes it. Whether there was such a pause to take; a
/// task paused any other way is theirs to resume already, and is left alone.
pub(crate) fn take_over_pause(
    store: &mut Store,
    task: &TaskRow,
    by: &str,
) -> Result<bool, LedgerError> {
    let taken = store.db.execute(
        "UPDATE task SET paused_with_project = 0
         WHERE id = ? AND state = 'paused' AND paused_with_project = 1",
        [task.id],
    )? > 0;
    if taken {
        store.log(
            task.project_id,
            "task.pause-taken",
            json!({ "task": task.number, "by": by }),
        )?;
    }
    Ok(taken)
}

/// The move to paused, which is a stop of its own: the task counts it, so two
/// pauses in one millisecond are two. Its time is kept as the task's last
/// pause (a tell that reaches the window from then on counts,
/// `told_since_paused`), and, `with_project`, that its project's suspension
/// paused it, while the project is suspended. Its window's questions have their
/// doors shut before anything presses a key, and what a door claimed and nobody
/// acknowledged is the ledger's again.
fn pause(
    store: &mut Store,
    task: &TaskRow,
    mut detail: Map<String, Value>,
    with_project: bool,
) -> Result<(), LedgerError> {
    detail.insert("stop".into(), json!(task.stop_seq + 1));
    store.move_task(task, "paused", Value::Object(detail))?;
    store.db.execute(
        "UPDATE task SET paused_at = updated_at, stop_seq = stop_seq + 1 WHERE id = ?",
        [task.id],
    )?;
    if with_project {
        store.db.execute(
            "UPDATE task SET paused_with_project = 1
             WHERE id = ? AND (SELECT state FROM project WHERE id = task.project_id) = 'suspended'",
            [task.id],
        )?;
    }
    let Some(window) = task.assignee_id else {
        return Ok(());
    };
    shut_doors(store, task, window)?;
    let claimed: Vec<i64> = store
        .db
        .prepare(
            "SELECT id FROM message WHERE task_id = ? AND recipient_id = ? AND kind = 'answer'
               AND claimed_at IS NOT NULL ORDER BY id",
        )?
        .query_map(params![task.id, window], |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    for answer in claimed {
        store.db.execute(
            "UPDATE message SET claimed_at = NULL WHERE id = ?",
            [answer],
        )?;
        store.log(
            task.project_id,
            "delivery.unclaimed",
            json!({ "message": answer, "because": format!("T-{} was paused", task.number) }),
        )?;
    }
    Ok(())
}

/// The doors of the questions `window` asked on `task` through its harness's own
/// tool are shut, from the time the task last moved (so the move comes first):
/// the next poll of each is told so, and the window's question ends at once
/// rather than waiting on an answer that cannot come. A pause does it before
/// anything presses a key, and so does the acceptance of the task.
pub(super) fn shut_doors(store: &Store, task: &TaskRow, window: i64) -> Result<(), LedgerError> {
    store.db.execute(
        "UPDATE message SET door_closed_at = (SELECT updated_at FROM task WHERE id = ?1)
         WHERE task_id = ?1 AND sender_id = ?2 AND kind = 'question' AND door_closed_at IS NULL",
        params![task.id, window],
    )?;
    Ok(())
}

/// The daemon holds a task with its window while its member is out of
/// quota: paused, with the time it goes on by itself (`until`, an ISO
/// time). Its agent stops as any paused task's does, its window waits to go
/// on at the reset, and what was on its way to it waits too: the daemon
/// resumes it with fixed words, and the chief's own are what is kept.
pub(crate) fn hold_task(
    store: &mut Store,
    project_id: i64,
    number: i64,
    until: &str,
    because: &str,
) -> Result<TaskView, LedgerError> {
    model::require_text(because, "because", 1000)?;
    if time::parse(until).is_none() {
        return Err(LedgerError::refused(
            "invalid-until",
            "a hold names when it ends",
        ));
    }
    store.write(|store| {
        let task = store.task_row(project_id, number)?;
        let holdable = [&["queued"], &ACTIVE_TASK_STATES[..]].concat();
        require_task_state(&task, &holdable, "hold")?;
        let mut detail = Map::new();
        detail.insert("by".into(), Value::Null);
        detail.insert("because".into(), json!(because));
        detail.insert("until".into(), json!(until));
        pause(store, &task, detail, false)?;
        store.db.execute(
            "UPDATE task SET held_until = ? WHERE id = ?",
            params![until, task.id],
        )?;
        task_by_id(store, task.id)
    })
}

/// The daemon ends a task's hold without resuming it, when it cannot go on at
/// its time: the task stays paused, as the chief's or the human's pause leaves
/// it, and is not due again. It goes on only when one of them resumes it, or
/// is called off.
pub(crate) fn clear_hold(
    store: &mut Store,
    project_id: i64,
    number: i64,
    because: &str,
) -> Result<TaskView, LedgerError> {
    model::require_text(because, "because", 1000)?;
    store.write(|store| {
        let task = store.task_row(project_id, number)?;
        require_task_state(&task, &["paused"], "clear the hold of")?;
        let at = store.at();
        store.db.execute(
            "UPDATE task SET held_until = NULL, updated_at = ? WHERE id = ?",
            params![at, task.id],
        )?;
        store.log(
            project_id,
            "task.hold-cleared",
            json!({ "task": number, "because": because }),
        )?;
        task_by_id(store, task.id)
    })
}

/// The held tasks whose time has come at `now` (an ISO time), oldest first.
pub(crate) fn held_tasks_due(store: &Store, now: &str) -> Result<Vec<HeldTask>, LedgerError> {
    Ok(store
        .db
        .prepare(
            "SELECT project_id, number, assignee_id FROM task
       WHERE state = 'paused' AND held_until IS NOT NULL AND held_until <= ? ORDER BY id",
        )?
        .query_map([now], |row| {
            Ok(HeldTask {
                project_id: row.get("project_id")?,
                number: row.get("number")?,
                assignee_id: row.get("assignee_id")?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?)
}

/// The stops asked of a window's task: its task, and how many stops its
/// pauses asked of it. The task is the one the window works on: the one the
/// newest task message begun for it (being pasted, pasted or read) was about,
/// if it still holds it. A task none of whose words was ever given it is not
/// one it works on, paused or not, so a pause of it stops nothing; and a
/// stop the window paid for one task never stands in for another's. A window
/// that holds none is asked for none. What a window owes is what this counts
/// past the last stop it paid for that task.
///
/// A member session holds at most one task at a time: every door that gives
/// it one is held to that (`require_free`). So the task it works on is the
/// one it holds, and the newest words only tell that task from none: its
/// brief still queued, or the task paused before any words of it came, while
/// the last words the window had were of a task that is over. Only a
/// participant that holds several (a member's own lane, given tasks by name,
/// or a ledger written before the rule) has the newest words choose between
/// them.
pub(crate) fn stop_of(store: &Store, participant_id: i64) -> Result<Option<Stop>, LedgerError> {
    Ok(store
        .db
        .query_row(
            "SELECT t.id, t.number, t.stop_seq FROM task t
             WHERE t.assignee_id = ?1 AND t.state IN ('queued', 'working', 'waiting', 'paused')
               AND t.id = (SELECT m.task_id FROM message m
                           WHERE m.recipient_id = ?1 AND m.kind = 'task' AND m.task_id IS NOT NULL
                             AND m.state IN ('delivering', 'delivered', 'read')
                           ORDER BY m.id DESC LIMIT 1)",
            [participant_id],
            |row| {
                Ok(Stop {
                    task_id: row.get("id")?,
                    number: row.get("number")?,
                    seq: row.get("stop_seq")?,
                })
            },
        )
        .optional()?)
}

/// Who asked the stop `seq` of task `number`: the pause that counted it, by the
/// chief or the human (its `by`), as its move to paused was logged; none where
/// ConsensFlow asked it (a hold, a closed window) or no move says so.
pub(crate) fn stop_asked_by(
    store: &Store,
    project_id: i64,
    number: i64,
    seq: i64,
) -> Result<Option<String>, LedgerError> {
    let by: Option<Option<String>> = store
        .db
        .query_row(
            "SELECT json_extract(data, '$.by') FROM event
             WHERE project_id = ?1 AND kind = 'task.state' AND json_valid(data)
               AND json_extract(data, '$.task') = ?2 AND json_extract(data, '$.stop') = ?3
             ORDER BY id DESC LIMIT 1",
            params![project_id, number, seq],
            |row| row.get(0),
        )
        .optional()?;
    Ok(by.flatten())
}

/// The task a participant's window is on as far as a question or a note it
/// sends goes: the one it holds (queued, working or waiting), else the paused
/// one. A question asked in a window that the pause has not yet stopped is
/// still about its task.
///
/// A member session holds at most one task at a time: every door that gives
/// it one is held to that (`require_free`). So the task found is its only
/// one, and nothing is chosen between two. A participant that holds several
/// (a member's own lane, given tasks by name, or a ledger written before the
/// rule) has the oldest found, a paused one last, and a question about
/// another of them would be attached to the wrong task.
pub(crate) fn task_in_hand(
    store: &Store,
    participant_id: i64,
) -> Result<Option<TaskThread>, LedgerError> {
    task_found(
        store,
        "SELECT project_id, number FROM task
       WHERE assignee_id = ? AND state IN ('queued', 'working', 'waiting', 'paused')
       ORDER BY state = 'paused', id LIMIT 1",
        participant_id,
    )
}

/// The paused task a participant still holds, or none. Node's ledger asks it
/// of each look at a window; this engine asks the task's stops (`stop_of`)
/// instead, and it stays for the recordings that call it.
pub(crate) fn paused_task(
    store: &Store,
    participant_id: i64,
) -> Result<Option<TaskThread>, LedgerError> {
    let found = store
        .db
        .query_row(
            "SELECT project_id, number FROM task WHERE assignee_id = ? AND state = 'paused'
       ORDER BY id LIMIT 1",
            [participant_id],
            |row| {
                Ok((
                    row.get::<_, i64>("project_id")?,
                    row.get::<_, i64>("number")?,
                ))
            },
        )
        .optional()?;
    match found {
        Some((project_id, number)) => task(store, project_id, number),
        None => Ok(None),
    }
}

/// The words that resume a paused task (`message`: the resume queued them as it moved
/// the task from paused to queued), given up before its window had them (`reason`): the
/// task is paused again, never failed, as a task whose window closes is (field 3.0.6),
/// and not failed as a first brief that never reached its window fails its task. The
/// words fail, for they never went in, and its requester is told so, and why, in the
/// note of the pause (the engine's, [`resumed_in_vain`]); what they carried waits for
/// the next resume, which carries it
/// (`fold`). Whether they were such words (T-77: a resume whose window never came up
/// failed its task, and left the words kept for it queued under a failed task).
pub(crate) fn pause_again(
    store: &mut Store,
    message: &MessageView,
    task: &TaskRow,
    reason: &str,
) -> Result<bool, LedgerError> {
    if message.kind != "task" || task.state != "queued" {
        return Ok(false);
    }
    let resumed = store
        .db
        .prepare(
            "SELECT 1 FROM event
       WHERE project_id = ? AND kind = 'task.state' AND json_extract(data, '$.task') = ?
         AND json_extract(data, '$.from') = 'paused' AND json_extract(data, '$.to') = 'queued'
         AND json_extract(data, '$.message') = ?",
        )?
        .exists(params![task.project_id, task.number, message.id])?;
    if !resumed {
        return Ok(false);
    }
    store.db.execute(
        "UPDATE message SET state = 'failed', reason = ? WHERE id = ?",
        params![reason, message.id],
    )?;
    store.log(
        message.project_id,
        "delivery.failed",
        json!({ "message": message.id, "reason": reason }),
    )?;
    release_carried(store, task.id)?;
    let mut detail = Map::new();
    detail.insert("by".into(), Value::Null);
    detail.insert("because".into(), json!(resumed_in_vain(message, reason)));
    pause(store, task, detail, false)?;
    Ok(true)
}

/// Why a task is paused again whose resume's words, `message`, never reached its
/// window (`reason`, on one line): what its pause says ([`pause_again`]), and its
/// note, which the engine writes with the stalls it tells of together.
pub fn resumed_in_vain(message: &MessageView, reason: &str) -> String {
    let reason = reason.lines().collect::<Vec<_>>().join(" ");
    format!(
        "the words that resumed it, m-{}, did not reach @{}'s window: {reason}",
        message.id, message.recipient
    )
}

/// Whether a tell for this task has reached the participant's window since
/// the task was last paused. Kept as `paused_task` is.
pub(crate) fn told_since_paused(
    store: &Store,
    participant_id: i64,
    task_id: i64,
) -> Result<bool, LedgerError> {
    Ok(store
        .db
        .prepare(
            "SELECT 1 FROM message q JOIN task t ON t.id = q.task_id
           WHERE q.recipient_id = ? AND q.task_id = ?
             AND q.kind = 'question' AND q.urgent = 1
             AND q.state IN ('delivering', 'delivered', 'read')
             AND q.created_at >= t.paused_at",
        )?
        .exists(params![participant_id, task_id])?)
}

/// A task its window went on with by itself, in a turn of its own that got
/// through: Claude Code continues once its limit resets, or the human typed into
/// it (T-95). Held for its member's quota, it goes on with no words, its
/// requester told so as a resume tells it; resumed by the daemon with words still
/// queued that carry nothing, those are withdrawn, never to follow the turn.
/// Anything else is refused: words that carry more, or are on their way, go in.
pub(crate) fn go_on_in_window(
    store: &mut Store,
    project_id: i64,
    number: i64,
) -> Result<TaskView, LedgerError> {
    store.write(|store| {
        let task = store.task_row(project_id, number)?;
        let Some(window) = task.assignee_id else {
            return Err(not_going_on(number));
        };
        let held: bool = store.db.query_row(
            "SELECT held_until IS NOT NULL FROM task WHERE id = ?",
            [task.id],
            |row| row.get(0),
        )?;
        match task.state.as_str() {
            "paused" if held => {
                leave_pause_notes(
                    store,
                    &task,
                    "its window went on by itself",
                    Afterwards::MovesOn,
                )?;
                store.move_task(
                    &task,
                    "queued",
                    json!({ "by": null, "because": "its window went on by itself" }),
                )?;
                tell_goes_on(store, &task)?;
            }
            "queued" => {
                let fixed = format!("Resumed: {RESUME_WORDS}");
                // Each task message for the window not yet pasted: its id, and
                // whether it is the daemon's bare resume, queued.
                let words: Vec<(i64, bool)> = store
                    .db
                    .prepare(
                        "SELECT m.id, m.state = 'queued' AND m.body = ? AND m.sender_id IS NULL
                                AND NOT EXISTS (SELECT 1 FROM message c WHERE c.carried_by = m.id)
                           FROM message m
                          WHERE m.task_id = ? AND m.recipient_id = ? AND m.kind = 'task'
                            AND m.carried_by IS NULL
                            AND m.state IN ('queued', 'gated', 'delivering')",
                    )?
                    .query_map(params![fixed, task.id, window], |row| {
                        Ok((row.get(0)?, row.get(1)?))
                    })?
                    .collect::<rusqlite::Result<_>>()?;
                if words.is_empty() || !words.iter().all(|(_, bare)| *bare) {
                    return Err(not_going_on(number));
                }
                for (id, ..) in &words {
                    cancel_message(store, *id, "its window went on by itself")?;
                }
                tell_goes_on(store, &task)?;
            }
            _ => return Err(not_going_on(number)),
        }
        reconcile(store, task.id)?;
        task_by_id(store, task.id)
    })
}

/// A task the daemon resumed went on: its window got a turn through after the
/// words that resumed it. Its requester, told it waits, is told once it goes on.
pub(crate) fn tell_it_goes_on(
    store: &mut Store,
    project_id: i64,
    number: i64,
) -> Result<(), LedgerError> {
    store.write(|store| {
        let task = store.task_row(project_id, number)?;
        tell_goes_on(store, &task)
    })
}

/// The refusal of a task that does not go on in its window by itself.
fn not_going_on(number: i64) -> LedgerError {
    LedgerError::refused_with(
        "not-held",
        format!("T-{number} is neither held nor waiting for the bare words that resume it"),
        409,
    )
}

/// A paused task goes on with the words that resume it:
/// - into the same window when its session is still there: the words are a
///   task message of their own, which carries what the window kept, and the
///   task's brief first when none is received or on its way;
/// - back on the board for its tier when the session has ended, with what
///   the window kept in its brief, once; a task given by name has no other
///   session to go to, and is refused.
///
/// A session at work on another task is refused, as a door refuses it
/// (`require_free`): a follow-up paused while it waited on the board for its
/// needs may find its session on a task given it since.
///
/// `by` is none when the daemon resumes a held task in its own name. Whoever
/// resumes it, what its requester was told of the pause and has not been
/// given yet is withdrawn: it would arrive after the pause was over. What the
/// daemon resumes in the window it was held with is told to a requester who
/// was given the note that it waits (`tell_goes_on`): the time that note named
/// was only when the member was expected back.
pub(crate) fn resume_task(
    store: &mut Store,
    project_id: i64,
    number: i64,
    by: Option<&str>,
    body: &str,
) -> Result<TaskMoved, LedgerError> {
    model::require_text(body, "body", MAX_BODY)?;
    store.write(|store| {
        let author = by
            .map(|by| store.participant_by_handle(project_id, by))
            .transpose()?;
        let task = store.task_row(project_id, number)?;
        require_task_state(&task, &["paused"], "resume")?;
        // A refusal below writes nothing, this withdrawal with it.
        leave_pause_notes(store, &task, "resumed", Afterwards::MovesOn)?;
        let assignee = task
            .assignee_id
            .map(|id| store.participant_row(id))
            .transpose()?;
        let Some(assignee) = assignee else {
            let at = store.at();
            store.db.execute(
                "UPDATE task SET body = ?, updated_at = ? WHERE id = ?",
                params![format!("{}\n\nResumed: {body}", task.body), at, task.id],
            )?;
            store.move_task(&task, "open", json!({ "by": by }))?;
            return Ok(TaskMoved {
                task: task_by_id(store, task.id)?,
                message: None,
                withdrawn: Vec::new(),
                landing: Vec::new(),
            });
        };
        if assignee.left_at.is_some() {
            if task.pool.is_none() {
                return Err(LedgerError::refused_with(
                    "session-ended",
                    format!(
                        "the window that had T-{number} has ended: cancel it and open the work for its tier"
                    ),
                    409,
                ));
            }
            let carried = transfer(store, &task, &assignee, &delivery_body(&task))?;
            let at = store.at();
            // Taken from its session: no Resume of its project is to resume it.
            store.db.execute(
                "UPDATE task SET assignee_id = NULL, state = 'open', body = ?, updated_at = ?,
             paused_with_project = 0
           WHERE id = ?",
                params![
                    format!(
                        "{}{}\n\nResumed after a pause, in a fresh window (the one that had it ended; check the working tree for partial changes): {body}",
                        task.body, carried.kept
                    ),
                    at,
                    task.id
                ],
            )?;
            store.log(
                project_id,
                "task.state",
                json!({ "task": number, "from": "paused", "to": "open", "by": by }),
            )?;
            if !carried.gated.is_empty() {
                let to = match &author {
                    Some(author) => author.handle.clone(),
                    None => store.participant_row(task.requester_id)?.handle,
                };
                send(
                    store,
                    project_id,
                    &Sent {
                        to: &to,
                        task: Some(number),
                        kind: "note",
                        body: &format!(
                            "Withdrawn with it, still waiting for the human: {}.",
                            m_list(&carried.gated)
                        ),
                        ..Sent::default()
                    },
                )?;
            }
            return Ok(TaskMoved {
                task: task_by_id(store, task.id)?,
                message: None,
                withdrawn: Vec::new(),
                landing: Vec::new(),
            });
        }
        require_free(store, &assignee, Giving::Resumed(number))?;
        let words = carrier_body(store, &task, assignee.id, &format!("Resumed: {body}"))?;
        let message_id = queue(
            store,
            project_id,
            &Queued {
                to: assignee.id,
                from: author.as_ref().map(|author| author.id),
                kind: "task",
                task_id: Some(task.id),
                body: &words,
                ..Queued::default()
            },
        )?;
        fold(store, &task, assignee.id, message_id)?;
        store.move_task(
            &task,
            "queued",
            json!({ "by": by, "message": message_id }),
        )?;
        Ok(TaskMoved {
            task: task_by_id(store, task.id)?,
            message: store.message(message_id)?,
            withdrawn: Vec::new(),
            landing: Vec::new(),
        })
    })
}

/// The human's Resume of a project, in the write that opens it: each task its
/// suspension paused ([`pause_with_project`]) is at work again, as a quit leaves
/// a task (waiting, where its window asked a question nobody has answered), so
/// its window comes back with the project on its own conversation, as each
/// window with a task at work or waiting does. The words that told its requester
/// of the pause were never given (a suspended project delivers nothing): they
/// are withdrawn, as they are when it is resumed. A task whose session left the
/// project meanwhile has no window to come back in, and stays paused, for its
/// requester to resume or call off.
///
/// A window that holds more than one task comes back for none of them
/// ([`leave_several_paused`]), and what an earlier build or an earlier Resume left
/// queued telling a window to go on goes where it is not for the one task its
/// window holds at work (`continuations`): the windows are told anew as they come
/// back.
pub(crate) fn resume_with_project(store: &mut Store, project_id: i64) -> Result<(), LedgerError> {
    leave_several_paused(store, project_id)?;
    let numbers: Vec<i64> = store
        .db
        .prepare(
            "SELECT t.number FROM task t JOIN participant p ON p.id = t.assignee_id
             WHERE t.project_id = ? AND t.state = 'paused' AND t.paused_with_project = 1
               AND p.left_at IS NULL
             ORDER BY t.id",
        )?
        .query_map([project_id], |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    for number in numbers {
        let task = store.task_row(project_id, number)?;
        leave_pause_notes(store, &task, "resumed", Afterwards::MovesOn)?;
        store.move_task(
            &task,
            "working",
            json!({ "by": null, "because": "the project was resumed" }),
        )?;
        reconcile(store, task.id)?;
    }
    withdraw_stale(store, project_id)
}

/// The Resume's part of a window that holds more than one task (a session a
/// ledger wrote before the rule of one, or a member's own lane, which holds to
/// none): no word tells it to go on, as none could tell it which of its tasks
/// still stand (the lead's design of 2026-10-17, after both reviews of four
/// rounds found a continuation naming several tasks escaping one check or
/// another). Each of its tasks is its requester's to resume, task by task, and
/// none goes on by itself (both reviews of 2026-10-17: a queued one was handed its
/// window by the next pass):
/// - one its project's suspension paused stays paused, the pause its requester's
///   from now on;
/// - one queued or at work (a quit leaves it so) is paused now;
/// - one paused any other way stays as it was, its pause told already.
///
/// The requester is told of the first two in one note of all it is told of,
/// which says to resume them one at a time: the session goes on with one, and
/// takes the next once that one has its result (`staff`'s `require_free`). What
/// the suspension's note said of a task goes, but for a warning, which is said
/// again (`leave_pause_notes`).
fn leave_several_paused(store: &mut Store, project_id: i64) -> Result<(), LedgerError> {
    let held = sql_list(&[&HELD_TASK_STATES[..], &["paused"]].concat());
    let lanes: Vec<(i64, String)> = store
        .db
        .prepare(&format!(
            "SELECT p.id, p.handle FROM participant p
             WHERE p.project_id = ? AND p.role NOT IN ('chief', 'human')
               AND (SELECT COUNT(*) FROM task t
                    WHERE t.assignee_id = p.id AND t.state IN ({held})) > 1
             ORDER BY p.id"
        ))?
        .query_map([project_id], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let mut told: Vec<(i64, i64)> = Vec::new();
    for (lane, handle) in lanes {
        let numbers: Vec<i64> = store
            .db
            .prepare(&format!(
                "SELECT number FROM task WHERE assignee_id = ? AND state IN ({held}) ORDER BY number"
            ))?
            .query_map([lane], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        let named: Vec<String> = numbers.iter().map(|number| format!("T-{number}")).collect();
        let because = format!(
            "@{handle}'s window held {}, and a window is told to go on only when it held one task: resume them one at a time",
            named.join(", ")
        );
        for number in numbers {
            let task = store.task_row(project_id, number)?;
            let left = match task.state.as_str() {
                "paused" => {
                    let suspension = store.db.execute(
                        "UPDATE task SET paused_with_project = 0
                         WHERE id = ? AND paused_with_project = 1",
                        [task.id],
                    )? > 0;
                    if suspension {
                        leave_pause_notes(store, &task, "stays paused", Afterwards::StaysPaused)?;
                    }
                    suspension
                }
                "queued" | "working" | "waiting" => {
                    pause_task_as(store, project_id, number, None, Some(&because), false)?;
                    true
                }
                _ => false,
            };
            if left {
                tell_left_paused(store, &mut told, &task, &because)?;
            }
        }
    }
    Ok(())
}

/// Tells `task`'s requester that the Resume left it paused, `because`: in the
/// note of this Resume it was told in already (`told`, by requester), or in one
/// of its own.
fn tell_left_paused(
    store: &mut Store,
    told: &mut Vec<(i64, i64)>,
    task: &TaskRow,
    because: &str,
) -> Result<(), LedgerError> {
    let earlier = told
        .iter()
        .find(|(requester, _)| *requester == task.requester_id)
        .map(|(_, note)| *note);
    if let Some(note) = earlier {
        if join_pause_note(store, note, task.number, because)?.is_some() {
            return Ok(());
        }
    }
    let to = store.participant_row(task.requester_id)?.handle;
    let note = note_pause(store, task.project_id, &to, task.number, because)?;
    told.push((task.requester_id, note.id));
    Ok(())
}

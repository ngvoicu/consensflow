//! Tasks, along the state machine in the crate's rules: given by a coordinator
//! to a participant by name, or opened for a pool and tier of the staff and
//! assigned by the daemon, or taken back to the board (`giving`); paused, held
//! and resumed (`pausing`); finished with a result and accepted, sent back,
//! cancelled, failed, and deleted from the board by the human (`finishing`),
//! the chief's own step by its word (`own_steps`); their briefs replaced before
//! a window has them (`editing`); and read with their threads.

mod editing;
mod finishing;
mod giving;
mod outage;
mod own_steps;
mod pausing;

use cf_proto::ledger::{MessageView, TaskThread, TaskView};
use rusqlite::{params, OptionalExtension};

use crate::model::LedgerError;
use crate::store::Store;
use crate::views::{message_view, task_view, TaskRow, MESSAGE_SELECT, TASK_SELECT};

pub(crate) use editing::{edit_task, edit_task_with, retitle_task, tell_open};
pub use finishing::reopen_or_accept;
pub(crate) use finishing::{
    accept_failed_task, accept_task, call_off, cancel_task, delete_tasks, fail_task, give_up_task,
    left_running, note_stranded_waits, record_finished, record_result, release_ready, reopen_task,
    tell_stranded_waiters, tell_waiting_for_failed, waiting_for_it_of, Ending,
};
pub use giving::{asked_role, NewTask};
pub(crate) use giving::{
    assign_task, check_release, create_task, release_inside, release_task, retier_open_task,
};
pub(crate) use outage::{no_longer_failed, no_longer_waiting, show_closed, tell_closed};
pub use outage::{window_closed, SAME_ERROR_WITHIN_MS};
pub(crate) use own_steps::withdrawn_late;
pub(crate) use pausing::{
    clear_hold, go_on_in_window, held_for_context, held_tasks_due, hold_for_context, hold_task,
    pause_again, pause_task, pause_task_naming, pause_with_project, paused_by_consensflow,
    paused_task, relaunch_stalled, rest_while_resumed, resume_task, resume_with_project,
    resume_words, resume_words_not_in, stop_asked_by, stop_of, take_over_pause, task_in_hand,
    task_relaunched, tell_it_goes_on, told_since_paused, unmark_pause, was_relaunched,
    work_while_resumed,
};
pub use pausing::{resumed_in_vain, NEVER_BEGAN};

/// What a paused task's window is told when it goes on: the human's Resume
/// and the daemon's alike.
pub const RESUME_WORDS: &str = "Go on where you stopped.";

/// What a critical task's lead holds it to, in the role texts' own words: it
/// changes no project file — true of every project, where forbidding
/// specifications presumed one, and the word the advisor's and the reviewer's
/// texts use, since a critical reviewer puts a long log in a scratch file —
/// and its findings go to the chief, whom the role texts name.
const CRITICAL_RULE: &str =
    "It changes no project file: return analysis, evidence and recommendations to the chief.";

/// How a task reads when it is handed over: critical work leads with its purpose.
fn delivery_body(task: &TaskRow) -> String {
    match &task.purpose {
        None => task.body.clone(),
        Some(purpose) => format!("Critical work: {purpose}. {CRITICAL_RULE}\n\n{}", task.body),
    }
}

/// The words that send a window on, as the task message that carries them
/// reads: with the task's brief before them when no task message for the
/// window was received or is on its way, so that the body never builds a
/// brief that a row still holds.
fn carrier_body(
    store: &Store,
    task: &TaskRow,
    window: i64,
    words: &str,
) -> Result<String, LedgerError> {
    let held = store
        .db
        .prepare(
            "SELECT 1 FROM message WHERE task_id = ? AND recipient_id = ? AND kind = 'task'
               AND state IN ('delivered', 'read', 'queued', 'gated', 'delivering')",
        )?
        .exists(params![task.id, window])?;
    Ok(if held {
        words.to_owned()
    } else {
        format!("{}\n\n{words}", delivery_body(task))
    })
}

/// Messages by their numbers, as a note names them: `m-3, m-5`.
fn m_list(ids: &[i64]) -> String {
    ids.iter()
        .map(|id| format!("m-{id}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The asides of task `task_id` end, `at`: a note of ConsensFlow's own
/// that was shown, not typed (`queue`'s `SHOWN`), says what its task's line says only while
/// it holds, and it holds until the task moves on. The note stays, as it was given; its
/// receipt says since when it is over. Any timed background reminders not handed over are
/// withdrawn with it; a handed reminder that comes back later is withdrawn on its return.
pub(crate) fn end_asides(store: &mut Store, task_id: i64, at: &str) -> Result<(), LedgerError> {
    withdraw_background_reminders(store, task_id)?;
    store.db.execute(
        "UPDATE message SET receipt = json_set(receipt, '$.over', ?1)
         WHERE task_id = ?2 AND state = 'read' AND sender_id IS NULL AND kind = 'note'
           AND json_valid(receipt) AND json_extract(receipt, '$.shown') IS NOT NULL
           AND json_extract(receipt, '$.over') IS NULL",
        params![at, task_id],
    )?;
    Ok(())
}

/// Withdraw unhanded reminders of a stretch that has ended. The words identify the engine's
/// own timed reminders, including ones an older build queued without a receipt marker.
/// `handed_at` is stamped on a carrier's rows too, so a paste already handed stays intact.
pub(crate) fn withdraw_background_reminders(
    store: &mut Store,
    task_id: i64,
) -> Result<(), LedgerError> {
    let (project, number): (i64, i64) = store.db.query_row(
        "SELECT project_id, number FROM task WHERE id = ?",
        [task_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let prefix = format!("T-{number} has been working in the background for ");
    let reminders: Vec<(i64, bool)> = store
        .db
        .prepare(
            "SELECT id, handed_at IS NOT NULL FROM message
         WHERE task_id = ?1 AND kind = 'note' AND sender_id IS NULL
           AND state IN ('queued', 'delivering') AND claimed_at IS NULL
           AND substr(body, 1, length(?2)) = ?2 ORDER BY id",
        )?
        .query_map(params![task_id, prefix], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?
        .collect::<rusqlite::Result<_>>()?;
    let ids: Vec<i64> = reminders
        .iter()
        .filter_map(|(id, handed)| (!handed).then_some(*id))
        .collect();
    for (id, handed) in reminders {
        if handed {
            // Keep the already handed paste intact. If it is later refused, this durable
            // event lets the return path withdraw it instead of typing it a second time.
            store.log(
                project,
                "background.reminder_ended",
                serde_json::json!({ "message": id }),
            )?;
        }
    }
    crate::messages::withdraw_each(
        store,
        project,
        &ids,
        &format!("T-{number}'s background-hold stretch ended"),
    )
}

/// A handed reminder returned to the queue after its stretch ended.
pub(crate) fn returned_background_reminder(
    store: &Store,
    message: i64,
) -> Result<bool, LedgerError> {
    Ok(store
        .db
        .prepare(
            "SELECT 1 FROM message m JOIN event e ON e.project_id = m.project_id
           AND e.kind = 'background.reminder_ended' AND json_extract(e.data, '$.message') = m.id
         WHERE m.id = ? AND m.state IN ('queued', 'delivering') AND m.handed_at IS NULL",
        )?
        .exists([message])?)
}

/// An exchange ends a stretch as soon as it is sent by its worker or received by it.
/// The two directions have different receipt rules, just as the engine's stretch clock does.
pub(crate) fn background_exchange(
    store: &mut Store,
    message: i64,
    received: bool,
) -> Result<(), LedgerError> {
    let task: Option<i64> = store
        .db
        .query_row(
            "SELECT m.task_id FROM message m JOIN task t ON t.id = m.task_id
          WHERE m.id = ?1 AND m.sender_id IS NOT NULL AND (
            (?2 = 1 AND m.recipient_id = t.assignee_id AND m.state IN ('delivered', 'read'))
            OR (?2 = 0 AND m.sender_id = t.assignee_id AND m.recipient_id = t.requester_id
                AND m.state IN ('queued', 'delivering', 'delivered', 'read')))",
            params![message, received],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(task) = task {
        withdraw_background_reminders(store, task)?;
    }
    Ok(())
}

/// End one shown note when the thing it described ends, leaving other task asides alone.
pub(crate) fn end_aside(store: &mut Store, project: i64, aside: i64) -> Result<(), LedgerError> {
    store.write(|store| {
        let at = store.at();
        store.db.execute(
            "UPDATE message SET receipt = json_set(receipt, '$.over', ?1)
             WHERE id = ?2 AND project_id = ?3 AND state = 'read' AND sender_id IS NULL
               AND kind = 'note' AND json_valid(receipt)
               AND json_extract(receipt, '$.shown') IS NOT NULL
               AND json_extract(receipt, '$.over') IS NULL",
            params![at, aside, project],
        )?;
        Ok(())
    })
}

/// The note T-`number`'s line says now: its newest note shown, not typed, that holds (the
/// one `TaskView::aside` is read from). None where it says none.
pub(crate) fn aside_note(
    store: &Store,
    project_id: i64,
    number: i64,
) -> Result<Option<MessageView>, LedgerError> {
    let task = store.task_row(project_id, number)?;
    let id: Option<i64> = store
        .db
        .query_row(
            "SELECT id FROM message
             WHERE task_id = ? AND state = 'read' AND sender_id IS NULL AND kind = 'note'
               AND json_valid(receipt) AND json_extract(receipt, '$.shown') IS NOT NULL
               AND json_extract(receipt, '$.over') IS NULL
             ORDER BY id DESC LIMIT 1",
            [task.id],
            |row| row.get(0),
        )
        .optional()?;
    match id {
        Some(id) => store.message(id),
        None => Ok(None),
    }
}

/// A worker held again: update the line and count the re-hold without restarting the
/// stretch or its reminder. The shown note's body stays as it was written.
pub(crate) fn renew_aside(
    store: &mut Store,
    project_id: i64,
    aside: i64,
    said: &str,
) -> Result<bool, LedgerError> {
    store.write(|store| {
        let renewed = store.db.execute(
            "UPDATE message
                SET receipt = json_set(receipt, '$.shown', ?1, '$.reholds',
                    COALESCE(json_extract(receipt, '$.reholds'), 0) + 1)
              WHERE id = ?2 AND project_id = ?3 AND state = 'read' AND json_valid(receipt)
                AND json_extract(receipt, '$.shown') IS NOT NULL
                AND json_extract(receipt, '$.over') IS NULL",
            params![said, aside, project_id],
        )?;
        Ok(renewed == 1)
    })
}

/// The task exchanged a message, then held: the same shown note says the line, but
/// this is the first hold of a new stretch. A previous reminder belongs to the old one.
pub(crate) fn restart_aside(
    store: &mut Store,
    project_id: i64,
    aside: i64,
    said: &str,
) -> Result<bool, LedgerError> {
    store.write(|store| {
        let at = store.at();
        let restarted = store.db.execute(
            "UPDATE message
                SET receipt = json_remove(json_set(receipt, '$.shown', ?1, '$.since', ?2,
                    '$.reholds', 0, '$.stretch', COALESCE(json_extract(receipt, '$.stretch'), 0) + 1),
                    '$.reminded', '$.reminded_at')
              WHERE id = ?3 AND project_id = ?4 AND state = 'read' AND json_valid(receipt)
                AND json_extract(receipt, '$.shown') IS NOT NULL
                AND json_extract(receipt, '$.over') IS NULL",
            params![said, at, aside, project_id],
        )?;
        Ok(restarted == 1)
    })
}

/// A newly shown hold continues a stretch that began in an immediately typed hold.
/// Carry its time, count and last reminder without changing the new note's line.
pub(crate) fn carry_aside(
    store: &mut Store,
    project_id: i64,
    aside: i64,
    since: &str,
    reholds: i64,
    previous: Option<(i64, &str)>,
) -> Result<bool, LedgerError> {
    store.write(|store| {
        let (id, at) = previous.map_or((None, None), |(id, at)| (Some(id), Some(at)));
        let carried = store.db.execute(
            "UPDATE message
                SET receipt = json_set(receipt, '$.since', ?1, '$.reholds', ?2,
                    '$.reminded', ?3, '$.reminded_at', ?4)
              WHERE id = ?5 AND project_id = ?6 AND state = 'read' AND json_valid(receipt)
                AND json_extract(receipt, '$.shown') IS NOT NULL
                AND json_extract(receipt, '$.over') IS NULL",
            params![since, reholds, id, at, aside, project_id],
        )?;
        Ok(carried == 1)
    })
}

/// Note `aside`, shown and not typed, whose line a move of its task ended (`$.over`), holds
/// again from now (`$.since`), with no reminder sent of it since: the task's line says it
/// once more, and nothing is typed. False where it holds already or was never shown.
pub(crate) fn show_again(
    store: &mut Store,
    project_id: i64,
    aside: i64,
) -> Result<bool, LedgerError> {
    store.write(|store| {
        let at = store.at();
        let again = store.db.execute(
            "UPDATE message
                SET receipt = json_remove(json_set(receipt, '$.since', ?1), '$.over',
                    '$.reminded', '$.reminded_at', '$.reholds')
              WHERE id = ?2 AND project_id = ?3 AND state = 'read' AND sender_id IS NULL
                AND kind = 'note' AND json_valid(receipt)
                AND json_extract(receipt, '$.shown') IS NOT NULL
                AND json_extract(receipt, '$.over') IS NOT NULL",
            params![at, aside, project_id],
        )?;
        Ok(again == 1)
    })
}

/// Send a due reminder of `aside`, provided the last reminder is still `previous` and
/// the stretch is still `stretch`.
/// The comparison and write are one ledger transaction, so two looks cannot send the
/// same reminder. Its time and id stay in the receipt for the next interval.
pub(crate) fn remind_of_aside(
    store: &mut Store,
    project_id: i64,
    aside: i64,
    stretch: i64,
    previous: Option<i64>,
    note: &crate::NewNote,
) -> Result<Option<MessageView>, LedgerError> {
    store.write(|store| {
        let holds: bool = store.db.query_row(
            "SELECT EXISTS (SELECT 1 FROM message
               WHERE id = ? AND project_id = ? AND state = 'read' AND json_valid(receipt)
                 AND json_extract(receipt, '$.shown') IS NOT NULL
                 AND json_extract(receipt, '$.over') IS NULL
                 AND COALESCE(json_extract(receipt, '$.stretch'), 0) = ?
                 AND json_extract(receipt, '$.reminded') IS ?)",
            params![aside, project_id, stretch, previous],
            |row| row.get(0),
        )?;
        if !holds {
            return Ok(None);
        }
        let sent = crate::messages::note(store, project_id, note)?;
        store.db.execute(
            "UPDATE message SET receipt = json_set(receipt, '$.reminded', ?, '$.reminded_at', ?)
             WHERE id = ?",
            params![sent.id, sent.created_at, aside],
        )?;
        Ok(Some(sent))
    })
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

/// The task a participant's window was last given, whatever its state now, or
/// none: the one the newest task message that began to reach it (being pasted,
/// pasted or read) was about. A task message queued for it since is not given
/// yet: a cancel that frees a session releases what waited on the board for it
/// in the same step, and the window is on the cancelled task until it is
/// stopped.
pub(crate) fn task_given(
    store: &Store,
    participant_id: i64,
) -> Result<Option<TaskThread>, LedgerError> {
    task_found(
        store,
        "SELECT t.project_id, t.number FROM message m JOIN task t ON t.id = m.task_id
       WHERE m.recipient_id = ? AND m.kind = 'task'
         AND m.state IN ('delivering', 'delivered', 'read')
       ORDER BY m.id DESC LIMIT 1",
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
pub(crate) fn require_task_state(
    task: &TaskRow,
    states: &[&str],
    action: &str,
) -> Result<(), LedgerError> {
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

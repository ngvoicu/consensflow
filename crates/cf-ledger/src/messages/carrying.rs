//! Rows that ride in another's paste. A row *kept* for a window (a task, an
//! answer or a note for it about its task, not urgent, still `queued`) is
//! never lost to a pause: when the window goes on, what it keeps goes in
//! with the words that send it on, in one paste with one marker, and what
//! the paste proves arrived is each of them. The carrier is a top-level task
//! message (`carried_by IS NULL`) that other rows point at; what it carries
//! is flat (a constituent is never a carrier), and is told only when its
//! delivery begins, from the rows still `queued`: the set is frozen from
//! begin to confirm, and a row that comes later is pasted on its own.

use cf_base::text::utf16_prefix;
use cf_proto::ledger::MessageView;
use rusqlite::params;
use serde_json::json;

use super::delivery::cancel_message;
use super::messages;
use crate::model::LedgerError;
use crate::store::Store;
use crate::tasks::RESUME_WORDS;
use crate::views::{message_view, ParticipantRow, TaskRow, MESSAGE_SELECT};

/// How much of a question an answer's label quotes, in UTF-16 units.
const QUESTION_QUOTED: usize = 160;

/// The rows of one task for one window that a carrier may take in: what is
/// kept (a task, an answer or a note, never a tell, still waiting its turn).
const KEPT: &str = "task_id = ?1 AND recipient_id = ?2 AND urgent = 0
     AND kind IN ('task', 'answer', 'note') AND state = 'queued'";

/// Every id a query finds, in the order it says.
fn ids(store: &Store, sql: &str, values: impl rusqlite::Params) -> Result<Vec<i64>, LedgerError> {
    Ok(store
        .db
        .prepare(sql)?
        .query_map(values, |row| row.get::<_, i64>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?)
}

/// The rows a carrier carries and has not yet delivered, oldest first: what
/// its paste is made of besides its own words.
pub(crate) fn carried(store: &Store, carrier: i64) -> Result<Vec<MessageView>, LedgerError> {
    Ok(store
        .db
        .prepare(&format!(
            "{MESSAGE_SELECT} WHERE m.carried_by = ? AND m.state = 'queued' ORDER BY m.id"
        ))?
        .query_map([carrier], message_view)?
        .collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Takes into `carrier`, the task message a resume or a reopening just
/// queued for `recipient`, everything its window keeps about `task`:
/// - every kept row, top-level or carried by an older carrier, now rides in
///   `carrier`; a carrier in an older paste that is still on its way is left
///   with its own;
/// - an older carrier that says only what the daemon says when it resumes a
///   hold (`Resumed: Go on where you stopped.`) is superseded, not carried:
///   two of them say what one does;
/// - what is not kept stays where it is: a tell, a row still held at the gate,
///   and a row in flight;
/// - what Node's window read at once with no proof it arrived, after the
///   task was last paused, was read by a door that was dead by then, and is
///   kept again first.
pub(crate) fn fold(
    store: &mut Store,
    task: &TaskRow,
    recipient: i64,
    carrier: i64,
) -> Result<(), LedgerError> {
    let requeued = requeue_unreceived_reads(store, task, recipient)?;
    let fixed = format!("Resumed: {RESUME_WORDS}");
    let superseded = ids(
        store,
        "SELECT id FROM message WHERE task_id = ?1 AND recipient_id = ?2 AND kind = 'task'
           AND state = 'queued' AND carried_by IS NULL AND id != ?3 AND body = ?4 ORDER BY id",
        params![task.id, recipient, carrier, fixed],
    )?;
    for id in &superseded {
        cancel_message(store, *id, &format!("superseded by m-{carrier}"))?;
    }
    let taken = ids(
        store,
        &format!(
            "SELECT id FROM message WHERE {KEPT} AND id != ?3
               AND (carried_by IS NULL
                    OR carried_by IN (SELECT id FROM message WHERE state != 'delivering'))
             ORDER BY id"
        ),
        params![task.id, recipient, carrier],
    )?;
    for id in &taken {
        store.db.execute(
            "UPDATE message SET carried_by = ? WHERE id = ?",
            params![carrier, id],
        )?;
    }
    if !(taken.is_empty() && superseded.is_empty() && requeued.is_empty()) {
        store.log(
            task.project_id,
            "message.carried",
            json!({
                "carrier": carrier,
                "carried": taken,
                "superseded": superseded,
                "requeued": requeued,
            }),
        )?;
    }
    Ok(())
}

/// What Node's ledger marked read the moment a choice answer was written,
/// with no proof, since `task` was last paused: the door that asked had been
/// shut by the pause, so nobody read it. Kept for `recipient` again, in the
/// order of their ids.
fn requeue_unreceived_reads(
    store: &mut Store,
    task: &TaskRow,
    recipient: i64,
) -> Result<Vec<i64>, LedgerError> {
    let Some(paused_at) = &task.paused_at else {
        return Ok(Vec::new());
    };
    let unread = ids(
        store,
        "SELECT id FROM message WHERE task_id = ?1 AND recipient_id = ?2 AND kind = 'answer'
           AND state = 'read' AND receipt IS NULL AND created_at >= ?3 ORDER BY id",
        params![task.id, recipient, paused_at],
    )?;
    for id in &unread {
        store
            .db
            .execute("UPDATE message SET state = 'queued' WHERE id = ?", [id])?;
    }
    Ok(unread)
}

/// A row that has just become kept (an answer or a note that landed `queued`,
/// a message the human approved, a delivery tried again) joins the task
/// message that waits for its window, `queued` or `gated`:
/// - an answer or a note joins the newest one;
/// - a task message joins only one newer than itself;
/// - one that already carries rows brings them along, so the set stays flat;
/// - one in flight adopts nothing.
pub(crate) fn adopt(store: &mut Store, id: i64) -> Result<(), LedgerError> {
    let adopted: Option<i64> = store
        .db
        .prepare(
            "SELECT c.id FROM message m
             JOIN message c ON c.task_id = m.task_id AND c.recipient_id = m.recipient_id
             WHERE m.id = ?1 AND m.urgent = 0 AND m.state = 'queued' AND m.carried_by IS NULL
               AND m.kind IN ('task', 'answer', 'note')
               AND c.kind = 'task' AND c.carried_by IS NULL AND c.state IN ('queued', 'gated')
               AND c.id != m.id AND (m.kind != 'task' OR c.id > m.id)
             ORDER BY c.id DESC LIMIT 1",
        )?
        .query_map([id], |row| row.get(0))?
        .next()
        .transpose()?;
    if let Some(carrier) = adopted {
        store.db.execute(
            "UPDATE message SET carried_by = ?1 WHERE id = ?2 OR carried_by = ?2",
            params![carrier, id],
        )?;
    }
    Ok(())
}

/// What a carrier that was cancelled, or failed, carried is its own again:
/// still kept, still `queued`, for the next carrier to take. Withdrawing a
/// carrier's transport never takes what it carried with it.
pub(crate) fn release_carried(store: &Store, task_id: i64) -> Result<(), LedgerError> {
    store.db.execute(
        "UPDATE message SET carried_by = NULL
         WHERE task_id = ?1 AND state = 'queued'
           AND carried_by IN (SELECT id FROM message WHERE task_id = ?1
                              AND state IN ('cancelled', 'failed'))",
        [task_id],
    )?;
    Ok(())
}

/// What a task takes on from a window that is gone: the words kept for it,
/// as the section of the brief that follows the task's own, and the rows it
/// still held at the gate that went with it.
pub(crate) struct Transfer {
    /// Empty when nothing was kept.
    pub(crate) kept: String,
    pub(crate) gated: Vec<i64>,
}

/// A task goes to another window, and what the old one kept for it goes
/// with it, once, in the brief it is given: a window that ended before it
/// received them, or one the task was taken back from, kept no more. In one
/// step:
/// - what it kept (a task, an answer or a note, not urgent, still `queued`
///   or being pasted), and what Node read at once with no proof, is cancelled
///   as carried into the next brief, which says each in turn, by id, under
///   a label of who wrote it; the task's own brief, if it is among them, is
///   not said twice;
/// - what it had held at the gate, and any tell, is withdrawn: the human
///   gave no word for the next window, and the one that is gone was the tell's;
/// - what it asked, and no one has seen yet, is withdrawn with it. Only the
///   next window's own questions count for the task.
pub(crate) fn transfer(
    store: &mut Store,
    task: &TaskRow,
    window: &ParticipantRow,
    brief: &str,
) -> Result<Transfer, LedgerError> {
    requeue_unreceived_reads(store, task, window.id)?;
    let rows = messages(
        store,
        &format!(
            "{MESSAGE_SELECT} WHERE m.task_id = ?1 AND m.recipient_id = ?2 AND m.urgent = 0
               AND m.kind IN ('task', 'answer', 'note') AND m.state IN ('queued', 'delivering')
             ORDER BY m.id"
        ),
        params![task.id, window.id],
    )?;
    let mut said = String::new();
    for row in rows
        .iter()
        .filter(|row| !(row.kind == "task" && row.body == brief))
    {
        let from = row
            .sender
            .as_ref()
            .map_or_else(|| "ConsensFlow".to_owned(), |sender| format!("@{sender}"));
        let label = match (row.kind.as_str(), row.reply_to) {
            ("answer", Some(question)) => {
                let asked = store.message(question)?.map_or_else(String::new, |asked| {
                    let first = asked.body.split('\n').next().unwrap_or_default().to_owned();
                    utf16_prefix(&first, QUESTION_QUOTED).into_owned()
                });
                format!(
                    "(answer m-{} from {from} to m-{question} of @{}: {asked})",
                    row.id, window.handle
                )
            }
            (kind, _) => format!("({kind} m-{} from {from})", row.id),
        };
        said.push_str(&format!("\n\n{label}\n{}", row.body));
    }
    let kept = if said.is_empty() {
        said
    } else {
        format!(
            "\n\nKept from before, never delivered to @{}:{said}",
            window.handle
        )
    };
    store.db.execute(
        "UPDATE message SET state = 'cancelled', reason = ?3
         WHERE task_id = ?1 AND recipient_id = ?2 AND urgent = 0
           AND kind IN ('task', 'answer', 'note') AND state IN ('queued', 'delivering')",
        params![
            task.id,
            window.id,
            format!("carried into T-{}'s brief for its next window", task.number)
        ],
    )?;
    let gated = ids(
        store,
        "SELECT id FROM message WHERE task_id = ?1 AND recipient_id = ?2 AND state = 'gated'
         ORDER BY id",
        params![task.id, window.id],
    )?;
    store.db.execute(
        "UPDATE message SET state = 'cancelled', reason = ?3
         WHERE task_id = ?1 AND recipient_id = ?2 AND state IN ('queued', 'delivering', 'gated')",
        params![
            task.id,
            window.id,
            format!("withdrawn: @{}'s window ended first", window.handle)
        ],
    )?;
    store.db.execute(
        "UPDATE message SET state = 'cancelled', reason = ?3
         WHERE task_id = ?1 AND sender_id = ?2 AND kind = 'question' AND urgent = 0
           AND state IN ('queued', 'gated')",
        params![
            task.id,
            window.id,
            format!("@{} no longer has T-{}", window.handle, task.number)
        ],
    )?;
    Ok(Transfer { kept, gated })
}

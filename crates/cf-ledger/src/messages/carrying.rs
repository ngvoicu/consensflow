//! Rows that ride in another's paste. A row *kept* for a window (a task, an
//! answer or a note for it about its task, not urgent, still `queued`) is
//! never lost to a pause: when the window goes on, what it keeps goes in
//! with the words that send it on, in one paste with one marker, and what
//! the paste proves arrived is each of them. The carrier is a top-level task
//! message (`carried_by IS NULL`) that other rows point at; what it carries
//! is flat (a constituent is never a carrier), and is told when its first
//! delivery begins, from the rows still `queued`: from then on the carrier
//! takes in no row, through every paste of it to its confirm, and a row that
//! comes later goes in a newer carrier, or is pasted on its own (`adopt`). A
//! resume that comes while a carrier waits again may still take its rows into
//! the newer carrier it queues (`fold`): a set never grows after its paste, so a
//! marker found never confirms a row the paste did not hold. A start that finds
//! the carrier still on its way settles the set with it. A set that
//! the Node daemon, which knows no carriers, left behind is released when the
//! ledger is opened (`release_stranded`).

use cf_base::text::utf16_prefix;
use cf_proto::ledger::MessageView;
use rusqlite::params;
use serde_json::json;

use super::delivery::cancel_message;
use super::messages;
use super::receipt::reconcile;
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
/// - one in flight adopts nothing, and nor does one whose paste began once and
///   that waits again: its set is what that paste held, and the marker of the
///   paste, the same in every one of its pastes, proves each row in the set
///   arrived. A row that joined it later would be taken for received where the
///   window's record shows that first paste (a review of 2026-10-16); it goes in
///   a newer carrier, or on its own. The log says which pastes began
///   (`delivery.begun`): the attempt a paste counted is given back when the window
///   went before it could land.
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
               AND NOT EXISTS (SELECT 1 FROM event e
                               WHERE e.project_id = c.project_id AND e.kind = 'delivery.begun'
                                 AND json_extract(e.data, '$.message') = c.id)
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

/// The schema of the migration (0019) that let go of what rode in every carrier on
/// its way when a build before this one stopped (`release_stranded`).
pub(crate) const RELEASED_AT: i64 = 19;

/// What a daemon that knows no carriers left riding in one. A row is in the
/// paste of a carrier while the carrier waits for it (`queued` or `gated`) and
/// while it goes in (`delivering`), and this ledger settles a carrier's set in
/// the step that delivers, cancels or fails the carrier. The Node daemon pastes
/// each row as a message of its own and never reads `carried_by`, so it can leave
/// a row `queued` under a carrier it delivered, cancelled or failed. Skipped as
/// carried, such a row would never be delivered here: when the ledger is opened,
/// the start of this daemon, each is an ordinary queued message again, delivered
/// in the order of its id. The tasks they are about are reconciled (`receipt`),
/// once each: Node moved one on when it counted a queued answer as received, and
/// that answer is still to be. A row Node was pasting on its own when it stopped
/// is `delivering`, not `queued`: it is left to the delivery that settles it, and
/// `retry_delivery` lets it go of its carrier then.
///
/// A carrier this daemon was pasting when it stopped keeps its set: the rows rode
/// in its paste, and the start settles them with it, received with it where the
/// window's record shows its marker and queued again with it where it does not.
/// Let go of it here, they went again on their own after a paste that had them (a
/// review of 2026-10-16: the answers kept for a window pasted twice). Only a carrier
/// this ledger began is so: it froze the set at the first paste. One an earlier
/// build was pasting may hold a row its paste never had (the Node daemon of
/// 3.0.0-alpha.82 pasted a carrier's words alone, and up to 3.0.4 a row could join a
/// carrier whose first paste was late and that waited again), and migration 0019 let
/// go of what each such carrier carried, in the step that wrote its version, as
/// every earlier build's start did (the reviews of 2026-10-16: a marker found then
/// took an answer no paste had held for received). The tasks those rows are about
/// are reconciled by the start that ran it (`reconcile_released`).
pub(crate) fn release_stranded(store: &mut Store) -> Result<(), LedgerError> {
    store.write(|store| {
        let carriers: Vec<(i64, i64)> = store
            .db
            .prepare(
                "SELECT DISTINCT m.carried_by, m.project_id FROM message m
                 JOIN message c ON c.id = m.carried_by
                 WHERE m.state = 'queued' AND c.state NOT IN ('queued', 'gated', 'delivering')
                 ORDER BY m.carried_by",
            )?
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        let mut tasks = Vec::new();
        for (carrier, project) in carriers {
            let released = ids(
                store,
                "SELECT id FROM message WHERE carried_by = ? AND state = 'queued' ORDER BY id",
                [carrier],
            )?;
            for task in ids(
                store,
                "SELECT DISTINCT task_id FROM message
                 WHERE carried_by = ? AND state = 'queued' AND task_id IS NOT NULL",
                [carrier],
            )? {
                if !tasks.contains(&task) {
                    tasks.push(task);
                }
            }
            store.db.execute(
                "UPDATE message SET carried_by = NULL WHERE carried_by = ? AND state = 'queued'",
                [carrier],
            )?;
            store.log(
                project,
                "message.uncarried",
                json!({ "carrier": carrier, "released": released }),
            )?;
        }
        for task in tasks {
            reconcile(store, task)?;
        }
        Ok(())
    })
}

/// The tasks of the rows migration 0019 let go of, reconciled once each by the start
/// that ran it, as the start of an earlier build reconciled what it let go of: a row
/// the log says was let go of that waits still, on its own. Run at a start that took
/// the ledger past [`RELEASED_AT`]; one that stopped before this goes on to the
/// carrier's own settling, whose receipt reconciles the same task.
pub(crate) fn reconcile_released(store: &mut Store) -> Result<(), LedgerError> {
    store.write(|store| {
        let tasks = ids(
            store,
            "SELECT DISTINCT m.task_id FROM event e, json_each(e.data, '$.released') r
             JOIN message m ON m.id = r.value
             WHERE e.kind = 'message.uncarried' AND m.state = 'queued'
               AND m.carried_by IS NULL AND m.task_id IS NOT NULL
             ORDER BY m.task_id",
            [],
        )?;
        for task in tasks {
            reconcile(store, task)?;
        }
        Ok(())
    })
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

//! A message's way into a window: the head of a participant's queue that
//! may go now, its delivery begun, confirmed by the harness's own record,
//! cancelled, retried or failed, and what is still on its way. What a
//! window keeps rides in the paste of the message that sends it on
//! (`carrying`), and what the paste proves arrived is each of them.

use cf_base::json::js_order;
use cf_proto::ledger::{Begun, MessageView};
use rusqlite::params;
use serde_json::{json, Value};

use super::again::tell_failed_answer;
use super::carrying::{adopt, carried, release_carried};
use super::continuations::{withdraw_stale_among, withdrawn_if_stale};
use super::pauses::withdrawn_on_its_way;
use super::receipt::reconcile;
use super::{known_message, message_task, messages, require_message};
use crate::conversations::{current_conversation, set_conversation_role_text};
use crate::model::{self, sql_list, LedgerError, HELD_TASK_STATES, MEMBER_ROLES};
use crate::store::Store;
use crate::tasks::release_ready;
use crate::views::MESSAGE_SELECT;

/// The participants something waits on: a message on its way to them, or a
/// task in their hands. A session with neither and no window has nothing a
/// pass could do for it.
pub(crate) fn with_work(store: &Store, project_id: i64) -> Result<Vec<i64>, LedgerError> {
    let ids = store
        .db
        .prepare(
            "SELECT recipient_id AS id FROM message
         WHERE project_id = ? AND state IN ('queued', 'delivering')
         UNION
         SELECT assignee_id FROM task
         WHERE project_id = ? AND assignee_id IS NOT NULL AND state IN ('queued', 'working', 'waiting')",
        )?
        .query_map(params![project_id, project_id], |row| row.get::<_, i64>("id"))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut distinct = Vec::with_capacity(ids.len());
    for id in ids {
        if !distinct.contains(&id) {
            distinct.push(id);
        }
    }
    Ok(distinct)
}

/// The head of a participant's queue that may go now, or none. A member's
/// session is its task's: a row of its goes only for a task it holds (queued,
/// working or waiting), a tell for the window goes whatever its task, and a
/// task message waits while it holds one at work. A row that rides in another
/// message's paste, or that a door took for its harness, is not the head; and
/// while a task message for the window waits for the human's approval, nothing
/// else about its task goes, whoever wrote it and in whatever order they are
/// approved: the words that came first are not passed over.
pub(crate) fn next_delivery(
    store: &Store,
    participant_id: i64,
) -> Result<Option<MessageView>, LedgerError> {
    next_delivery_among(store, participant_id, None)
}

/// `message_id`, if it is what the head of the participant's queue would be were
/// it the only row waiting: it may go by every rule the head goes by, whatever
/// else waits before it. The engine asks it of a message it puts ahead of the
/// queue.
pub(crate) fn next_delivery_of(
    store: &Store,
    participant_id: i64,
    message_id: i64,
) -> Result<Option<MessageView>, LedgerError> {
    next_delivery_among(store, participant_id, Some(message_id))
}

/// The head of the participant's queue that may go now, of the one row `only`
/// when it names one.
fn next_delivery_among(
    store: &Store,
    participant_id: i64,
    only: Option<i64>,
) -> Result<Option<MessageView>, LedgerError> {
    let participant = store.participant_row(participant_id)?;
    if participant.role == "human" {
        return Ok(None);
    }
    let busy = store
        .db
        .prepare("SELECT 1 FROM message WHERE recipient_id = ? AND state = 'delivering'")?
        .exists([participant_id])?;
    if busy {
        return Ok(None);
    }
    let serial = i64::from(MEMBER_ROLES.contains(&participant.role.as_str()));
    let next = store
        .db
        .prepare(&format!(
            "SELECT m.id FROM message m
       WHERE m.recipient_id = ?1 AND m.state = 'queued'
         AND m.carried_by IS NULL AND m.claimed_at IS NULL
         AND (m.kind != 'task' OR ?2 = 0 OR NOT EXISTS (
           SELECT 1 FROM task WHERE assignee_id = ?1 AND state IN ('working', 'waiting')
         ))
         AND (?2 = 0 OR m.urgent = 1 OR m.task_id IN (
           SELECT id FROM task WHERE assignee_id = ?1 AND state IN ({})
         ))
         AND (m.urgent = 1 OR m.task_id IS NULL OR NOT EXISTS (
           SELECT 1 FROM message words WHERE words.task_id = m.task_id
             AND words.recipient_id = m.recipient_id AND words.kind = 'task'
             AND words.state = 'gated'
         ))
         AND (?3 IS NULL OR m.id = ?3)
       ORDER BY m.id LIMIT 1",
            sql_list(&HELD_TASK_STATES)
        ))?
        .query_map(params![participant_id, serial, only], |row| {
            row.get::<_, i64>(0)
        })?
        .next()
        .transpose()?;
    next.map_or(Ok(None), |id| store.message(id))
}

/// The newest message pasted into a participant's window about a task, and
/// proved by what the window's record showed: a task message or an answer
/// that is `delivered` and rides in no other's paste. What the window wrote
/// after its marker is the task's result.
pub(crate) fn last_pasted(
    store: &Store,
    participant_id: i64,
    task_id: i64,
) -> Result<Option<MessageView>, LedgerError> {
    Ok(messages(
        store,
        &format!(
            "{MESSAGE_SELECT} WHERE m.task_id = ? AND m.recipient_id = ?
         AND m.kind IN ('task', 'answer') AND m.state = 'delivered' AND m.carried_by IS NULL
       ORDER BY m.id DESC LIMIT 1"
        ),
        params![task_id, participant_id],
    )?
    .into_iter()
    .next())
}

/// The first task message a participant's window received about a task that
/// was pasted on its own (the brief, most often), with what its paste
/// carried. A window that starts afresh on the task begins from it.
pub(crate) fn first_received(
    store: &Store,
    participant_id: i64,
    task_id: i64,
) -> Result<Option<Begun>, LedgerError> {
    let first = messages(
        store,
        &format!(
            "{MESSAGE_SELECT} WHERE m.task_id = ? AND m.recipient_id = ?
         AND m.kind = 'task' AND m.state IN ('delivered', 'read') AND m.carried_by IS NULL
       ORDER BY m.id LIMIT 1"
        ),
        params![task_id, participant_id],
    )?
    .into_iter()
    .next();
    first
        .map(|message| {
            let carried = messages(
                store,
                &format!(
                    "{MESSAGE_SELECT} WHERE m.carried_by = ? AND m.state = 'delivered' ORDER BY m.id"
                ),
                params![message.id],
            )?;
            Ok(Begun { message, carried })
        })
        .transpose()
}

/// A message's delivery begins: it is what the paste is told by, with the
/// rows it carries, which are the ones still waiting now and no others.
pub(crate) fn begin_delivery(store: &mut Store, message_id: i64) -> Result<Begun, LedgerError> {
    store.write(|store| {
        let message = require_message(store, message_id, "queued")?;
        if message.recipient_role == "human" {
            return Err(LedgerError::refused_with(
                "human-reads-in-app",
                "the human reads messages in the app",
                409,
            ));
        }
        let (carrier, claimed): (Option<i64>, Option<String>) = store.db.query_row(
            "SELECT carried_by, claimed_at FROM message WHERE id = ?",
            [message_id],
            |row| Ok((row.get("carried_by")?, row.get("claimed_at")?)),
        )?;
        if let Some(carrier) = carrier {
            return Err(LedgerError::refused_with(
                "invalid-transition",
                format!("message {message_id} rides in the paste of m-{carrier}"),
                409,
            ));
        }
        if claimed.is_some() {
            return Err(LedgerError::refused_with(
                "invalid-transition",
                format!("message {message_id} is claimed by the door that asked for it"),
                409,
            ));
        }
        let busy = store
            .db
            .prepare("SELECT id FROM message WHERE recipient_id = ? AND state = 'delivering'")?
            .exists([message.recipient_id])?;
        let working = message.kind == "task"
            && MEMBER_ROLES.contains(&message.recipient_role.as_str())
            && store
                .db
                .prepare(
                    "SELECT 1 FROM task WHERE assignee_id = ? AND state IN ('working', 'waiting')",
                )?
                .exists([message.recipient_id])?;
        if busy || working {
            return Err(LedgerError::refused_with(
                "recipient-busy",
                format!(
                    "{} is still receiving or working; message {message_id} waits",
                    message.recipient
                ),
                409,
            ));
        }
        store.db.execute(
            "UPDATE message SET state = 'delivering', attempts = attempts + 1, reason = NULL
         WHERE id = ?",
            [message_id],
        )?;
        store.log(
            message.project_id,
            "delivery.begun",
            json!({ "message": message_id, "attempt": message.attempts + 1 }),
        )?;
        Ok(Begun {
            message: known_message(store, message_id)?,
            carried: carried(store, message_id)?,
        })
    })
}

/// What the transport is to do with a hand-over begun ([`admit`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandOver {
    /// It goes to the window now, and is the window's from then on.
    Admitted,
    /// Nothing goes in: the project is closed, and the message waits for the Resume.
    Closed,
    /// Nothing goes in: the message, or a row its paste carries, was withdrawn
    /// since its delivery began (its task moved on, `continuations`).
    Withdrawn,
}

/// The transport is about to let a hand-over begun ([`begin_delivery`]) go to its
/// window, after every wait of its own: the paste asked of the pane host, the
/// inbox rename made, the request given to its connection, the command line its
/// window opens with. It goes while the project is open, the message still on
/// its way, and every row of `begun_with` (what its paste was begun with) still
/// riding in it; then the message and those rows are marked handed, in this
/// write, and a reader of a closed project reads their text from now on (the
/// daemon's `Waits`). A delivery that is only begun is not handed: what is still
/// to come of it may never come (Sol's review of 2026-10-16, round 7). A
/// continuation in it, on its own or riding, goes only while its task is at work
/// in its window, and is withdrawn in this write otherwise (`continuations`).
pub(crate) fn admit(
    store: &mut Store,
    message_id: i64,
    begun_with: &[i64],
) -> Result<HandOver, LedgerError> {
    store.write(|store| {
        let Some(message) = store.message(message_id)? else {
            return Ok(HandOver::Withdrawn);
        };
        let open = store
            .db
            .prepare("SELECT 1 FROM project WHERE id = ? AND state = 'open'")?
            .exists([message.project_id])?;
        if !open {
            return Ok(HandOver::Closed);
        }
        let mut stale = withdrawn_if_stale(store, &message)?;
        for row in carried(store, message_id)? {
            stale |= withdrawn_if_stale(store, &row)?;
        }
        if stale {
            return Ok(HandOver::Withdrawn);
        }
        let riding: Vec<i64> = store
            .db
            .prepare("SELECT id FROM message WHERE carried_by = ? AND state = 'queued'")?
            .query_map([message_id], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        if message.state != "delivering" || begun_with.iter().any(|id| !riding.contains(id)) {
            return Ok(HandOver::Withdrawn);
        }
        let at = store.at();
        store.db.execute(
            "UPDATE message SET handed_at = ?1
             WHERE id = ?2 OR (carried_by = ?2 AND state = 'queued')",
            params![at, message_id],
        )?;
        Ok(HandOver::Admitted)
    })
}

/// The first row of `begun_with` (what the paste of `message_id`, handed over
/// already, was begun with) that its window has since: read, or delivered, while
/// it rode in the paste. A row the window has is no row withdrawn: the window took
/// the paste, and ran `cf` with it (Sol's review of 2026-10-20, round 10: an
/// answer `cf task get` marked read was taken for withdrawn, and the words that
/// carried it went again). None while the window has none of them.
pub(crate) fn received_of(
    store: &Store,
    message_id: i64,
    begun_with: &[i64],
) -> Result<Option<i64>, LedgerError> {
    let received: Vec<i64> = store
        .db
        .prepare(
            "SELECT id FROM message
             WHERE carried_by = ? AND state IN ('read', 'delivered') ORDER BY id",
        )?
        .query_map([message_id], |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(received.into_iter().find(|id| begun_with.contains(id)))
}

/// What a receipt says of a row that arrived though it was withdrawn after the
/// paste that carried it was handed over.
pub const WITHDRAWN_AFTER_HANDED: &str = "after it was handed over";

/// The window's record shows a paste it was handed whose Enter the engine
/// withheld, the message given back to its queue or withdrawn since (`arrival`):
/// the human sent what the box held (Calliope's review of 2026-10-21, round 11:
/// the engine then pasted it again). The paste was admitted, or there would be
/// nothing in the box, so the message arrived, whether it waits again or was
/// withdrawn since, and in one write: it is begun as though pasted now, out of
/// any carrier that took it since (`fold`), with the rows of `held` (what its
/// paste was begun with) riding in it again and handed with it, a row withdrawn
/// since among them, and confirmed with `receipt` (`confirm_delivery`, which
/// marks those rows arrived too; with `telling`, `confirm_delivery_telling`). A
/// message withdrawn since says so in its receipt. The message confirmed, if it
/// waited or was withdrawn; none otherwise.
pub(crate) fn confirm_sent_by_hand(
    store: &mut Store,
    message_id: i64,
    held: &[i64],
    receipt: &Value,
    telling: bool,
) -> Result<Option<MessageView>, LedgerError> {
    store.write(|store| {
        let Some(message) = store.message(message_id)? else {
            return Ok(None);
        };
        let withdrawn = message.state == "cancelled";
        if !withdrawn && message.state != "queued" {
            return Ok(None);
        }
        let at = store.at();
        store.db.execute(
            "UPDATE message SET carried_by = NULL WHERE id = ?",
            [message_id],
        )?;
        for row in held {
            store.db.execute(
                "UPDATE message SET carried_by = ?1, handed_at = ?2
                 WHERE id = ?3 AND id != ?1 AND state IN ('queued', 'cancelled')",
                params![message_id, at, row],
            )?;
        }
        store.db.execute(
            "UPDATE message SET state = 'delivering', attempts = attempts + 1, handed_at = ?1,
               reason = CASE WHEN state = 'queued' THEN NULL ELSE reason END
             WHERE id = ?2",
            params![at, message_id],
        )?;
        store.log(
            message.project_id,
            "delivery.begun",
            json!({ "message": message_id, "attempt": message.attempts + 1, "sentByHand": true }),
        )?;
        let mut receipt = receipt.clone();
        if withdrawn {
            receipt["cancelled"] = json!(WITHDRAWN_AFTER_HANDED);
        }
        let confirmed = if telling {
            confirm_delivery_telling(store, message_id, Some(&receipt))?
        } else {
            confirm_delivery(store, message_id, Some(&receipt))?
        };
        Ok(Some(confirmed))
    })
}

/// The messages of a project whose hand-over has begun at their window and is not
/// over: one on its way that its transport let go ([`admit`]), and an answer a
/// question's door claimed for its harness (`claim_answer`). Every other message
/// still to be given was handed nothing.
pub(crate) fn handed_over(store: &Store, project_id: i64) -> Result<Vec<i64>, LedgerError> {
    Ok(store
        .db
        .prepare(
            "SELECT id FROM message
             WHERE project_id = ?
               AND ((state = 'delivering' AND handed_at IS NOT NULL)
                    OR (state = 'queued' AND (handed_at IS NOT NULL OR claimed_at IS NOT NULL)))
             ORDER BY id",
        )?
        .query_map([project_id], |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?)
}

/// The harness's own record proves the message arrived: `receipt` is what it
/// showed. What its paste carried arrived with it, in the same step; then
/// the task it is about is reconciled once.
pub(crate) fn confirm_delivery(
    store: &mut Store,
    message_id: i64,
    receipt: Option<&Value>,
) -> Result<MessageView, LedgerError> {
    store.write(|store| {
        let message = require_message(store, message_id, "delivering")?;
        let at = store.at();
        let receipt = js_order(receipt.cloned().unwrap_or(Value::Null)).to_string();
        store.db.execute(
            "UPDATE message SET state = 'delivered', delivered_at = ?, receipt = ? WHERE id = ?",
            params![at, receipt, message_id],
        )?;
        let rode: Vec<i64> = carried(store, message_id)?
            .into_iter()
            .map(|row| row.id)
            .collect();
        store.db.execute(
            "UPDATE message SET state = 'delivered', delivered_at = ?, receipt = ?
         WHERE carried_by = ? AND state = 'queued'",
            params![at, json!({ "carrier": message_id }).to_string(), message_id],
        )?;
        // A row withdrawn after the paste that carried it was handed over (a pause
        // or a cancel took it back) went in the paste all the same: the record says
        // it arrived, and why it reads so (the chief's decision on Calliope's review
        // of 2026-10-21, round 11). One withdrawn before the hand-over was in no
        // paste, and stays withdrawn.
        // Handed with this paste: `admit` and `confirm_sent_by_hand` stamp the
        // carrier and its rows with one time, and a row handed with an earlier one
        // and carried again since was in no paste of this (Calliope's review of
        // 2026-10-21, round 12).
        let late: Vec<i64> = store
            .db
            .prepare(
                "SELECT id FROM message
                 WHERE carried_by = ?1 AND state = 'cancelled'
                   AND handed_at >= (SELECT handed_at FROM message WHERE id = ?1)
                 ORDER BY id",
            )?
            .query_map([message_id], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        store.db.execute(
            "UPDATE message SET state = 'delivered', delivered_at = ?1, receipt = ?2
         WHERE carried_by = ?3 AND state = 'cancelled'
           AND handed_at >= (SELECT handed_at FROM message WHERE id = ?3)",
            params![
                at,
                json!({ "carrier": message_id, "cancelled": WITHDRAWN_AFTER_HANDED }).to_string(),
                message_id
            ],
        )?;
        let mut data = json!({ "message": message_id });
        if !rode.is_empty() {
            data["carried"] = json!(rode);
        }
        if !late.is_empty() {
            data["withdrawnAfterHanded"] = json!(late);
        }
        store.log(message.project_id, "delivery.confirmed", data)?;
        if let Some(task) = message_task(store, message_id)? {
            reconcile(store, task.id)?;
        }
        known_message(store, message_id)
    })
}

/// The harness's own record proves that a message that says its recipient's
/// conversation a role (its `role_text`) arrived: the message is confirmed and
/// the recipient's current conversation recorded as told that role, in one write.
/// Neither stands without the other: a note confirmed with the role not recorded
/// would be said again at the next Resume, and a role recorded for a note not
/// confirmed would never be. A message that says no role is confirmed as any is,
/// and so is one whose recipient has no current conversation (it ended while the
/// note was on its way): there is none to have told.
pub(crate) fn confirm_delivery_telling(
    store: &mut Store,
    message_id: i64,
    receipt: Option<&Value>,
) -> Result<MessageView, LedgerError> {
    store.write(|store| {
        let confirmed = confirm_delivery(store, message_id, receipt)?;
        let role: Option<String> = store.db.query_row(
            "SELECT role_text FROM message WHERE id = ?",
            [message_id],
            |row| row.get(0),
        )?;
        if let Some(role) = role {
            if let Some(conversation) = current_conversation(store, confirmed.recipient_id)? {
                set_conversation_role_text(store, conversation.id, &role)?;
            }
        }
        Ok(confirmed)
    })
}

/// A message not yet delivered that no longer applies: it will not be
/// delivered, and what it carried is its own again.
pub(crate) fn cancel_message(
    store: &mut Store,
    message_id: i64,
    reason: &str,
) -> Result<MessageView, LedgerError> {
    model::require_text(reason, "reason", 1000)?;
    store.write(|store| {
        let message = store
            .message(message_id)?
            .filter(|message| message.state == "queued" || message.state == "delivering")
            .ok_or_else(|| {
                LedgerError::refused_with(
                    "not-pending",
                    format!("message {message_id} is not waiting to be delivered"),
                    409,
                )
            })?;
        store.db.execute(
            "UPDATE message SET state = 'cancelled', reason = ? WHERE id = ?",
            params![reason, message_id],
        )?;
        store.log(
            message.project_id,
            "delivery.cancelled",
            json!({ "message": message_id, "reason": reason }),
        )?;
        if let Some(task) = message_task(store, message_id)? {
            release_carried(store, task.id)?;
        }
        known_message(store, message_id)
    })
}

/// Queued again; `refund` gives back the attempt when the window went before
/// it could land. It joins the task message that waits for its window, if one
/// does: a resume that came while it was on its way. A row that was left
/// under a carrier that cannot carry it any more (the Node daemon was pasting
/// it on its own when it stopped, which `release_stranded` leaves to the
/// delivery that settles it) is let go of that carrier first: carried, it
/// would be skipped as one and never delivered. It was handed nothing, nor
/// was what it carried, whatever its transport let go: it is handed again, or
/// never (`admit`). A pause note naming a task that moved on while it was on its
/// way leaves it, and says its warning again (`withdrawn_on_its_way`). A
/// continuation whose task is no longer at work in its window is withdrawn
/// instead, and so is one that rides in it (`continuations`).
pub(crate) fn retry_delivery(
    store: &mut Store,
    message_id: i64,
    reason: &str,
    refund: bool,
) -> Result<MessageView, LedgerError> {
    model::require_text(reason, "reason", 1000)?;
    store.write(|store| {
        let message = require_message(store, message_id, "delivering")?;
        if withdrawn_on_its_way(store, &message)? || withdrawn_if_stale(store, &message)? {
            return known_message(store, message_id);
        }
        let riding: Vec<i64> = carried(store, message_id)?
            .iter()
            .map(|row| row.id)
            .collect();
        withdraw_stale_among(store, &riding)?;
        store.db.execute(
            "UPDATE message SET state = 'queued', reason = ?, attempts = MAX(attempts - ?, 0),
               handed_at = NULL
         WHERE id = ?",
            params![reason, i64::from(refund), message_id],
        )?;
        // A row withdrawn since keeps its mark: the paste goes again without it,
        // handed later than it was, and its confirmation reads it delivered no more
        // (`confirm_delivery` takes only the rows handed with the paste).
        store.db.execute(
            "UPDATE message SET handed_at = NULL WHERE carried_by = ? AND state = 'queued'",
            [message_id],
        )?;
        store.log(
            message.project_id,
            "delivery.retried",
            json!({ "message": message_id, "reason": reason }),
        )?;
        store.db.execute(
            "UPDATE message SET carried_by = NULL
             WHERE id = ?1
               AND carried_by IN (SELECT id FROM message WHERE state NOT IN ('queued', 'gated'))",
            [message_id],
        )?;
        adopt(store, message_id)?;
        known_message(store, message_id)
    })
}

/// A task message that arrived, and that its window then took back out of its
/// conversation (Claude, stopped before a word of its answer, puts the message
/// in its input box again and answers from what came before it), was not
/// received after all. It is `queued` again, with `reason`: kept for the
/// window as a task paused before its brief arrived keeps the brief, so the
/// words that resume the task carry it. They do, whichever came first:
/// - the resume not yet made: it takes the message with what is kept;
/// - the resume made while the stop was being paid: the message joins it, as a
///   delivery tried again does (`adopt`).
///
/// What its paste carried was in the window's conversation no longer than it
/// was, so each row it carried is kept again too, and rides with it, but for a
/// continuation whose task is no longer at work (`continuations`). The
/// receipt it was given is no receipt: the carrier's will be. The task's state
/// is not moved: it is paused, or queued behind the words that resume it. It
/// and its rows were handed to the window, which holds them in its input box:
/// a reader of a closed project reads them still, whether or not their transport
/// marked them so (one delivered before migration 0020 has no mark, and its
/// delivery is its mark; Astra's review of 2026-10-17).
pub(crate) fn take_back(
    store: &mut Store,
    message_id: i64,
    reason: &str,
) -> Result<MessageView, LedgerError> {
    model::require_text(reason, "reason", 1000)?;
    store.write(|store| {
        let message = require_message(store, message_id, "delivered")?;
        let carried_by: Option<i64> = store.db.query_row(
            "SELECT carried_by FROM message WHERE id = ?",
            [message_id],
            |row| row.get(0),
        )?;
        if message.kind != "task" || carried_by.is_some() {
            return Err(LedgerError::refused_with(
                "invalid-transition",
                format!("message {message_id} is not a task message of its own"),
                409,
            ));
        }
        store.db.execute(
            "UPDATE message SET state = 'queued', reason = ?, delivered_at = NULL, receipt = NULL,
               handed_at = COALESCE(handed_at, delivered_at)
         WHERE id = ?",
            params![reason, message_id],
        )?;
        // A row that read delivered only because the paste that carried it arrived
        // after it was withdrawn is withdrawn again, not kept (`confirm_delivery`).
        store.db.execute(
            "UPDATE message SET state = 'cancelled', delivered_at = NULL, receipt = NULL
             WHERE carried_by = ? AND state = 'delivered'
               AND CASE WHEN json_valid(receipt)
                        THEN json_extract(receipt, '$.cancelled') END IS NOT NULL",
            [message_id],
        )?;
        let kept: Vec<i64> = store
            .db
            .prepare(
                "SELECT id FROM message WHERE carried_by = ? AND state = 'delivered' ORDER BY id",
            )?
            .query_map([message_id], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        store.db.execute(
            "UPDATE message SET state = 'queued', delivered_at = NULL, receipt = NULL,
               handed_at = COALESCE(handed_at, delivered_at)
         WHERE carried_by = ? AND state = 'delivered'",
            [message_id],
        )?;
        store.log(
            message.project_id,
            "delivery.taken-back",
            json!({ "message": message_id, "reason": reason, "kept": kept }),
        )?;
        withdraw_stale_among(store, &kept)?;
        adopt(store, message_id)?;
        known_message(store, message_id)
    })
}

/// A delivery given up: a task it carried, still queued, fails with it, and
/// what it carried is its own again, for the reopening to take. A pause note
/// whose task was cancelled while it was on its way is withdrawn instead, and
/// says its warning again in a note of its own, which has its own attempts
/// (`withdrawn_on_its_way`). A continuation whose task is no longer at work in
/// its window is withdrawn instead too: nothing failed that was still to go
/// (`continuations`).
pub(crate) fn fail_delivery(
    store: &mut Store,
    message_id: i64,
    reason: &str,
) -> Result<MessageView, LedgerError> {
    model::require_text(reason, "reason", 1000)?;
    store.write(|store| {
        let message = require_message(store, message_id, "delivering")?;
        if withdrawn_on_its_way(store, &message)? || withdrawn_if_stale(store, &message)? {
            return known_message(store, message_id);
        }
        store.db.execute(
            "UPDATE message SET state = 'failed', reason = ? WHERE id = ?",
            params![reason, message_id],
        )?;
        store.log(
            message.project_id,
            "delivery.failed",
            json!({ "message": message_id, "reason": reason }),
        )?;
        if let Some(task) = message_task(store, message_id)? {
            release_carried(store, task.id)?;
            if message.kind == "task" && task.state == "queued" {
                store.move_task(&task, "failed", json!({}))?;
                release_ready(store, task.project_id)?;
            }
            if message.kind == "answer" {
                tell_failed_answer(store, &message, &task, reason)?;
            }
        }
        known_message(store, message_id)
    })
}

/// Every message on its way to a window, oldest first. At start none of
/// those windows is left (they died with the previous process), so each is
/// settled before anything else is delivered.
pub(crate) fn in_flight(store: &Store) -> Result<Vec<MessageView>, LedgerError> {
    messages(
        store,
        &format!("{MESSAGE_SELECT} WHERE m.state = 'delivering' ORDER BY m.id"),
        [],
    )
}

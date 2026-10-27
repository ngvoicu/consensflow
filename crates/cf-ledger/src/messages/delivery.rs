//! A message's way into a window: the head of a participant's queue that
//! may go now, its delivery begun, confirmed by the harness's own record,
//! cancelled, retried or failed, and what is still on its way. What a
//! window keeps rides in the paste of the message that sends it on
//! (`carrying`), and what the paste proves arrived is each of them.

use cf_base::json::js_order;
use cf_proto::ledger::{Begun, MessageView};
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};

use super::again::tell_failed_answer;
use super::carrying::{adopt, carried, release_carried};
use super::continuations::{withdraw_stale_among, withdrawn_if_stale};
use super::pauses::withdrawn_on_its_way;
use super::receipt::reconcile;
use super::told::{arrived_after_all, behind_held_words, fail_for, kept_for_resume, told};
use super::{known_message, message_task, messages, require_message, settle_answered};
use crate::conversations::{current_conversation, set_conversation_role_text};
use crate::model::{self, sql_list, LedgerError, HELD_TASK_STATES, MEMBER_ROLES};
use crate::store::Store;
use crate::tasks::{pause_again, release_ready, withdrawn_late};
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
/// approved: the words that came first are not passed over. Nor are older words
/// told to the window (`told`) that the human still holds.
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
         AND NOT {}
         AND (?3 IS NULL OR m.id = ?3)
       ORDER BY m.id LIMIT 1",
            sql_list(&HELD_TASK_STATES),
            behind_held_words("m")
        ))?
        .query_map(params![participant_id, serial, only], |row| {
            row.get::<_, i64>(0)
        })?
        .next()
        .transpose()?;
    next.map_or(Ok(None), |id| store.message(id))
}

/// The message last pasted into a participant's window about a task, and
/// proved by what the window's record showed: a task message, an answer or
/// words told to the window (`told`) that is `delivered` and rides in no
/// other's paste. What the window wrote after its marker is the task's result.
/// The last is the last delivered, whatever their ids say: the human may pass
/// on held words in another order than they were written.
pub(crate) fn last_pasted(
    store: &Store,
    participant_id: i64,
    task_id: i64,
) -> Result<Option<MessageView>, LedgerError> {
    Ok(messages(
        store,
        &format!(
            "{MESSAGE_SELECT} WHERE m.task_id = ? AND m.recipient_id = ?
         AND (m.kind IN ('task', 'answer') OR ({}))
         AND m.state = 'delivered' AND m.carried_by IS NULL
       ORDER BY m.delivered_at DESC, m.id DESC LIMIT 1",
            told("m")
        ),
        params![task_id, participant_id],
    )?
    .into_iter()
    .next())
}

/// The newest message a participant's window was given and its record showed
/// (`delivered`, or `read` since): none where it was given none. One still on
/// its way (`delivering`) is not given yet.
pub(crate) fn last_given(store: &Store, participant_id: i64) -> Result<Option<i64>, LedgerError> {
    Ok(store.db.query_row(
        "SELECT MAX(id) FROM message WHERE recipient_id = ? AND state IN ('delivered', 'read')",
        [participant_id],
        |row| row.get(0),
    )?)
}

/// The `limit` messages last pasted into a participant's window, newest first,
/// each with the rows its paste carried: what its screen can show of
/// ConsensFlow's own. A message on its way is pasted already (its record may
/// show it later); one still queued is not. Known limit (P3-2 of Calliope's
/// re-check T-305, filed as its own task): newest by id, which is not the order a
/// member's window took them in. The rows a paste carried stay
/// `queued` while it is on its way (`begin_delivery` moves the carrier alone),
/// and were typed with it all the same.
pub(crate) fn last_pastes(
    store: &Store,
    participant_id: i64,
    limit: i64,
) -> Result<Vec<Begun>, LedgerError> {
    let pasted = messages(
        store,
        &format!(
            "{MESSAGE_SELECT} WHERE m.recipient_id = ?
         AND m.state IN ('delivering', 'delivered', 'read') AND m.carried_by IS NULL
       ORDER BY m.id DESC LIMIT ?"
        ),
        params![participant_id, limit],
    )?;
    pasted
        .into_iter()
        .map(|message| {
            let carried = messages(
                store,
                &format!(
                    "{MESSAGE_SELECT} WHERE m.recipient_id = ?
                 AND m.state IN ('queued', 'delivering', 'delivered', 'read') AND m.carried_by = ?
               ORDER BY m.id"
                ),
                params![participant_id, message.id],
            )?;
            Ok(Begun { message, carried })
        })
        .collect()
}

/// The newest note the engine wrote a participant after the message `after`
/// whose words hold `words`, of whatever state.
pub(crate) fn note_since(
    store: &Store,
    recipient_id: i64,
    after: i64,
    words: &str,
) -> Result<Option<i64>, LedgerError> {
    Ok(store.db.query_row(
        "SELECT MAX(id) FROM message WHERE recipient_id = ? AND id > ? AND kind = 'note'
       AND sender_id IS NULL AND instr(body, ?) > 0",
        params![recipient_id, after, words],
        |row| row.get(0),
    )?)
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

/// The receipt of a delivered message is forgotten (`Ledger::forget_receipt`).
#[cfg(feature = "test-support")]
pub(crate) fn forget_receipt(store: &mut Store, message_id: i64) -> Result<(), LedgerError> {
    store.write(|store| {
        store.db.execute(
            "UPDATE message SET receipt = 'null' WHERE id = ?",
            params![message_id],
        )?;
        Ok(())
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
/// in its window, and is withdrawn in this write otherwise (`continuations`). So
/// does the brief of the chief's own step finished while it was on its way, in
/// the finish's words (`withdrawn_late`): the transport asks again for an Enter
/// that sends a paste left in the chief's input box, and a step accepted is no
/// brief to send (the merge of main's field 3.0.6, T-392).
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
        if let Some(task) = message_task(store, message_id)? {
            if withdrawn_late(store, &message, &task)? {
                return Ok(HandOver::Withdrawn);
            }
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

/// What a receipt says of a message that arrived though its delivery was given up.
const GIVEN_UP: &str = "it had been given up";

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
/// message withdrawn since, or given up since, says so in its receipt. The
/// message confirmed, if it waited, was withdrawn or was given up; none
/// otherwise.
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
        // Withdrawn since, or given up since (a window that never went quiet for
        // the key that empties its box; Calliope's review of 2026-10-21, round 13),
        // it arrived all the same.
        let withdrawn = message.state == "cancelled";
        let given_up = message.state == "failed";
        if !withdrawn && !given_up && message.state != "queued" {
            return Ok(None);
        }
        // What the paste held, as it was before it arrived: told words a cancel
        // said the window never had are told to have arrived after all.
        let mut pasted = vec![message.clone()];
        for row in held.iter().filter(|row| **row != message_id) {
            pasted.extend(store.message(*row)?);
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
        if given_up {
            receipt["failed"] = json!(GIVEN_UP);
        }
        let confirmed = if telling {
            confirm_delivery_telling(store, message_id, Some(&receipt))?
        } else {
            confirm_delivery(store, message_id, Some(&receipt))?
        };
        arrived_after_all(store, &pasted)?;
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
/// it could land. A question answered while it was being pasted is not: the
/// one asked has it (`settle_answered`). It joins the task message that waits
/// for its window, if one does: a resume that came while it was on its way. A row that was left
/// under a carrier that cannot carry it any more (the Node daemon was pasting
/// it on its own when it stopped, which `release_stranded` leaves to the
/// delivery that settles it) is let go of that carrier first: carried, it
/// would be skipped as one and never delivered. It was handed nothing, nor
/// was what it carried, whatever its transport let go: it is handed again, or
/// never (`admit`). A pause note naming a task that moved on while it was on its
/// way leaves it, and says its warning again (`withdrawn_on_its_way`). A
/// continuation whose task is no longer at work in its window is withdrawn
/// instead, and so is one that rides in it (`continuations`). A brief of the
/// chief's own step done while it was on its way is not queued again
/// (`withdrawn_late`), nor the brief of a task that failed meanwhile.
pub(crate) fn retry_delivery(
    store: &mut Store,
    message_id: i64,
    reason: &str,
    refund: bool,
) -> Result<MessageView, LedgerError> {
    model::require_text(reason, "reason", 1000)?;
    store.write(|store| {
        let message = require_message(store, message_id, "delivering")?;
        if let Some(task) = message_task(store, message_id)? {
            if withdrawn_late(store, &message, &task)? {
                return known_message(store, message_id);
            }
            // The brief of a task the daemon failed while it was being pasted (its pane
            // died) has no window to go to again: only the task's reopening wants it, and
            // that brings the brief itself.
            if message.kind == "task" && task.state == "failed" {
                cancel_message(
                    store,
                    message_id,
                    &format!("T-{} failed before it landed", task.number),
                )?;
                return known_message(store, message_id);
            }
        }
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
        settle_answered(store, message_id)?;
        known_message(store, message_id)
    })
}

/// A delivery given up: a task it carried, still queued, fails with it, and
/// what it carried is its own again, for the reopening to take. Words told to
/// the window fail their task too (`told`), but while it is paused, when they
/// are kept for its resume; a brief of the chief's own step done while it
/// was on its way is withdrawn (`withdrawn_late`); and the words that resume a
/// paused task pause it again (`pause_again`). None of these is failed. A pause
/// note whose task was cancelled while it was on its way is withdrawn instead,
/// and says its warning again in a note of its own, which has its own attempts
/// (`withdrawn_on_its_way`). A continuation whose task is no longer at work in
/// its window is withdrawn instead too: nothing failed that was still to go
/// (`continuations`).
pub(crate) fn fail_delivery(
    store: &mut Store,
    message_id: i64,
    reason: &str,
) -> Result<MessageView, LedgerError> {
    give_up_delivery(store, message_id, reason).map(|(message, _)| message)
}

/// What a delivery given up came to ([`give_up_delivery`]), which only the ledger
/// knows: whoever tells of it says what was done, never what a state suggests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GivenUp {
    /// The message failed (`failed`), and so did what it fails with it.
    Failed,
    /// Words told to the window, kept for their paused task's resume
    /// (`kept_for_resume`): they wait, `queued`, and did not fail.
    Kept,
    /// Withdrawn (`cancelled`), and no failure: a brief of the chief's own step
    /// done while it was on its way (`withdrawn_late`), a pause note whose task
    /// moved on meanwhile, which says its warning again in a note of its own
    /// (`withdrawn_on_its_way`), or a continuation whose task is no longer at
    /// work in its window (`continuations`).
    Withdrawn,
    /// The words that resume a paused task, which never reached its window
    /// (`pause_again`): they failed, and the task is paused again, its requester
    /// told why in the note of the pause.
    Paused,
}

/// [`fail_delivery`], saying what it did ([`GivenUp`]).
pub(crate) fn give_up_delivery(
    store: &mut Store,
    message_id: i64,
    reason: &str,
) -> Result<(MessageView, GivenUp), LedgerError> {
    model::require_text(reason, "reason", 1000)?;
    store.write(|store| {
        let message = require_message(store, message_id, "delivering")?;
        if let Some(task) = message_task(store, message_id)? {
            if withdrawn_late(store, &message, &task)? {
                return Ok((known_message(store, message_id)?, GivenUp::Withdrawn));
            }
            if kept_for_resume(store, &message, &task, reason)? {
                return Ok((known_message(store, message_id)?, GivenUp::Kept));
            }
            if pause_again(store, &message, &task, reason)? {
                return Ok((known_message(store, message_id)?, GivenUp::Paused));
            }
        }
        if withdrawn_on_its_way(store, &message)? || withdrawn_if_stale(store, &message)? {
            return Ok((known_message(store, message_id)?, GivenUp::Withdrawn));
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
            fail_for(store, &message, &task)?;
            if message.kind == "answer" {
                tell_failed_answer(store, &message, &task, reason)?;
            }
        }
        Ok((known_message(store, message_id)?, GivenUp::Failed))
    })
}

/// Message `message_id`, on its way, goes into the conversation `conversation_id` of an
/// open window (`pasted_into`, which a trigger clears once it is on its way no more). A
/// launch's first message is not: its window's conversation is made after it is begun.
pub(crate) fn paste_into(
    store: &mut Store,
    message_id: i64,
    conversation_id: i64,
) -> Result<(), LedgerError> {
    store.write(|store| {
        store.db.execute(
            "UPDATE message SET pasted_into = ? WHERE id = ? AND state = 'delivering'",
            [conversation_id, message_id],
        )?;
        Ok(())
    })
}

/// The native session of the conversation message `message_id` was pasted into, while
/// it is on its way (the trigger clears it after) and that conversation has one: none
/// otherwise.
pub(crate) fn pasted_into(store: &Store, message_id: i64) -> Result<Option<String>, LedgerError> {
    Ok(store
        .db
        .query_row(
            "SELECT c.native_session FROM message m JOIN conversation c ON c.id = m.pasted_into
              WHERE m.id = ?",
            [message_id],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten()
        .filter(|session| !session.is_empty()))
}

/// When the attempt of message `message_id` on its way now began (its latest
/// `delivery.begun`: a start's give-back does not count the attempt, so its number comes
/// again), to the second, in milliseconds of the ledger's clock: none where it is not on
/// its way. A start asks a harness that keeps its own word of what its window took for
/// one written since (Devin's history row): an earlier attempt's is no word of this one.
pub(crate) fn delivery_begun_ms(
    store: &Store,
    message_id: i64,
) -> Result<Option<i64>, LedgerError> {
    Ok(store
        .db
        .query_row(
            "SELECT CAST(strftime('%s', e.at) AS INTEGER) * 1000 FROM message m
               JOIN event e ON e.project_id = m.project_id AND e.kind = 'delivery.begun'
                AND json_extract(e.data, '$.message') = m.id
              WHERE m.id = ? AND m.state = 'delivering'
              ORDER BY e.id DESC LIMIT 1",
            [message_id],
            |row| row.get::<_, Option<i64>>(0),
        )
        .optional()?
        .flatten())
}

/// Whether message `message_id` was ever given back to its queue for `reason` (a
/// `delivery.retried` that says it), whatever gave it back after. The engine words a
/// paste by it: one a start gave back though its window took it is sent again saying
/// so, every time it goes, since that first copy may be there still.
pub(crate) fn given_back_because(
    store: &Store,
    message_id: i64,
    reason: &str,
) -> Result<bool, LedgerError> {
    Ok(store
        .db
        .prepare(
            "SELECT 1 FROM message m
               JOIN event e ON e.project_id = m.project_id AND e.kind = 'delivery.retried'
                AND json_extract(e.data, '$.message') = m.id
                AND json_extract(e.data, '$.reason') = ?2
              WHERE m.id = ?1",
        )?
        .exists(params![message_id, reason])?)
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

//! Words told to a task's window before the window has the task (`cf tell` to a
//! task still queued, its window not up yet): a note from the teller about the
//! task, kept for the window. Nothing is paused for them, and nothing waits for
//! an answer. While the brief waits for its paste, they ride in it, after the
//! brief (`carrying`); once the brief is on its way (a launch has begun), they go
//! in as the window's next message, and what the window answers is its task's
//! result only once it has had them ([`told_waiting`]): its `cf task done` is
//! refused till then. Several go in the order
//! they were told, whatever order the human passes them on in
//! ([`behind_held_words`]); words that will never reach the window fail their
//! task ([`fail_for`]), as a brief that never does fails it, but for words whose
//! last paste failed while the task was paused, which the resume carries
//! ([`kept_for_resume`]).
//!
//! Told words are a note with a sender, about the task, for its window: no other
//! note a member's window is given has one (ConsensFlow's own have none). They go
//! with their task when it is called off first, and whoever told them hears so
//! ([`Dropped`]): at the cancel, or, for words in the first message of a launch
//! still being prepared, once the launch is over with no window opened
//! ([`launch_called_off`]); and once more if the human sends them from the
//! window's input box after all ([`arrived_after_all`]).

use cf_proto::ledger::MessageView;
use rusqlite::params;
use serde_json::json;

use super::carrying::{adopt, carried_into};
use super::pauses::tell_kept;
use super::{known_message, message_task, messages};
use crate::model::{self, LedgerError, HELD_TASK_STATES, MAX_BODY, MEMBER_ROLES};
use crate::queue::{send, Sent};
use crate::store::Store;
use crate::tasks::{release_ready, require_task_state};
use crate::views::{TaskRow, MESSAGE_SELECT};

/// Whether the message `alias` of a query is told words: a note from a
/// participant, never urgent.
pub(super) fn told(alias: &str) -> String {
    format!("{alias}.kind = 'note' AND {alias}.sender_id IS NOT NULL AND {alias}.urgent = 0")
}

/// Whether `message` is told words to the window of its task, whose assignee is
/// `assignee` (a handle): a note from a participant, never urgent, to that
/// window. A note about the task to anyone else (a worker's to the chief) is
/// none, and never fails the task (the second review, 2026-10-16).
pub fn is_told(message: &MessageView, assignee: Option<&str>) -> bool {
    message.kind == "note"
        && message.sender.is_some()
        && !message.urgent
        && assignee == Some(message.recipient.as_str())
}

/// The handle of `task`'s assignee, if it has one: what [`is_told`] is told.
pub(super) fn assignee_of(store: &Store, task: &TaskRow) -> Result<Option<String>, LedgerError> {
    task.assignee_id
        .map(|id| store.participant_row(id).map(|row| row.handle))
        .transpose()
}

/// Whether the message `alias` of a query, told words, waits behind older words
/// told to the same window about its task that the human still holds: told
/// words go in the order they were told.
pub(super) fn behind_held_words(alias: &str) -> String {
    format!(
        "({told_it} AND EXISTS (SELECT 1 FROM message o
           WHERE o.task_id = {alias}.task_id AND o.recipient_id = {alias}.recipient_id
             AND {told_o} AND o.id < {alias}.id AND o.state = 'gated'))",
        told_it = told(alias),
        told_o = told("o"),
    )
}

/// `from` tells T-`number`'s window `body` while the task is queued for it: the
/// words are kept for the window, the note that says them. A task that has gone
/// on (its window has it now) or is over is refused, and so is the chief's own,
/// which has no window but the chief's.
pub(crate) fn tell_queued(
    store: &mut Store,
    project_id: i64,
    number: i64,
    from: &str,
    body: &str,
) -> Result<MessageView, LedgerError> {
    model::require_text(body, "body", MAX_BODY)?;
    store.write(|store| {
        store.participant_by_handle(project_id, from)?;
        let task = store.task_row(project_id, number)?;
        require_task_state(&task, &["queued"], "keep words for the window of")?;
        let window = store.participant_of(task.assignee_id)?;
        if !MEMBER_ROLES.contains(&window.role.as_str()) {
            return Err(LedgerError::refused_with(
                "own-work",
                format!("T-{number} is the chief's own: finish or cancel it"),
                409,
            ));
        }
        send(
            store,
            project_id,
            &Sent {
                from: Some(from),
                to: &window.handle,
                body,
                task: Some(number),
                kind: "note",
                ..Sent::default()
            },
        )
    })
}

/// Whether words told to a window about its task are still on their way to it:
/// queued, held for the human, or being pasted. The window's answer is not the
/// task's result until they are in, whether read from it or given by its
/// `cf task done` (`record_result`).
pub(crate) fn told_waiting(
    store: &Store,
    participant_id: i64,
    task_id: i64,
) -> Result<bool, LedgerError> {
    Ok(store
        .db
        .prepare(&format!(
            "SELECT 1 FROM message m WHERE m.task_id = ? AND m.recipient_id = ? AND {}
               AND m.state IN ('queued', 'gated', 'delivering')",
            told("m")
        ))?
        .exists(params![task_id, participant_id])?)
}

/// Told words `message`, given up for good, fail their task while it still waits
/// on its window (queued, at work, or waiting on an answer): what the window
/// answers is to a task without them, and is no result. It is the step of the
/// delivery's failure; the engine tells the requester why, as for a brief.
pub(super) fn fail_for(
    store: &mut Store,
    message: &MessageView,
    task: &TaskRow,
) -> Result<(), LedgerError> {
    if !HELD_TASK_STATES.contains(&task.state.as_str())
        || !is_told(message, assignee_of(store, task)?.as_deref())
    {
        return Ok(());
    }
    store.move_task(task, "failed", json!({ "told": message.id }))?;
    release_ready(store, task.project_id)
}

/// Told words `message` whose last paste failed, `reason` why, while their task
/// was paused: they are kept for the window, as a pause keeps what was kept for
/// it, and the words that resume the task carry them (the second review,
/// 2026-10-16: they were given up, and the window resumed without them). They
/// keep the attempts they had, and the task's requester hears why and which words
/// wait ([`tell_kept`]). Back in their queue they were handed nothing, as on every
/// way back there (Calliope's review of the merge, T-408). Whether they were kept.
pub(super) fn kept_for_resume(
    store: &mut Store,
    message: &MessageView,
    task: &TaskRow,
    reason: &str,
) -> Result<bool, LedgerError> {
    if task.state != "paused" || !is_told(message, assignee_of(store, task)?.as_deref()) {
        return Ok(false);
    }
    store.db.execute(
        "UPDATE message SET state = 'queued', reason = ?, carried_by = NULL, handed_at = NULL
         WHERE id = ?",
        params![
            format!("kept for T-{}'s resume: {reason}", task.number),
            message.id
        ],
    )?;
    store.log(
        message.project_id,
        "delivery.kept",
        json!({ "message": message.id, "reason": reason }),
    )?;
    adopt(store, message.id)?;
    tell_kept(store, task, message, reason)?;
    Ok(true)
}

/// Words told to the window of T-`number`, which ConsensFlow has just paused as its
/// window went away (`why`), that still wait for it: queued, or held for the human
/// (`gated`), in no paste under way (one on its way is settled by its delivery, kept
/// for the resume as its paste fails: `kept_for_resume`). They wait for the resume, as
/// those do, and the queued say so; its requester hears which, and why, in the note of
/// the pause ([`tell_kept`]). Not `except`, which its caller names itself. Each of
/// them, for the engine to tell the human; none where the task is not paused.
pub(crate) fn keep_waiting(
    store: &mut Store,
    project_id: i64,
    number: i64,
    why: &str,
    except: Option<i64>,
) -> Result<Vec<MessageView>, LedgerError> {
    store.write(|store| {
        let task = store.task_row(project_id, number)?;
        if task.state != "paused" {
            return Ok(Vec::new());
        }
        let waiting = messages(
            store,
            &format!(
                "{MESSAGE_SELECT} WHERE m.task_id = ?1 AND {told} AND m.recipient_id = ?2
                   AND m.state IN ('queued', 'gated') AND m.id IS NOT ?3
                   AND (m.carried_by IS NULL
                        OR m.carried_by NOT IN (SELECT id FROM message WHERE state = 'delivering'))
                 ORDER BY m.id",
                told = told("m")
            ),
            params![task.id, task.assignee_id, except],
        )?;
        let mut kept = Vec::with_capacity(waiting.len());
        for message in waiting {
            // A row held for the human keeps the gate's own reason (none).
            store.db.execute(
                "UPDATE message SET reason = ? WHERE id = ? AND state = 'queued'",
                params![format!("kept for T-{number}'s resume: {why}"), message.id],
            )?;
            store.log(
                project_id,
                "delivery.kept",
                json!({ "message": message.id, "reason": why }),
            )?;
            tell_kept(store, &task, &message, why)?;
            kept.push(known_message(store, message.id)?);
        }
        Ok(kept)
    })
}

/// The words told to T-`number`'s windows that no window ever had, oldest first:
/// what a cancel now drops. Those still on their way to its window, in no paste
/// under way; and those a task taken back to the board said in its next brief
/// (`transfer`), while no brief since has reached a window or is on its way.
pub(crate) fn told_unreceived(
    store: &Store,
    project_id: i64,
    number: i64,
) -> Result<Vec<MessageView>, LedgerError> {
    let task = store.task_row(project_id, number)?;
    unreceived(store, &task)
}

fn unreceived(store: &Store, task: &TaskRow) -> Result<Vec<MessageView>, LedgerError> {
    unreceived_as(store, task, false)
}

/// The words told to T-`task`'s window that no window had: those [`unreceived`]
/// names, or, as a window that leaves with the task takes them (a member's
/// removal, `leaving`), those on their way too: riding in a paste or a launch
/// under way, or being pasted on their own, which the removal ends in the same
/// step, so no later call tells of them (Calliope's review of T-389, T-437).
fn unreceived_as(
    store: &Store,
    task: &TaskRow,
    leaving: bool,
) -> Result<Vec<MessageView>, LedgerError> {
    let (on_their_way, brief_on_its_way) = if leaving {
        ("m.state IN ('queued', 'gated', 'delivering')", "")
    } else {
        (
            "m.state IN ('queued', 'gated')
                AND (m.carried_by IS NULL
                     OR m.carried_by NOT IN (SELECT id FROM message WHERE state = 'delivering'))",
            "'delivering', ",
        )
    };
    messages(
        store,
        &format!(
            "{MESSAGE_SELECT} WHERE m.task_id = ?1 AND {told} AND (
               (m.recipient_id = ?2 AND {on_their_way})
               OR (m.state = 'cancelled' AND m.reason = ?3
                   AND NOT EXISTS (SELECT 1 FROM message b
                     WHERE b.task_id = ?1 AND b.kind = 'task' AND b.id > m.id
                       AND b.state IN ({brief_on_its_way}'delivered', 'read'))))
             ORDER BY m.id",
            told = told("m")
        ),
        params![task.id, task.assignee_id, carried_into(task.number)],
    )
}

/// The words told to `window` for its tasks that are paused, which a member's
/// removal leaves kept, as the pause kept them: the resume of a task for a tier
/// carries them into its next window's brief (`transfer`), and the cancel of one
/// bound to the window that left tells of them (`Dropped`). One being pasted is
/// given back, handed nothing, as a paste a pause stops is, and says why, `why`,
/// in its reason and the log, as [`kept_for_resume`] does; one riding in a carrier
/// to the window, which the removal's sweep cancels, is its own again, as
/// `release_carried` leaves it. Their ids, for the sweep to pass by (Calliope's
/// reviews of T-389, T-437 and T-446).
pub(crate) fn kept_through_leaving(
    store: &mut Store,
    window: i64,
    why: &str,
) -> Result<Vec<i64>, LedgerError> {
    let paused = format!(
        "recipient_id = ?1 AND {told} AND task_id IN (SELECT id FROM task WHERE state = 'paused')",
        told = told("message")
    );
    let given_back: Vec<(i64, i64, i64)> = store
        .db
        .prepare(&format!(
            "SELECT message.id, message.project_id, task.number FROM message
               JOIN task ON task.id = message.task_id
             WHERE {paused} AND message.state = 'delivering' ORDER BY message.id",
            paused = paused.replace("recipient_id", "message.recipient_id")
        ))?
        .query_map([window], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
        .collect::<rusqlite::Result<_>>()?;
    for (id, project_id, number) in given_back {
        store.db.execute(
            "UPDATE message SET state = 'queued', reason = ?2, carried_by = NULL, handed_at = NULL
             WHERE id = ?1",
            params![id, format!("kept for T-{number}'s resume: {why}")],
        )?;
        store.log(
            project_id,
            "delivery.kept",
            json!({ "message": id, "reason": why }),
        )?;
    }
    store.db.execute(
        &format!(
            "UPDATE message SET carried_by = NULL, handed_at = NULL
             WHERE {paused} AND state IN ('queued', 'gated')
               AND carried_by IN (SELECT id FROM message WHERE recipient_id = ?1
                                  AND state IN ('queued', 'delivering', 'gated'))"
        ),
        [window],
    )?;
    Ok(store
        .db
        .prepare(&format!(
            "SELECT id FROM message WHERE {paused} AND state IN ('queued', 'gated') ORDER BY id"
        ))?
        .query_map([window], |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?)
}

/// A failed task's queued messages are never delivered, but the words told to its
/// window, which wait for its reopen, which carries them (`fold`), as the words a
/// delivery given up for good leaves do (the chief's decision on Calliope's P3-4
/// of T-446).
pub(crate) fn drop_queued_but_told(store: &Store, task_id: i64) -> Result<(), LedgerError> {
    store.db.execute(
        &format!(
            "UPDATE message SET state = 'cancelled'
             WHERE task_id = ? AND state IN ('queued', 'gated') AND NOT ({told})",
            told = told("message")
        ),
        [task_id],
    )?;
    Ok(())
}

/// A launch called off before its window opened: its first message, `carrier`,
/// went with its cancelled task while it was prepared, and so did the words told
/// to the window that rode in it, or that a task taken back to the board said in
/// the brief it is: whoever told them hears that the window never had them. The
/// cancel said nothing of them, as they were on their way. A launch over for
/// anything else (a pause gives its message back, a task taken back says them in
/// its next brief) tells nobody.
pub(crate) fn launch_called_off(store: &mut Store, carrier: i64) -> Result<(), LedgerError> {
    store.write(|store| {
        let (Some(message), Some(task)) = (store.message(carrier)?, message_task(store, carrier)?)
        else {
            return Ok(());
        };
        if message.kind != "task" || message.state != "cancelled" || task.state != "cancelled" {
            return Ok(());
        }
        let rode = messages(
            store,
            &format!(
                "{MESSAGE_SELECT} WHERE m.task_id = ?1 AND {told} AND m.state = 'cancelled' AND (
                   m.carried_by = ?2
                   OR (m.reason = ?3 AND m.id < ?2
                       AND NOT EXISTS (SELECT 1 FROM message b
                         WHERE b.task_id = ?1 AND b.kind = 'task' AND b.id > m.id
                           AND b.state IN ('delivered', 'read'))))
                 ORDER BY m.id",
                told = told("m")
            ),
            params![task.id, carrier, carried_into(task.number)],
        )?;
        let lead = format!("T-{} was cancelled before its window opened.", task.number);
        Dropped(rode).tell_the_others(store, &task, &lead, &[])
    })
}

/// Told words of a task called off whose paste sat in its window's input box, its
/// Enter withheld, so that they were back in their queue and the cancel told whoever
/// told them that the window never had them; then the human sent what the box held,
/// and they arrived (Calliope's review of the merge, T-408). Of `pasted`, the rows of
/// a paste sent by hand as they were before it was confirmed, each that was told
/// words withdrawn with its cancelled task and reads delivered now is told to whoever
/// told it, in a note about the task: the window had it after all. Once: a paste is
/// confirmed once, and its rows leave their withdrawal with it.
pub(super) fn arrived_after_all(
    store: &mut Store,
    pasted: &[MessageView],
) -> Result<(), LedgerError> {
    let mut told_to: Vec<(TaskRow, String, Vec<String>)> = Vec::new();
    for row in pasted.iter().filter(|row| row.state == "cancelled") {
        let (Some(task), Some(sender)) = (message_task(store, row.id)?, row.sender.as_deref())
        else {
            continue;
        };
        let arrived = store
            .message(row.id)?
            .is_some_and(|now| now.state == "delivered");
        if task.state != "cancelled"
            || !arrived
            || !is_told(row, assignee_of(store, &task)?.as_deref())
        {
            continue;
        }
        let id = format!("m-{}", row.id);
        match told_to
            .iter_mut()
            .find(|(of, to, _)| of.id == task.id && to == sender)
        {
            Some((_, _, ids)) => ids.push(id),
            None => told_to.push((task, sender.to_owned(), vec![id])),
        }
    }
    for (task, sender, ids) in told_to {
        let body = format!(
            "T-{} was cancelled, but the human sent what its window's input box held: \
             the window had what you told it, {}, after all.",
            task.number,
            ids.join(", ")
        );
        send(
            store,
            task.project_id,
            &Sent {
                to: &sender,
                task: Some(task.number),
                kind: "note",
                body: &body,
                ..Sent::default()
            },
        )?;
    }
    Ok(())
}

/// What tells which of the words `told` a failed task's window wait for its reopen,
/// queued or held for the human, of `handle`'s, or of anyone's when it is none:
/// nothing when none do. One fate, said in these words wherever a task fails: the
/// ledger's give-up ([`Dropped::waiting_for_the_reopen`]) and the engine's note of a
/// delivery that failed its task (T-414 P3-4, T-446 P3-4).
pub fn waiting_for_the_reopen(told: &[MessageView], handle: Option<&str>) -> String {
    let ids: Vec<String> = told
        .iter()
        .filter(|told| matches!(told.state.as_str(), "queued" | "gated"))
        .filter(|told| handle.is_none() || told.sender.as_deref() == handle)
        .map(|told| format!("m-{}", told.id))
        .collect();
    if ids.is_empty() {
        return String::new();
    }
    format!(
        "; the words told its window wait for the reopen: {}",
        ids.join(", ")
    )
}

/// What a task called off took with it of the words told to its windows, which
/// no window ever had.
pub(crate) struct Dropped(Vec<MessageView>);

impl Dropped {
    /// What of `task` is to go, read before it goes, or, for a task that fails,
    /// to wait for its reopen ([`Dropped::waiting_for_the_reopen`]).
    pub(crate) fn of(store: &Store, task: &TaskRow) -> Result<Self, LedgerError> {
        Ok(Self(unreceived(store, task)?))
    }

    /// What of `task` goes with its window, which leaves the board with it, read
    /// before it goes: with what is on its way ([`unreceived_as`]).
    pub(crate) fn leaving(store: &Store, task: &TaskRow) -> Result<Self, LedgerError> {
        Ok(Self(unreceived_as(store, task, true)?))
    }

    /// What tells `handle` which of its words went: nothing when none did.
    pub(crate) fn said_to(&self, handle: &str) -> String {
        let ids: Vec<String> = self
            .0
            .iter()
            .filter(|told| told.sender.as_deref() == Some(handle))
            .map(|told| format!("m-{}", told.id))
            .collect();
        match ids.as_slice() {
            [] => String::new(),
            [one] => format!(" What you told its window, {one}, went with it: it never had it."),
            several => format!(
                " What you told its window, {}, went with it: it never had them.",
                several.join(", ")
            ),
        }
    }

    /// What tells which of the words wait for the task's reopen, of `handle`'s, or of
    /// anyone's when it is none ([`waiting_for_the_reopen`]).
    pub(crate) fn waiting_for_the_reopen(&self, handle: Option<&str>) -> String {
        waiting_for_the_reopen(&self.0, handle)
    }

    /// Tells each one who told words that went, but those `told` already (who
    /// called the task off, and whom the step tells in a note of its own, with
    /// [`Dropped::said_to`]): a note about the task, `lead` saying what became
    /// of it.
    pub(crate) fn tell_the_others(
        &self,
        store: &mut Store,
        task: &TaskRow,
        lead: &str,
        told: &[&str],
    ) -> Result<(), LedgerError> {
        self.tell_each(store, task, told, |sender| {
            format!("{lead}{}", self.said_to(sender))
        })
    }

    /// Tells each one who told these words, but those `told` already, a note
    /// about the task in the words `body` has for it.
    pub(crate) fn tell_each(
        &self,
        store: &mut Store,
        task: &TaskRow,
        told: &[&str],
        body: impl Fn(&str) -> String,
    ) -> Result<(), LedgerError> {
        let mut senders: Vec<&str> = Vec::new();
        for sender in self.0.iter().filter_map(|told| told.sender.as_deref()) {
            if !told.contains(&sender) && !senders.contains(&sender) {
                senders.push(sender);
            }
        }
        for sender in senders {
            let body = body(sender);
            send(
                store,
                task.project_id,
                &Sent {
                    to: sender,
                    task: Some(task.number),
                    kind: "note",
                    body: &body,
                    ..Sent::default()
                },
            )?;
        }
        Ok(())
    }
}

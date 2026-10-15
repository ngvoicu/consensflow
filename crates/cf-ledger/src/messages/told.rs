//! Words told to a task's window before the window has the task (`cf tell` to a
//! task still queued, its window not up yet): a note from the teller about the
//! task, kept for the window. Nothing is paused for them, and nothing waits for
//! an answer. While the brief waits for its paste, they ride in it, after the
//! brief (`carrying`); once the brief is on its way (a launch has begun), they go
//! in as the window's next message, and what the window answers is its task's
//! result only once it has had them ([`told_waiting`]). Several go in the order
//! they were told, whatever order the human passes them on in
//! ([`behind_held_words`]); words that will never reach the window fail their
//! task ([`fail_for`]), as a brief that never does fails it.
//!
//! Told words are a note with a sender, about the task, for its window: no other
//! note a member's window is given has one (ConsensFlow's own have none). They go
//! with their task when it is called off first, and whoever told them hears so
//! ([`Dropped`]): at the cancel, or, for words in the first message of a launch
//! still being prepared, once the launch is over with no window opened
//! ([`launch_called_off`]).

use cf_proto::ledger::MessageView;
use rusqlite::params;
use serde_json::json;

use super::carrying::carried_into;
use super::{message_task, messages};
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

/// Whether `message` is told words.
pub fn is_told(message: &MessageView) -> bool {
    message.kind == "note" && message.sender.is_some() && !message.urgent
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
/// task's result until they are in.
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
    if !is_told(message) || !HELD_TASK_STATES.contains(&task.state.as_str()) {
        return Ok(());
    }
    store.move_task(task, "failed", json!({ "told": message.id }))?;
    release_ready(store, task.project_id)
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
    messages(
        store,
        &format!(
            "{MESSAGE_SELECT} WHERE m.task_id = ?1 AND {told} AND (
               (m.recipient_id = ?2 AND m.state IN ('queued', 'gated')
                AND (m.carried_by IS NULL
                     OR m.carried_by NOT IN (SELECT id FROM message WHERE state = 'delivering')))
               OR (m.state = 'cancelled' AND m.reason = ?3
                   AND NOT EXISTS (SELECT 1 FROM message b
                     WHERE b.task_id = ?1 AND b.kind = 'task' AND b.id > m.id
                       AND b.state IN ('delivering', 'delivered', 'read'))))
             ORDER BY m.id",
            told = told("m")
        ),
        params![task.id, task.assignee_id, carried_into(task.number)],
    )
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

/// What a task called off took with it of the words told to its windows, which
/// no window ever had.
pub(crate) struct Dropped(Vec<MessageView>);

impl Dropped {
    /// What of `task` is to go, read before it goes.
    pub(crate) fn of(store: &Store, task: &TaskRow) -> Result<Self, LedgerError> {
        Ok(Self(unreceived(store, task)?))
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
        let mut senders: Vec<&str> = Vec::new();
        for sender in self.0.iter().filter_map(|told| told.sender.as_deref()) {
            if !told.contains(&sender) && !senders.contains(&sender) {
                senders.push(sender);
            }
        }
        for sender in senders {
            let body = format!("{lead}{}", self.said_to(sender));
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

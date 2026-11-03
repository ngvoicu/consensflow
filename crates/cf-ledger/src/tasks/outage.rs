//! **One note for one cause, and no note of a failure that is no longer true**
//! (field board of 2026-11-03: the Mac's DNS dropped, six working tasks failed on the
//! harness's "API Error: Can't reach the API server … (ENOTFOUND)" over 25 minutes, and the
//! chief, given its notes one at a time, took 13 of them, most after it had reopened the six).
//!
//! What the requester of a failed task is told is a *view* of the failures it has not yet been
//! given: the note that tells of them is queued, and the event `failure.told` keeps what it
//! is made of (`Told`: its members, each with the words of its failure, and the cause that tells
//! one outage from the next). Whenever what it is made of changes while it is still queued,
//! it is written again and the old one is withdrawn, `superseded by m-<new>`, as a continuation
//! is (`continuations`):
//!
//! - **A failure with the same cause** joins the queued note of the requester whose newest
//!   failure came [`SAME_ERROR_WITHIN_MS`] before at most (`tell_failure`). The windows of one
//!   outage give up each at its own pace (six of them over 25 minutes, up to 9 minutes 47
//!   seconds apart), and the span is no more than how long ago the last one may be: what
//!   makes them one note is that the requester has not been given the first yet. A failure
//!   with another cause, or none the engine could tell, is a note of its own, and so is one
//!   that finds the note before it given already. A note of one failure is the words a single
//!   failure has always had.
//! - **A close note** of a member's window (`tell_closed`) is a line of a note of several,
//!   and the close notes of its members that are queued already go in as it is written. A note
//!   of one failure leaves them as they were.
//! - **A task that is no longer failed** (reopened, called off, accepted) is left out of the
//!   note that names it, which is withdrawn when no failure is left in it, and so is the note
//!   of a task that waits for it, and a waiter that no longer waits is left out of the list of
//!   what waits (`no_longer_failed`, `no_longer_waiting`: called by the write that moves the
//!   task, as `withdraw_continuations` is, so a note in the queue is never one that moved on).
//!   One handed over, or being handed, is the requester's, as any message is.

use cf_base::time;
use cf_proto::ledger::MessageView;
use rusqlite::params;
use serde::{Deserialize, Serialize};

use super::finishing::{reopen_or_accept, waiting_for_it, waiting_of, waits_for_failed};
use crate::messages::{note, withdraw_each, NewNote};
use crate::model::LedgerError;
use crate::queue::{send, Sent};
use crate::store::Store;
use crate::views::TaskRow;

/// How long before a failure the newest one of a queued note of the same cause may have
/// come, for the failure to be told in that note: half an hour. Six windows of one outage
/// failed over 25 minutes, the longest wait between two of them 9 minutes 47 seconds; one
/// the next hour with the same words is another outage.
pub const SAME_ERROR_WITHIN_MS: i64 = 30 * 60_000;

/// The event that keeps what a queued note of failures is made of.
const TOLD: &str = "failure.told";

/// A message the requester has not been given and may still not be: queued, or in its
/// delivery but not handed over (`handed_at`), whose hand-over the transport then refuses.
const PENDING: &str = "(state = 'queued' OR (state = 'delivering' AND handed_at IS NULL))";

/// What a note says of a task whose window closed, as the engine words it.
pub fn window_closed(number: i64) -> String {
    format!("T-{number}'s window closed")
}

/// One failed task in a note: when it failed, the words of its failure and of what was told
/// to its window that waits for its reopen, and the close notes of its window that the note
/// holds.
#[derive(Clone, Serialize, Deserialize)]
struct Member {
    task: i64,
    at: i64,
    reason: String,
    told: String,
    #[serde(default)]
    closed: Vec<String>,
}

/// What a queued note of failures is made of: the message that tells it, whom it tells,
/// what the failures said (empty where the engine could not tell), and the failures.
#[derive(Clone, Serialize, Deserialize)]
struct Told {
    message: i64,
    to: String,
    key: String,
    members: Vec<Member>,
}

/// The time now, in milliseconds.
fn now(store: &mut Store) -> i64 {
    time::parse(&store.at()).unwrap_or(0)
}

/// The notes of failures `project_id` has still to give, oldest first.
fn pending(store: &Store, project_id: i64) -> Result<Vec<Told>, LedgerError> {
    // What tells of failures is written after the note it tells, so the events to read are
    // those since the oldest note of ConsensFlow's still to be given: none where there is none.
    let oldest: Option<String> = store.db.query_row(
        &format!(
            "SELECT MIN(created_at) FROM message
             WHERE project_id = ? AND kind = 'note' AND sender_id IS NULL AND {PENDING}"
        ),
        [project_id],
        |row| row.get(0),
    )?;
    let Some(oldest) = oldest else {
        return Ok(Vec::new());
    };
    let written: Vec<String> = store
        .db
        .prepare(
            "SELECT data FROM event WHERE project_id = ? AND kind = ? AND at >= ? ORDER BY id DESC",
        )?
        .query_map(params![project_id, TOLD, oldest], |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    let mut found = Vec::new();
    for data in written {
        let Ok(told) = serde_json::from_str::<Told>(&data) else {
            continue;
        };
        let queued: bool = store.db.query_row(
            &format!("SELECT EXISTS (SELECT 1 FROM message WHERE id = ? AND {PENDING})"),
            [told.message],
            |row| row.get(0),
        )?;
        if queued {
            found.push(told);
        }
    }
    found.reverse();
    Ok(found)
}

/// The members of `members` whose task is failed still.
fn failed_still(
    store: &Store,
    project_id: i64,
    members: &[Member],
) -> Result<Vec<Member>, LedgerError> {
    let mut live = Vec::new();
    for member in members {
        if store.task_row(project_id, member.task)?.state == "failed" {
            live.push(member.clone());
        }
    }
    Ok(live)
}

/// "T-1, T-2 and T-3".
fn names(numbers: &[i64]) -> String {
    let named: Vec<String> = numbers.iter().map(|number| format!("T-{number}")).collect();
    match named.as_slice() {
        [] => String::new(),
        [only] => only.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// The note of `members`, all failed: for one, the words a single failure has always had
/// (and the close notes of its window, each a line after them, where it was a note of several
/// before); for several, the failures, what waits for them, what their windows closed with,
/// and how to reopen them all.
fn render(
    store: &Store,
    project_id: i64,
    to: &str,
    members: &[Member],
) -> Result<String, LedgerError> {
    if let [only] = members {
        let task = store.task_row(project_id, only.task)?;
        let words = format!(
            "T-{0} failed: {1}{2}.{3}",
            only.task,
            only.reason,
            only.told,
            reopen_or_accept(only.task)
        );
        let waiting = waiting_for_it(store, &task, to)?;
        let closed: String = only.closed.iter().map(|line| format!("\n{line}")).collect();
        return Ok(format!("{words}{waiting}{closed}"));
    }
    let numbers: Vec<i64> = members.iter().map(|member| member.task).collect();
    let first = members.iter().map(|member| member.at).min().unwrap_or(0);
    let last = members.iter().map(|member| member.at).max().unwrap_or(0);
    let minutes = ((last - first) + 59_999) / 60_000;
    let within = match minutes.max(1) {
        1 => "1 minute".to_owned(),
        many => format!("{many} minutes"),
    };
    let mut body = format!(
        "{} failed within {within} of each other with the same error, which looks like an outage of the network or the API, not their work:",
        names(&numbers)
    );
    for member in members {
        body.push_str(&format!(
            "\nT-{}: {}{}",
            member.task, member.reason, member.told
        ));
    }
    let mut waiting = Vec::new();
    for member in members {
        let task = store.task_row(project_id, member.task)?;
        let tasks = waiting_of(store, &task, to)?;
        match tasks.as_slice() {
            [] => {}
            [only] => waiting.push(format!("T-{only} waits for T-{}", member.task)),
            _ => waiting.push(format!("{} wait for T-{}", names(&tasks), member.task)),
        }
    }
    if !waiting.is_empty() {
        body.push_str(&format!(
            "\nWaiting for them: {}. Cancel what waits, or reopen what it waits for.",
            waiting.join("; ")
        ));
    }
    for member in members {
        for line in &member.closed {
            body.push_str(&format!("\n{line}"));
        }
    }
    let reopen: Vec<String> = numbers
        .iter()
        .map(|number| format!("cf task reopen T-{number} \"…\""))
        .collect();
    body.push_str(&format!("\nReopen them all with: {}", reopen.join("; ")));
    Ok(body)
}

/// The note that tells `members` to `to`, written, with the notes in `replaces` withdrawn in
/// favour of it; none where no member is failed any more, and then they are withdrawn with
/// `why`. A note of one failure is about its task, a note of several about none.
fn put(
    store: &mut Store,
    project_id: i64,
    to: &str,
    key: &str,
    members: &[Member],
    replaces: &[i64],
    why: &str,
) -> Result<Option<MessageView>, LedgerError> {
    let live = failed_still(store, project_id, members)?;
    if live.is_empty() {
        withdraw_each(store, project_id, replaces, why)?;
        return Ok(None);
    }
    let body = render(store, project_id, to, &live)?;
    let task = match live.as_slice() {
        [only] => Some(only.task),
        _ => None,
    };
    let sent = send(
        store,
        project_id,
        &Sent {
            to,
            task,
            kind: "note",
            body: &body,
            ..Sent::default()
        },
    )?;
    store.log(
        project_id,
        TOLD,
        serde_json::to_value(Told {
            message: sent.id,
            to: to.to_owned(),
            key: key.to_owned(),
            members: live,
        })
        .map_err(|error| LedgerError::refused("invalid-text", error.to_string()))?,
    )?;
    withdraw_each(
        store,
        project_id,
        replaces,
        &format!("superseded by m-{}", sent.id),
    )?;
    Ok(Some(sent))
}

/// The close notes of the windows of `members` that wait to be given to `to`: their task's
/// number, their message and their words.
fn queued_closes(
    store: &Store,
    project_id: i64,
    to: &str,
    members: &[Member],
) -> Result<Vec<(i64, i64, String)>, LedgerError> {
    let recipient = store.participant_by_handle(project_id, to)?.id;
    let mut found = Vec::new();
    for member in members {
        let task = store.task_row(project_id, member.task)?;
        let opening = window_closed(member.task);
        let rows: Vec<(i64, String)> = store
            .db
            .prepare(&format!(
                "SELECT id, body FROM message
                 WHERE project_id = ?1 AND recipient_id = ?2 AND task_id = ?3 AND kind = 'note'
                   AND sender_id IS NULL AND {PENDING} AND substr(body, 1, ?4) = ?5
                 ORDER BY id"
            ))?
            .query_map(
                params![
                    project_id,
                    recipient,
                    task.id,
                    i64::try_from(opening.chars().count()).unwrap_or(i64::MAX),
                    opening
                ],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?
            .collect::<rusqlite::Result<_>>()?;
        found.extend(rows.into_iter().map(|(id, body)| (member.task, id, body)));
    }
    Ok(found)
}

/// The failed task `task` is told to its requester `to`: in the words a single failure has
/// always had, or, where `cause` says what the failure said and the requester has still to
/// be given a failure that said the same, [`SAME_ERROR_WITHIN_MS`] after at most, in one
/// note with it. `reason` is how the task failed and `told` what of the words told to its
/// window waits for its reopen.
pub(super) fn tell_failure(
    store: &mut Store,
    task: &TaskRow,
    to: &str,
    reason: &str,
    told: &str,
    cause: &str,
) -> Result<(), LedgerError> {
    let project_id = task.project_id;
    let at = now(store);
    let earlier: Vec<Told> = if cause.is_empty() {
        Vec::new()
    } else {
        pending(store, project_id)?
            .into_iter()
            .filter(|note| {
                note.to == to
                    && note.key == cause
                    && note
                        .members
                        .iter()
                        .map(|member| member.at)
                        .max()
                        .is_some_and(|newest| at - newest <= SAME_ERROR_WITHIN_MS)
            })
            .collect()
    };
    let mut members: Vec<Member> = earlier
        .iter()
        .flat_map(|note| note.members.iter().cloned())
        .filter(|member| member.task != task.number)
        .collect();
    members.push(Member {
        task: task.number,
        at,
        reason: reason.to_owned(),
        told: told.to_owned(),
        closed: Vec::new(),
    });
    members.sort_by_key(|member| member.at);
    let mut replaces: Vec<i64> = earlier.iter().map(|note| note.message).collect();
    if members.len() > 1 {
        for (number, message, line) in queued_closes(store, project_id, to, &members)? {
            if let Some(member) = members.iter_mut().find(|member| member.task == number) {
                if !member.closed.contains(&line) {
                    member.closed.push(line);
                }
            }
            replaces.push(message);
        }
    }
    put(
        store,
        project_id,
        to,
        cause,
        &members,
        &replaces,
        "superseded",
    )?;
    Ok(())
}

/// The note that its window closed, which the engine writes to the requester of `note.task`:
/// a line of the queued note of several failures that holds the task, and a note of its own
/// anywhere else.
pub(crate) fn tell_closed(
    store: &mut Store,
    project_id: i64,
    new: &NewNote,
) -> Result<MessageView, LedgerError> {
    store.write(|store| {
        if let Some(number) = new.task {
            for told in pending(store, project_id)? {
                if told.to != new.to {
                    continue;
                }
                let mut members = failed_still(store, project_id, &told.members)?;
                if members.len() < 2 || !members.iter().any(|member| member.task == number) {
                    continue;
                }
                if let Some(member) = members.iter_mut().find(|member| member.task == number) {
                    if !member.closed.contains(&new.body) {
                        member.closed.push(new.body.clone());
                    }
                }
                let again = [told.message];
                if let Some(sent) = put(
                    store,
                    project_id,
                    &told.to,
                    &told.key,
                    &members,
                    &again,
                    "superseded",
                )? {
                    return Ok(sent);
                }
            }
        }
        note(store, project_id, new)
    })
}

/// The failed task `task` is no longer failed (`to` is its state now): what tells that it
/// is, to whoever has not been given it, goes. The notes of failures that name it are written
/// again without it, or withdrawn where it was their last; so is a note of one failure that
/// no note of failures kept (a teller's, the engine's) and the notes of the tasks that wait
/// for it. Called by the write that moves the task.
pub(crate) fn no_longer_failed(
    store: &mut Store,
    task: &TaskRow,
    to: &str,
) -> Result<(), LedgerError> {
    let project_id = task.project_id;
    let why = format!("T-{} is {to} now", task.number);
    for note in pending(store, project_id)? {
        if note.members.iter().any(|member| member.task == task.number) {
            mend(store, project_id, &note, &why)?;
        }
    }
    let opening = format!("T-{} failed: ", task.number);
    let plain: Vec<i64> = store
        .db
        .prepare(&format!(
            "SELECT id FROM message
             WHERE project_id = ?1 AND task_id = ?2 AND kind = 'note' AND sender_id IS NULL
               AND {PENDING} AND substr(body, 1, ?3) = ?4
             ORDER BY id"
        ))?
        .query_map(
            params![
                project_id,
                task.id,
                i64::try_from(opening.chars().count()).unwrap_or(i64::MAX),
                opening
            ],
            |row| row.get(0),
        )?
        .collect::<rusqlite::Result<_>>()?;
    withdraw_each(store, project_id, &plain, &why)?;
    let waiting: Vec<(i64, i64)> = store
        .db
        .prepare(
            "SELECT t.id, t.number FROM task_need n JOIN task t ON t.id = n.task_id
             WHERE n.needs_id = ? ORDER BY t.number",
        )?
        .query_map([task.id], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    for (id, number) in waiting {
        let said = waits_for_failed(number, task.number);
        withdraw_waits(store, project_id, id, &said, &why)?;
    }
    Ok(())
}

/// The task `task`, which waited for tasks (`to` is its state now), waits no more: its
/// notes that it waits for a failed task go, and the notes of failures that list it as waiting
/// are written again without it. Called by the write that moves the task.
pub(crate) fn no_longer_waiting(
    store: &mut Store,
    task: &TaskRow,
    to: &str,
) -> Result<(), LedgerError> {
    let project_id = task.project_id;
    let failed: Vec<(i64, i64)> = store
        .db
        .prepare(
            "SELECT f.id, f.number FROM task_need n JOIN task f ON f.id = n.needs_id
             WHERE n.task_id = ? AND f.state = 'failed' ORDER BY f.number",
        )?
        .query_map([task.id], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    if failed.is_empty() {
        return Ok(());
    }
    let why = format!("T-{} is {to} now", task.number);
    for (_, number) in &failed {
        let said = waits_for_failed(task.number, *number);
        withdraw_waits(store, project_id, task.id, &said, &why)?;
    }
    for note in pending(store, project_id)? {
        if note
            .members
            .iter()
            .any(|member| failed.iter().any(|(_, number)| *number == member.task))
        {
            mend(store, project_id, &note, &why)?;
        }
    }
    Ok(())
}

/// The notes about the task `waiting` (its id) that say `said`, which have not been given.
fn withdraw_waits(
    store: &mut Store,
    project_id: i64,
    waiting: i64,
    said: &str,
    why: &str,
) -> Result<(), LedgerError> {
    let found: Vec<i64> = store
        .db
        .prepare(&format!(
            "SELECT id FROM message
             WHERE project_id = ?1 AND task_id = ?2 AND kind = 'note' AND sender_id IS NULL
               AND {PENDING} AND body = ?3
             ORDER BY id"
        ))?
        .query_map(params![project_id, waiting, said], |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    withdraw_each(store, project_id, &found, why)
}

/// A queued note of failures, written again as the board is now: without the tasks no longer
/// failed and with who waits now, or withdrawn where nothing in it is true. Left as it is where
/// it says the same.
fn mend(store: &mut Store, project_id: i64, told: &Told, why: &str) -> Result<(), LedgerError> {
    let live = failed_still(store, project_id, &told.members)?;
    if live.is_empty() {
        return withdraw_each(store, project_id, &[told.message], why);
    }
    let body = render(store, project_id, &told.to, &live)?;
    let current = store.message(told.message)?.map(|message| message.body);
    if live.len() == told.members.len() && current.as_deref() == Some(body.as_str()) {
        return Ok(());
    }
    put(
        store,
        project_id,
        &told.to,
        &told.key,
        &live,
        &[told.message],
        why,
    )?;
    Ok(())
}

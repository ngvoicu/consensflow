//! Continuations: what ConsensFlow tells a member's window whose turn a quit or
//! a Close cut, when the human's Resume brings it back: the task it was working
//! on, and to go on. It is a `note` of ConsensFlow's own, told by its words
//! ([`continuation_tasks`]), as a pause note is.
//!
//! A continuation is about the one task its window holds, and lives only while
//! that task is at work as it was written for: a pause, a cancel, a release, a
//! reassignment or an end of the task withdraws it, in the write that moves the
//! task, unless its window was handed it already ([`withdraw_continuations`]); a
//! reassignment never carries it into the next window's brief
//! ([`drop_continuations`]). One its window was handed runs its course; if it
//! comes back (its paste refused, the window gone, a restart, a carrier given up
//! or taken back), it goes on only while its task is still at work, and is
//! withdrawn in that write otherwise, as the transport's admission withdraws it
//! ([`withdrawn_if_stale`]; Astra's review of 2026-10-17: a continuation handed
//! over, its task paused while the paste waited, came back after a refusal and was
//! read as a word to go on). The next Resume writes one anew from the task as it
//! is then. A window that holds several tasks (a session a ledger wrote before the
//! rule of one, or a member's own lane) is told to go on with none: the Resume
//! leaves its work paused, for the chief to resume task by task (`tasks`'
//! `resume_with_project`), and withdraws what an earlier build left queued telling
//! it to go on with several ([`withdraw_stale`]). Four rounds of reviews of
//! 2026-10-16 found a continuation naming several tasks escaping one check or
//! another; none is written now.

use cf_proto::ledger::MessageView;
use rusqlite::{params, OptionalExtension};
use serde_json::json;

use crate::model::LedgerError;
use crate::queue::withdraw;
use crate::store::Store;
use crate::tasks::RESUME_WORDS;
use crate::views::TaskRow;

/// How a continuation begins, before the tasks it names.
const STOPPED: &str = "ConsensFlow stopped while you were working on ";

/// How 3.0.5 and the builds before it began one, which a ledger may still hold
/// queued for a window that never came.
const QUIT: &str = "ConsensFlow quit while you were working on ";

/// The words of a continuation of task `number`.
pub fn continuation(number: i64) -> String {
    format!("{STOPPED}{}. {RESUME_WORDS}", named(&[number]))
}

/// The tasks a continuation names, in its words or in those of 3.0.5 and the
/// builds before it, which named every task a window held; none for any other
/// text. Only the words those builds and [`continuation`] write are read, so no
/// other note is taken for one.
pub fn continuation_tasks(body: &str) -> Option<Vec<i64>> {
    let (start, rest) = [STOPPED, QUIT]
        .into_iter()
        .find_map(|start| Some((start, body.strip_prefix(start)?)))?;
    let list = rest.strip_suffix(&format!(". {RESUME_WORDS}"))?;
    let tasks = list
        .split(", ")
        .map(|task| task.strip_prefix("T-")?.parse().ok())
        .collect::<Option<Vec<i64>>>()?;
    let written = format!("{start}{}. {RESUME_WORDS}", named(&tasks));
    (written == body).then_some(tasks)
}

/// The tasks as a continuation names them.
fn named(tasks: &[i64]) -> String {
    let tasks: Vec<String> = tasks.iter().map(|number| format!("T-{number}")).collect();
    tasks.join(", ")
}

/// The continuations that `states` holds about `task`, oldest first, each with
/// the window it is for: ConsensFlow's own notes in their words, tied to the task
/// or naming it to the window that has it (an earlier build's may be tied to no
/// task, or to another it names).
fn continuations_of(
    store: &Store,
    task: &TaskRow,
    states: &str,
) -> Result<Vec<(i64, i64)>, LedgerError> {
    let found: Vec<(i64, i64, Option<i64>, String)> = store
        .db
        .prepare(&format!(
            "SELECT id, recipient_id, task_id, body FROM message
             WHERE project_id = ?1 AND sender_id IS NULL AND kind = 'note' AND ({states})
               AND (task_id = ?2 OR recipient_id = ?3)
             ORDER BY id"
        ))?
        .query_map(params![task.project_id, task.id, task.assignee_id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(found
        .into_iter()
        .filter(|(_, _, tied, body)| {
            continuation_tasks(body)
                .is_some_and(|named| *tied == Some(task.id) || named.contains(&task.number))
        })
        .map(|(id, recipient, _, _)| (id, recipient))
        .collect())
}

/// Withdraws each of `ids` with `reason`, as the log says.
fn withdraw_each(
    store: &mut Store,
    project_id: i64,
    ids: &[i64],
    reason: &str,
) -> Result<(), LedgerError> {
    for id in ids {
        withdraw(store, *id, reason)?;
        store.log(
            project_id,
            "delivery.cancelled",
            json!({ "message": id, "reason": reason }),
        )?;
    }
    Ok(())
}

/// `task` moves on from the work its window was told to go on with (it is
/// paused, cancelled, released, reassigned or ended; `to` is its state now): what
/// told the window to go on and was not handed to it is withdrawn, in the write
/// that moves the task. That is one still queued, on its own or riding in another's
/// paste, and one on its way that the transport has not let go yet (`handed_at`,
/// [`super::delivery::admit`]), whose hand-over the transport then refuses. One
/// handed over is the window's already, as any message is.
pub(crate) fn withdraw_continuations(
    store: &mut Store,
    task: &TaskRow,
    to: &str,
) -> Result<(), LedgerError> {
    let unsent: Vec<i64> = continuations_of(
        store,
        task,
        "state = 'queued' OR (state = 'delivering' AND handed_at IS NULL)",
    )?
    .into_iter()
    .map(|(id, _)| id)
    .collect();
    withdraw_each(
        store,
        task.project_id,
        &unsent,
        &format!("T-{} is {to} now", task.number),
    )
}

/// `task` goes from `window` to another (it is reassigned, or taken back to the
/// board): whatever told `window` to go on with it is withdrawn, handed or not, as
/// what the task takes on from the window is carried into its next brief
/// (`carrying`'s `transfer`). A continuation is no part of that: it is about a
/// turn of the window's own, and would tell the next window to go on with work it
/// never began (both reviews of 2026-10-16, round 7).
pub(crate) fn drop_continuations(
    store: &mut Store,
    task: &TaskRow,
    window: i64,
) -> Result<(), LedgerError> {
    let pending: Vec<i64> = continuations_of(store, task, "state IN ('queued', 'delivering')")?
        .into_iter()
        .filter(|(_, recipient)| *recipient == window)
        .map(|(id, _)| id)
        .collect();
    withdraw_each(
        store,
        task.project_id,
        &pending,
        &format!("T-{} went from its window", task.number),
    )
}

/// Why `message`, a continuation, is no longer to be given to its window: its
/// task is not at work there (working, or waiting for an answer), or it names
/// several tasks, as 3.0.5 and the builds before it wrote them. None while its
/// task is at work there, for one no longer to be given at all (given already,
/// or withdrawn), and for any other message.
fn stale(store: &Store, message: &MessageView) -> Result<Option<String>, LedgerError> {
    if message.kind != "note"
        || message.sender.is_some()
        || !matches!(message.state.as_str(), "queued" | "delivering")
    {
        return Ok(None);
    }
    let Some(named) = continuation_tasks(&message.body) else {
        return Ok(None);
    };
    let [number] = named.as_slice() else {
        return Ok(Some("it names several tasks".to_owned()));
    };
    let task: Option<(String, Option<i64>)> = store
        .db
        .query_row(
            "SELECT state, assignee_id FROM task WHERE project_id = ? AND number = ?",
            params![message.project_id, number],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    Ok(match task {
        Some((state, Some(window))) if window == message.recipient_id => {
            (!matches!(state.as_str(), "working" | "waiting"))
                .then(|| format!("T-{number} is {state} now"))
        }
        _ => Some(format!("T-{number} went from its window")),
    })
}

/// `message`, about to be handed to its window or back in its queue, withdrawn in
/// this write if it is a continuation whose task is no longer at work there
/// ([`stale`]): whether it was. The transport's admission asks it of what it is to
/// let go (`delivery`'s `admit`), and every way a message comes back to its queue
/// asks it of what comes back: a delivery tried again or given up, a carrier
/// given up, cancelled or taken back, what a restart lets go of.
pub(crate) fn withdrawn_if_stale(
    store: &mut Store,
    message: &MessageView,
) -> Result<bool, LedgerError> {
    let Some(reason) = stale(store, message)? else {
        return Ok(false);
    };
    withdraw_each(store, message.project_id, &[message.id], &reason)?;
    Ok(true)
}

/// Each of `ids` that came back to its queue, withdrawn if it is a continuation
/// whose task is no longer at work in its window ([`withdrawn_if_stale`]).
pub(crate) fn withdraw_stale_among(store: &mut Store, ids: &[i64]) -> Result<(), LedgerError> {
    for id in ids {
        if let Some(message) = store.message(*id)? {
            withdrawn_if_stale(store, &message)?;
        }
    }
    Ok(())
}

/// The human's Resume of a project, in the write that opens it: what an earlier
/// build or an earlier Resume left queued telling a window to go on goes unless it
/// names the one task its window holds, at work. A continuation that names several
/// is 3.0.5's or an earlier build's, and this one tells a window that holds
/// several to go on with none; one whose task stands otherwise is for work as it was. Of several
/// left for the same task, the oldest stays and the others go: the window is told
/// once (Sol's review of 2026-10-17). The Resume writes one anew where it is due.
pub(crate) fn withdraw_stale(store: &mut Store, project_id: i64) -> Result<(), LedgerError> {
    let queued: Vec<(i64, i64, String)> = store
        .db
        .prepare(
            "SELECT id, recipient_id, body FROM message
             WHERE project_id = ? AND sender_id IS NULL AND kind = 'note' AND state = 'queued'
             ORDER BY id",
        )?
        .query_map([project_id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?
        .collect::<rusqlite::Result<_>>()?;
    let mut stale = Vec::new();
    let mut kept: Vec<(i64, i64)> = Vec::new();
    for (id, recipient, body) in queued {
        let Some(named) = continuation_tasks(&body) else {
            continue;
        };
        let held: Vec<(i64, String)> = store
            .db
            .prepare(
                "SELECT number, state FROM task
                 WHERE assignee_id = ? AND state IN ('queued', 'working', 'waiting', 'paused')",
            )?
            .query_map(params![recipient], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        let at_work = match (named.as_slice(), held.as_slice()) {
            ([number], [(holds, state)])
                if number == holds && matches!(state.as_str(), "working" | "waiting") =>
            {
                Some(*number)
            }
            _ => None,
        };
        match at_work {
            Some(number) if !kept.contains(&(recipient, number)) => kept.push((recipient, number)),
            _ => stale.push(id),
        }
    }
    withdraw_each(
        store,
        project_id,
        &stale,
        "the Resume tells its window to go on with its work as it is now",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_is_written_reads_back_and_the_words_of_older_builds_are_read_too() {
        assert_eq!(continuation_tasks(&continuation(7)), Some(vec![7]));
        for tasks in [vec![1], vec![2, 10, 3]] {
            let quit = format!("{QUIT}{}. {RESUME_WORDS}", named(&tasks));
            assert_eq!(continuation_tasks(&quit), Some(tasks.clone()));
            let stopped = format!("{STOPPED}{}. {RESUME_WORDS}", named(&tasks));
            assert_eq!(continuation_tasks(&stopped), Some(tasks));
        }
    }

    #[test]
    fn no_other_note_is_taken_for_a_continuation() {
        for body in [
            "",
            "ConsensFlow stopped in the middle of your turn, which was cut. Go on where you stopped.",
            "ConsensFlow stopped while you were working on T-1.",
            "ConsensFlow stopped while you were working on T-01. Go on where you stopped.",
            "ConsensFlow stopped while you were working on T-1,T-2. Go on where you stopped.",
            "ConsensFlow stopped while you were working on . Go on where you stopped.",
            "Heads up. ConsensFlow stopped while you were working on T-1. Go on where you stopped.",
        ] {
            assert_eq!(continuation_tasks(body), None, "{body:?}");
        }
    }
}

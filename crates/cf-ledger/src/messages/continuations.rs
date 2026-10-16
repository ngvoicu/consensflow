//! Continuations: what ConsensFlow tells a member's window whose turn a quit or
//! a Close cut, when the human's Resume brings it back: the tasks it was working
//! on, and to go on. It is a `note` of ConsensFlow's own, told by its words
//! ([`continuation_tasks`]), as a pause note is.
//!
//! A continuation names the tasks its window is to go on with, and those can move
//! while it waits: the human pauses one, the chief cancels another. Whenever it is
//! begun, on its own or riding in another's paste, it names only the tasks its
//! window still holds, and one that holds none goes no more ([`narrow`]). A launch
//! or a delivery that hands one over watches what it names until the hand-over
//! (the engine's). Its checks so go with it for its whole life, however often it
//! goes back to its queue (both reviews of 2026-10-16, round 6: a continuation
//! a launch gave back named a task paused since, and the next launch gave it
//! whole).

use cf_proto::ledger::MessageView;
use rusqlite::{params, OptionalExtension};
use serde_json::json;

use crate::model::LedgerError;
use crate::queue::withdraw;
use crate::store::Store;
use crate::tasks::RESUME_WORDS;

/// How a continuation begins, before the tasks it names.
const STOPPED: &str = "ConsensFlow stopped while you were working on ";

/// How builds before 3.0.5 began one, which a ledger may still hold queued for a
/// window that never came.
const QUIT: &str = "ConsensFlow quit while you were working on ";

/// The words of a continuation naming `tasks` (their numbers), in that order.
pub fn continuation(tasks: &[i64]) -> String {
    format!("{STOPPED}{}. {RESUME_WORDS}", named(tasks))
}

/// The tasks a continuation names, in its words or in those of builds before
/// 3.0.5; none for any other text. Only the words [`continuation`] writes are
/// read, so no other note is taken for one.
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

/// Whether `message` is a continuation still waiting to be begun: ConsensFlow's
/// own note in its words, `queued`. One withdrawn or begun meanwhile is left as
/// it is, for its beginning to refuse as any.
fn is_continuation(message: &MessageView) -> bool {
    message.kind == "note"
        && message.sender.is_none()
        && message.state == "queued"
        && continuation_tasks(&message.body).is_some()
}

/// A message about to be begun, as it stands: a continuation names only the
/// tasks its window still holds (queued with it, at work or waiting), and one
/// that holds none is withdrawn. None when it was; the message as it is now
/// otherwise, any other message as it was.
pub(crate) fn as_it_stands(
    store: &mut Store,
    message_id: i64,
) -> Result<Option<MessageView>, LedgerError> {
    store.write(|store| {
        let Some(message) = store.message(message_id)? else {
            return Ok(None);
        };
        if !narrow(store, &message)? {
            return Ok(None);
        }
        store.message(message_id)
    })
}

/// The tasks a continuation names that its window still holds (queued with it,
/// at work or waiting), in the order it names them: their numbers and ids.
fn held(store: &Store, message: &MessageView) -> Result<Vec<(i64, i64)>, LedgerError> {
    let named = continuation_tasks(&message.body).unwrap_or_default();
    let mut held: Vec<(i64, i64)> = Vec::with_capacity(named.len());
    for number in named {
        let task: Option<i64> = store
            .db
            .query_row(
                "SELECT id FROM task WHERE project_id = ? AND number = ? AND assignee_id = ?
                   AND state IN ('queued', 'working', 'waiting')",
                params![message.project_id, number, message.recipient_id],
                |row| row.get(0),
            )
            .optional()?;
        held.extend(task.map(|id| (number, id)));
    }
    Ok(held)
}

/// A continuation back in its queue (its launch or its delivery did not hand it
/// over) is about the first task it names that its window still holds, where it
/// holds one: about a task paused meanwhile, it would wait for that task and hold
/// the others back with it. Its words name them all until it is begun
/// ([`narrow`]); one whose window holds none of them waits, about the task it was.
pub(crate) fn repoint(store: &Store, message: &MessageView) -> Result<(), LedgerError> {
    if !is_continuation(message) {
        return Ok(());
    }
    if let Some(&(_, first)) = held(store, message)?.first() {
        store.db.execute(
            "UPDATE message SET task_id = ? WHERE id = ?",
            params![first, message.id],
        )?;
    }
    Ok(())
}

/// Narrows `message` if it is a continuation, as [`as_it_stands`] says: whether
/// it still goes. Nothing for any other message, which goes.
pub(crate) fn narrow(store: &mut Store, message: &MessageView) -> Result<bool, LedgerError> {
    if !is_continuation(message) {
        return Ok(true);
    }
    let named = continuation_tasks(&message.body).unwrap_or_default();
    let held = held(store, message)?;
    let Some(&(_, first)) = held.first() else {
        let reason = "the work it named stopped before it was given";
        withdraw(store, message.id, reason)?;
        store.log(
            message.project_id,
            "delivery.cancelled",
            json!({ "message": message.id, "reason": reason }),
        )?;
        return Ok(false);
    };
    if held.len() < named.len() {
        let numbers: Vec<i64> = held.iter().map(|(number, _)| *number).collect();
        store.db.execute(
            "UPDATE message SET body = ?, task_id = ? WHERE id = ?",
            params![continuation(&numbers), first, message.id],
        )?;
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_is_written_reads_back_and_the_words_of_older_builds_are_read_too() {
        for tasks in [vec![1], vec![2, 10, 3]] {
            assert_eq!(
                continuation_tasks(&continuation(&tasks)),
                Some(tasks.clone())
            );
            let quit = format!("{QUIT}{}. {RESUME_WORDS}", named(&tasks));
            assert_eq!(continuation_tasks(&quit), Some(tasks));
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

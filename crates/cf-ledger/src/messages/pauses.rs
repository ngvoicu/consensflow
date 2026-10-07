//! Pause notes: what ConsensFlow tells a task's requester when it pauses the
//! task because its window went away. The tasks that stall together are told
//! in one note: the first stall of a pass queues it, and the others join it.
//!
//! It is a `note` like any other of ConsensFlow's own, told by its words
//! ([`read`]), which are written here and nowhere else: a kind of its own
//! would be a migration, and the schema is held byte for byte by Node's. A
//! note names its tasks, and while it is queued they come and go. One that is
//! resumed or cancelled leaves it ([`leave_pause_notes`]), and a note with
//! none left is withdrawn. A note already pasted into a window has been read,
//! and stays as it was.

use cf_proto::ledger::MessageView;
use rusqlite::params;

use crate::model::LedgerError;
use crate::queue::{send, withdraw, Sent};
use crate::store::Store;
use crate::views::TaskRow;

/// What a pause note says of a task: its number, and why it stopped.
#[derive(Debug, PartialEq)]
struct Paused {
    number: i64,
    because: String,
}

/// What follows the one task a note names.
const ONE_TAIL: &str = "; its window comes back on its own conversation.";

/// The last line of a note that names several.
const MANY_TAIL: &str = "Each window comes back on its own conversation.";

/// How a note says that a task is paused, and how to resume it.
fn clause(paused: &Paused) -> String {
    format!(
        "T-{0} is paused: {1}. Resume it with: cf task resume T-{0} \"…\"",
        paused.number, paused.because
    )
}

/// The words of a note naming `paused`: a sentence for one task, a line each
/// for several, in the order given.
fn render(paused: &[Paused]) -> String {
    match paused {
        [one] => format!("{}{ONE_TAIL}", clause(one)),
        many => {
            let mut lines: Vec<String> = many.iter().map(clause).collect();
            lines.push(MANY_TAIL.to_owned());
            lines.join("\n")
        }
    }
}

/// The tasks a note names, or none when it is no pause note. Only the words
/// [`render`] writes are read, so no other note is taken for one.
fn read(body: &str) -> Option<Vec<Paused>> {
    let paused = match body.strip_suffix(ONE_TAIL) {
        Some(one) => vec![read_clause(one)?],
        None => body
            .strip_suffix(MANY_TAIL)?
            .strip_suffix('\n')?
            .split('\n')
            .map(read_clause)
            .collect::<Option<Vec<_>>>()?,
    };
    (render(&paused) == body).then_some(paused)
}

/// The task a [`clause`] names.
fn read_clause(text: &str) -> Option<Paused> {
    let (number, rest) = text.strip_prefix("T-")?.split_once(" is paused: ")?;
    let number: i64 = number.parse().ok()?;
    let because = rest.strip_suffix(&format!(
        ". Resume it with: cf task resume T-{number} \"…\""
    ))?;
    Some(Paused {
        number,
        because: because.to_owned(),
    })
}

/// Tells `to` that T-`number` is paused and why: a note of the task alone,
/// as ConsensFlow's note of a stall always was, which the other stalls of its
/// pass join.
pub(crate) fn note_pause(
    store: &mut Store,
    project_id: i64,
    to: &str,
    number: i64,
    because: &str,
) -> Result<MessageView, LedgerError> {
    let body = render(&[Paused {
        number,
        because: because.to_owned(),
    }]);
    send(
        store,
        project_id,
        &Sent {
            to,
            body: &body,
            task: Some(number),
            kind: "note",
            ..Sent::default()
        },
    )
}

/// Adds T-`number` to the pause note `id`, whose words then name it as they
/// name the others. None when the note cannot take it: it is no longer queued
/// (its reader has it, or it was withdrawn) or it is no pause note, and the
/// caller tells with a note of its own. A task the note names already stays
/// as it is.
pub(crate) fn join_pause_note(
    store: &mut Store,
    id: i64,
    number: i64,
    because: &str,
) -> Result<Option<MessageView>, LedgerError> {
    store.write(|store| {
        let Some(note) = store
            .message(id)?
            .filter(|note| note.kind == "note" && note.sender.is_none() && note.state == "queued")
        else {
            return Ok(None);
        };
        let Some(mut paused) = read(&note.body) else {
            return Ok(None);
        };
        if paused.iter().all(|named| named.number != number) {
            paused.push(Paused {
                number,
                because: because.to_owned(),
            });
            paused.sort_by_key(|named| named.number);
            set(store, id, note.project_id, &paused)?;
        }
        store.message(id)
    })
}

/// A task that is no longer paused (it was resumed, or called off) is told no
/// more. What its requester was told of it and has not been given yet is
/// withdrawn, if it was told of the task alone, or loses the task if it was
/// told of several. What it was given stays. Only ConsensFlow's own notes,
/// still queued, are looked at: whatever else was said of the task is not
/// about its pause.
pub(crate) fn leave_pause_notes(
    store: &Store,
    task: &TaskRow,
    why: &str,
) -> Result<(), LedgerError> {
    let reason = format!("T-{} {why}", task.number);
    // A pause drops what is queued for the task, so a note of its own that is
    // queued now was written since: a stall's, a hold's, a refusal's, each
    // says something about the pause.
    store.db.execute(
        "UPDATE message SET state = 'cancelled', reason = ?
       WHERE task_id = ? AND recipient_id = ? AND sender_id IS NULL AND kind = 'note'
         AND state = 'queued'",
        params![reason, task.id, task.requester_id],
    )?;
    let several = store
        .db
        .prepare(
            "SELECT id, body FROM message
       WHERE project_id = ? AND recipient_id = ? AND sender_id IS NULL AND kind = 'note'
         AND task_id IS NULL AND state = 'queued' ORDER BY id",
        )?
        .query_map(params![task.project_id, task.requester_id], |row| {
            Ok((row.get::<_, i64>("id")?, row.get::<_, String>("body")?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for (id, body) in several {
        let Some(mut paused) = read(&body) else {
            continue;
        };
        let before = paused.len();
        paused.retain(|named| named.number != task.number);
        if paused.len() == before {
            continue;
        }
        if paused.is_empty() {
            withdraw(store, id, &reason)?;
        } else {
            set(store, id, task.project_id, &paused)?;
        }
    }
    Ok(())
}

/// Writes the words of a note naming `paused`. A note naming one task is tied
/// to it, as a note of the task alone is; one naming several is tied to none.
fn set(store: &Store, id: i64, project_id: i64, paused: &[Paused]) -> Result<(), LedgerError> {
    let task_id = match paused {
        [one] => Some(store.task_row(project_id, one.number)?.id),
        _ => None,
    };
    store.db.execute(
        "UPDATE message SET body = ?, task_id = ? WHERE id = ?",
        params![render(paused), task_id, id],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paused(number: i64, because: &str) -> Paused {
        Paused {
            number,
            because: because.to_owned(),
        }
    }

    #[test]
    fn a_task_alone_is_told_in_the_words_a_stall_always_used() {
        assert_eq!(
            render(&[paused(1, "@zeus's window is gone")]),
            "T-1 is paused: @zeus's window is gone. Resume it with: cf task resume T-1 \"…\"; its window comes back on its own conversation."
        );
    }

    #[test]
    fn several_tasks_are_a_line_each_and_a_last_line_for_all() {
        assert_eq!(
            render(&[
                paused(3, "@zeus's window is gone"),
                paused(5, "@diana's window closed"),
            ]),
            "T-3 is paused: @zeus's window is gone. Resume it with: cf task resume T-3 \"…\"\n\
             T-5 is paused: @diana's window closed. Resume it with: cf task resume T-5 \"…\"\n\
             Each window comes back on its own conversation."
        );
    }

    #[test]
    fn what_is_written_reads_back() {
        for tasks in [
            vec![paused(1, "@zeus's window is gone")],
            vec![paused(2, "a"), paused(10, "b. Resume it with: not this")],
            vec![
                paused(1, "x"),
                paused(2, "y"),
                paused(30, "@zeus's window closed"),
            ],
        ] {
            assert_eq!(read(&render(&tasks)), Some(tasks));
        }
    }

    #[test]
    fn no_other_note_is_taken_for_a_pause_note() {
        for body in [
            "",
            "T-1 is paused: for a reason.",
            "T-1 waits with @zeus: out of quota until 2026-10-07T18:00:00.000Z; it goes on by itself then.",
            "T-1 stays paused: it could not go on when its hold ended. It waits for your decision.",
            // The words of one task, with the last line of several.
            "T-1 is paused: a. Resume it with: cf task resume T-1 \"…\"\nEach window comes back on its own conversation.",
            // A number the words do not write the same way.
            "T-+1 is paused: a. Resume it with: cf task resume T-+1 \"…\"; its window comes back on its own conversation.",
            "T-01 is paused: a. Resume it with: cf task resume T-01 \"…\"; its window comes back on its own conversation.",
            // Another task's number in the command.
            "T-1 is paused: a. Resume it with: cf task resume T-2 \"…\"; its window comes back on its own conversation.",
            // Words added to a pause note.
            "Heads up. T-1 is paused: a. Resume it with: cf task resume T-1 \"…\"; its window comes back on its own conversation.",
        ] {
            assert_eq!(read(body), None, "{body:?}");
        }
    }
}

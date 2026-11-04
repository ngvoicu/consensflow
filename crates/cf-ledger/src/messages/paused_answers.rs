//! The note that gives a requester the last complete answer of a paused task's window
//! (T-442): the chief's `cf tell` pauses a working task and puts its words to the window;
//! the window answered, went on in the same turn, and ended it with its complete answer,
//! which no result could be taken from while the task was paused. The engine finds that
//! answer (`turn_end/paused.rs`) and the ledger writes the note, once for each tell: the
//! words, and the two ways on (`cf task finish T-n` takes them as the result, a resume
//! sends the window on).
//!
//! The note holds the window's last words and, where it wrote other words first, before its
//! first tool call, those too (T-1151): the window's reply to the tell is the first thing it
//! writes, and a turn that goes on (a tool call, then more text) leaves it behind the last
//! words, in the transcript alone. What it wrote between its tools stays there.
//!
//! It is a `note` of ConsensFlow's own, told by its words ([`read`]), which are written
//! here and nowhere else: a kind of its own would be a migration, and the schema is held
//! byte for byte by Node's. What the note says stays true as the task moves:
//!
//! - **resumed, or taken back to the board** (a resume, a release): the task is paused no
//!   more, and the answer is what a pause's turn gave, not the task's result. A note not
//!   yet pasted into its reader's window is not withdrawn with the pause notes
//!   (`leave_pause_notes`), as the chief's resume would lose the answer if it came before
//!   the note did; its words are written again for what is true then ([`keep_moved`]): the
//!   task has moved on, its result will be its next complete answer, and this is what the
//!   turn on the tell ended with. A note already pasted has been read, and stays as it was.
//!   The same words are written at once where the task moved on before the window's turn
//!   ended (the chief resumed while the window was still at work on its answer): its
//!   resume is queued behind the turn, and the turn's end is told all the same.
//! - **staying paused** (a Resume of a project that leaves one of a window's several tasks
//!   paused): the task has not moved on, and its note stays as it was.
//! - **failed** (the task fails with the note unread, an early resume's turn that failed): the
//!   queued messages dropped with it are not this one ([`queued`], [`put_back`]); it is put
//!   back queued as it was, in the words of a task that has moved on, and is given to its
//!   reader at its next rest like any note (the delivery of a note to a chief does not wait
//!   for the task's state), before the note that the task failed. Nothing waits for the
//!   reopen but the task.
//! - **finished** (`cf task finish`, the drawer's Finish): the note's words are the result,
//!   and the note is moot: it is withdrawn if not yet pasted ([`withdraw`]).
//! - **cancelled**: the task's messages on their way are withdrawn with it, as always.

use cf_proto::ledger::MessageView;
use rusqlite::params;

use crate::model::LedgerError;
use crate::queue::{send, withdraw as withdraw_message, Sent};
use crate::store::Store;
use crate::views::TaskRow;

/// What a note tells a requester it can do with the words: the chief takes them with `cf
/// task finish`, or sends the window on with `cf task resume`; the human, who writes to no
/// agent, resumes the task in its drawer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Next {
    Chief,
    Human,
}

/// Whether a note's task is still paused, or has moved on: in the words written now, or
/// in those of a build before T-958 (`MOVED_ON_BEFORE`), which a note keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Then {
    Paused(Next),
    Moved,
    MovedBefore,
}

/// What a note says: the task, the window's handle, the tell whose words the turn was on,
/// what the window wrote first (T-1151), the words the turn ended with, and how it stands now.
#[derive(Debug, PartialEq, Eq)]
struct Answered {
    number: i64,
    handle: String,
    tell: i64,
    reply: Option<String>,
    words: String,
    then: Then,
}

const IS_PAUSED: &str = " is paused, and @";
const WAS_PAUSED: &str = ", paused when @";
const TURN_ON: &str = "'s turn on the words put to its window (m-";
const IS_OVER: &str = ") is over. What it said last:\n\n";
const MOVED_ON: &str = ") began, has moved on since (resumed, or back on the board). This is not its result: that comes as a message of its own, the answer of the turn its resume, or its next assignment, begins, and it is the one to decide. What the turn on those words ended with:\n\n";
/// Where the window wrote other words first (T-1151): these, then its reply, then [`LAST`].
const IS_OVER_AFTER_A_REPLY: &str = ") is over. What it wrote first, before any tool:\n\n";
const LAST: &str = "\n\nWhat it said last:\n\n";
/// What a note of a task that moved on says of the words it was given at the end of its
/// turn, and, with a reply, of those it wrote first.
const MOVED_ON_ENDED: &str = "What the turn on those words ended with:\n\n";
const MOVED_ON_AFTER_A_REPLY: &str =
    "What the turn on those words wrote first, before any tool:\n\n";
const ENDED_WITH: &str = "\n\nand ended with:\n\n";
/// What a note of a task that moved on said before T-958, which a ledger may still hold
/// queued: read, never written. Such a note keeps these words: `keep_moved` rewrites only a
/// note of a task still paused.
const MOVED_ON_BEFORE: &str = ") began, has moved on since (resumed, or back on the board): its result will be its next complete answer, not this. What that turn ended with:\n\n";

/// The words after the answer's, while the task is paused: what to do next.
fn tail(number: i64, next: Next) -> String {
    match next {
        Next::Chief => format!(
            "\n\nIf that is the result, take it: cf task finish T-{number}. To send it on: cf task resume T-{number} \"…\"."
        ),
        Next::Human => "\n\nTo take it as the result, Finish it in the task's drawer; to send it on, Resume it there.".to_owned(),
    }
}

/// The words of a note.
fn render(answered: &Answered) -> String {
    let Answered {
        number,
        handle,
        tell,
        reply,
        words,
        then,
    } = answered;
    // A note of a build before T-958 is written no more, and has no reply.
    if let Some(reply) = reply.as_deref().filter(|_| *then != Then::MovedBefore) {
        return render_after_a_reply(answered, reply);
    }
    match then {
        Then::Paused(next) => format!(
            "T-{number}{IS_PAUSED}{handle}{TURN_ON}{tell}{IS_OVER}{words}{}",
            tail(*number, *next)
        ),
        Then::Moved => format!("T-{number}{WAS_PAUSED}{handle}{TURN_ON}{tell}{MOVED_ON}{words}"),
        Then::MovedBefore => {
            format!("T-{number}{WAS_PAUSED}{handle}{TURN_ON}{tell}{MOVED_ON_BEFORE}{words}")
        }
    }
}

/// The words of a note where the window wrote other words first, before its first tool call
/// (T-1151): what it wrote first, then what it said last.
fn render_after_a_reply(answered: &Answered, reply: &str) -> String {
    let Answered {
        number,
        handle,
        tell,
        words,
        then,
        ..
    } = answered;
    match then {
        Then::Paused(next) => format!(
            "T-{number}{IS_PAUSED}{handle}{TURN_ON}{tell}{IS_OVER_AFTER_A_REPLY}{reply}{LAST}{words}{}",
            tail(*number, *next)
        ),
        Then::Moved | Then::MovedBefore => {
            let head = MOVED_ON.strip_suffix(MOVED_ON_ENDED).unwrap_or(MOVED_ON);
            format!("T-{number}{WAS_PAUSED}{handle}{TURN_ON}{tell}{head}{MOVED_ON_AFTER_A_REPLY}{reply}{ENDED_WITH}{words}")
        }
    }
}

/// The digits that begin `text`, and what follows them.
fn number_in(text: &str) -> Option<(i64, &str)> {
    let digits = text.bytes().take_while(u8::is_ascii_digit).count();
    Some((text[..digits].parse().ok()?, &text[digits..]))
}

/// What a note says, or none when `body` is no such note. Only the words [`render`] writes
/// are read, so no other note is taken for one.
fn read(body: &str) -> Option<Answered> {
    let (number, rest) = number_in(body.strip_prefix("T-")?)?;
    let (paused, rest) = match rest.strip_prefix(IS_PAUSED) {
        Some(rest) => (true, rest),
        None => (false, rest.strip_prefix(WAS_PAUSED)?),
    };
    let (handle, rest) = rest.split_once(TURN_ON)?;
    let (tell, rest) = number_in(rest)?;
    if let Some((reply, words, then)) = after_a_reply(number, paused, rest) {
        let answered = Answered {
            number,
            handle: handle.to_owned(),
            tell,
            reply: Some(reply.to_owned()),
            words: words.to_owned(),
            then,
        };
        return (render(&answered) == body).then_some(answered);
    }
    let (words, then) = if paused {
        let rest = rest.strip_prefix(IS_OVER)?;
        [Next::Chief, Next::Human]
            .into_iter()
            .find_map(|next| Some((rest.strip_suffix(&tail(number, next))?, Then::Paused(next))))?
    } else if let Some(words) = rest.strip_prefix(MOVED_ON) {
        (words, Then::Moved)
    } else {
        (rest.strip_prefix(MOVED_ON_BEFORE)?, Then::MovedBefore)
    };
    let answered = Answered {
        number,
        handle: handle.to_owned(),
        tell,
        reply: None,
        words: words.to_owned(),
        then,
    };
    (render(&answered) == body).then_some(answered)
}

/// What a note says where the window wrote other words first (T-1151), or none: after `rest`,
/// the note's words from the tell on, its reply, the words it ended with, and how it stands.
/// A reply ends where the words that follow it begin; `read` proves the split was the note's
/// own by writing it again, whichever of those words the reply quoted.
fn after_a_reply(number: i64, paused: bool, rest: &str) -> Option<(&str, &str, Then)> {
    if paused {
        let rest = rest.strip_prefix(IS_OVER_AFTER_A_REPLY)?;
        let (kept, then) = [Next::Chief, Next::Human]
            .into_iter()
            .find_map(|next| Some((rest.strip_suffix(&tail(number, next))?, Then::Paused(next))))?;
        let (reply, words) = kept.split_once(LAST)?;
        return Some((reply, words, then));
    }
    let head = MOVED_ON.strip_suffix(MOVED_ON_ENDED).unwrap_or(MOVED_ON);
    let rest = rest
        .strip_prefix(head)?
        .strip_prefix(MOVED_ON_AFTER_A_REPLY)?;
    let (reply, words) = rest.split_once(ENDED_WITH)?;
    Some((reply, words, Then::Moved))
}

/// The words every note of T-`number` begins with, paused or moved on: what a withdrawal of
/// the pause notes leaves (`leave_pause_notes`).
pub(super) fn prefixes(number: i64) -> [String; 2] {
    [
        format!("T-{number}{IS_PAUSED}"),
        format!("T-{number}{WAS_PAUSED}"),
    ]
}

/// Tells T-`number`'s requester what the window of `handle` ended its turn on tell `tell`
/// with: `words`, the last complete answer it wrote after the tell, and `reply`, what it wrote
/// first, before its first tool call, where that is other words (T-1151); the task is paused, or
/// has been resumed since (its resume waits for the turn's end). A note for each tell,
/// however often it is asked: none is written where the task's thread has one for this tell
/// already, whatever became of it. The note, or none where there is one.
pub(crate) fn note(
    store: &mut Store,
    project_id: i64,
    number: i64,
    handle: &str,
    tell: i64,
    reply: Option<&str>,
    words: &str,
) -> Result<Option<MessageView>, LedgerError> {
    store.write(|store| {
        let task = store.task_row(project_id, number)?;
        let notes = notes_of(store, &task, None)?;
        if notes.iter().any(|(_, said)| said.tell == tell) {
            return Ok(None);
        }
        let requester = store.participant_row(task.requester_id)?;
        let next = if requester.role == "human" {
            Next::Human
        } else {
            Next::Chief
        };
        // Paused still, or resumed meanwhile with the resume queued behind the window's turn.
        let then = if task.state == "paused" {
            Then::Paused(next)
        } else {
            Then::Moved
        };
        let body = render(&Answered {
            number,
            handle: handle.to_owned(),
            tell,
            reply: reply.map(str::to_owned),
            words: words.to_owned(),
            then,
        });
        send(
            store,
            project_id,
            &Sent {
                to: &requester.handle,
                body: &body,
                task: Some(number),
                kind: "note",
                ..Sent::default()
            },
        )
        .map(Some)
    })
}

/// The notes of this kind that ConsensFlow wrote to `task`'s requester about it, with their
/// ids, in the order written; only those in `state` when it is given.
fn notes_of(
    store: &Store,
    task: &TaskRow,
    state: Option<&str>,
) -> Result<Vec<(i64, Answered)>, LedgerError> {
    let rows = store
        .db
        .prepare(
            "SELECT id, body FROM message
             WHERE task_id = ?1 AND recipient_id = ?2 AND sender_id IS NULL AND kind = 'note'
               AND (?3 IS NULL OR state = ?3) ORDER BY id",
        )?
        .query_map(params![task.id, task.requester_id, state], |row| {
            Ok((row.get::<_, i64>("id")?, row.get::<_, String>("body")?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows
        .into_iter()
        .filter_map(|(id, body)| read(&body).map(|said| (id, said)))
        .collect())
}

/// The newest note that gave T-`number`'s requester the answer of its window's turn on a
/// tell, or is to give it (queued, being pasted, given, read), since the task's last result
/// to that requester before `result`: what `result`'s paste names (T-958: the chief was given
/// the turn's report in the note and again in the result, and could not tell which to
/// decide). None where there is none, where Finish took its words as the result and withdrew
/// it, or where an earlier result came after it: that note was the earlier result's to name,
/// and a result after a reopen names none of it (Calliope's T-987).
pub(crate) fn given(
    store: &Store,
    project_id: i64,
    number: i64,
    result: i64,
) -> Result<Option<i64>, LedgerError> {
    let task = store.task_row(project_id, number)?;
    let earlier: Option<i64> = store.db.query_row(
        "SELECT MAX(id) FROM message
         WHERE task_id = ?1 AND recipient_id = ?2 AND kind = 'result' AND id < ?3",
        params![task.id, task.requester_id, result],
        |row| row.get(0),
    )?;
    let mut given = None;
    for state in ["queued", "delivering", "delivered", "read"] {
        for (id, _) in notes_of(store, &task, Some(state))? {
            if id < result && earlier.is_none_or(|earlier| id > earlier) {
                given = given.max(Some(id));
            }
        }
    }
    Ok(given)
}

/// Whether the chief's tell to `participant`'s window about `task` is still on its way to it:
/// queued behind the turn the pause stopped, held for the human, or being pasted. Until it is
/// in, the window's record holds the turn from before the tell, and what it says last is not
/// the answer to it.
pub(crate) fn tell_on_its_way(
    store: &Store,
    participant_id: i64,
    task_id: i64,
) -> Result<bool, LedgerError> {
    Ok(store
        .db
        .prepare(
            "SELECT 1 FROM message
             WHERE task_id = ? AND recipient_id = ? AND kind = 'question' AND urgent = 1
               AND state IN ('queued', 'gated', 'delivering')",
        )?
        .exists(params![task_id, participant_id])?)
}

/// The task is paused no more, and goes on (a resume) or goes back to the board: the notes
/// not yet pasted into their reader's window say so, and that what they hold is not the
/// task's result.
pub(crate) fn keep_moved(store: &Store, task: &TaskRow) -> Result<(), LedgerError> {
    for (id, said) in notes_of(store, task, Some("queued"))? {
        if matches!(said.then, Then::Paused(_)) {
            let body = render(&Answered {
                then: Then::Moved,
                ..said
            });
            store.db.execute(
                "UPDATE message SET body = ? WHERE id = ?",
                params![body, id],
            )?;
        }
    }
    Ok(())
}

/// The notes not yet pasted that give T-`task`'s requester an answer of its window: their
/// ids. A task that fails drops what is queued for it, and these wait for its reopen
/// ([`put_back`]).
pub(crate) fn queued(store: &Store, task: &TaskRow) -> Result<Vec<i64>, LedgerError> {
    Ok(notes_of(store, task, Some("queued"))?
        .into_iter()
        .map(|(id, _)| id)
        .collect())
}

/// The notes `ids` queued again, as they were before the task's queued messages were dropped
/// ([`queued`]): the answer of the window's turn on a tell is not lost with a task that failed
/// before its requester was given it.
pub(crate) fn put_back(store: &Store, ids: &[i64]) -> Result<(), LedgerError> {
    for id in ids {
        store
            .db
            .execute("UPDATE message SET state = 'queued' WHERE id = ?", [id])?;
    }
    Ok(())
}

/// The task is finished with these words as its result: the notes not yet pasted are moot.
pub(crate) fn withdraw(store: &Store, task: &TaskRow, why: &str) -> Result<(), LedgerError> {
    let reason = format!("T-{} {why}", task.number);
    for (id, _) in notes_of(store, task, Some("queued"))? {
        withdraw_message(store, id, &reason)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn said(then: Then) -> Answered {
        Answered {
            number: 7,
            handle: "zeus-amber-pine".to_owned(),
            tell: 12,
            reply: None,
            words: "Done.\n\nThe parser is on grammar v2.".to_owned(),
            then,
        }
    }

    #[test]
    fn a_note_is_read_back_as_it_was_written_in_each_of_its_words() {
        for then in [
            Then::Paused(Next::Chief),
            Then::Paused(Next::Human),
            Then::Moved,
            Then::MovedBefore,
        ] {
            let answered = said(then);
            assert_eq!(read(&render(&answered)), Some(answered));
        }
    }

    #[test]
    fn a_note_with_the_windows_first_words_is_read_back_as_it_was_written_in_each_of_its_words() {
        for then in [
            Then::Paused(Next::Chief),
            Then::Paused(Next::Human),
            Then::Moved,
        ] {
            for reply in [
                "Stopped.",
                "Stopped.\n\nThe lexer is next.",
                // Words of the note's own in the reply split it elsewhere, and render the same.
                "It said:\n\nWhat it said last:\n\nnothing",
                "Done\n\nand ended with:\n\nnothing",
            ] {
                let answered = Answered {
                    reply: Some(reply.to_owned()),
                    ..said(then)
                };
                let body = render(&answered);
                let read = read(&body).unwrap_or_else(|| panic!("{body:?}"));
                assert_eq!((read.number, read.tell, read.then), (7, 12, then));
                assert_eq!(render(&read), body, "{reply:?}");
            }
        }
    }

    #[test]
    fn a_build_before_t_958_has_no_reply_to_write_and_keeps_its_note() {
        let before = Answered {
            reply: Some("ignored".to_owned()),
            ..said(Then::MovedBefore)
        };
        assert_eq!(render(&before), render(&said(Then::MovedBefore)));
    }

    #[test]
    fn the_words_of_a_note_with_a_reply_say_what_the_window_wrote_first_and_what_it_said_last() {
        let with_a_reply = |then| Answered {
            reply: Some("Stopped.".to_owned()),
            ..said(then)
        };
        assert_eq!(
            render(&with_a_reply(Then::Paused(Next::Chief))),
            "T-7 is paused, and @zeus-amber-pine's turn on the words put to its window (m-12) is over. What it wrote first, before any tool:\n\nStopped.\n\nWhat it said last:\n\nDone.\n\nThe parser is on grammar v2.\n\nIf that is the result, take it: cf task finish T-7. To send it on: cf task resume T-7 \"…\"."
        );
        assert_eq!(
            render(&with_a_reply(Then::Moved)),
            "T-7, paused when @zeus-amber-pine's turn on the words put to its window (m-12) began, has moved on since (resumed, or back on the board). This is not its result: that comes as a message of its own, the answer of the turn its resume, or its next assignment, begins, and it is the one to decide. What the turn on those words wrote first, before any tool:\n\nStopped.\n\nand ended with:\n\nDone.\n\nThe parser is on grammar v2."
        );
    }

    #[test]
    fn the_words_of_a_note_that_moved_on_say_so_and_name_what_the_window_said() {
        assert_eq!(
            render(&said(Then::Moved)),
            "T-7, paused when @zeus-amber-pine's turn on the words put to its window (m-12) began, has moved on since (resumed, or back on the board). This is not its result: that comes as a message of its own, the answer of the turn its resume, or its next assignment, begins, and it is the one to decide. What the turn on those words ended with:\n\nDone.\n\nThe parser is on grammar v2."
        );
        let [paused, moved] = prefixes(7);
        assert!(render(&said(Then::Moved)).starts_with(&moved));
        assert!(render(&said(Then::Paused(Next::Chief))).starts_with(&paused));
        assert!(render(&said(Then::Paused(Next::Human))).starts_with(&paused));
    }

    #[test]
    fn no_other_note_is_taken_for_one() {
        let paused = render(&said(Then::Paused(Next::Chief)));
        for body in [
            String::new(),
            "T-7 is paused: @zeus's window is gone. Resume it with: cf task resume T-7 \"…\"; its window comes back on its own conversation.".to_owned(),
            // Words added to a note, or cut from it.
            format!("Heads up. {paused}"),
            paused.replace("cf task finish", "cf task done"),
            paused.strip_suffix('.').unwrap().to_owned(),
            // A tail that names another task.
            paused.replace("T-7. To send", "T-8. To send"),
        ] {
            assert_eq!(read(&body), None, "{body:?}");
        }
    }
}

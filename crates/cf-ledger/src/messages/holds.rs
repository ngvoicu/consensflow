//! Hold notes: what ConsensFlow tells a task's requester when it holds the
//! task while its member is out of quota, and what it tells it when the task
//! goes on. A hold note names the reset the harness gave as the time the
//! member is expected back, and says the task goes on sooner if its account
//! has quota again: the human may log the harness into another account, and
//! the daemon then resumes what it held within minutes. A requester who was
//! given the note is told when the task goes on, however it did; one who was
//! not given it is told nothing, for the note is withdrawn when the task is
//! resumed (`leave_pause_notes`).
//!
//! Both are `note`s of ConsensFlow's own, told by their words, which are
//! written here and nowhere else: a kind of its own would be a migration, and
//! the schema is held byte for byte by Node's.

use cf_proto::ledger::MessageView;
use rusqlite::params;

use crate::model::LedgerError;
use crate::queue::{send, Sent};
use crate::store::Store;
use crate::views::TaskRow;

/// How a hold note ends: the time before it is an expected reset, not a promise.
const TAIL: &str = ", or sooner if its account has quota again; it goes on by itself.";

/// How a hold note ended before it said the account could be back sooner.
/// A task held by an older build has its note so, delivered or not.
const OLD_TAIL: &str = "; it goes on by itself then.";

/// How a hold note of an unavailable account ends: nothing resets such an account.
const TAIL_UNAVAILABLE: &str =
    ", and nothing resets that; it goes on when the human says the account works again.";

/// The words of a hold note of an account that is unavailable for `because`
/// (MUS-23): no time is named, for there is none.
fn render_unavailable(number: i64, handle: &str, because: &str) -> String {
    format!(
        "T-{number} waits with @{handle}: its account is unavailable ({because}){TAIL_UNAVAILABLE}"
    )
}

/// The words of a hold note: T-`number` waits with `handle`, who is out of
/// quota until `until`, an ISO time.
fn render(number: i64, handle: &str, until: &str) -> String {
    format!("T-{number} waits with @{handle}: out of quota until {until}{TAIL}")
}

/// What a requester is told when the task it was told waits goes on. It is
/// news, not a word of the wait: a later pause of the task does not withdraw it
/// (`leave_pause_notes`), or the requester would keep the promise it corrects.
pub(super) fn goes_on(number: i64) -> String {
    format!("T-{number} goes on: its account has quota again.")
}

/// Whether `body` is the hold note of T-`number`, as this build or an older
/// one wrote it. Only the words of a hold note are taken for one: its
/// handle and time are free, and nothing else ConsensFlow says begins and
/// ends so.
fn is_hold_note(number: i64, body: &str) -> bool {
    body.starts_with(&format!("T-{number} waits with @"))
        && [TAIL, OLD_TAIL, TAIL_UNAVAILABLE]
            .iter()
            .any(|tail| body.ends_with(tail))
}

/// Tells `to` that T-`number` is held with `handle` while its quota is out,
/// until `until`: a note of the task, as ConsensFlow's note of a hold always
/// was.
pub(crate) fn note_hold(
    store: &mut Store,
    project_id: i64,
    to: &str,
    number: i64,
    handle: &str,
    until: &str,
) -> Result<MessageView, LedgerError> {
    send(
        store,
        project_id,
        &Sent {
            to,
            body: &render(number, handle, until),
            task: Some(number),
            kind: "note",
            ..Sent::default()
        },
    )
}

/// Tells `to` that T-`number` is held with `handle` because its account is
/// unavailable for `because`, which no time cures: the human's word does.
pub(crate) fn note_hold_unavailable(
    store: &mut Store,
    project_id: i64,
    to: &str,
    number: i64,
    handle: &str,
    because: &str,
) -> Result<MessageView, LedgerError> {
    send(
        store,
        project_id,
        &Sent {
            to,
            body: &render_unavailable(number, handle, because),
            task: Some(number),
            kind: "note",
            ..Sent::default()
        },
    )
}

/// A held task went on: its window got a turn through after the words that
/// resumed it, or went on with it by itself. Its requester is told it goes on,
/// once, if it was given a note that said it waits and has not been told it
/// goes on since: that note reached its reader or is reaching it; one still
/// queued was withdrawn by the resume, and its reader was told nothing to
/// correct. Not when the words go in: a limit still spent refuses them at once,
/// and the wait goes on with no word to correct (T-96).
pub(crate) fn tell_goes_on(store: &mut Store, task: &TaskRow) -> Result<(), LedgerError> {
    let notes = store
        .db
        .prepare(
            "SELECT body, state FROM message
           WHERE task_id = ? AND recipient_id = ? AND sender_id IS NULL AND kind = 'note'
           ORDER BY id",
        )?
        .query_map(params![task.id, task.requester_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let told = goes_on(task.number);
    let given = notes.iter().rposition(|(body, state)| {
        is_hold_note(task.number, body)
            && matches!(state.as_str(), "delivering" | "delivered" | "read")
    });
    let Some(given) = given else {
        return Ok(());
    };
    if notes[given..].iter().any(|(body, _)| *body == told) {
        return Ok(());
    }
    let requester = store.participant_row(task.requester_id)?;
    send(
        store,
        task.project_id,
        &Sent {
            to: &requester.handle,
            body: &goes_on(task.number),
            task: Some(task.number),
            kind: "note",
            ..Sent::default()
        },
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hold_note_names_when_the_member_is_expected_and_that_the_task_may_go_on_sooner() {
        assert_eq!(
            render(166, "artemis-dusky-kestrel", "2026-10-13T08:00:00.000Z"),
            "T-166 waits with @artemis-dusky-kestrel: out of quota until 2026-10-13T08:00:00.000Z, or sooner if its account has quota again; it goes on by itself."
        );
    }

    #[test]
    fn what_a_requester_is_told_when_the_task_goes_on() {
        assert_eq!(goes_on(166), "T-166 goes on: its account has quota again.");
    }

    #[test]
    fn the_hold_note_of_a_task_is_known_by_its_words_as_this_build_and_an_older_one_wrote_them() {
        let until = "2026-10-07T18:00:00.000Z";
        assert!(is_hold_note(1, &render(1, "zeus", until)));
        assert!(is_hold_note(
            1,
            "T-1 waits with @zeus: out of quota until 2026-10-07T18:00:00.000Z; it goes on by itself then."
        ));
    }

    #[test]
    fn no_other_note_is_taken_for_the_hold_note_of_a_task() {
        let until = "2026-10-07T18:00:00.000Z";
        for (number, body) in [
            (1, String::new()),
            // Another task's.
            (2, render(1, "zeus", until)),
            (1, render(10, "zeus", until)),
            // Other notes about a wait.
            (1, "T-1 is paused: @zeus's window is gone.".to_owned()),
            (
                1,
                "T-1 was taken back from @zeus (ran out of quota) and waits for another standard worker."
                    .to_owned(),
            ),
            (1, "T-1 goes on: its account has quota again.".to_owned()),
            // Words added to a hold note, or cut from it.
            (1, format!("Heads up. {}", render(1, "zeus", until))),
            (
                1,
                "T-1 waits with @zeus: out of quota until 2026-10-07T18:00:00.000Z".to_owned(),
            ),
        ] {
            assert!(!is_hold_note(number, &body), "{number}: {body:?}");
        }
    }
}

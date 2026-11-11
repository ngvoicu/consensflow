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
//! the schema is held byte for byte by Node's. A chief is typed the first note of a hold
//! (quota, an unavailable account, a moved model) for each task, once ([`told_this_hold`]):
//! its turn ends while it waits and nothing else wakes it, only the chief asks the human for
//! an account or a model, and a weekly limit may make it give the work to another tier. The
//! same hold said again, and a task's going on, are shown, not typed (`queue`'s `SHOWN`):
//! the chief's own move or the human's. Its task's line says the hold either way (`said`).

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

/// How a hold note of a window whose model its harness moved ends: nothing resets that
/// either, and only the human's word for the member lets it go on (MUS-24).
const TAIL_MOVED: &str = "; it goes on when the human says it may.";

/// The words of a hold note of a window whose model moved on its own, `because`: the
/// reader's words say who moved it, from what, to what.
fn render_moved(number: i64, handle: &str, because: &str) -> String {
    format!("T-{number} waits with @{handle}: {because}{TAIL_MOVED}")
}

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

/// What a hold note of a window whose model was busy says of it (T-1112): its provider
/// refused for a moment, which waiting cures, and no failure.
const BUSY: &str = ": its model is busy, a refusal of its provider for a moment and no failure";

/// The words of a hold note of a window whose model was busy, `because` (the harness's own
/// words): T-`number` is held with `handle` and tried again after a backoff, a few times;
/// one given by tier (`tiered`) then goes to another free member of its tier, if there is
/// one. The requester is told once: neither a try nor the task going on is told again, and
/// it is no hold note `tell_goes_on` corrects (`is_hold_note`).
fn render_busy(number: i64, handle: &str, because: &str, tiered: bool) -> String {
    let then = if tiered {
        "then gives it to another free member of its tier, or tries every 5 minutes while none is free"
    } else {
        "then every 5 minutes"
    };
    format!(
        "T-{number} waits with @{handle}{BUSY} ({because}). ConsensFlow tries it again in 1, 2 and 4 minutes, {then}; you are not told of each try."
    )
}

/// Whether `body` is the hold note of T-`number` that its window's model was busy
/// ([`render_busy`]): the requester was told it already, unless the note was withdrawn.
pub fn is_busy_note(number: i64, body: &str) -> bool {
    body.contains(BUSY) && body.starts_with(&format!("T-{number} waits with @"))
}

/// What a requester is told when a task held for its window's moved model goes on: the
/// human said it may (MUS-24), not that an account has quota again.
pub(super) fn goes_on_moved(number: i64) -> String {
    format!("T-{number} goes on: the human said its model may go on.")
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
        && [TAIL, OLD_TAIL, TAIL_UNAVAILABLE, TAIL_MOVED]
            .iter()
            .any(|tail| body.ends_with(tail))
}

/// What T-`number`'s line says of the hold whose note is `body` (quota, an unavailable
/// account, a moved model), as long as it holds: the note without its task and its tail. None
/// for any other words.
pub(crate) fn said(number: i64, body: &str) -> Option<String> {
    let rest = body.strip_prefix(&format!("T-{number} "))?;
    if !rest.starts_with("waits with @") {
        return None;
    }
    if let Some(quota) = rest
        .strip_suffix(TAIL)
        .or_else(|| rest.strip_suffix(OLD_TAIL))
    {
        return Some(quota.to_owned());
    }
    if let Some(account) = rest.strip_suffix(TAIL_UNAVAILABLE) {
        return Some(format!("{account} until the human says it works"));
    }
    rest.strip_suffix(TAIL_MOVED)
        .map(|moved| format!("{moved}, until the human says it may go on"))
}

/// Which hold a note of T-`number` says, `body`: one of quota with its end, one of an
/// unavailable account, one of a moved model. A hold is the same hold while its kind is, and
/// a quota's end; who holds it, and the words the harness gave, may differ.
#[derive(PartialEq)]
enum Hold<'a> {
    Quota(&'a str),
    Unavailable,
    Moved,
}

fn hold_of(number: i64, body: &str) -> Option<Hold<'_>> {
    let rest = body.strip_prefix(&format!("T-{number} waits with @"))?;
    if let Some(quota) = rest
        .strip_suffix(TAIL)
        .or_else(|| rest.strip_suffix(OLD_TAIL))
    {
        return quota
            .split_once(": out of quota until ")
            .map(|(_, until)| Hold::Quota(until));
    }
    if rest.ends_with(TAIL_UNAVAILABLE) {
        Some(Hold::Unavailable)
    } else if rest.ends_with(TAIL_MOVED) {
        Some(Hold::Moved)
    } else {
        None
    }
}

/// Whether `to` was typed the note of the hold `body` says of T-`number` already, or has it
/// on its way: a note of the same hold to it, typed or still to be, not withdrawn, with no
/// note since that the task went on. Read from the notes the ledger holds and nothing else:
/// a note withdrawn before it was given told nobody, and a hold after the task went on is
/// another. Then the note now is the same hold said again, which is shown, not typed.
fn told_this_hold(
    store: &Store,
    project_id: i64,
    to: &str,
    number: i64,
    body: &str,
) -> Result<bool, LedgerError> {
    let Some(hold) = hold_of(number, body) else {
        return Ok(false);
    };
    let task = store.task_row(project_id, number)?;
    let reader = store.participant_by_handle(project_id, to)?;
    let notes = store
        .db
        .prepare(
            "SELECT body, state IN ('queued', 'delivering', 'delivered', 'read')
                      AND NOT (json_valid(receipt) AND json_extract(receipt, '$.shown') IS NOT NULL)
               FROM message
              WHERE task_id = ? AND recipient_id = ? AND sender_id IS NULL AND kind = 'note'
              ORDER BY id DESC",
        )?
        .query_map(params![task.id, reader.id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, bool>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for (said, typed) in &notes {
        if *said == goes_on(number) || *said == goes_on_moved(number) {
            return Ok(false);
        }
        if *typed && hold_of(number, said).is_some_and(|told| told == hold) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Sends the note of a hold, `body`, to `to`: typed to a chief the first time, shown the same
/// hold said again ([`told_this_hold`]); to anyone else queued as any note.
fn send_hold(
    store: &mut Store,
    project_id: i64,
    to: &str,
    number: i64,
    body: &str,
) -> Result<MessageView, LedgerError> {
    let again = told_this_hold(store, project_id, to, number, body)?;
    let words = said(number, body);
    send(
        store,
        project_id,
        &Sent {
            to,
            body,
            task: Some(number),
            kind: "note",
            shown: words.as_deref().filter(|_| again),
            ..Sent::default()
        },
    )
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
    send_hold(
        store,
        project_id,
        to,
        number,
        &render(number, handle, until),
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
    let body = render_unavailable(number, handle, because);
    send_hold(store, project_id, to, number, &body)
}

/// Tells `to` that T-`number` is held with `handle` because its harness moved its window's
/// model on its own, `because` (MUS-24): no time resets that, and the human's word does.
pub(crate) fn note_hold_moved(
    store: &mut Store,
    project_id: i64,
    to: &str,
    number: i64,
    handle: &str,
    because: &str,
) -> Result<MessageView, LedgerError> {
    send_hold(
        store,
        project_id,
        to,
        number,
        &render_moved(number, handle, because),
    )
}

/// Tells `to` that T-`number` is held with `handle` because its model was busy, `because`
/// (T-1112): it is tried again after a backoff of the daemon's, and is no failure.
pub(crate) fn note_hold_busy(
    store: &mut Store,
    project_id: i64,
    to: &str,
    number: i64,
    handle: &str,
    because: &str,
    tiered: bool,
) -> Result<MessageView, LedgerError> {
    send(
        store,
        project_id,
        &Sent {
            to,
            body: &render_busy(number, handle, because, tiered),
            task: Some(number),
            kind: "note",
            shown: Some(&format!(
                "waits with @{handle}: its model is busy, and ConsensFlow tries it again"
            )),
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
    let given = notes.iter().rposition(|(body, state)| {
        is_hold_note(task.number, body)
            && matches!(state.as_str(), "delivering" | "delivered" | "read")
    });
    let Some(given) = given else {
        return Ok(());
    };
    // Said in the words of the hold it corrects: a model that moved is let go on by the human,
    // not an account that has quota again.
    let told = if notes[given].0.ends_with(TAIL_MOVED) {
        goes_on_moved(task.number)
    } else {
        goes_on(task.number)
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
            body: &told,
            task: Some(task.number),
            kind: "note",
            shown: told.strip_prefix(&format!("T-{} ", task.number)),
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

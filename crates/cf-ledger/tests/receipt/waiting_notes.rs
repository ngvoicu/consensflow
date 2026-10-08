//! The notes of a task's wait (`messages/pauses.rs`, `messages/holds.rs`): the
//! ones its requester has not been given go when the task moves on, whether
//! another member takes it or it is taken back to the board, as they go when it
//! is resumed or called off; and a requester who was given the note that a
//! task is held is told when the daemon resumes it. A resume of the chief's
//! own, and a task that goes back on the board, tell nothing of the kind.

use cf_ledger::{MessageView, NewNote};

use crate::fixture::{world, World};

/// The end of the hold the fixture names.
const UNTIL: &str = "2026-10-10T15:00:00.000Z";

/// What ConsensFlow tells a requester of a task that goes on.
const GOES_ON: &str = "T-1 goes on: its account has quota again.";

/// ConsensFlow's note to `to` about task `number`, saying `body`.
fn consensflow_notes(w: &mut World, to: &str, number: Option<i64>, body: &str) -> MessageView {
    w.ledger
        .note(
            w.project,
            &NewNote {
                from: None,
                to: to.into(),
                body: body.into(),
                task: number,
            },
        )
        .expect("a note")
}

/// What the chief's inbox holds from ConsensFlow, oldest first.
fn told_chief(w: &World) -> Vec<MessageView> {
    let mut notes: Vec<MessageView> = w
        .ledger
        .inbox(w.id("chief"), 100)
        .expect("the chief's inbox")
        .into_iter()
        .filter(|message| message.kind == "note" && message.sender.is_none())
        .collect();
    notes.reverse();
    notes
}

/// A message's state and the reason it was withdrawn, if it was.
fn fate(w: &World, id: i64) -> (String, Option<String>) {
    let message = w.message(id);
    (message.state, message.reason)
}

/// T-1 for the standard workers, in zeus's session, its brief delivered.
fn with_zeus(w: &mut World) -> (i64, String) {
    let task = w.open("Parser").task.number;
    let moved = w.assign(task, "zeus");
    let session = moved.task.assignee.clone().expect("a session");
    w.deliver(moved.message.expect("its brief").id);
    (task, session)
}

/// The note that tells the chief T-`task` is held with `session`, without holding it.
fn held_note(w: &mut World, task: i64, session: &str) -> MessageView {
    w.ledger
        .note_hold(w.project, "chief", task, session, UNTIL)
        .expect("the chief is told")
}

/// The hold of T-`task` with `session`, and the note that tells the chief of it.
fn held(w: &mut World, task: i64, session: &str) -> MessageView {
    w.hold(task);
    held_note(w, task, session)
}

#[test]
fn a_task_another_member_takes_withdraws_the_notes_of_its_wait_its_requester_has_not_been_given() {
    let mut w = world();
    let (task, session) = with_zeus(&mut w);
    // zeus ran out of quota: the daemon takes the task back, and says that
    // nobody is free; both notes wait, the chief being in a turn.
    w.ledger
        .release_task(w.project, task, "ran out of quota after starting")
        .expect("released");
    let waits = consensflow_notes(
        &mut w,
        "chief",
        Some(task),
        "T-1 waits for a free standard worker: @zeus is out of quota until noon.",
    );
    let taken_back = told_chief(&w)
        .into_iter()
        .find(|note| note.body.starts_with("T-1 was taken back from"))
        .expect("the chief is told it was taken back");
    assert_eq!(
        taken_back.body,
        format!(
            "T-1 was taken back from @{session} (ran out of quota after starting) and waits for another standard worker."
        )
    );
    assert_eq!(w.states(&[taken_back.id, waits.id]), ["queued", "queued"]);

    w.assign(task, "diana");
    let withdrawn = (
        "cancelled".to_owned(),
        Some("T-1 was taken by @diana".to_owned()),
    );
    assert_eq!(fate(&w, taken_back.id), withdrawn);
    assert_eq!(fate(&w, waits.id), withdrawn);
}

#[test]
fn a_task_another_member_takes_leaves_what_was_given_what_others_wrote_and_what_is_about_another() {
    let mut w = world();
    let (task, _) = with_zeus(&mut w);
    let other = w.open("Lexer").task.number;
    w.ledger
        .release_task(w.project, task, "ran out of quota after starting")
        .expect("released");
    let given = told_chief(&w).remove(0);
    w.deliver(given.id);
    let chiefs = w.note("chief", "chief", task, "A reminder from the chief.");
    let for_the_human = consensflow_notes(&mut w, "human", Some(task), "T-1 is back on the board.");
    let of_another = consensflow_notes(
        &mut w,
        "chief",
        Some(other),
        "T-2 waits for a free standard worker: @zeus is out of quota until noon.",
    );
    let of_none = consensflow_notes(
        &mut w,
        "chief",
        None,
        "Something else the chief should know.",
    );

    w.assign(task, "diana");
    assert_eq!(
        fate(&w, given.id),
        ("delivered".to_owned(), None),
        "a note its reader has been given"
    );
    for (kept, why) in [
        (chiefs.id, "a note of the chief's"),
        (for_the_human.id, "a note for another reader"),
        (of_another.id, "a note about another task"),
        (of_none.id, "a note about none"),
    ] {
        assert_eq!(w.message(kept).state, "queued", "{why}");
    }
}

#[test]
fn a_task_another_member_takes_leaves_a_note_of_several_that_named_it() {
    let mut w = world();
    let (task, session) = with_zeus(&mut w);
    let other = w.open("Lexer").task.number;
    let other_session = w.assign(other, "diana").task.assignee.expect("a session");
    // Both stalled in a pass, and told in one note.
    let because = |session: &str| format!("@{session}'s window is gone");
    w.ledger
        .pause_task(w.project, task, None, Some(&because(&session)))
        .expect("paused");
    let note = w
        .ledger
        .note_pause(w.project, "chief", task, &because(&session))
        .expect("the chief is told");
    w.ledger
        .pause_task(w.project, other, None, Some(&because(&other_session)))
        .expect("paused");
    w.ledger
        .join_pause_note(note.id, other, &because(&other_session))
        .expect("asked")
        .expect("the note takes it");

    // The human gives T-1 to another member: it is no longer paused.
    w.ledger
        .release_task(w.project, task, "by @human")
        .expect("released");
    w.assign(task, "zeus");
    let narrowed = w.message(note.id);
    assert_eq!(narrowed.state, "queued");
    assert!(
        narrowed.body.starts_with("T-2 is paused:") && !narrowed.body.contains("T-1 is paused"),
        "{:?}",
        narrowed.body
    );
    assert_eq!(narrowed.task_number, Some(other), "it is T-2's alone now");
}

#[test]
fn a_task_taken_back_withdraws_the_notes_of_the_wait_it_leaves_and_keeps_its_own() {
    let mut w = world();
    let (task, session) = with_zeus(&mut w);
    let hold = held(&mut w, task, &session);
    let stall = consensflow_notes(
        &mut w,
        "chief",
        Some(task),
        &format!("T-1 is paused: @{session}'s window is gone."),
    );
    assert_eq!(w.states(&[hold.id, stall.id]), ["queued", "queued"]);

    // The human gives it to another member while it is held.
    w.ledger
        .release_task(w.project, task, "by @human")
        .expect("released");
    let withdrawn = (
        "cancelled".to_owned(),
        Some("T-1 was taken back".to_owned()),
    );
    assert_eq!(fate(&w, hold.id), withdrawn);
    assert_eq!(fate(&w, stall.id), withdrawn);
    let told = told_chief(&w);
    let release = told.last().expect("the note of the release");
    assert_eq!(
        (release.state.as_str(), release.body.as_str()),
        (
            "queued",
            format!(
                "T-1 was taken back from @{session} (by @human) and waits for another standard worker."
            )
            .as_str()
        ),
        "what the release says of itself stays"
    );
}

#[test]
fn a_requester_given_the_note_that_a_task_is_held_is_told_when_the_daemon_resumes_it() {
    let mut w = world();
    let (task, session) = with_zeus(&mut w);
    let hold = held(&mut w, task, &session);
    assert_eq!(
        hold.body,
        format!(
            "T-1 waits with @{session}: out of quota until {UNTIL}, or sooner if its account has quota again; it goes on by itself."
        )
    );
    w.deliver(hold.id);

    w.daemon_resumes(task);
    let told = told_chief(&w);
    let correction = told.last().expect("the chief is told");
    assert_eq!(correction.body, GOES_ON);
    assert_eq!(
        (
            correction.state.as_str(),
            correction.task_number,
            correction.sender.as_deref(),
            correction.recipient.as_str()
        ),
        ("queued", Some(task), None, "chief")
    );
    assert_eq!(
        fate(&w, hold.id),
        ("delivered".to_owned(), None),
        "what the chief was given stays"
    );
    assert_eq!(told.len(), 2, "one note more");
}

#[test]
fn a_requester_being_given_the_hold_note_is_told_too_but_one_not_given_it_is_told_nothing() {
    let mut w = world();
    let (task, session) = with_zeus(&mut w);
    let hold = held(&mut w, task, &session);
    // Being pasted into the chief's window as the daemon resumes the task.
    w.begin(hold.id);
    w.daemon_resumes(task);
    assert_eq!(
        told_chief(&w).last().map(|note| note.body.as_str()),
        Some(GOES_ON),
        "it reaches the chief's window, and the chief is told it goes on"
    );

    // Another hold: the note is still queued when the daemon resumes the task.
    w.confirm(hold.id);
    let again = held(&mut w, task, &session);
    let before = told_chief(&w).len();
    w.daemon_resumes(task);
    assert_eq!(
        fate(&w, again.id),
        ("cancelled".to_owned(), Some("T-1 resumed".to_owned()))
    );
    assert_eq!(
        told_chief(&w).len(),
        before,
        "nothing is said of a note nobody read"
    );
}

#[test]
fn a_resume_of_the_chiefs_own_tells_nothing_more() {
    let mut w = world();
    let (task, session) = with_zeus(&mut w);
    let hold = held(&mut w, task, &session);
    w.deliver(hold.id);
    w.resume(task, "Go on");
    assert_eq!(w.state(task), "queued");
    let told = told_chief(&w);
    assert!(
        told.iter().all(|note| note.body != GOES_ON),
        "the chief resumed it: it knows"
    );
}

#[test]
fn only_the_note_of_the_hold_the_task_is_in_counts() {
    let mut w = world();
    let (task, session) = with_zeus(&mut w);
    let hold = held(&mut w, task, &session);
    w.deliver(hold.id);
    w.daemon_resumes(task);
    assert_eq!(told_chief(&w).len(), 2);

    // The member runs into its limit again with no note told of it (a window
    // whose turn its quota cut short): the hold before it is not this one.
    w.hold(task);
    w.daemon_resumes(task);
    assert_eq!(
        told_chief(&w).len(),
        2,
        "the first hold's note was answered once, and is not this hold's"
    );
}

#[test]
fn the_news_that_a_held_task_went_on_is_not_taken_back_by_a_hold_that_comes_after_it() {
    let mut w = world();
    let (task, session) = with_zeus(&mut w);
    let hold = held(&mut w, task, &session);
    w.deliver(hold.id);
    w.daemon_resumes(task);
    let news = told_chief(&w).last().expect("the chief is told").id;
    assert_eq!(w.message(news).body, GOES_ON);

    // The new account runs into its limit before the chief is given the news:
    // the note of this hold goes when the daemon resumes the task again, and
    // the news does not, or the chief would keep the promise it corrects.
    let again = held(&mut w, task, &session);
    w.daemon_resumes(task);
    assert_eq!(
        fate(&w, again.id),
        ("cancelled".to_owned(), Some("T-1 resumed".to_owned()))
    );
    assert_eq!(fate(&w, news), ("queued".to_owned(), None));

    // A task called off is no more news for anyone.
    w.ledger
        .cancel_task(w.project, task, "chief")
        .expect("called off");
    assert_eq!(w.message(news).state, "cancelled");
}

#[test]
fn a_hold_note_of_an_older_build_counts_and_one_that_failed_or_is_not_one_does_not() {
    let mut w = world();
    let (task, session) = with_zeus(&mut w);
    w.hold(task);
    let old = consensflow_notes(
        &mut w,
        "chief",
        Some(task),
        &format!(
            "T-1 waits with @{session}: out of quota until {UNTIL}; it goes on by itself then."
        ),
    );
    w.deliver(old.id);
    w.daemon_resumes(task);
    assert_eq!(
        told_chief(&w).last().map(|note| note.body.as_str()),
        Some(GOES_ON),
        "a hold an older build told is corrected too"
    );

    // A hold note that never reached the chief, and notes that are none.
    w.hold(task);
    let failed = held_note(&mut w, task, &session);
    w.begin(failed.id);
    w.ledger
        .fail_delivery(failed.id, "the window went away")
        .expect("failed");
    let other = consensflow_notes(&mut w, "chief", Some(task), "T-1 is paused: only so.");
    w.deliver(other.id);
    let before = told_chief(&w).len();
    w.daemon_resumes(task);
    assert_eq!(told_chief(&w).len(), before, "none was given the chief");
}

#[test]
fn a_task_that_goes_back_on_the_board_when_its_hold_ends_is_not_told_to_go_on() {
    let mut w = world();
    let (task, session) = with_zeus(&mut w);
    let hold = held(&mut w, task, &session);
    w.deliver(hold.id);
    // The session that held it was deleted meanwhile.
    w.ledger
        .end_session(w.project, &session, "human")
        .expect("a session that holds only a paused task ends");

    w.daemon_resumes(task);
    assert_eq!(w.state(task), "open", "back on the board for its tier");
    assert!(
        told_chief(&w).iter().all(|note| note.body != GOES_ON),
        "it does not go on in its window"
    );
}

//! One task per member session, at every door: a session takes one task at a
//! time, whichever door gives it (a follow-up, a reopen, a task named for it,
//! or what waited on the board for its needs), refused or made to wait in the
//! one rule and words; and a window's stop is the stop of the task that window
//! works on.

use cf_ledger::LedgerError;

use crate::fixture::{world, World};

/// A task opened for the standard workers and given to a session of zeus,
/// its brief received and its result recorded: the session and the task.
fn done_by_a_session(w: &mut World) -> (String, i64) {
    let task = w.open("Parser").task.number;
    let moved = w.assign(task, "zeus");
    let session = moved.task.assignee.clone().expect("a session");
    w.deliver(moved.message.expect("its brief").id);
    w.ledger
        .record_result(w.project, task, "Parser, done")
        .expect("a result");
    assert_eq!(w.state(task), "done");
    (session, task)
}

/// A follow-up of `after` for its session, delivered and at work: the task.
fn at_work_on_a_follow_up(w: &mut World, after: i64, body: &str) -> i64 {
    let given = w.follow_up(after, body, &[]).expect("a follow-up");
    w.deliver(given.message.expect("its brief").id);
    assert_eq!(w.state(given.task.number), "working");
    given.task.number
}

/// A door's refusal of a session that is spoken for: always the same code and
/// words, whichever door.
fn assert_busy(refused: LedgerError, session: &str) {
    assert_eq!(refused.code(), Some("session-busy"));
    assert_eq!(
        refused.to_string(),
        format!(
            "@{session} is still on its work: wait for its result, or open the task for its tier"
        )
    );
}

/// The task a session's window is asked to stop for, and how many times.
fn stop_of(w: &World, session: &str) -> Option<(i64, i64)> {
    w.ledger
        .stop_of(w.id(session))
        .expect("a read")
        .map(|stop| (stop.number, stop.seq))
}

/// Whether the session's window has a task in hand.
fn in_hand(w: &World, session: &str) -> bool {
    w.ledger.has_task_in_hand(w.id(session)).expect("a read")
}

/// The head of the session's queue is delivered.
fn deliver_next(w: &mut World, session: &str) {
    let head = w.next(session).expect("something for the window");
    w.deliver(head);
}

#[test]
fn a_follow_up_is_refused_while_its_session_is_on_its_work_and_goes_once_it_is_free() {
    let mut w = world();
    let (session, a) = done_by_a_session(&mut w);
    let b = at_work_on_a_follow_up(&mut w, a, "Tests");

    let refused = w
        .follow_up(a, "More tests", &[])
        .expect_err("the session is on its work");
    assert_busy(refused, &session);
    assert!(
        w.ledger.task(w.project, b + 1).expect("a read").is_none(),
        "nothing of the refused task is left"
    );

    w.ledger
        .record_result(w.project, b, "Tests, done")
        .expect("a result");
    let more = w
        .follow_up(a, "More tests", &[])
        .expect("the session is free");
    assert_eq!(w.state(more.task.number), "queued");
    assert_eq!(w.next(&session), Some(more.message.expect("its brief").id));
}

#[test]
fn a_task_is_not_reopened_onto_a_session_that_has_another_and_it_is_told_what_a_follow_up_is() {
    let mut w = world();
    let (session, a) = done_by_a_session(&mut w);
    // B goes to the session that did A, and is at work.
    let b = at_work_on_a_follow_up(&mut w, a, "Tests");

    // A is sent back to the session that is on B: refused, in the words a follow-up gets.
    let reopened = w
        .ledger
        .reopen_task(w.project, a, "chief", "More")
        .expect_err("the session is on its work");
    let followed = w
        .follow_up(a, "More tests", &[])
        .expect_err("the session is on its work");
    assert_eq!(reopened.to_string(), followed.to_string());
    assert_busy(reopened, &session);
    assert_eq!(
        w.state(a),
        "done",
        "nothing of the refused reopening is left"
    );
    assert_eq!(w.next(&session), None, "no words for the window");

    // So A can never be paused behind B's window, and B's window is asked nothing.
    w.ledger
        .pause_task(w.project, a, Some("chief"), None)
        .expect_err("a task that is done is not paused");
    assert_eq!(stop_of(&w, &session), Some((b, 0)));

    // Once B is over, the session is free for A again.
    w.ledger
        .record_result(w.project, b, "Tests, done")
        .expect("a result");
    let words = w
        .ledger
        .reopen_task(w.project, a, "chief", "More")
        .expect("the session is free")
        .message
        .expect("its words");
    w.deliver(words.id);
    assert_eq!(w.state(a), "working");
}

#[test]
fn a_task_named_for_a_session_is_refused_while_it_is_on_its_work_and_goes_once_it_is_free() {
    let mut w = world();
    let (session, a) = done_by_a_session(&mut w);
    let b = w
        .give_to(&session, &[], "Tests")
        .expect("the session is free");
    assert_eq!(w.state(b.task.number), "queued");

    // Spoken for, the second is refused whether it would go to the window now
    // or wait on the board for what it needs.
    for needs in [&[][..], &[a][..]] {
        let refused = w
            .give_to(&session, needs, "More")
            .expect_err("the session has a task");
        assert_busy(refused, &session);
    }
    assert!(
        w.ledger
            .task(w.project, b.task.number + 1)
            .expect("a read")
            .is_none(),
        "nothing of the refused tasks is left"
    );

    w.deliver(b.message.expect("its brief").id);
    w.ledger
        .record_result(w.project, b.task.number, "Tests, done")
        .expect("a result");
    let more = w
        .give_to(&session, &[], "More")
        .expect("the session is free");
    assert_eq!(w.state(more.task.number), "queued");
}

#[test]
fn the_rule_is_a_member_sessions_and_the_chief_and_a_members_own_lane_are_held_to_none() {
    let mut w = world();
    // A member's own lane and the chief's take tasks by name, one after another
    // (only a ledger's own callers name a participant: `cf` names the chief itself).
    for to in ["zeus", "chief"] {
        let (one, two) = (w.give(to, "One"), w.give(to, "Two"));
        assert_eq!(
            (w.state(one.task.number), w.state(two.task.number)),
            ("queued".to_owned(), "queued".to_owned()),
            "@{to} is no session"
        );
    }
}

#[test]
fn a_task_opened_for_a_tier_goes_to_a_session_of_its_own_whatever_its_member_has_in_hand() {
    let mut w = world();
    let first = w.open("Parser").task.number;
    let second = w.open("Lexer").task.number;
    let (first, second) = (w.assign(first, "zeus"), w.assign(second, "zeus"));
    let sessions = [&first, &second].map(|moved| moved.task.assignee.clone().expect("a session"));
    assert_ne!(sessions[0], sessions[1], "a new session at each assignment");
    assert!(sessions.iter().all(|session| in_hand(&w, session)));
}

#[test]
fn a_follow_up_waiting_for_what_it_needs_makes_its_session_busy_to_every_door_though_its_window_has_nothing_in_hand(
) {
    let mut w = world();
    let (session, a) = done_by_a_session(&mut w);
    // A follow-up that waits on the board for A to be accepted. The session
    // is free of A (its result is in), and it is this follow-up's.
    let later = w
        .follow_up(a, "Later", &[a])
        .expect("a follow-up")
        .task
        .number;
    assert_eq!(w.state(later), "open");
    assert!(!in_hand(&w, &session), "nothing of it reached the window");

    // What gave the session a second task once (the fix round's first tests
    // built their two-task state with the first of these): another follow-up,
    // for the window at once or waiting on the board as well, a task sent back
    // to it, and one named for it, in either way.
    let refusals = [
        w.follow_up(a, "Tests", &[]).err(),
        w.follow_up(a, "Tests, waiting", &[a]).err(),
        w.ledger.reopen_task(w.project, a, "chief", "More").err(),
        w.give_to(&session, &[], "By name").err(),
        w.give_to(&session, &[a], "By name, waiting").err(),
    ];
    for refusal in refusals {
        assert_busy(refusal.expect("the session is spoken for"), &session);
    }

    // A is accepted: the follow-up goes to the window, which takes it.
    w.ledger
        .accept_task(w.project, a, "chief")
        .expect("accepted");
    assert_eq!(w.state(later), "queued");
    assert!(in_hand(&w, &session));
    deliver_next(&mut w, &session);
    assert_eq!(w.state(later), "working");
}

/// A session that did A, with two follow-ups waiting on the board for A to be
/// accepted: the first one the door gave it, the second one handed to it as a
/// ledger written before one task per session could have (Node's, in which
/// `--after` did not count a follow-up that waited): the session, A, the first
/// and the second.
fn two_follow_ups_for_one_session(w: &mut World) -> (String, i64, i64, i64) {
    let (session, a) = done_by_a_session(w);
    let first = w
        .follow_up(a, "First", &[a])
        .expect("a follow-up")
        .task
        .number;
    let second = w.open_needing("Second", &[a]).task.number;
    w.hand_to(second, &session);
    (session, a, first, second)
}

/// A is accepted: the first is given to the session, and the second, which is
/// as ready, waits for it. Their session, the first and the second.
fn first_goes_and_second_waits(w: &mut World) -> (String, i64, i64) {
    let (session, a, first, second) = two_follow_ups_for_one_session(w);
    w.ledger
        .accept_task(w.project, a, "chief")
        .expect("accepted");
    assert_eq!(
        (w.state(first), w.state(second)),
        ("queued".to_owned(), "open".to_owned()),
        "one task is given to the session, the other waits for it"
    );
    (session, first, second)
}

/// The same, with the first at work, and the second waiting behind it.
fn second_waits_behind_the_first(w: &mut World) -> (String, i64, i64) {
    let (session, first, second) = first_goes_and_second_waits(w);
    deliver_next(w, &session);
    assert_eq!(w.state(first), "working");
    assert_eq!(w.state(second), "open", "the session is at work");
    (session, first, second)
}

/// The second is released and taken.
fn second_goes(w: &mut World, session: &str, second: i64) {
    assert_eq!(w.state(second), "queued");
    deliver_next(w, session);
    assert_eq!(w.state(second), "working");
}

#[test]
fn a_release_to_a_session_that_has_another_in_hand_waits_and_goes_with_that_tasks_result() {
    let mut w = world();
    let (session, first, second) = second_waits_behind_the_first(&mut w);
    w.ledger
        .record_result(w.project, first, "First, done")
        .expect("a result");
    second_goes(&mut w, &session, second);
}

#[test]
fn a_release_to_a_session_that_has_another_in_hand_waits_and_goes_when_that_task_is_called_off() {
    let mut w = world();
    let (session, first, second) = second_waits_behind_the_first(&mut w);
    w.ledger
        .cancel_task(w.project, first, "chief")
        .expect("called off");
    second_goes(&mut w, &session, second);
}

#[test]
fn a_release_to_a_session_that_has_another_in_hand_waits_and_goes_when_that_task_fails() {
    let mut w = world();
    let (session, first, second) = second_waits_behind_the_first(&mut w);
    w.ledger
        .fail_task(w.project, first, "its pane died")
        .expect("failed");
    second_goes(&mut w, &session, second);
}

#[test]
fn a_release_to_a_session_that_has_another_in_hand_waits_and_goes_when_that_tasks_delivery_fails_for_good(
) {
    let mut w = world();
    let (session, first, second) = first_goes_and_second_waits(&mut w);
    // The first's brief is on its way into the window when the delivery is given up.
    let brief = w.next(&session).expect("the first's brief");
    w.begin(brief);
    w.ledger
        .fail_delivery(brief, "its window never came up")
        .expect("given up");
    assert_eq!(w.state(first), "failed");
    second_goes(&mut w, &session, second);
}

#[test]
fn a_release_to_a_session_that_has_another_in_hand_waits_and_goes_when_that_task_is_taken_back_for_its_tier(
) {
    let mut w = world();
    // A is done and accepted: what a task can wait for.
    let (_, a) = done_by_a_session(&mut w);
    w.ledger
        .accept_task(w.project, a, "chief")
        .expect("accepted");
    // Docs is given to a session of diana and is at work on it; a task that
    // waited for A is handed to that session as an older ledger could have, and
    // waits for it, though it is as ready as can be.
    let docs = w.open("Docs").task.number;
    let moved = w.assign(docs, "diana");
    let session = moved.task.assignee.clone().expect("a session");
    w.deliver(moved.message.expect("its brief").id);
    let later = w.open_needing("Later", &[a]).task.number;
    w.hand_to(later, &session);
    assert_eq!(w.state(later), "open");

    // Docs goes back to the board for its tier, and the session is free.
    w.ledger
        .release_task(w.project, docs, "its member is out of quota")
        .expect("taken back");
    assert_eq!(w.state(docs), "open");
    second_goes(&mut w, &session, later);
}

#[test]
fn a_task_waiting_for_a_session_stays_through_what_does_not_free_it() {
    let mut w = world();
    let (session, first, second) = second_waits_behind_the_first(&mut w);
    // A pause keeps the session spoken for, and so does an acceptance elsewhere.
    w.ledger
        .pause_task(w.project, first, Some("chief"), None)
        .expect("paused");
    assert_eq!(w.state(second), "open");
    let other = w.open("Docs").task.number;
    let other = w.assign(other, "diana");
    w.deliver(other.message.expect("its brief").id);
    let other = other.task.number;
    w.ledger
        .record_result(w.project, other, "Docs, done")
        .expect("a result");
    w.ledger
        .accept_task(w.project, other, "chief")
        .expect("accepted");
    assert_eq!(w.state(second), "open", "its session still holds the first");
    assert_eq!(w.next(&session), None);
}

#[test]
fn a_pause_of_a_task_the_window_was_never_given_is_not_a_stop_of_its_window() {
    let mut w = world();
    let (session, a) = done_by_a_session(&mut w);
    // B follows A, and is paused before any words of it reach the window: the
    // window's last words were A's, and a stop of B is nothing it could pay.
    let b = w
        .follow_up(a, "Tests", &[])
        .expect("a follow-up")
        .task
        .number;
    w.ledger
        .pause_task(w.project, b, Some("human"), None)
        .expect("the human pauses it");
    assert_eq!(stop_of(&w, &session), None, "nothing was given it of B");

    // The words that resume B are the first it is given: B is its task from
    // then on, and its stop is the pause that came before them.
    let words = w.resume(b, "Go on").message.expect("its words");
    w.deliver(words.id);
    assert_eq!(w.state(b), "working");
    assert_eq!(stop_of(&w, &session), Some((b, 1)));
    w.ledger
        .pause_task(w.project, b, Some("human"), None)
        .expect("the human pauses B");
    assert_eq!(
        stop_of(&w, &session),
        Some((b, 2)),
        "and its next pause is a stop of it"
    );
}

#[test]
fn a_stop_paid_for_one_task_never_hides_a_later_stop_of_the_task_the_window_goes_on_with() {
    let mut w = world();
    let (session, a) = done_by_a_session(&mut w);
    let b = at_work_on_a_follow_up(&mut w, a, "Tests");
    w.ledger
        .pause_task(w.project, b, Some("human"), None)
        .expect("the human pauses B");
    assert_eq!(stop_of(&w, &session), Some((b, 1)));

    // B is called off, and the window goes on with the next follow-up.
    w.ledger
        .cancel_task(w.project, b, "chief")
        .expect("called off");
    let c = at_work_on_a_follow_up(&mut w, a, "More");
    assert_eq!(
        stop_of(&w, &session),
        Some((c, 0)),
        "B's stop, paid or not, is not the one of the task it works on now"
    );

    // C is paused once, and the stop that is the window's is C's.
    w.ledger
        .pause_task(w.project, c, Some("human"), None)
        .expect("the human pauses it");
    assert_eq!(stop_of(&w, &session), Some((c, 1)));
}

//! One task per session, and a window's stop is the stop of the task that
//! window works on: a task is not reopened onto a session that holds another,
//! and where a session does hold two (a follow-up that waited on the board
//! for what it needed, then went to a window already at work) a pause of the
//! one it is not working on stops nothing.

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

/// The task a session's window is asked to stop for, and how many times.
fn stop_of(w: &World, session: &str) -> Option<(i64, i64)> {
    w.ledger
        .stop_of(w.id(session))
        .expect("a read")
        .map(|stop| (stop.number, stop.seq))
}

/// The head of the session's queue is delivered.
fn deliver_next(w: &mut World, session: &str) {
    let head = w.next(session).expect("something for the window");
    w.deliver(head);
}

#[test]
fn a_task_is_not_reopened_onto_a_session_that_has_another_and_it_is_told_what_a_follow_up_is() {
    let mut w = world();
    let (session, a) = done_by_a_session(&mut w);
    // B goes to the session that did A, and is at work.
    let b = w.follow_up(a, "Tests", &[]).expect("a follow-up");
    w.deliver(b.message.expect("its brief").id);
    let b = b.task.number;
    assert_eq!(w.state(b), "working");

    // A is sent back to the session that is on B: refused, in the words a follow-up gets.
    let reopened = w
        .ledger
        .reopen_task(w.project, a, "chief", "More")
        .expect_err("the session is on its work");
    let followed = w
        .follow_up(a, "More tests", &[])
        .expect_err("the session is on its work");
    assert_eq!(reopened.code(), Some("session-busy"));
    assert_eq!(
        reopened.to_string(),
        format!(
            "@{session} is still on its work: wait for its result, or open the task for its tier"
        )
    );
    assert_eq!(reopened.to_string(), followed.to_string());
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
fn a_pause_of_a_task_the_session_holds_but_does_not_work_on_is_not_a_stop_of_its_window() {
    let mut w = world();
    let (session, a) = done_by_a_session(&mut w);
    // A follow-up that waits on the board for A to be accepted; while it does
    // the session is free, and another follow-up goes to it at once.
    let later = w
        .follow_up(a, "Later", &[a])
        .expect("a follow-up")
        .task
        .number;
    let b = w.follow_up(a, "Tests", &[]).expect("a follow-up");
    w.deliver(b.message.expect("its brief").id);
    let b = b.task.number;
    assert_eq!(w.state(b), "working");

    // A is accepted: the waiting follow-up is the session's too, queued behind B.
    w.ledger
        .accept_task(w.project, a, "chief")
        .expect("accepted");
    assert_eq!(w.state(later), "queued");
    assert_eq!(w.next(&session), None, "its words wait for B to be over");
    w.ledger
        .pause_task(w.project, later, Some("human"), None)
        .expect("the human pauses it");

    assert_eq!(
        stop_of(&w, &session),
        Some((b, 0)),
        "the window works on B: nothing was ever given it of the other"
    );
    w.ledger
        .pause_task(w.project, b, Some("human"), None)
        .expect("the human pauses B");
    assert_eq!(
        stop_of(&w, &session),
        Some((b, 1)),
        "and B's own pause is a stop of it"
    );
}

#[test]
fn a_stop_paid_for_one_task_never_hides_a_later_stop_of_the_task_the_window_goes_on_with() {
    let mut w = world();
    let (session, a) = done_by_a_session(&mut w);
    let later = w
        .follow_up(a, "Later", &[a])
        .expect("a follow-up")
        .task
        .number;
    let b = w.follow_up(a, "Tests", &[]).expect("a follow-up");
    w.deliver(b.message.expect("its brief").id);
    let b = b.task.number;
    w.ledger
        .pause_task(w.project, b, Some("human"), None)
        .expect("the human pauses B");
    assert_eq!(stop_of(&w, &session), Some((b, 1)));

    // A is accepted; what waited for it goes to the window, B being paused,
    // and the window goes on with it.
    w.ledger
        .accept_task(w.project, a, "chief")
        .expect("accepted");
    deliver_next(&mut w, &session);
    assert_eq!(w.state(later), "working");
    assert_eq!(
        stop_of(&w, &session),
        Some((later, 0)),
        "B's stop, paid or not, is not the one of the task it works on now"
    );

    // Both are paused once, and the stop that is the window's is the later task's.
    w.ledger
        .pause_task(w.project, later, Some("human"), None)
        .expect("the human pauses it");
    assert_eq!(stop_of(&w, &session), Some((later, 1)));
}

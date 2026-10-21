//! A follow-up queued behind a window's own work (T-110, 3.0.6; the chief of
//! 2026-10-21, running this project on 3.0.5): `--after T-3` was refused while
//! T-3's window was still on T-3, so the chief could not queue the next step of
//! a window's work while it ran. The follow-up is taken now, waits on the board
//! for T-3 to be accepted (T-3 is one of the tasks it needs), and goes into
//! that window then, as the follow-up of a finished task does. A window that
//! has a task in hand other than the one followed is refused as before.

use cf_ledger::LedgerError;

use crate::fixture::{world, World};

/// A task opened for the standard workers, given to a session of zeus and at
/// work: the session and the task.
fn at_work(w: &mut World) -> (String, i64) {
    let task = w.open("Parser").task.number;
    let moved = w.assign(task, "zeus");
    let session = moved.task.assignee.clone().expect("a session");
    w.deliver(moved.message.expect("its brief").id);
    assert_eq!(w.state(task), "working");
    (session, task)
}

/// The result of `number` is recorded and accepted.
fn finished(w: &mut World, number: i64) {
    w.ledger
        .record_result(w.project, number, "Done")
        .expect("a result");
    w.ledger
        .accept_task(w.project, number, "chief")
        .expect("accepted");
}

/// The numbers a task is blocked by, in order.
fn blocked_by(w: &World, number: i64) -> Vec<i64> {
    let mut blocked = w
        .ledger
        .task(w.project, number)
        .expect("a read")
        .expect("the task")
        .task
        .blocked_by;
    blocked.sort_unstable();
    blocked
}

/// The notes the chief has been sent about task `number`, oldest first.
fn notes_to_the_chief(w: &World, number: i64) -> Vec<String> {
    let mut notes: Vec<_> = w
        .ledger
        .inbox(w.id("chief"), 100)
        .expect("the chief's inbox")
        .into_iter()
        .filter(|message| message.kind == "note" && message.task_number == Some(number))
        .collect();
    notes.sort_by_key(|message| message.id);
    notes.into_iter().map(|message| message.body).collect()
}

#[test]
fn a_follow_up_of_a_task_at_work_waits_for_it_and_goes_to_its_window_once_it_is_accepted() {
    let mut w = world();
    let (session, a) = at_work(&mut w);

    let b = w
        .follow_up(a, "Tests", &[])
        .expect("taken while the window is on its work");
    assert!(
        b.message.is_none(),
        "nothing of it is queued for the window"
    );
    assert_eq!(b.task.state, "open");
    assert_eq!(b.task.assignee.as_deref(), Some(session.as_str()));
    assert_eq!(b.task.blocked_by, [a], "T-{a} is what it waits for");
    let b = b.task.number;
    assert_eq!(w.next(&session), None, "the window is left to its work");

    // The result is in, and not accepted: the follow-up still waits.
    w.ledger
        .record_result(w.project, a, "Parser, done")
        .expect("a result");
    assert_eq!(w.state(b), "open");
    w.ledger
        .accept_task(w.project, a, "chief")
        .expect("accepted");
    assert_eq!(w.state(b), "queued");
    assert!(blocked_by(&w, b).is_empty());
    let head = w.next(&session).expect("the follow-up for the window");
    assert_eq!(w.message(head).task_number, Some(b));
    assert_eq!(w.message(head).body, "Tests");
    w.deliver(head);
    assert_eq!(w.state(b), "working");
}

#[test]
fn two_follow_ups_of_one_window_at_work_go_to_it_in_order() {
    let mut w = world();
    let (session, a) = at_work(&mut w);
    let b = w.follow_up(a, "First", &[]).expect("a follow-up").task;
    let c = w.follow_up(a, "Second", &[]).expect("a second one").task;
    assert_eq!((b.state.as_str(), c.state.as_str()), ("open", "open"));
    assert_eq!(c.assignee.as_deref(), Some(session.as_str()));

    finished(&mut w, a);
    assert_eq!(w.state(b.number), "queued", "the first goes");
    assert_eq!(w.state(c.number), "open", "the second waits behind it");
    let head = w.next(&session).expect("the first");
    assert_eq!(w.message(head).task_number, Some(b.number));
    w.deliver(head);
    assert_eq!(w.state(c.number), "open", "while the first is at work");

    w.ledger
        .record_result(w.project, b.number, "First, done")
        .expect("a result");
    assert_eq!(
        w.state(c.number),
        "queued",
        "the second goes with its result"
    );
    let head = w.next(&session).expect("the second");
    assert_eq!(w.message(head).task_number, Some(c.number));
}

#[test]
fn a_follow_up_of_a_follow_up_that_waits_waits_for_it_too() {
    let mut w = world();
    let (session, a) = at_work(&mut w);
    let b = w
        .follow_up(a, "First", &[])
        .expect("a follow-up")
        .task
        .number;
    let c = w
        .follow_up(b, "Second", &[])
        .expect("a follow-up of the one that waits");
    assert_eq!(c.task.state, "open");
    assert_eq!(c.task.blocked_by, [b]);
    let c = c.task.number;

    finished(&mut w, a);
    assert_eq!(w.state(b), "queued");
    assert_eq!(w.state(c), "open", "T-{c} waits for T-{b} to be accepted");
    let head = w.next(&session).expect("the first");
    w.deliver(head);
    finished(&mut w, b);
    assert_eq!(w.state(c), "queued");
}

#[test]
fn a_follow_up_that_needs_other_tasks_too_waits_for_all_of_them() {
    let mut w = world();
    let (session, a) = at_work(&mut w);
    let other = w.open("Docs").task.number;
    let b = w
        .follow_up(a, "Tests", &[other, a])
        .expect("a follow-up with its needs");
    assert_eq!(blocked_by(&w, b.task.number), [a, other], "T-{a} once");
    let b = b.task.number;

    finished(&mut w, a);
    assert_eq!(w.state(b), "open", "T-{other} is not accepted");
    let moved = w.assign(other, "diana");
    w.deliver(moved.message.expect("its brief").id);
    finished(&mut w, other);
    assert_eq!(w.state(b), "queued");
    assert!(w.next(&session).is_some());
}

#[test]
fn a_follow_up_of_a_task_whose_window_is_on_another_is_refused_as_before() {
    let mut w = world();
    let (session, a) = at_work(&mut w);
    finished(&mut w, a);
    // B is the window's now; A is accepted, and a follow-up of A is not B's.
    let b = w
        .follow_up(a, "Tests", &[])
        .expect("a follow-up")
        .task
        .number;
    w.deliver(w.next(&session).expect("its brief"));
    assert_eq!(w.state(b), "working");

    let refused = w
        .follow_up(a, "More", &[])
        .expect_err("the window is on another task");
    assert_eq!(refused.code(), Some("session-busy"));
    assert_eq!(
        refused.to_string(),
        format!(
            "@{session} is still on its work: wait for its result, or open the task for its tier"
        )
    );
}

#[test]
fn a_task_that_fails_leaves_its_follow_up_waiting_and_the_chief_is_told_once() {
    let mut w = world();
    let (session, a) = at_work(&mut w);
    let b = w
        .follow_up(a, "Tests", &[])
        .expect("a follow-up")
        .task
        .number;
    assert!(notes_to_the_chief(&w, b).is_empty());

    w.ledger
        .fail_task(w.project, a, "its pane died")
        .expect("failed");
    assert_eq!(w.state(b), "open", "it is not released, nor lost");
    assert_eq!(blocked_by(&w, b), [a]);
    assert_eq!(
        notes_to_the_chief(&w, b),
        [format!(
            "T-{b} waits for T-{a}, which failed: cancel T-{b}, or reopen T-{a} and let it finish."
        )]
    );

    // Reopened, T-a goes on in its window, and its follow-up goes when it is accepted.
    let words = w
        .ledger
        .reopen_task(w.project, a, "chief", "Again")
        .expect("reopened")
        .message
        .expect("its words");
    w.deliver(words.id);
    assert_eq!(w.state(a), "working");
    assert_eq!(w.state(b), "open");
    finished(&mut w, a);
    assert_eq!(w.state(b), "queued");
    assert!(w.next(&session).is_some());
    assert_eq!(notes_to_the_chief(&w, b).len(), 1, "told once");
}

#[test]
fn a_task_with_a_follow_up_waiting_is_not_cancelled_until_the_follow_up_is() {
    let mut w = world();
    let (session, a) = at_work(&mut w);
    let b = w
        .follow_up(a, "Tests", &[])
        .expect("a follow-up")
        .task
        .number;

    let refused: LedgerError = w
        .ledger
        .cancel_task(w.project, a, "chief")
        .expect_err("T-b waits for it");
    assert_eq!(refused.code(), Some("waited-for"));
    assert_eq!(
        refused.to_string(),
        format!("T-{a} cannot be cancelled while T-{b} waits for it: cancel T-{b} first, or let T-{a} finish")
    );
    assert_eq!(w.state(a), "working");

    w.ledger
        .cancel_task(w.project, b, "chief")
        .expect("the follow-up is cancelled");
    w.ledger
        .cancel_task(w.project, a, "chief")
        .expect("and then the task");
    assert_eq!(w.state(a), "cancelled");
    assert_eq!(w.next(&session), None);
}

//! A failed task whose work is done (T-1111, a designer's session died with "Selected
//! model is at capacity" after it had saved both final files, and `cf task accept` was
//! refused: the task that needed it stayed stuck until the chief cancelled it and gave it
//! again without the need). The chief that has checked the work accepts the failed task
//! with the words of what it checked, which are its result; what waits for it is released
//! as for any accepted task. A failed task nobody accepts stays failed, and a reopening
//! of it is as it was.

use cf_ledger::{LedgerError, MessageView};

use crate::fixture::{world, World};
use crate::queued_tells::{queued, tell};

const CHECKED: &str = "Both final files are saved and whole";

/// A task opened for the standard workers and given to a session of zeus, at work: the
/// session and the task.
fn at_work(w: &mut World) -> (String, i64) {
    let task = w.open("Parser").task.number;
    let moved = w.assign(task, "zeus");
    let session = moved.task.assignee.clone().expect("a session");
    w.deliver(moved.message.expect("its brief").id);
    assert_eq!(w.state(task), "working");
    (session, task)
}

/// Task `number` fails as the designer's did.
fn fail(w: &mut World, number: i64) {
    w.ledger
        .fail_task(w.project, number, "Selected model is at capacity")
        .expect("failed");
    assert_eq!(w.state(number), "failed");
}

/// A task at work that failed: the session and the task.
fn failed(w: &mut World) -> (String, i64) {
    let (session, task) = at_work(w);
    fail(w, task);
    (session, task)
}

/// The messages of task `number`'s thread that are results.
fn results(w: &World, number: i64) -> Vec<MessageView> {
    w.ledger
        .task(w.project, number)
        .expect("a read")
        .expect("the task")
        .messages
        .into_iter()
        .filter(|message| message.kind == "result")
        .collect()
}

/// The numbers a task is blocked by.
fn blocked_by(w: &World, number: i64) -> Vec<i64> {
    w.ledger
        .task(w.project, number)
        .expect("a read")
        .expect("the task")
        .task
        .blocked_by
}

#[test]
fn a_failed_task_is_accepted_with_what_the_chief_checked_and_those_words_are_its_result() {
    let mut w = world();
    let (_, a) = failed(&mut w);

    let accepted = w
        .ledger
        .accept_failed_task(w.project, a, "chief", CHECKED)
        .expect("accepted on the chief's word");
    assert_eq!(accepted.state, "accepted");
    assert_eq!(w.state(a), "accepted");

    // The words are the task's result: in its thread, from the chief that wrote them,
    // received as they were written. Nothing is pasted anywhere for them.
    let [result] = results(&w, a).try_into().expect("one result");
    assert_eq!(result.body, CHECKED);
    assert_eq!(result.sender.as_deref(), Some("chief"));
    assert_eq!(result.state, "read");
    assert_eq!(w.next("chief"), None, "nothing waits to be pasted");

    // The board's card reads them as it reads any result.
    let board = w.ledger.board(w.project).expect("a board");
    let card = board
        .lanes
        .iter()
        .flat_map(|lane| lane.tasks.iter())
        .chain(board.open.iter())
        .find(|card| card.number == a)
        .expect("its card");
    assert_eq!(card.result.as_deref(), Some(CHECKED));

    // An accepted task is final: it is not reopened.
    let refused: LedgerError = w
        .ledger
        .reopen_task(w.project, a, "chief", "Again")
        .expect_err("accepted for good");
    assert_eq!(
        refused.to_string(),
        format!("cannot reopen T-{a}: it is accepted")
    );
}

#[test]
fn a_failed_task_is_not_accepted_without_the_words_of_what_was_checked() {
    let mut w = world();
    let (_, a) = failed(&mut w);

    for words in ["", "  \n "] {
        let refused: LedgerError = w
            .ledger
            .accept_failed_task(w.project, a, "chief", words)
            .expect_err("nothing was said of what was checked");
        assert_eq!(refused.code(), Some("checked-needed"), "{words:?}");
        assert_eq!(
            refused.to_string(),
            format!(
                "T-{a} failed: accepting it takes the words of what you checked, which are its result: cf task accept T-{a} \"what I checked\""
            )
        );
    }
    // The words fit the same bound as any other.
    let refused = w
        .ledger
        .accept_failed_task(w.project, a, "chief", &"x".repeat(1_000_001))
        .expect_err("too long");
    assert_eq!(refused.code(), Some("invalid-text"));
    assert_eq!(w.state(a), "failed", "it stays failed");
    assert!(results(&w, a).is_empty(), "and has no result");
}

#[test]
fn what_waits_for_a_failed_task_goes_on_when_it_is_accepted_and_not_before() {
    let mut w = world();
    let (session, a) = at_work(&mut w);
    // Its follow-up (for the window that did it) and a task for a tier that needs it,
    // both given while it was at work.
    let b = w
        .follow_up(a, "Tests", &[])
        .expect("a follow-up")
        .task
        .number;
    let c = w.open_needing("Docs", &[a]).task.number;
    fail(&mut w, a);
    assert_eq!((blocked_by(&w, b), blocked_by(&w, c)), (vec![a], vec![a]));

    // Refused for want of words, it changes nothing: they wait for T-a still.
    w.ledger
        .accept_failed_task(w.project, a, "chief", "")
        .expect_err("no words");
    assert_eq!(
        (w.state(b), blocked_by(&w, b)),
        ("open".to_owned(), vec![a])
    );
    assert_eq!(blocked_by(&w, c), [a]);
    assert_eq!(w.next(&session), None);

    w.ledger
        .accept_failed_task(w.project, a, "chief", CHECKED)
        .expect("accepted");
    assert_eq!(w.state(b), "queued", "the follow-up goes to its window");
    assert!(blocked_by(&w, b).is_empty());
    let head = w.next(&session).expect("the follow-up for the window");
    assert_eq!(w.message(head).task_number, Some(b));
    assert!(
        blocked_by(&w, c).is_empty(),
        "and the task for the tier is free to be given"
    );
    assert_eq!(w.state(c), "open", "it waits for a member, not for T-a");
}

#[test]
fn words_told_to_a_failed_tasks_window_that_it_never_had_go_with_the_accept() {
    // Told while the brief was on its way; the daemon gives up on the task, and the words
    // wait for its reopen. Accepted instead, they would be pasted into the window of a
    // task that is over.
    let mut w = world();
    let (task, session, brief) = queued(&mut w);
    w.begin(brief);
    let told = tell(&mut w, task, "Use the v2 grammar");
    w.confirm(brief);
    w.ledger
        .give_up_task(w.project, task, "the harness reported a failure")
        .expect("failed");
    assert_eq!(w.states(&[told.id]), ["queued"], "kept for the reopen");

    w.ledger
        .accept_failed_task(w.project, task, "chief", CHECKED)
        .expect("accepted");
    assert_eq!(w.states(&[told.id]), ["cancelled"]);
    assert_eq!(
        w.message(told.id).reason.as_deref(),
        Some(format!("T-{task} was accepted before it reached @{session}").as_str())
    );
    assert_eq!(w.next(&session), None);
}

#[test]
fn only_a_failed_task_is_accepted_on_the_words_of_a_check() {
    let mut w = world();
    let working = w.open("Parser").task.number;
    let moved = w.assign(working, "zeus");
    w.deliver(moved.message.expect("its brief").id);
    let refused = w
        .ledger
        .accept_failed_task(w.project, working, "chief", CHECKED)
        .expect_err("it has not failed");
    assert_eq!(refused.code(), Some("invalid-transition"));
    assert_eq!(
        refused.to_string(),
        format!("cannot accept T-{working}: it is working")
    );

    w.ledger
        .record_result(w.project, working, "Parser, done")
        .expect("a result");
    let refused = w
        .ledger
        .accept_failed_task(w.project, working, "chief", CHECKED)
        .expect_err("a done task is accepted as it was");
    assert_eq!(
        refused.to_string(),
        format!("cannot accept T-{working}: it is done")
    );
}

#[test]
fn a_failed_task_nobody_accepts_is_reopened_as_it_was() {
    let mut w = world();
    let (session, a) = failed(&mut w);
    let words = w
        .ledger
        .reopen_task(w.project, a, "chief", "Again")
        .expect("reopened")
        .message
        .expect("its words");
    assert_eq!(w.next(&session), Some(words.id));
    w.deliver(words.id);
    assert_eq!(w.state(a), "working");
}

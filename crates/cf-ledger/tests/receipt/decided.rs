//! A result is put to its requester to decide on. The decision (accepting the
//! task or sending it back) withdraws a result the requester was not given
//! yet, in its own transaction, so it is never pasted after the decision it
//! asks for: the chief's turn of 2026-10-08, which read T-9's result queued
//! behind it, accepted the task and put T-10 after it, was then given the
//! result with "Decide with: cf task accept T-9 …". A result being pasted or
//! given stays. A cancel is no decision on a result: a task that is done is
//! not called off.

use cf_ledger::{MessageView, NewNote};

use crate::fixture::{world, World};

/// A task opened for the standard workers and given to a session of `member`,
/// its brief received and its result recorded: the task, and its result,
/// which waits in the chief's queue.
fn finished(w: &mut World, member: &str, body: &str) -> (i64, MessageView) {
    let task = w.open(body).task.number;
    let moved = w.assign(task, member);
    w.deliver(moved.message.expect("its brief").id);
    let result = w
        .ledger
        .record_result(w.project, task, &format!("{body}, done"))
        .expect("a result")
        .message
        .expect("its result");
    assert_eq!(w.state(task), "done");
    (task, result)
}

fn accept(w: &mut World, task: i64) {
    w.ledger
        .accept_task(w.project, task, "chief")
        .expect("accepted");
}

fn send_back(w: &mut World, task: i64) {
    w.ledger
        .reopen_task(w.project, task, "chief", "Handle empty input too")
        .expect("sent back");
}

/// A decision on a task's result: what the chief does, how it is made, and
/// the reason it gives a result that it withdraws.
struct Decision {
    what: &'static str,
    make: fn(&mut World, i64),
    reason: &'static str,
}

/// The two decisions on a result.
const DECISIONS: [Decision; 2] = [
    Decision {
        what: "accepts",
        make: accept,
        reason: "T-1 was accepted",
    },
    Decision {
        what: "sends back",
        make: send_back,
        reason: "T-1 was sent back",
    },
];

/// What a message is now: its state and the reason it was given.
fn fate(w: &World, id: i64) -> (String, Option<String>) {
    let message = w.message(id);
    (message.state, message.reason)
}

/// ConsensFlow's own note to the chief about a task.
fn told_chief(w: &mut World, task: i64, body: &str) -> MessageView {
    w.ledger
        .note(
            w.project,
            &NewNote {
                from: None,
                to: "chief".into(),
                body: body.into(),
                task: Some(task),
            },
        )
        .expect("a note")
}

#[test]
fn the_chief_that_read_a_result_and_accepted_its_task_in_its_turn_is_not_given_the_result_after() {
    let mut w = world();
    let (task, result) = finished(&mut w, "zeus", "Parser");
    // The chief's own turn is running: the result waits at the head of its
    // queue, and `cf task get` shows it without receiving it.
    assert_eq!(w.next("chief"), Some(result.id));
    let thread = w
        .ledger
        .task(w.project, task)
        .expect("a read")
        .expect("T-1");
    assert!(
        thread
            .messages
            .iter()
            .any(|message| message.id == result.id && message.state == "queued"),
        "the thread it printed shows the result still queued"
    );
    assert_eq!(
        w.message(result.id).state,
        "queued",
        "a read receives nothing"
    );

    // It decides, and puts the next task after this one.
    accept(&mut w, task);
    let next = w
        .follow_up(task, "Lexer", &[])
        .expect("T-2 after T-1")
        .message
        .expect("its brief");

    assert_eq!(
        fate(&w, result.id),
        ("cancelled".to_owned(), Some("T-1 was accepted".to_owned()))
    );
    assert_eq!(
        w.next("chief"),
        None,
        "its turn ends, and nothing is pasted"
    );
    assert_eq!(w.state(task), "accepted");
    assert_eq!(w.message(next.id).state, "queued", "the next task goes on");
}

#[test]
fn the_chief_that_read_a_result_and_sent_its_task_back_in_its_turn_is_not_given_the_result_after() {
    let mut w = world();
    let (task, result) = finished(&mut w, "zeus", "Parser");
    assert_eq!(w.next("chief"), Some(result.id));

    let moved = w
        .ledger
        .reopen_task(w.project, task, "chief", "Handle empty input too")
        .expect("sent back");
    let follow_up = moved.message.expect("its follow-up");

    assert_eq!(
        fate(&w, result.id),
        ("cancelled".to_owned(), Some("T-1 was sent back".to_owned()))
    );
    assert_eq!(
        w.next("chief"),
        None,
        "its turn ends, and nothing is pasted"
    );
    assert_eq!(w.state(task), "queued");
    assert_eq!(
        w.message(follow_up.id).state,
        "queued",
        "the work sent back goes to its window"
    );
}

#[test]
fn a_decision_withdraws_the_result_of_its_task_and_nothing_else_the_chief_is_waiting_for() {
    for decision in DECISIONS {
        let mut w = world();
        let (task, result) = finished(&mut w, "zeus", "Parser");
        let (other, other_result) = finished(&mut w, "diana", "Lexer");
        let note = told_chief(&mut w, task, "T-1 waits with @zeus: out of quota.");
        let elsewhere = told_chief(&mut w, other, "T-2 waits with @diana: out of quota.");

        (decision.make)(&mut w, task);

        assert_eq!(
            fate(&w, result.id),
            ("cancelled".to_owned(), Some(decision.reason.to_owned()))
        );
        for (kept, why) in [
            (other_result.id, "the result of another task"),
            (note.id, "a note about the same task"),
            (elsewhere.id, "a note about another task"),
        ] {
            assert_eq!(w.message(kept).state, "queued", "{why}");
        }
        assert_eq!(
            w.next("chief"),
            Some(other_result.id),
            "the chief is still given what it waits for, oldest first"
        );
        assert_eq!(w.state(other), "done", "the other task is as it was");
    }
}

#[test]
fn a_result_being_pasted_when_the_decision_comes_stays_and_arrives() {
    for decision in DECISIONS {
        let mut w = world();
        let (task, result) = finished(&mut w, "zeus", "Parser");
        w.begin(result.id);

        (decision.make)(&mut w, task);

        assert_eq!(
            fate(&w, result.id),
            ("delivering".to_owned(), None),
            "the chief {} T-1 while its result is being pasted: it is in the window",
            decision.what
        );
        w.confirm(result.id);
        assert_eq!(
            w.message(result.id).state,
            "delivered",
            "and the paste is proved, as ever"
        );
    }
}

#[test]
fn a_result_the_chief_was_given_already_stays_when_it_decides() {
    for decision in DECISIONS {
        let mut w = world();
        let (task, result) = finished(&mut w, "zeus", "Parser");
        w.deliver(result.id);
        let given = w.message(result.id);

        (decision.make)(&mut w, task);

        let after = w.message(result.id);
        assert_eq!(
            (after.state.as_str(), after.reason.as_deref()),
            ("delivered", None),
            "the chief {} T-1 after it was given the result",
            decision.what
        );
        assert_eq!(after.receipt, given.receipt, "with its receipt");
    }
}

#[test]
fn a_refused_decision_withdraws_nothing() {
    let mut w = world();
    let (task, result) = finished(&mut w, "zeus", "Parser");
    w.ledger
        .remove_member(w.project, "zeus")
        .expect("the member leaves the staff");

    // The session that did the work cannot go on: it is not sent back to it.
    let refused = w
        .ledger
        .reopen_task(w.project, task, "chief", "Again")
        .expect_err("the session has ended");
    assert_eq!(refused.code(), Some("session-ended"));
    assert_eq!(
        w.message(result.id).state,
        "queued",
        "so the chief is still to be given it"
    );
    assert_eq!(w.state(task), "done");
}

#[test]
fn a_done_task_is_not_called_off_and_its_result_waits_for_the_decision() {
    let mut w = world();
    let (task, result) = finished(&mut w, "zeus", "Parser");

    let refused = w
        .ledger
        .cancel_task(w.project, task, "chief")
        .expect_err("a task that is done is accepted or sent back");
    assert_eq!(refused.code(), Some("invalid-transition"));
    assert_eq!(w.message(result.id).state, "queued");
    assert_eq!(w.next("chief"), Some(result.id));
}

#[test]
fn a_cancel_withdraws_a_result_that_an_older_build_left_queued_for_a_task_sent_back() {
    let mut w = world();
    let (task, result) = finished(&mut w, "zeus", "Parser");
    send_back(&mut w, task);
    // An older build never withdrew it: the task went back to its window with
    // its result still waiting in the chief's queue.
    w.edit(|db| {
        db.execute(
            "UPDATE message SET state = 'queued', reason = NULL WHERE id = ?",
            [result.id],
        )
        .expect("the result is queued again");
    });
    assert_eq!(w.next("chief"), Some(result.id));

    w.ledger
        .cancel_task(w.project, task, "chief")
        .expect("called off");

    assert_eq!(
        fate(&w, result.id),
        (
            "cancelled".to_owned(),
            Some("cancelled by @chief".to_owned())
        )
    );
    assert_eq!(w.next("chief"), None);
}

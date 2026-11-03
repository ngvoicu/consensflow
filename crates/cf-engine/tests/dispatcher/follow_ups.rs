//! A follow-up that waits on the board for what it needs is its session's
//! already, and the session's window does not wait for it: the window closes
//! with the task it had, as any window that has none in hand, and is opened
//! again on the session's conversation when the follow-up goes. The test is
//! not held to a Node recording: Node's ledger did not count such a follow-up
//! as the session's work at all.

use cf_engine::testing::Context;
use cf_ledger::NewTask;

use crate::fixtures::{assert_match, closed, given, last_launch, last_message, native_of, Tiers};

#[test]
fn a_follow_up_waiting_for_what_it_needs_keeps_no_window_open_and_goes_to_one_opened_again_on_the_conversation(
) {
    let context = Context::new();
    // The host proves each window's program gone, as the app's does: the follow-up,
    // another task, goes on with the session's conversation (`ownership`).
    context.host.exits_showing(Some(0), None, None);
    let tiers = Tiers::new(&context, &["zeus"]);
    let project = tiers.project.id;
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working");
    let session = tiers.assignee(1);
    let window = context.host.last(&session).unwrap().pane;
    let native = native_of(&context, tiers.id(&session));

    // The result is in before the window's look, and a follow-up for the same
    // session waits on the board for T-1 to be accepted.
    context
        .ledger
        .borrow_mut()
        .record_result(project, 1, "Parser done")
        .unwrap();
    context.create_task(
        project,
        NewTask {
            from: "chief".to_owned(),
            after: Some(1),
            needs: vec![1],
            body: "Now the lexer".to_owned(),
            ..NewTask::default()
        },
    );
    assert_eq!(tiers.task(2).task.state, "open");
    context.pass().unwrap();
    assert!(
        closed(&context, &window),
        "nothing of the follow-up has reached the window: it has no task in hand"
    );
    assert_eq!(tiers.task(2).task.state, "open", "and the follow-up waits");

    // T-1 is accepted: the follow-up goes to a window opened on the same conversation.
    context
        .ledger
        .borrow_mut()
        .accept_task(project, 1, "chief")
        .unwrap();
    context.pass().unwrap();
    assert_eq!(last_launch(&context), (session, native));
    assert_match(
        &last_message(&context),
        r"^\[ConsensFlow m-\d+ · T-2 · task from @chief\]\nNow the lexer$",
    );
    context.pass().unwrap();
    assert_eq!(tiers.task(2).task.state, "working");
}

/// T-1118, from Gabriel's other board: T-8, a follow-up of T-6 whose result was in and
/// not yet accepted, went into the window at once, before the chief had decided T-6.
/// It waits for T-6 to be accepted, as one of a task still at work does, and the
/// window takes it then.
#[test]
fn a_follow_up_of_a_task_with_its_result_in_waits_for_its_acceptance_and_goes_to_its_window_then() {
    let context = Context::new();
    context.host.exits_showing(Some(0), None, None);
    let tiers = Tiers::new(&context, &["zeus"]);
    let project = tiers.project.id;
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    let session = tiers.assignee(1);
    let native = native_of(&context, tiers.id(&session));
    context.adapter.answer("zeus", "Parser done");
    context.pass().unwrap();
    assert_eq!(
        tiers.task(1).task.state,
        "done",
        "its result is in: the chief's call"
    );
    let launches = context.adapter.prepared().len();

    context.create_task(
        project,
        NewTask {
            from: "chief".to_owned(),
            after: Some(1),
            body: "Now the lexer".to_owned(),
            ..NewTask::default()
        },
    );
    let waiting = tiers.task(2).task;
    assert_eq!(
        (waiting.state.as_str(), waiting.blocked_by),
        ("open", vec![1]),
        "it waits for T-1, though T-1's result is in"
    );
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(
        context.adapter.prepared().len(),
        launches,
        "no window was opened for it before the chief decided T-1"
    );
    assert_eq!(tiers.task(2).task.state, "open");

    context
        .ledger
        .borrow_mut()
        .accept_task(project, 1, "chief")
        .unwrap();
    context.pass().unwrap();
    assert_eq!(last_launch(&context), (session, native));
    assert_match(
        &last_message(&context),
        r"^\[ConsensFlow m-\d+ · T-2 · task from @chief\]\nNow the lexer$",
    );
    context.pass().unwrap();
    assert_eq!(tiers.task(2).task.state, "working");
}

/// A window takes nothing new while a task of it has its result in and waits for the
/// chief's decision, which could send it back: a follow-up of an older, accepted task
/// waits for that result too (T-1118).
#[test]
fn a_follow_up_of_an_older_accepted_task_waits_while_the_window_holds_a_result_the_chief_has_not_decided(
) {
    let context = Context::new();
    context.host.exits_showing(Some(0), None, None);
    let tiers = Tiers::new(&context, &["zeus"]);
    let project = tiers.project.id;
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    let session = tiers.assignee(1);
    let native = native_of(&context, tiers.id(&session));
    context.adapter.answer("zeus", "Parser done");
    context.pass().unwrap();
    context
        .ledger
        .borrow_mut()
        .accept_task(project, 1, "chief")
        .unwrap();
    // T-2 follows the accepted T-1 and has its result in.
    context.create_task(
        project,
        NewTask {
            from: "chief".to_owned(),
            after: Some(1),
            body: "Now the lexer".to_owned(),
            ..NewTask::default()
        },
    );
    for _ in 0..3 {
        context.pass().unwrap();
    }
    context.adapter.answer("zeus-amber-pine", "Lexer done");
    for _ in 0..2 {
        context.pass().unwrap();
    }
    assert_eq!(
        tiers.task(2).task.state,
        "done",
        "its result is in, undecided"
    );
    let launches = context.adapter.prepared().len();

    // T-3 follows T-1 too: the window holds T-2's result, so it waits for T-2.
    context.create_task(
        project,
        NewTask {
            from: "chief".to_owned(),
            after: Some(1),
            body: "And the docs".to_owned(),
            ..NewTask::default()
        },
    );
    let waiting = tiers.task(3).task;
    assert_eq!(
        (waiting.state.as_str(), waiting.blocked_by),
        ("open", vec![2])
    );
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(
        context.adapter.prepared().len(),
        launches,
        "no window was opened for it before the chief decided T-2"
    );

    context
        .ledger
        .borrow_mut()
        .accept_task(project, 2, "chief")
        .unwrap();
    context.pass().unwrap();
    assert_eq!(last_launch(&context), (session, native));
    assert_match(
        &last_message(&context),
        r"^\[ConsensFlow m-\d+ · T-3 · task from @chief\]\nAnd the docs$",
    );
}

/// A follow-up that waited for what it needs while its session was at work on
/// another, given since, is released when that one is called off, in the same
/// step: the window is still at work on the task it was given, which the pass
/// stops, and the follow-up goes to it once it is at rest. Two follow-ups queued
/// behind the window's task go in order: the first is given the window when the
/// task is accepted, and the second is ready, waiting for it.
#[test]
fn a_follow_up_released_by_the_cancel_of_its_sessions_task_goes_once_the_window_is_stopped() {
    let context = Context::new();
    // The host proves each window's program gone, as the app's does: the session's next task
    // goes on with its conversation (`ownership`), as this test is about something else.
    context.host.exits_showing(Some(0), None, None);
    let tiers = Tiers::new(&context, &["zeus"]);
    let project = tiers.project.id;
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    let session = tiers.assignee(1);

    // T-2 and T-3 wait on the board for T-1 to be accepted, in that order.
    for body in ["First the tests", "Now the lexer"] {
        context.create_task(
            project,
            NewTask {
                from: "chief".to_owned(),
                after: Some(1),
                body: body.to_owned(),
                ..NewTask::default()
            },
        );
    }
    context.adapter.answer("zeus", "Parser done");
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "done");
    context
        .ledger
        .borrow_mut()
        .accept_task(project, 1, "chief")
        .unwrap();
    for _ in 0..4 {
        context.pass().unwrap();
    }
    assert_eq!(tiers.task(2).task.state, "working");
    assert_eq!(tiers.assignee(2), session);
    assert_eq!(
        tiers.task(3).task.state,
        "open",
        "ready, and waiting for the window"
    );

    // T-2 is called off, and T-3 is queued for the window at once.
    context
        .ledger
        .borrow_mut()
        .cancel_task(project, 2, "human")
        .unwrap();
    assert_eq!(tiers.task(3).task.state, "queued");
    let presses = context.host.inputs().len();
    context.pass().unwrap();
    assert_eq!(
        context.host.inputs().len(),
        presses + 1,
        "the window is stopped: it is at work on T-2"
    );

    // The agent stops, and T-3 goes to its window, the same one.
    context.adapter.answer("zeus", "Stopped.");
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(tiers.task(3).task.state, "working");
    assert_eq!(tiers.task(2).task.state, "cancelled");
    let words = given(&context, "zeus");
    assert_match(
        words.last().unwrap(),
        r"^\[ConsensFlow m-\d+ · T-3 · task from @chief\]\nNow the lexer$",
    );
}

/// A session that a follow-up waits for on the board is not deleted by the human: the
/// follow-up's release would queue it for a session no window opens for again. Once the
/// follow-up is cancelled, it is.
#[test]
fn a_session_a_follow_up_waits_for_is_not_deleted_until_the_follow_up_is_cancelled() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    let project = tiers.project.id;
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    let session = tiers.assignee(1);
    context.adapter.answer("zeus", "Parser done");
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "done");
    context.create_task(
        project,
        NewTask {
            from: "chief".to_owned(),
            after: Some(1),
            needs: vec![1],
            body: "Now the lexer".to_owned(),
            ..NewTask::default()
        },
    );
    assert_eq!(tiers.task(2).task.state, "open");

    let refused = context.end_session(project, &session).unwrap_err();
    assert_eq!(refused.refusal().code, "session-busy");
    assert!(tiers.participant(&session).is_some(), "still on the board");

    context
        .ledger
        .borrow_mut()
        .cancel_task(project, 2, "chief")
        .unwrap();
    context.end_session(project, &session).unwrap();
    assert!(tiers.participant(&session).is_none());
}

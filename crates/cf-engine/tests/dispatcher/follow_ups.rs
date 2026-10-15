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

/// A follow-up that waited for what it needs while its session was at work on
/// another, given since, is released when that one is called off, in the same
/// step: the window is still at work on the task it was given, which the pass
/// stops, and the follow-up goes to it once it is at rest.
#[test]
fn a_follow_up_released_by_the_cancel_of_its_sessions_task_goes_once_the_window_is_stopped() {
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

    // T-2 waits on the board for T-1 to be accepted; T-3 goes to the window.
    for (needs, body) in [(vec![1], "Now the lexer"), (Vec::new(), "First the tests")] {
        context.create_task(
            project,
            NewTask {
                from: "chief".to_owned(),
                after: Some(1),
                needs,
                body: body.to_owned(),
                ..NewTask::default()
            },
        );
    }
    for _ in 0..4 {
        context.pass().unwrap();
    }
    assert_eq!(tiers.task(3).task.state, "working");
    assert_eq!(tiers.assignee(3), session);
    context
        .ledger
        .borrow_mut()
        .accept_task(project, 1, "chief")
        .unwrap();
    assert_eq!(
        tiers.task(2).task.state,
        "open",
        "ready, and waiting for the window"
    );

    // T-3 is called off, and T-2 is queued for the window at once.
    context
        .ledger
        .borrow_mut()
        .cancel_task(project, 3, "human")
        .unwrap();
    assert_eq!(tiers.task(2).task.state, "queued");
    let presses = context.host.inputs().len();
    context.pass().unwrap();
    assert_eq!(
        context.host.inputs().len(),
        presses + 1,
        "the window is stopped: it is at work on T-3"
    );

    // The agent stops, and T-2 goes to its window, the same one.
    context.adapter.answer("zeus", "Stopped.");
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(tiers.task(2).task.state, "working");
    assert_eq!(tiers.task(3).task.state, "cancelled");
    let words = given(&context, "zeus");
    assert_match(
        words.last().unwrap(),
        r"^\[ConsensFlow m-\d+ · T-2 · task from @chief\]\nNow the lexer$",
    );
}

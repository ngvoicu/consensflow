//! One task per member session, the human's hand on a session's window: a
//! window they open stays until they hide it, and then closes as any that
//! holds no task and whose agent is not at work on a turn.

use cf_engine::testing::Context;
use cf_engine::ActivityState;
use cf_harness::contract::Waiting;
use cf_harness::records::Role;

use crate::fixtures::{closed, finished, Tiers};
use crate::traces::held_to;

const SUITES: &[&str] = &["one task per member session"];

/// The window of a session `handle`, as the pane host last opened it.
fn window(context: &Context, handle: &str) -> cf_harness::contract::Pane {
    context.host.last(handle).unwrap().pane
}

/// The human asks the agent something in its window, and its turn is at work.
fn asks_zeus(context: &Context) {
    let asked = context
        .adapter
        .item(Role::User, "What did you change so far?");
    context.adapter.with("zeus", |agent| {
        agent.items.push(asked);
        agent.settled = false;
    });
}

#[test]
fn keeps_a_session_after_its_work_until_the_human_ends_it_and_keeps_a_window_the_human_opened_through_the_work_that_comes(
) {
    let context = Context::new();
    let tiers = finished(&context);
    let project = tiers.project.id;
    context.pass().unwrap();
    assert!(
        tiers.participant("zeus-amber-pine").is_some(),
        "nothing expires"
    );
    assert_eq!(
        tiers.task(1).task.state,
        "done",
        "its work stays for the chief to accept"
    );
    let native = context
        .ledger
        .borrow()
        .current_conversation(tiers.id("zeus-amber-pine"))
        .unwrap()
        .unwrap()
        .native_session;

    // The human opens the window again: on its conversation, with nothing to deliver, and it stays.
    context.open_window(project, "zeus-amber-pine").unwrap();
    let reopened = context.adapter.prepared().last().cloned().unwrap();
    assert_eq!(
        (
            reopened["participant"]["handle"].as_str(),
            reopened["resume"].as_str(),
            reopened["message"].is_null()
        ),
        ("zeus-amber-pine".into(), native.as_deref(), true)
    );
    let killed = context.host.killed().len();
    let kept = window(&context, "zeus-amber-pine");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(
        context.host.killed().len(),
        killed,
        "a window the human opened is not retired"
    );

    // Ending the session closes its window and folds it away.
    tiers.open_body("Docs");
    context.pass().unwrap();
    context.pass().unwrap();
    let working = tiers.task(2).task;
    assert_eq!(working.state, "working");
    let handle = working.assignee.unwrap();
    let busy = context.end_session(project, &handle).unwrap_err();
    assert_eq!(busy.refusal().code, "session-busy");
    context
        .ledger
        .borrow_mut()
        .cancel_task(project, 2, "chief")
        .unwrap();
    context.pass().unwrap();
    assert_eq!(
        context.host.killed().last().unwrap().id,
        format!("p{project}-{handle}"),
        "the window closes with its work, the session stays"
    );
    context.end_session(project, &handle).unwrap();
    assert!(tiers.participant(&handle).is_none());
    // The window the human opened stayed through that work: ending its
    // session is what closes it.
    assert!(!closed(&context, &kept));
    context.end_session(project, "zeus-amber-pine").unwrap();
    assert!(closed(&context, &kept));
    held_to(
        context.close(),
        SUITES,
        "keeps a session after its work until the human ends it, and keeps a window the human opened through the work that comes",
    );
}

#[test]
fn closes_a_window_the_human_opened_once_they_hide_it_and_show_opens_it_again_on_the_same_conversation(
) {
    let context = Context::new();
    let tiers = finished(&context);
    let project = tiers.project.id;
    let session = tiers.id("zeus-amber-pine");
    let native = context
        .ledger
        .borrow()
        .current_conversation(session)
        .unwrap()
        .unwrap()
        .native_session;
    context.open_window(project, "zeus-amber-pine").unwrap();
    let shown = window(&context, "zeus-amber-pine");
    for _ in 0..2 {
        context.pass().unwrap();
    }
    assert!(
        !closed(&context, &shown),
        "shown, it stays open with nothing to do"
    );

    context.hide_window(project, "zeus-amber-pine").unwrap();
    context.pass().unwrap();
    assert!(
        closed(&context, &shown),
        "hidden, it closes as any window that is free"
    );
    assert_eq!(context.dispatcher.pane(session), None);
    assert!(
        !context.dispatcher.hidden(session),
        "closed, it is no longer hidden"
    );
    assert_eq!(
        context
            .ledger
            .borrow()
            .current_conversation(session)
            .unwrap()
            .unwrap()
            .native_session,
        native
    );

    context.open_window(project, "zeus-amber-pine").unwrap();
    let again = context.adapter.prepared().last().cloned().unwrap();
    assert_eq!(
        (
            again["participant"]["handle"].as_str(),
            again["resume"].as_str(),
            again["message"].is_null()
        ),
        ("zeus-amber-pine".into(), native.as_deref(), true),
        "Show opens it on the same conversation, with nothing to deliver"
    );
    held_to(
        context.close(),
        SUITES,
        "closes a window the human opened once they hide it, and Show opens it again on the same conversation",
    );
}

#[test]
fn keeps_a_window_the_human_hid_while_its_agent_is_at_work_and_closes_it_once_the_turn_ends() {
    let context = Context::new();
    let tiers = finished(&context);
    let project = tiers.project.id;
    context.open_window(project, "zeus-amber-pine").unwrap();
    let shown = window(&context, "zeus-amber-pine");
    // The human asks the agent something in the window, and hides it while it works.
    asks_zeus(&context);
    context.pass().unwrap();
    context.hide_window(project, "zeus-amber-pine").unwrap();
    for _ in 0..3 {
        context.pass().unwrap();
    }
    let session = tiers.id("zeus-amber-pine");
    assert_eq!(
        context.dispatcher.activity(session).state,
        ActivityState::Working
    );
    assert!(
        !closed(&context, &shown),
        "not while its agent is at work on the turn"
    );
    // A turn waiting on the human for a permission is a turn that has not ended either.
    context.adapter.with("zeus", |agent| {
        agent.waiting = Some(Waiting {
            reason: Some("permission to run a command".to_owned()),
        });
    });
    context.pass().unwrap();
    assert_eq!(
        context.dispatcher.activity(session).state,
        ActivityState::Waiting
    );
    assert!(
        !closed(&context, &shown),
        "nor while it waits on a prompt in the turn"
    );
    context.adapter.with("zeus", |agent| agent.waiting = None);
    context.adapter.answer("zeus", "Only the parser.");
    context.pass().unwrap();
    assert!(closed(&context, &shown), "the turn ended: now it closes");
    held_to(
        context.close(),
        SUITES,
        "keeps a window the human hid while its agent is at work, and closes it once the turn ends",
    );
}

#[test]
fn makes_a_window_the_human_hid_theirs_again_when_they_show_it_while_it_still_works() {
    let context = Context::new();
    let tiers = finished(&context);
    let project = tiers.project.id;
    context.open_window(project, "zeus-amber-pine").unwrap();
    let shown = window(&context, "zeus-amber-pine");
    asks_zeus(&context);
    context.pass().unwrap();
    context.hide_window(project, "zeus-amber-pine").unwrap();
    let session = tiers.id("zeus-amber-pine");
    assert!(
        context.dispatcher.hidden(session),
        "the board says it is hidden"
    );
    let launches = context.adapter.prepared().len();
    context.open_window(project, "zeus-amber-pine").unwrap();
    assert_eq!(
        context.adapter.prepared().len(),
        launches,
        "its window is open: no new one"
    );
    assert!(!context.dispatcher.hidden(session));
    context.adapter.answer("zeus", "Only the parser.");
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert!(
        !closed(&context, &shown),
        "shown again, it stays open after its turn"
    );
    held_to(
        context.close(),
        SUITES,
        "makes a window the human hid theirs again when they show it while it still works",
    );
}

#[test]
fn keeps_a_window_the_human_hid_while_it_holds_a_task_and_closes_it_once_the_task_ends() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    let session = tiers.assignee(1);
    context.open_window(tiers.project.id, &session).unwrap();
    context.hide_window(tiers.project.id, &session).unwrap();
    let pane = window(&context, "zeus");
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(tiers.task(1).task.state, "working");
    assert!(!closed(&context, &pane), "it holds its task");
    context.adapter.answer("zeus", "Parser done");
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "done");
    assert!(
        closed(&context, &pane),
        "its task ended and its turn with it"
    );
    held_to(
        context.close(),
        SUITES,
        "keeps a window the human hid while it holds a task, and closes it once the task ends",
    );
}

#[test]
fn closes_a_window_the_human_never_opened_as_its_task_ends_though_its_agent_is_at_work_and_one_they_hid_only_after_its_turn(
) {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    let project = tiers.project.id;
    tiers.open();
    tiers.open_body("Write the docs");
    context.pass().unwrap();
    context.pass().unwrap();
    let (opened, never) = (tiers.assignee(1), tiers.assignee(2));
    assert_eq!(
        [tiers.task(1).task.state, tiers.task(2).task.state],
        ["working", "working"]
    );
    context.open_window(project, &opened).unwrap();
    // Hide tells the daemon about any terminal; it frees only what the human opened.
    for handle in [&opened, &never] {
        context.hide_window(project, handle).unwrap();
    }
    // Both tasks end while their agents are still at work on their turns.
    for number in [1, 2] {
        context
            .ledger
            .borrow_mut()
            .record_result(project, number, "Done")
            .unwrap();
    }
    context.pass().unwrap();
    assert!(
        closed(&context, &window(&context, &never)),
        "nobody opened it"
    );
    assert!(
        !closed(&context, &window(&context, &opened)),
        "the human did: its turn first"
    );
    context.adapter.answer(&opened, "Done.");
    context.pass().unwrap();
    assert!(closed(&context, &window(&context, &opened)));
    held_to(
        context.close(),
        SUITES,
        "closes a window the human never opened as its task ends, though its agent is at work, and one they hid only after its turn",
    );
}

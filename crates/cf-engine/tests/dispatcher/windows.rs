//! A worker's window through its life: closed mid-task and resumed on its
//! conversation, paused in place, never opened, or never showing its first
//! message (`describe('the dispatcher')`).

use std::rc::Rc;

use cf_engine::testing::Context;
use serde_json::{json, Value};

use crate::matching::found;
use crate::traces::held_to;

const SUITES: &[&str] = &["the dispatcher"];

/// The native session a participant's conversation runs in.
fn native(context: &Context, project: i64, handle: &str) -> String {
    let id = context.id(project, handle);
    let conversation = context.ledger.borrow().current_conversation(id);
    conversation.unwrap().unwrap().native_session.unwrap()
}

/// What the last launch prepared was for and resumed.
fn resumed(context: &Context) -> (Value, Value) {
    let launch = context.adapter.prepared().pop().unwrap();
    (
        launch["participant"]["handle"].clone(),
        launch["resume"].clone(),
    )
}

#[test]
fn pauses_the_task_and_tells_the_requester_when_the_worker_window_closes_mid_task_and_resumes_it_on_its_conversation(
) {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    let session = native(&context, project.id, "zeus");
    context.exit("zeus");
    let task = context.task(project.id, 1);
    assert_eq!(
        (task.task.state.as_str(), task.task.assignee.as_deref()),
        ("paused", Some("zeus"))
    );
    let note = task.messages.iter().find(|m| m.kind == "note").unwrap();
    assert_eq!(note.recipient, "chief");
    assert!(found(
        r#"^T-1 is paused: @zeus's window closed\. Resume it with: cf task resume T-1 "…""#,
        &note.body
    ));
    context.pass().unwrap();
    assert_eq!(
        context.task(project.id, 1).task.state,
        "paused",
        "nothing happens on its own"
    );
    context.resume_task(project.id, 1, "Carry on");
    context.pass().unwrap();
    assert_eq!(resumed(&context), (json!("zeus"), json!(session)));
    let launch = context.adapter.prepared().pop().unwrap();
    assert!(found(
        r"T-1 · task from @chief\]\nResumed: Carry on$",
        launch["message"].as_str().unwrap()
    ));
    context.pass().unwrap();
    assert_eq!(context.task(project.id, 1).task.state, "working");
    held_to(
        context.close(),
        SUITES,
        "pauses the task and tells the requester when the worker window closes mid-task, and resumes it on its conversation",
    );
}

#[test]
fn brings_a_tell_to_a_task_whose_window_is_gone_on_that_windows_own_conversation_still_paused() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    let session = native(&context, project.id, "zeus");
    context.exit("zeus");
    assert_eq!(context.task(project.id, 1).task.state, "paused");
    let told = context.tell(project.id, "zeus", 1, "Which grammar did you start from?");
    context.pass().unwrap();
    assert_eq!(
        resumed(&context),
        (json!("zeus"), json!(session)),
        "the same conversation"
    );
    let launch = context.adapter.prepared().pop().unwrap();
    assert!(found(
        r"^\[ConsensFlow m-\d+ · T-1 · question from @chief\]\nWhich grammar did you start from\?\n\nT-1 is paused for this\.",
        launch["message"].as_str().unwrap()
    ));
    context.pass().unwrap();
    assert_eq!(
        context.task(project.id, 1).task.state,
        "paused",
        "the task waits for the chief"
    );
    assert_ne!(context.message(told.id).state, "queued");
    held_to(
        context.close(),
        SUITES,
        "brings a tell to a task whose window is gone on that window's own conversation, still paused",
    );
}

#[test]
fn pauses_a_working_task_in_its_open_window() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    let launches = context.adapter.prepared().len();
    context.pause_task(project.id, 1);
    context.pass().unwrap();
    context.pass().unwrap();
    let pane = context.host.last("zeus").unwrap().pane;
    assert_eq!(
        context.host.inputs(),
        [json!({ "id": pane.id, "generation": pane.generation, "bytes": [27] })],
        "Escape, once"
    );
    assert!(context.host.killed().is_empty(), "the window stays");
    // Still working three seconds later (a harness that ignored the key while it thought): again, up to three times.
    context.adapter.busy("zeus");
    context.advance(3_100);
    context.pass().unwrap();
    assert_eq!(
        context.host.inputs().len(),
        2,
        "pressed again while the window still works"
    );
    context.advance(3_100);
    context.pass().unwrap();
    context.advance(3_100);
    context.pass().unwrap();
    context.advance(3_100);
    context.pass().unwrap();
    assert_eq!(context.host.inputs().len(), 3, "and then no more");
    context.adapter.answer("zeus", "Parser done");
    context.pass().unwrap();
    assert_eq!(
        context.task(project.id, 1).task.state,
        "paused",
        "said after the pause: not a result"
    );
    context.resume_task(project.id, 1, "Add the tests too");
    context.pass().unwrap();
    assert_eq!(context.adapter.prepared().len(), launches, "no new window");
    let last = context.adapter.agent("zeus").items.pop().unwrap();
    assert!(found(
        r"T-1 · task from @chief\]\nResumed: Add the tests too$",
        &last.text
    ));
    context.pass().unwrap();
    assert_eq!(context.task(project.id, 1).task.state, "working");
    context.adapter.answer("zeus", "Parser and tests done");
    context.pass().unwrap();
    assert_eq!(context.task(project.id, 1).task.state, "done");
    held_to(
        context.close(),
        SUITES,
        "pauses a working task in its open window: the agent is interrupted once, its output not collected, and the chief's words resume it there",
    );
}

#[test]
fn fails_the_task_when_the_worker_window_cannot_open() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.host.refuse.set(true);
    context.pass().unwrap();
    let task = context.task(project.id, 1);
    assert_eq!(task.task.state, "failed");
    assert!(found(
        "refused by the test",
        task.messages[0].reason.as_deref().unwrap()
    ));
    held_to(
        context.close(),
        SUITES,
        "fails the task when the worker window cannot open",
    );
}

#[test]
fn forgets_the_files_a_launch_wrote_when_its_window_never_opens() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.host.refuse.set(true);
    context.pass().unwrap();
    assert_eq!(context.task(project.id, 1).task.state, "failed");
    let launched = context.adapter.prepared().pop().unwrap();
    let forgotten = || context.launch_files.forgotten.borrow().clone();
    assert!(
        forgotten().contains(&launched["launchId"].as_str().unwrap().to_owned()),
        "the host refused the window"
    );

    *context.adapter.fail_prepare.borrow_mut() =
        Some("the settings could not be written".to_owned());
    context.give(project.id, "zeus", "Lexer");
    context.pass().unwrap();
    assert_eq!(context.task(project.id, 2).task.state, "failed");
    let (_, asked) = context
        .recorder
        .calls("adapter:claude-code", &["prepare"])
        .pop()
        .unwrap();
    let written = asked[0]["launchId"].as_str().unwrap().to_owned();
    assert!(
        forgotten().contains(&written),
        "the adapter failed after writing them"
    );
    held_to(
        context.close(),
        SUITES,
        "forgets the files a launch wrote when its window never opens: refused by the host, or its adapter failed",
    );
}

#[test]
fn fails_a_launch_whose_first_message_never_arrives() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    // The window opens, but its record never shows the brief.
    let fake = Rc::downgrade(&context.adapter);
    *context.adapter.after_prepare.borrow_mut() = Some(Rc::new(move |handle| {
        if let Some(fake) = fake.upgrade() {
            fake.with(handle, |agent| agent.items.clear());
        }
    }));
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.advance(121_000);
    context.pass().unwrap();
    assert_eq!(context.task(project.id, 1).task.state, "failed");
    assert_eq!(
        context.host.killed().last().unwrap().id,
        context.host.last("zeus").unwrap().pane.id
    );
    held_to(
        context.close(),
        SUITES,
        "fails a launch whose first message never arrives",
    );
}

#[test]
fn fails_the_first_message_at_once_when_the_harness_cannot_take_it_after_the_window_opens() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    *context.adapter.fail_started.borrow_mut() = Some("the server never answered".to_owned());
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    let task = context.task(project.id, 1);
    assert_eq!(task.task.state, "failed");
    assert!(found(
        "the server never answered",
        task.messages[0].reason.as_deref().unwrap()
    ));
    assert_eq!(
        context.host.killed().last().unwrap().id,
        context.host.last("zeus").unwrap().pane.id
    );
    held_to(
        context.close(),
        SUITES,
        "fails the first message at once when the harness cannot take it after the window opens",
    );
}

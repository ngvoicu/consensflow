//! A worker's window through its life: closed mid-task and resumed on its
//! conversation, paused in place, never opened, or never showing its first
//! message (`describe('the dispatcher')`).

use std::cell::RefCell;
use std::rc::Rc;

use cf_engine::testing::Context;
use cf_engine::Unstopped;
use serde_json::{json, Value};

use crate::fixtures::assert_match;
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
    assert_match(
        &note.body,
        r#"^T-1 is paused: @zeus's window closed\. Resume it with: cf task resume T-1 "…""#,
    );
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
    assert_match(
        launch["message"].as_str().unwrap(),
        r"T-1 · task from @chief\]\nResumed: Carry on$",
    );
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
    assert_match(
        launch["message"].as_str().unwrap(),
        r"^\[ConsensFlow m-\d+ · T-1 · question from @chief\]\nWhich grammar did you start from\?\n\nT-1 is paused for this\.",
    );
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
    // The three rounds were ignored: the stop is given up on, said once to the
    // human and to the chief who gave the task, and the board shows it.
    let zeus = context.id(project.id, "zeus");
    assert_eq!(
        context.dispatcher.unstopped(zeus),
        Some(Unstopped { task: 1, rounds: 3 })
    );
    let said = |who: &str| -> Vec<String> {
        context
            .inbox(context.id(project.id, who))
            .into_iter()
            .filter(|message| message.kind == "note" && message.body.contains("did not stop"))
            .map(|message| message.body)
            .collect()
    };
    assert_eq!(
        said("human"),
        ["@zeus did not stop for T-1: it ignored the interrupt 3 times and is still on its earlier turn. What is for it waits until that turn ends, and what it writes before then is not T-1's result. To stop it now, cancel T-1, or reassign it if it was given by tier."]
    );
    assert_eq!(
        said("chief"),
        ["T-1's window (@zeus) did not stop: it ignored the interrupt and is still on its earlier turn. Your words wait until that turn ends; what it writes before then is not taken as T-1's result (cf task get T-1 --transcript shows it). To stop it now: cf task cancel T-1."]
    );
    context.adapter.answer("zeus", "Parser done");
    context.pass().unwrap();
    assert_eq!(
        context.dispatcher.unstopped(zeus),
        None,
        "paid once the window is at rest"
    );
    assert_eq!(
        context.task(project.id, 1).task.state,
        "paused",
        "said after the pause: not a result"
    );
    context.resume_task(project.id, 1, "Add the tests too");
    context.pass().unwrap();
    assert_eq!(context.adapter.prepared().len(), launches, "no new window");
    let last = context.adapter.agent("zeus").items.pop().unwrap();
    assert_match(
        &last.text,
        r"T-1 · task from @chief\]\nResumed: Add the tests too$",
    );
    context.pass().unwrap();
    assert_eq!(context.task(project.id, 1).task.state, "working");
    context.adapter.answer("zeus", "Parser and tests done");
    context.pass().unwrap();
    assert_eq!(context.task(project.id, 1).task.state, "done");
    // Not held to Node's recording: a stop ignored in every round is said now.
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
    assert_match(
        task.messages[0].reason.as_deref().unwrap(),
        "refused by the test",
    );
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

    let written = Rc::new(RefCell::new(String::new()));
    let seen = Rc::clone(&written);
    *context.adapter.prepare.borrow_mut() = Some(Rc::new(move |asked| {
        *seen.borrow_mut() = asked["launchId"].as_str().unwrap().to_owned();
        Err("the settings could not be written".to_owned())
    }));
    context.give(project.id, "zeus", "Lexer");
    context.pass().unwrap();
    assert_eq!(context.task(project.id, 2).task.state, "failed");
    assert!(
        forgotten().contains(&written.take()),
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
    *context.adapter.started.borrow_mut() =
        Some(Rc::new(|| Err("the server never answered".to_owned())));
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    let task = context.task(project.id, 1);
    assert_eq!(task.task.state, "failed");
    assert_match(
        task.messages[0].reason.as_deref().unwrap(),
        "the server never answered",
    );
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

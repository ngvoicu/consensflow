//! A worker's window through its life: closed mid-task and resumed on its
//! conversation, paused in place, never opened, or never showing its first
//! message (`describe('the dispatcher')`).

use std::cell::RefCell;
use std::rc::Rc;

use cf_engine::testing::Context;
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

/// A worker's window that ends by itself in the middle of its task (its harness
/// crashed, or exited) is paused with how it ended and what its screen showed last,
/// as far as the host said: the field report of 2026-10-15 (B2, a Devin worker on
/// Windows) had only "window closed", and nothing to tell its cause by.
#[test]
fn a_worker_window_that_ends_mid_task_says_how_it_ended_and_what_it_showed_in_the_pause_note() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    context.host.exits_showing(
        Some(0xC000_0005),
        None,
        Some(&[
            "Reading src/parser.rs",
            "thread 'main' panicked at src/tui.rs:88",
        ]),
    );
    context.exit("zeus");

    let task = context.task(project.id, 1);
    assert_eq!(task.task.state, "paused");
    let note = task.messages.iter().find(|m| m.kind == "note").unwrap();
    assert_eq!(
        note.body,
        "T-1 is paused: @zeus's window closed (exit code 0xC0000005); its screen ended with: \
         \"Reading src/parser.rs / thread 'main' panicked at src/tui.rs:88\". \
         Resume it with: cf task resume T-1 \"…\"; its window comes back on its own conversation."
    );
}

/// A screen that names a secret puts none of its lines in the pause note, which goes to
/// the board, the chief's window and the log: a passphrase of words is no token, and
/// both reviews of 2026-10-16 found the masker let one syntax after another through.
/// How the window ended is said all the same.
#[test]
fn a_screen_that_names_a_secret_puts_none_of_its_lines_in_the_pause_note() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    context.host.exits_showing(
        Some(1),
        None,
        Some(&[
            "password: correct horse battery staple",
            "export API_KEY='red green blue'",
            "panicked at src/main.rs:4",
        ]),
    );
    context.exit("zeus");

    let task = context.task(project.id, 1);
    let note = task.messages.iter().find(|m| m.kind == "note").unwrap();
    assert_eq!(
        note.body,
        "T-1 is paused: @zeus's window closed (exit code 1); its last screen lines named a \
         secret, so they are left out. \
         Resume it with: cf task resume T-1 \"…\"; its window comes back on its own conversation."
    );
}

/// A window whose program the host could not end in its bound: the pause note says so in
/// place of how it ended, and that it may still run: a resume opens its window beside it,
/// which nothing refuses.
#[test]
fn a_worker_window_whose_program_could_not_be_ended_says_so_in_its_pause_note() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    context
        .host
        .exits_showing(None, None, Some(&["cleaning up"]));
    context.host.exits_with_a_survivor(4242);
    context.exit("zeus");

    let task = context.task(project.id, 1);
    assert_eq!(task.task.state, "paused");
    let note = task.messages.iter().find(|m| m.kind == "note").unwrap();
    assert_eq!(
        note.body,
        "T-1 is paused: @zeus's window closed, but its program (process 4242) could not be \
         ended and may still run; its screen ended with: \"cleaning up\". Resume it with: cf \
         task resume T-1 \"…\"; its window comes back on its own conversation."
    );
}

/// Whether each launch of `handle` was told the program of its window before was
/// proven gone, in the order prepared.
fn told_gone(context: &Context, handle: &str) -> Vec<bool> {
    context
        .adapter
        .prepared()
        .iter()
        .zip(context.adapter.previous_gone())
        .filter(|(launch, _)| launch["participant"]["handle"] == handle)
        .map(|(_, gone)| gone)
        .collect()
}

/// A window opened again on its conversation is told whether the program of the
/// window before it is gone (`Launch::previous_gone`), which a Claude window
/// takes the record's length for: only where the host said how it ended and named
/// no program it could not end. T-442's reviews: one that may still run would
/// write past the length taken, as the new process's.
#[test]
fn a_window_opened_again_is_told_the_program_before_it_is_gone_only_where_the_host_proved_it() {
    let reopened = |says: &dyn Fn(&Context)| {
        let context = Context::new();
        let project = context.with_staff(&["zeus"]);
        context.give(project.id, "zeus", "Parser");
        context.pass().unwrap();
        context.pass().unwrap();
        says(&context);
        context.exit("zeus");
        context.resume_task(project.id, 1, "Carry on");
        context.pass().unwrap();
        told_gone(&context, "zeus")
    };
    let ended = |context: &Context| context.host.exits_showing(Some(0), None, None);
    assert_eq!(reopened(&ended), [false, true], "ended, and gone");
    let outlived = |context: &Context| {
        ended(context);
        context.host.exits_with_a_survivor(4242);
    };
    assert_eq!(reopened(&outlived), [false, false], "may still run");
    assert_eq!(
        reopened(&|_: &Context| {}),
        [false, false],
        "a host that said nothing of the end"
    );
}

/// The engine's own close settles a window before the host says how it ended: the
/// host's word, when it comes, is what the next launch is told.
#[test]
fn the_hosts_word_on_a_window_the_engine_closed_itself_is_what_its_next_launch_is_told() {
    let closed_then = |word: bool| {
        let context = Context::new();
        let project = context.with_staff(&["zeus"]);
        context.give(project.id, "zeus", "Parser");
        context.pass().unwrap();
        context.pass().unwrap();
        context.host.exits_showing(Some(0), None, None);
        context.host.hold_exits.set(true);
        context.close_project(project.id).unwrap();
        context.host.hold_exits.set(false);
        if word {
            context.exit("zeus");
        }
        context.resume_project(project.id).unwrap();
        context.pass().unwrap();
        context.resume_task(project.id, 1, "Carry on");
        context.pass().unwrap();
        context.pass().unwrap();
        told_gone(&context, "zeus")
    };
    assert_eq!(closed_then(true), [false, true]);
    assert_eq!(closed_then(false), [false, false]);
}

/// What the close ended goes between how the window ended and its screen.
#[test]
fn a_worker_window_that_ends_mid_task_with_work_running_says_both() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    context.host.exits_showing(Some(1), None, Some(&["Killed"]));
    context.host.exits_leaving(&["npm test -- --watch"]);
    context.exit("zeus");

    let task = context.task(project.id, 1);
    let note = task.messages.iter().find(|m| m.kind == "note").unwrap();
    assert_match(
        &note.body,
        r#"^T-1 is paused: @zeus's window closed \(exit code 1\) with background work still running: npm test -- --watch; its screen ended with: "Killed"\. Resume it with"#,
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
    assert_eq!(context.host.inputs().len(), 3, "and then no more");
    // Its turn ends after the third round, before the look that would close it:
    // paid at rest, and the window stays.
    context.advance(2_000);
    context.adapter.answer("zeus", "Parser done");
    context.advance(1_100);
    context.pass().unwrap();
    context.advance(3_100);
    context.pass().unwrap();
    assert_eq!(context.host.inputs().len(), 3);
    assert!(
        context.host.killed().is_empty(),
        "the window stopped: it stays"
    );
    let zeus = context.id(project.id, "zeus");
    assert_eq!(context.dispatcher.unstopped(zeus), None);
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
    // Not held to Node's recording: its stop is pressed in rounds (`stops.rs`).
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

//! A window that takes long (to open, or to take a paste) holds up only
//! itself: the pass moves on, the human's operations answer once the ledger
//! has their change, and a close waits for the paste its harness is taking.

use std::rc::Rc;

use cf_engine::testing::{Context, Gate};
use cf_engine::SwitchWhen;
use serde_json::json;

use crate::chiefs::{chief_of, to, with_codex};
use crate::fixtures::assert_match;
use crate::traces::held_to;

const SUITES: &[&str] = &["a window that takes long"];

/// Every window opened from now on waits until the gate is let go
/// ([`let_open`]): `holdOpens`.
fn hold_opens(context: &Context) -> Gate {
    let gate = Gate::default();
    *context.host.hold.borrow_mut() = Some(gate.clone());
    gate
}

/// The windows held open come up, and those opened after are not held
/// (`await opened()`).
fn let_open(context: &Context, gate: &Gate) {
    *context.host.hold.borrow_mut() = None;
    gate.open();
    context.settle();
}

#[test]
fn holds_up_only_itself_the_pass_moves_on_and_other_windows_are_delivered_to_and_looked_at_meanwhile(
) {
    let context = Context::new();
    let slow = context.with_staff(&["zeus"]);
    let quick = context
        .open_project(json!({
            "directory": "/work/api",
            "name": "api",
            "chief": { "harness": "claude-code", "agent": "apollo" },
        }))
        .unwrap()
        .unwrap();
    context.pass().unwrap();
    let slow_launch = context.adapter.prepared()[0]["launchId"]
        .as_str()
        .unwrap()
        .to_owned();
    // Pi waits up to 30 s for a paste's acknowledgement; this one waits for the test.
    let held = Gate::default();
    *context.adapter.hold_deliveries.borrow_mut() = Some((slow_launch, held.clone()));
    let one = context.note_from_consensflow(slow.id, "chief", "Slow");
    let two = context.note_from_consensflow(quick.id, "chief", "Quick");
    context.pass().unwrap();
    assert_eq!(context.message(two.id).state, "delivering");
    context.pass().unwrap();
    assert_eq!(
        [context.message(one.id).state, context.message(two.id).state],
        ["delivering", "delivered"],
        "the other window was looked at again while the slow one waited"
    );
    held.open();
    context.settle();
    context.pass().unwrap();
    assert_eq!(context.message(one.id).state, "delivered");
    held_to(
        context.close(),
        SUITES,
        "holds up only itself: the pass moves on, and other windows are delivered to and looked at meanwhile",
    );
}

#[test]
fn answers_new_project_switch_chief_resume_and_a_sessions_open_once_the_ledger_has_the_change_the_window_opens_after(
) {
    let context = with_codex();
    let mut opened = hold_opens(&context);
    let project = context
        .open_project(json!({
            "directory": "/work/app",
            "name": "app",
            "chief": { "harness": "claude-code", "agent": "apollo" },
            "staff": [{ "agent": "zeus", "harness": "claude-code", "role": "worker", "tier": "standard" }],
        }))
        .unwrap()
        .unwrap();
    assert_eq!(project.state, "open");
    assert_eq!(context.host.opened().len(), 0, "its chief is still opening");
    let_open(&context, &opened);
    let chief = chief_of(&context, project.id).id;
    assert_ne!(context.dispatcher.pane(chief), None);
    context.pass().unwrap();
    context.adapter.answer("chief", "Hello");
    context.pass().unwrap();

    opened = hold_opens(&context);
    context
        .switch_chief(project.id, to("codex", "astraeus"), SwitchWhen::Now, false)
        .unwrap();
    assert_eq!(
        chief_of(&context, project.id).harness.as_deref(),
        Some("codex")
    );
    assert_eq!(
        context.dispatcher.pane(chief),
        None,
        "the new chief is still opening"
    );
    let_open(&context, &opened);
    let handoff = context.codex.prepared().pop().unwrap();
    assert_match(
        handoff["message"].as_str().unwrap(),
        r"You are the chief now\.",
    );
    assert_ne!(context.dispatcher.pane(chief), None);

    context.close_project(project.id).unwrap();
    opened = hold_opens(&context);
    let resumed = context.resume_project(project.id).unwrap().unwrap();
    assert_eq!(resumed.state, "open");
    assert_eq!(context.dispatcher.pane(chief), None);
    let_open(&context, &opened);
    assert_ne!(context.dispatcher.pane(chief), None);

    context.pool_task(project.id, "worker", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    context.adapter.answer("zeus", "Parser done");
    context.pass().unwrap();
    let session = context.task(project.id, 1).task.assignee.unwrap();
    let session_id = context.id(project.id, &session);
    assert_eq!(
        context.dispatcher.pane(session_id),
        None,
        "its window went with its task"
    );
    opened = hold_opens(&context);
    context.open_window(project.id, &session).unwrap();
    assert_eq!(context.dispatcher.pane(session_id), None);
    let_open(&context, &opened);
    assert_ne!(context.dispatcher.pane(session_id), None);
    held_to(
        context.close(),
        SUITES,
        "answers New project, Switch chief, Resume and a session’s Open once the ledger has the change; the window opens after",
    );
}

#[test]
fn closes_a_window_only_once_the_paste_its_harness_is_taking_is_over_and_that_message_goes_again() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    // The harness takes the paste only once the test lets it.
    let held = Gate::default();
    let waiting = held.clone();
    *context.adapter.deliver.borrow_mut() = Some(Rc::new(move |taking| {
        let waiting = waiting.clone();
        Box::pin(async move {
            waiting.wait().await;
            Ok(taking())
        })
    }));
    let note = context.note(project.id, "zeus", "chief", "Late");
    context.pass().unwrap();
    let closing = context.begin_close_project(project.id);
    context.settle();
    assert!(
        context.host.killed().is_empty(),
        "not while the harness takes it"
    );
    held.open();
    context.finish(closing).unwrap();
    assert_eq!(context.host.killed().len(), 1);
    let sent = context.message(note.id);
    assert_eq!(
        (sent.state.as_str(), sent.attempts),
        ("queued", 1),
        "on its way when the window went: it goes again"
    );
    held_to(
        context.close(),
        SUITES,
        "closes a window only once the paste its harness is taking is over, and that message goes again",
    );
}

#[test]
fn opens_the_chief_again_when_the_human_resumes_a_project_that_is_still_closing() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    let closing = context.begin_close_project(project.id);
    context.resume_project(project.id).unwrap();
    context.finish(closing).unwrap();
    context.settle();
    assert_eq!(context.project(project.id).state, "open");
    assert_ne!(
        context.dispatcher.pane(context.id(project.id, "chief")),
        None
    );
    let chief_windows = context
        .host
        .opened()
        .iter()
        .filter(|open| open.pane.id == format!("p{}-chief", project.id))
        .count();
    assert_eq!(chief_windows, 2);
    held_to(
        context.close(),
        SUITES,
        "opens the chief again when the human resumes a project that is still closing",
    );
}

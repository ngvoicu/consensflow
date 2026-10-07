//! A pause that comes while a window is being made, or made ready for a
//! message: the stop a launch pays is the one asked before it began, so a
//! pause that comes in between is not forgiven and is not delivered through.
//! None of these tests is held to a Node recording: Node's launch forgave it.

use std::rc::Rc;

use cf_engine::delivery_text::marker_of;
use cf_engine::testing::Context;
use cf_harness::contract::Readiness;
use cf_ledger::NewNote;
use serde_json::json;

use crate::fixtures::assert_match;

#[test]
fn a_pause_during_a_launchs_preparation_opens_no_pane_and_the_message_is_given_back_and_carried() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    let preparing = context.adapter.prepare_holds.hold(|_| true);
    let brief = context.give(project.id, "zeus", "Parser").message.unwrap();
    context.pass().unwrap();
    assert_eq!(
        context.message(brief.id).state,
        "delivering",
        "its launch has begun"
    );

    // The pause, and the words that resume it, come while the window is prepared.
    context.pause_task(project.id, 1);
    context.resume_task(project.id, 1, "Carry on");
    preparing.open();
    context.settle();
    assert!(context.host.last("zeus").is_none(), "no pane opens for it");
    assert_eq!(
        context.message(brief.id).state,
        "queued",
        "given back, with its attempt"
    );
    assert!(
        context.launch_files.forgotten.borrow().len() == 1,
        "the files its launch made are forgotten"
    );

    // The next pass opens the window once, and its first message is the words, with the brief in their paste.
    context.pass().unwrap();
    let launch = context.adapter.prepared().pop().unwrap();
    assert_match(
        launch["message"].as_str().unwrap(),
        r"^\[ConsensFlow m-\d+ · T-1 · task from @chief\]\n\(kept for you: task m-\d+ from @chief\)\nParser\n\nResumed: Carry on$",
    );
    context.pass().unwrap();
    assert_eq!(context.task(project.id, 1).task.state, "working");
    assert_eq!(
        context.host.inputs(),
        Vec::<serde_json::Value>::new(),
        "the window opened after the pause owes nothing for it"
    );
}

#[test]
fn a_pause_while_the_pane_opens_stays_owed_and_the_first_look_interrupts() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    let opening = context.host.open_holds.hold(|_| true);
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    assert!(
        context.host.last("zeus").is_none(),
        "the pane is still opening"
    );
    context.pause_task(project.id, 1);
    context.resume_task(project.id, 1, "Carry on");
    opening.open();
    context.settle();
    assert!(
        context.host.last("zeus").is_some(),
        "the launch goes on: nothing of it was abandoned"
    );
    context.pass().unwrap();
    let pane = context.host.last("zeus").unwrap().pane;
    assert_eq!(
        context.host.inputs(),
        [json!({ "id": pane.id, "generation": pane.generation, "bytes": [27] })],
        "the turn the brief began is the one the pause stopped"
    );
    assert_eq!(context.task(project.id, 1).task.state, "queued");
}

#[test]
fn a_pause_while_the_window_gets_ready_hands_it_nothing() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    let note = context
        .ledger
        .borrow_mut()
        .note(
            project.id,
            &NewNote {
                from: Some("chief".to_owned()),
                to: "zeus".to_owned(),
                body: "Mind the tests".to_owned(),
                task: Some(1),
            },
        )
        .unwrap();
    // Its turn is over, with nothing written: the task is still working.
    context.adapter.with("zeus", |agent| agent.settled = true);
    *context.adapter.ready.borrow_mut() = Some(Rc::new(|| Ok(Readiness::Ready)));
    let readying = context.adapter.ready_holds.hold(|_| true);
    context.pass().unwrap();
    assert_eq!(
        context.message(note.id).state,
        "queued",
        "not yet handed over"
    );
    context.human_pause(project.id, 1);
    readying.open();
    context.settle();
    assert_eq!(
        context.message(note.id).state,
        "queued",
        "nothing was begun"
    );
    assert!(
        !context
            .adapter
            .agent("zeus")
            .items
            .iter()
            .any(|item| item.text.contains(&marker_of(note.id))),
        "nothing pasted into the window of a task that was stopped"
    );
}

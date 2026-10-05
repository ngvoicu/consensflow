//! A chief whose window does not come up (`describe('a chief whose window
//! does not come up')`): its first message waits for the next window, the
//! project it leads stays as it is, and it is tried again, ever more slowly.

use std::cell::Cell;
use std::rc::Rc;

use cf_engine::testing::Context;
use cf_engine::SwitchWhen;

use crate::chiefs::{handoffs_of, to, to_human, with_codex};
use crate::fixtures::assert_match;
use crate::traces::held_to;

const SUITES: &[&str] = &["a chief whose window does not come up"];

/// A project whose Claude chief has said something, so a switch has a history to hand over.
fn spoken(context: &Context) -> cf_ledger::ProjectView {
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    context.adapter.answer("chief", "Hello");
    context.pass().unwrap();
    project
}

#[test]
fn gives_the_new_chief_its_handoff_again_when_its_first_window_closes_before_showing_it() {
    let context = with_codex();
    let project = spoken(&context);
    context
        .switch_chief(project.id, to("codex", "astraeus"), SwitchWhen::Now, false)
        .unwrap();
    context.codex.with("chief", |agent| agent.items.clear());
    let handoff = handoffs_of(&context, project.id).remove(0);
    // The window crashes, or the human closes the project, before the handoff shows.
    context.exit("chief");
    assert_eq!(context.project(project.id).state, "suspended");
    let waiting = context.message(handoff.id);
    assert_eq!((waiting.state.as_str(), waiting.attempts), ("queued", 0));

    context.resume_project(project.id).unwrap();
    let launch = context.codex.prepared().last().cloned().unwrap();
    assert_match(
        launch["message"].as_str().unwrap(),
        r"^\[ConsensFlow m-\d+ · note from ConsensFlow\]\nYou are the chief now\.",
    );
    let handoffs: Vec<(i64, String)> = handoffs_of(&context, project.id)
        .into_iter()
        .map(|message| (message.id, message.state))
        .collect();
    assert_eq!(
        handoffs,
        [(handoff.id, "delivering".to_owned())],
        "the same handoff, and no other"
    );
    held_to(
        context.close(),
        SUITES,
        "gives the new chief its handoff again when its first window closes before showing it",
    );
}

#[test]
fn keeps_the_project_open_when_the_new_chief_cannot_take_its_handoff_and_opens_it_again_with_it() {
    let context = with_codex();
    let project = spoken(&context);
    let chief = context.id(project.id, "chief");
    let zeus = context.id(project.id, "zeus");
    let slow = Rc::new(Cell::new(true));
    *context.codex.started.borrow_mut() = Some(Rc::new(move || {
        if !slow.replace(false) {
            return Ok(());
        }
        Err("the Codex broker never named the thread it opened".to_owned())
    }));
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    let killed = context.host.killed().len();
    context
        .switch_chief(project.id, to("codex", "astraeus"), SwitchWhen::Now, false)
        .unwrap();
    assert_eq!(
        context.project(project.id).state,
        "open",
        "the staff keeps working"
    );
    assert_eq!(
        context.host.killed().len(),
        killed + 2,
        "the old chief, then the new window"
    );
    assert_eq!(context.dispatcher.pane(chief), None);
    assert_ne!(context.dispatcher.pane(zeus), None);
    let handoff = handoffs_of(&context, project.id).remove(0);
    assert_eq!((handoff.state.as_str(), handoff.attempts), ("queued", 0));
    assert_eq!(
        to_human(&context, project.id),
        ["The chief could not start: the window could not take its first message: the Codex broker never named the thread it opened. What comes for the chief waits for it, and ConsensFlow tries again; you may also switch the chief."]
    );

    context.pass().unwrap();
    assert_eq!(context.codex.prepared().len(), 1, "not again at once");
    context.advance(5_000);
    context.pass().unwrap();
    assert_eq!(context.codex.prepared().len(), 2);
    let launch = context.codex.prepared().last().cloned().unwrap();
    assert_match(
        launch["message"].as_str().unwrap(),
        r"You are the chief now\.",
    );
    assert_eq!(handoffs_of(&context, project.id).len(), 1);
    held_to(
        context.close(),
        SUITES,
        "keeps the project open when the new chief cannot take its handoff, and opens it again with it",
    );
}

#[test]
fn a_chief_whose_launch_keeps_failing_its_handoff_waits_no_other_is_written_it_is_tried_ever_more_slowly_and_the_human_hears_once(
) {
    let context = with_codex();
    let project = spoken(&context);
    let tries = Rc::new(Cell::new(0));
    let counted = Rc::clone(&tries);
    *context.codex.prepare.borrow_mut() = Some(Rc::new(move |_| {
        counted.set(counted.get() + 1);
        Err("codex is broken".to_owned())
    }));
    context
        .switch_chief(project.id, to("codex", "astraeus"), SwitchWhen::Now, false)
        .unwrap();
    let ready = context.note(project.id, "zeus", "chief", "Ready");
    for _ in 0..10 {
        context.pass().unwrap();
    }
    assert_eq!(tries.get(), 1, "no pass tries again at once");
    for wait in [5_000, 10_000, 20_000] {
        context.advance(wait - 1);
        context.pass().unwrap();
        context.advance(1);
        context.pass().unwrap();
    }
    assert_eq!(tries.get(), 4, "after 5, 10 and 20 seconds");
    let handoffs: Vec<(String, i64)> = handoffs_of(&context, project.id)
        .into_iter()
        .map(|message| (message.state, message.attempts))
        .collect();
    assert_eq!(
        handoffs,
        [("queued".to_owned(), 0)],
        "one handoff, never spent"
    );
    let waiting = context.message(ready.id);
    assert_eq!((waiting.state.as_str(), waiting.attempts), ("queued", 0));
    assert_eq!(to_human(&context, project.id).len(), 1, "told once");
    assert_match(
        &to_human(&context, project.id)[0],
        "could not start: the launch failed: codex is broken",
    );

    *context.codex.prepare.borrow_mut() = None;
    context.advance(40_000);
    context.pass().unwrap();
    let launch = context.codex.prepared().last().cloned().unwrap();
    assert_match(
        launch["message"].as_str().unwrap(),
        r"You are the chief now\.",
    );
    assert_eq!(to_human(&context, project.id).len(), 1);
    held_to(
        context.close(),
        SUITES,
        "a chief whose launch keeps failing: its handoff waits, no other is written, it is tried ever more slowly, and the human hears once",
    );
}

#[test]
fn notices_a_chief_window_that_exits_before_its_open_is_answered() {
    let context = with_codex();
    let project = spoken(&context);
    let chief = context.id(project.id, "chief");
    // The host sends the exit first, in the same read as its answer to the open.
    *context.host.exit_after_open.borrow_mut() = Some("chief".to_owned());
    context
        .switch_chief(project.id, to("codex", "astraeus"), SwitchWhen::Now, false)
        .unwrap();
    assert_eq!(context.dispatcher.pane(chief), None);
    assert_eq!(
        context.project(project.id).state,
        "suspended",
        "the chief went, as it would have later"
    );
    assert_eq!(
        handoffs_of(&context, project.id)[0].state,
        "queued",
        "its handoff waits"
    );

    context.resume_project(project.id).unwrap();
    assert_ne!(context.dispatcher.pane(chief), None);
    let launch = context.codex.prepared().last().cloned().unwrap();
    assert_match(
        launch["message"].as_str().unwrap(),
        r"You are the chief now\.",
    );
    held_to(
        context.close(),
        SUITES,
        "notices a chief window that exits before its open is answered",
    );
}

#[test]
fn keeps_what_was_queued_for_a_first_chief_whose_window_does_not_open() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    context.close_project(project.id).unwrap();
    context.host.refuse.set(true);
    context.resume_project(project.id).unwrap();
    assert_match(
        &to_human(&context, project.id)[0],
        r"^The chief could not start: the window did not open: refused by the test\.",
    );
    let result = context.note(project.id, "zeus", "chief", "A result");
    // Tried again with the result as its first message, and refused again.
    context.advance(5_000);
    context.host.refuse.set(true);
    context.pass().unwrap();
    let waiting = context.message(result.id);
    assert_eq!(
        (waiting.state.as_str(), waiting.attempts),
        ("queued", 0),
        "not spent on a window that did not open"
    );
    assert_eq!(to_human(&context, project.id).len(), 1);
    context.advance(10_000);
    context.pass().unwrap();
    let launch = context.adapter.prepared().last().cloned().unwrap();
    assert_match(
        launch["message"].as_str().unwrap(),
        r"note from @zeus\]\nA result$",
    );
    held_to(
        context.close(),
        SUITES,
        "keeps what was queued for a first chief whose window does not open",
    );
}

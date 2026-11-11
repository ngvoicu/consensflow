//! The window of a task that is done stays open while its own extension says it asked its
//! harness to compact the conversation at the window's rest for the project's size, and has
//! not said it over (Pi's `compacting/<launch>.json`, T-1475): a Pi member whose task is one
//! turn is at rest as its result is taken, and its summary takes 39 to 80 seconds, which the
//! close, ten seconds after the result, cut (the signal aborting it: the ledger of the owner's
//! cached compaction says `cancelled`). The result is the chief's at once as ever; only the
//! close waits, for the word to be taken back, or for the bound ([`COMPACTION_HOLD_MS`]).
//! A pause, a cancel, a failure and the human ending the session close at once.
//!
//! Not held to a Node recording: Node's dispatcher had no such word.

use cf_engine::testing::Context;
use cf_harness::contract::Pane;
use cf_harness::seams::Time;
use serde_json::json;

use crate::fixtures::{hold_notes, Tiers};

/// The longest the window waits for a compaction from its word: `windows.rs`'
/// `COMPACTION_HOLD_MS`.
const BOUND_MS: i64 = 300_000;

/// Zeus has T-1, delivered and at work in a window of its own: its pane. Its screen has been
/// still for a minute, so that the screen alone holds the window no longer.
fn at_work(context: &Context, project: i64) -> Pane {
    context.give(project, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.task(project, 1).task.state, "working");
    context
        .host
        .set_snapshot(json!({ "screenQuietMs": 60_000 }));
    context.host.last("zeus").unwrap().pane
}

/// The extension says it asked for a compaction `ago` ms ago (a negative `ago` is a word
/// whose time is ahead of the engine's clock), or takes its word back (`None`).
fn word(context: &Context, ago: Option<i64>) {
    let at = ago.map(|ago| context.time.wall_ms() - ago);
    context
        .adapter
        .with("zeus", |agent| agent.compaction_asked = at);
}

/// Zeus's agent answers, the extension asks for its compaction in the same moment, and the
/// engine looks at the window twice.
fn answers_compacting(context: &Context) {
    word(context, Some(0));
    context.adapter.answer("zeus", "Parser done");
    context.pass().unwrap();
    context.pass().unwrap();
}

fn open(context: &Context, window: &Pane) -> bool {
    !context.host.killed().contains(window)
}

fn has_result(context: &Context, project: i64, number: i64, words: &str) -> bool {
    context
        .task(project, number)
        .messages
        .iter()
        .any(|message| message.kind == "result" && message.body.contains(words))
}

#[test]
fn a_done_tasks_window_stays_open_while_its_extension_compacts_and_closes_in_the_step_the_word_is_taken_back(
) {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    let window = at_work(&context, project.id);
    answers_compacting(&context);
    // The result is the chief's at once; only the close waits.
    assert_eq!(context.task(project.id, 1).task.state, "done");
    assert!(has_result(&context, project.id, 1, "Parser done"));
    assert!(open(&context, &window), "its compaction is at work");

    // Eighty seconds on, the longest the records show, and still open; the chief accepts.
    context.advance(80_000);
    context
        .ledger
        .borrow_mut()
        .accept_task(project.id, 1, "chief")
        .unwrap();
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.task(project.id, 1).task.state, "accepted");
    assert!(open(&context, &window), "accepted, it still waits");

    // The compaction ends: the word is taken back, and the window closes, once.
    word(&context, None);
    context.pass().unwrap();
    assert_eq!(context.host.killed(), std::slice::from_ref(&window));
    context.pass().unwrap();
    assert_eq!(context.host.killed(), [window], "closed once");
    context.close();
}

#[test]
fn a_word_that_is_never_taken_back_holds_the_window_for_the_bound_from_the_word_and_no_longer() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    let window = at_work(&context, project.id);
    answers_compacting(&context);
    assert!(open(&context, &window));

    context.advance(BOUND_MS - 1);
    context.pass().unwrap();
    context.pass().unwrap();
    assert!(open(&context, &window), "a millisecond before the bound");
    assert_eq!(context.task(project.id, 1).task.state, "done");

    // The bound is over, the word standing: the window closes as it does without one.
    context.advance(1);
    context.pass().unwrap();
    assert_eq!(context.host.killed(), [window]);
    assert!(has_result(&context, project.id, 1, "Parser done"));
    context.close();
}

/// A word the engine reads only when the window is free: its bound is the word's own time, not
/// the time of the look that found the window free.
#[test]
fn the_bound_runs_from_the_words_time_not_from_the_look_that_found_the_window_free() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    let window = at_work(&context, project.id);
    // The extension asked 100 s ago; the task's result is taken only now.
    word(&context, Some(100_000));
    context.adapter.answer("zeus", "Parser done");
    context.pass().unwrap();
    context.pass().unwrap();
    assert!(open(&context, &window));
    context.advance(BOUND_MS - 100_000 - 1);
    context.pass().unwrap();
    assert!(open(&context, &window), "a millisecond before the bound");
    context.advance(1);
    context.pass().unwrap();
    assert_eq!(context.host.killed(), [window]);
    context.close();
}

#[test]
fn a_stale_word_holds_nothing() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    let window = at_work(&context, project.id);
    // A Pi that died, or hung, in a compaction asked for the bound ago.
    word(&context, Some(BOUND_MS));
    context.adapter.answer("zeus", "Parser done");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(
        context.host.killed(),
        [window],
        "closed in the step that took the result"
    );
    context.close();
}

#[test]
fn a_word_whose_time_is_ahead_of_the_clock_is_held_for_the_bound_from_the_first_look_at_it() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    let window = at_work(&context, project.id);
    word(&context, Some(-3_600_000));
    context.adapter.answer("zeus", "Parser done");
    context.pass().unwrap();
    context.pass().unwrap();
    assert!(open(&context, &window));
    context.advance(BOUND_MS - 1);
    context.pass().unwrap();
    assert!(open(&context, &window), "a millisecond before the bound");
    context.advance(1);
    context.pass().unwrap();
    assert_eq!(context.host.killed(), [window]);
    context.close();
}

#[test]
fn a_window_whose_task_is_not_done_is_not_held_for_the_word_a_cancel_closes_it_at_once() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    let window = at_work(&context, project.id);
    word(&context, Some(0));
    context
        .ledger
        .borrow_mut()
        .cancel_task(project.id, 1, "human")
        .unwrap();
    context.pass().unwrap();
    assert_eq!(context.host.killed(), [window]);
    context.close();
}

#[test]
fn the_human_ending_the_session_closes_its_window_at_once_under_the_compaction() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working");
    context
        .host
        .set_snapshot(json!({ "screenQuietMs": 60_000 }));
    let window = context.host.last("zeus").unwrap().pane;
    answers_compacting(&context);
    assert_eq!(tiers.task(1).task.state, "done");
    assert!(open(&context, &window), "its compaction is at work");
    context
        .end_session(tiers.project.id, "zeus-amber-pine")
        .unwrap();
    assert_eq!(context.host.killed(), [window]);
    context.close();
}

/// A follow-up for the session, queued behind the done task, does not go into the window
/// that holds the compaction (no other task goes into a window a task's message went into,
/// `ownership`): it waits for the window to close, its requester told nothing, and goes
/// into the window that opens after, once, on the conversation the compaction ended in.
#[test]
fn the_next_task_of_the_session_waits_for_the_compaction_and_goes_in_once_after_it() {
    let context = Context::new();
    // The host proves each window's program gone, as the app's does: the session's next task
    // goes on with its conversation (`ownership`).
    context.host.exits_showing(Some(0), None, None);
    let project = context.with_staff(&["zeus"]);
    let window = at_work(&context, project.id);
    answers_compacting(&context);
    context.give(project.id, "zeus", "Lexer");
    let opened = context.host.opened().len();
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert!(open(&context, &window), "its compaction is at work");
    assert_eq!(context.host.opened().len(), opened);
    assert_eq!(context.task(project.id, 2).task.state, "queued");
    assert!(
        hold_notes(&context, project.id, 2).is_empty(),
        "told nothing while it waits"
    );
    context.advance(60_000);
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert!(open(&context, &window));
    assert_eq!(context.task(project.id, 2).task.state, "queued");

    word(&context, None);
    for _ in 0..4 {
        context.pass().unwrap();
    }
    assert_eq!(context.host.killed(), std::slice::from_ref(&window));
    assert_eq!(context.host.opened().len(), opened + 1);
    assert_ne!(context.host.last("zeus").unwrap().pane, window);
    assert_eq!(context.task(project.id, 2).task.state, "working");
    context.close();
}

//! The notes of ConsensFlow's own that ask the chief no decision are shown, not typed (RS-25,
//! Gabriel on 2026-11-11: "many useless messages sent to the chief we act"). A task held in
//! the background by work that ends by itself is the most of them (T-1410: 10 of the chief's
//! last 20 notes): its task's line says it, `cf task get` holds the note, and the chief's
//! window is given nothing of it. One note is typed after a task has been held so for an hour
//! with no turn of its worker's own; a hold that nothing but Finish ends is typed as it was.

use cf_engine::testing::Context;

use crate::fixtures::{given, Tiers};
use crate::turn_end::pending;

/// The notification Claude Code begins a turn with when a job of its ends.
const ENDED: &str =
    "<task-notification>\n<task-id>b1</task-id>\n<status>completed</status>\n</task-notification>";

/// `STILL_WORKING_REMINDED_AFTER_MS`: an hour.
const AN_HOUR: i64 = 60 * 60_000;

/// A project of zeus and diana, and T-1 given to zeus by name, at work in its window.
fn zeus_at_work(context: &Context) -> Tiers<'_> {
    context.host.exits_showing(Some(0), None, None);
    let tiers = Tiers::new(context, &["zeus", "diana"]);
    context.give(tiers.project.id, "zeus", "Record the run");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working");
    tiers
}

/// The notes sent to the chief to be typed into its window (not shown), oldest first.
fn typed(context: &Context, tiers: &Tiers<'_>) -> Vec<String> {
    let mut notes: Vec<_> = context
        .ledger
        .borrow()
        .inbox(tiers.id("chief"), 100)
        .unwrap()
        .into_iter()
        .filter(|message| message.kind == "note" && message.receipt.get("shown").is_none())
        .map(|message| message.body)
        .collect();
    notes.reverse();
    notes
}

/// What T-1's line says beside its state.
fn aside(tiers: &Tiers<'_>) -> Option<String> {
    tiers.task(1).task.aside
}

/// Zeus ends its turn with `jobs` commands running in the background, and a pass looks.
fn held(context: &Context, words: &str, jobs: usize) {
    context
        .adapter
        .answer_leaving("zeus", words, pending(jobs, 0));
    context.pass().unwrap();
}

#[test]
fn a_hold_that_ends_by_itself_is_shown_on_its_task_and_not_typed() {
    let context = Context::new();
    let tiers = zeus_at_work(&context);
    held(&context, "The Windows run starts", 1);

    assert_eq!(typed(&context, &tiers), Vec::<String>::new());
    assert_eq!(
        aside(&tiers).as_deref(),
        Some("in the background: 1 command")
    );
    // `cf task get` holds what the note said, as it did.
    let thread = tiers.task(1);
    let note = thread
        .messages
        .iter()
        .find(|message| message.kind == "note")
        .expect("the note is in the task's thread");
    assert_eq!(
        note.body,
        "T-1 is still working in the background (1 command); its result comes when it ends. `cf task finish T-1` takes what it said last now.\n\nWhat it said:\n\nThe Windows run starts"
    );
    assert_eq!(note.state, "read");
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert!(
        given(&context, "chief")
            .iter()
            .all(|text| !text.contains("in the background")),
        "{:?}",
        given(&context, "chief")
    );
}

#[test]
fn a_turn_of_the_workers_own_that_leaves_it_held_again_says_what_runs_now_and_writes_no_note() {
    // poker-lab's T-442: a Monitor wakes its worker at each event; a note a turn would be a
    // note a tick, typed or not.
    let context = Context::new();
    let tiers = zeus_at_work(&context);
    held(&context, "The Windows run starts", 1);
    assert_eq!(
        aside(&tiers).as_deref(),
        Some("in the background: 1 command")
    );

    for (round, jobs) in [2, 3, 1].into_iter().enumerate() {
        context.adapter.wake("zeus", ENDED);
        context.pass().unwrap();
        held(&context, &format!("Round {round}"), jobs);
    }
    assert_eq!(
        aside(&tiers).as_deref(),
        Some("in the background: 1 command")
    );
    context.adapter.wake("zeus", ENDED);
    context.pass().unwrap();
    held(&context, "The Linux runs start", 2);
    assert_eq!(
        aside(&tiers).as_deref(),
        Some("in the background: 2 commands")
    );
    assert_eq!(typed(&context, &tiers), Vec::<String>::new());
    let notes: Vec<_> = tiers
        .task(1)
        .messages
        .into_iter()
        .filter(|message| message.kind == "note")
        .collect();
    assert_eq!(
        notes.len(),
        1,
        "one note for the hold, as before: {notes:?}"
    );

    // The hold ends with a question: the task waits, and the line says no more of the hold.
    context.adapter.wake("zeus", ENDED);
    context.pass().unwrap();
    context.adapter.answer(
        "zeus",
        "Both runs failed on the same test. Waiting for your answer: should I skip it or fix it?",
    );
    for _ in 0..2 {
        context.pass().unwrap();
    }
    assert_eq!(
        tiers.task(1).task.state,
        "waiting",
        "it waits for the chief's answer"
    );
    assert_eq!(aside(&tiers), None, "{:?}", tiers.task(1).task);
}

#[test]
fn one_note_is_typed_when_a_task_is_held_an_hour_with_no_turn_of_its_own_and_one_only() {
    let context = Context::new();
    let tiers = zeus_at_work(&context);
    held(&context, "The soak test starts", 1);

    context.advance(AN_HOUR - 1_000);
    context.pass().unwrap();
    assert_eq!(
        typed(&context, &tiers),
        Vec::<String>::new(),
        "not before the hour"
    );

    context.advance(1_000);
    context.pass().unwrap();
    let reminder = "T-1 has been working in the background for 60 minutes with no turn of its own (1 command). Its result comes when that ends; `cf task finish T-1` takes what it said last now.";
    assert_eq!(typed(&context, &tiers), [reminder]);
    assert_eq!(
        aside(&tiers).as_deref(),
        Some("in the background: 1 command"),
        "the line still says it"
    );

    for _ in 0..3 {
        context.advance(AN_HOUR);
        context.pass().unwrap();
    }
    assert_eq!(typed(&context, &tiers), [reminder], "once for the hold");

    // A turn of its own counts the hour again from the hold it leaves.
    context.adapter.wake("zeus", ENDED);
    context.pass().unwrap();
    held(&context, "The soak test goes on", 1);
    context.advance(AN_HOUR - 1_000);
    context.pass().unwrap();
    assert_eq!(typed(&context, &tiers).len(), 1);
    context.advance(1_000);
    context.pass().unwrap();
    assert_eq!(
        typed(&context, &tiers).len(),
        2,
        "{:?}",
        typed(&context, &tiers)
    );
}

#[test]
fn a_restart_ends_a_background_hold_and_writes_no_note_of_it_again() {
    // T-1500's finding 3 feared that a restart, which empties the engine's memory of a hold,
    // writes a fresh shown note whose time the hour then counts from. No window survives a
    // restart (`cf-daemon`'s start), and the hold ends with its window: on the Resume the
    // turn is read from its conversation and its result typed to the chief, and no note of
    // the hold is written again.
    let context = Context::new();
    context.adapter.resumes_records.set(true);
    let tiers = zeus_at_work(&context);
    held(&context, "The soak test starts", 1);
    let notes = |tiers: &Tiers<'_>| {
        tiers
            .task(1)
            .messages
            .into_iter()
            .filter(|message| message.kind == "note")
            .count()
    };
    assert_eq!(notes(&tiers), 1);

    context.advance(AN_HOUR - 10 * 60_000);
    context.ledger.borrow_mut().suspend_for_restart().unwrap();
    let after = context.make();
    after.settle_after_restart().unwrap();
    after.resume_project(tiers.project.id).unwrap();
    for _ in 0..4 {
        after.pass().unwrap();
    }
    let thread = tiers.task(1);
    assert_eq!(thread.task.state, "done", "{:#?}", thread.messages);
    assert_eq!(
        notes(&tiers),
        1,
        "the hold is told once: {:#?}",
        thread.messages
    );
    assert_eq!(aside(&tiers), None, "its line says no hold");
    let result = thread
        .messages
        .iter()
        .find(|message| message.kind == "result")
        .expect("its result");
    assert_eq!(result.recipient, "chief");
    assert_eq!(result.body, "The soak test starts");
}

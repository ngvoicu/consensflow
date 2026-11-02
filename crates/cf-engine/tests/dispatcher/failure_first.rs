//! **The chief is told that a task failed before it is told what the failure left
//! behind** (field report, the "kluris" board of 2026-11-02, ConsensFlow
//! 3.0.7-alpha.1, T-1148; the issue page "a note of what a failure left behind
//! reached the chief before the failure").
//!
//! The chief is given its notes one at a time, the oldest first, so the order they
//! are written in is the order it reads them in. On the field board T-302 failed (its
//! harness said the model was at capacity) and the daemon wrote "T-303 waits for
//! T-302, which failed" before "T-302 failed", in one step, a millisecond apart. The
//! chief acted on the consequence with the cause unknown (it cancelled T-303 and
//! opened another task), and the cause reached it 47 seconds later, when its turn on
//! the first was over, as news it had already dealt with.
//!
//! Each case is that shape on the engine's own dispatcher, on one way a task fails:
//! the turn failed, the launch of its window did not come up, the window went with
//! its brief on the way. Whatever the way, what the chief reads first is that the
//! task failed, and the tasks that wait for it come in that note or after it, never
//! before. T-2 is the chief's own task and needs T-1.

use std::rc::Rc;

use cf_engine::testing::Context;
use cf_ledger::NewTask;

use crate::background::working;
use crate::fixtures::Tiers;
use crate::queued_tells::told_on_its_way;

/// T-2, asked for by the chief, waits for T-1 on the board.
fn waits_for_t1(tiers: &Tiers<'_>) {
    tiers.context.create_task(
        tiers.project.id,
        NewTask {
            from: "chief".to_owned(),
            pool: Some("worker".to_owned()),
            tier: Some("standard".to_owned()),
            body: "Tests of the parser".to_owned(),
            needs: vec![1],
            ..NewTask::default()
        },
    );
    assert_eq!(tiers.task(2).task.state, "open", "T-2 waits for T-1");
}

/// The notes the chief was sent that name T-1 or T-2, oldest first: the cause and
/// what it left behind, nothing else the board says.
fn about_the_two(tiers: &Tiers<'_>) -> Vec<String> {
    tiers
        .notes("chief")
        .into_iter()
        .filter(|note| note.contains("T-1") || note.contains("T-2"))
        .collect()
}

#[test]
fn a_turn_that_failed_is_told_to_the_chief_before_the_task_that_waits_for_it() {
    let context = Context::new();
    let tiers = working(&context);
    waits_for_t1(&tiers);

    context.adapter.with("zeus", |agent| {
        agent.failed = true;
        agent.failure =
            Some("Selected model is at capacity. Please try a different model.".to_owned());
    });
    context.adapter.answer("zeus", "Starting on the parser");
    context.pass().unwrap();

    assert_eq!(tiers.task(1).task.state, "failed");
    assert_eq!(tiers.task(2).task.state, "open", "T-2 still waits");
    let told = about_the_two(&tiers);
    assert!(
        told.first()
            .is_some_and(|first| first.starts_with("T-1 failed")),
        "the chief reads the cause first: {told:?}"
    );
    assert!(
        told.iter().any(|note| note.contains("T-2")),
        "and is told T-2 waits for it: {told:?}"
    );
}

#[test]
fn a_launch_that_did_not_come_up_is_told_to_the_chief_before_the_task_that_waits_for_it() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    tiers.open();
    context.pass().unwrap();
    waits_for_t1(&tiers);

    // zeus's window exits right after its start, with its brief on its way.
    context.host.exits_showing(Some(3), None, None);
    context.exit("zeus");

    assert_eq!(tiers.task(1).task.state, "failed");
    let told = about_the_two(&tiers);
    assert!(
        told.first()
            .is_some_and(|first| first.starts_with("T-1 failed")),
        "the chief reads the cause first: {told:?}"
    );
    assert!(
        told.iter().any(|note| note.contains("T-2")),
        "and is told T-2 waits for it: {told:?}"
    );
}

#[test]
fn words_that_never_reached_the_window_are_told_to_the_chief_before_the_task_that_waits_for_it() {
    let context = Context::new();
    let (tiers, _session, told) = told_on_its_way(&context);
    waits_for_t1(&tiers);

    // The window answers the brief, and takes no paste after it: the words fail the task.
    context.adapter.answer("zeus", "Parser done");
    context.adapter.with("zeus", |agent| agent.admit = false);
    for _ in 0..5 {
        context.pass().unwrap();
    }

    assert_eq!(context.message(told).state, "failed");
    assert_eq!(tiers.task(1).task.state, "failed");
    let told = about_the_two(&tiers);
    assert!(
        told.first()
            .is_some_and(|first| first.starts_with("T-1 failed")),
        "the chief reads the cause first: {told:?}"
    );
    assert!(
        told.iter().any(|note| note.contains("T-2")),
        "and is told T-2 waits for it: {told:?}"
    );
}

#[test]
fn a_launch_that_never_showed_its_first_message_is_told_to_the_chief_before_the_task_that_waits_for_it(
) {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    // The window opens, but its record never shows the brief.
    let fake = Rc::downgrade(&context.adapter);
    *context.adapter.after_prepare.borrow_mut() = Some(Rc::new(move |handle| {
        if let Some(fake) = fake.upgrade() {
            fake.with(handle, |agent| agent.items.clear());
        }
    }));
    tiers.open();
    context.pass().unwrap();
    waits_for_t1(&tiers);
    context.advance(121_000);
    context.pass().unwrap();
    context.pass().unwrap();

    assert_eq!(tiers.task(1).task.state, "failed");
    let told = about_the_two(&tiers);
    assert!(
        told.first()
            .is_some_and(|first| first.starts_with("T-1 failed")),
        "the chief reads the cause first: {told:?}"
    );
    assert!(
        told.iter().any(|note| note.contains("T-2")),
        "and is told T-2 waits for it: {told:?}"
    );
}

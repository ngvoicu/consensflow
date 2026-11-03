//! `cf task get T-n --transcript` is the whole of what the task's window did, in the
//! order it came, newest last (T-1118, from Gabriel's other board: at 18:42 the
//! transcript of T-3 ended at the message m-382 sent to the window, while the board
//! listed m-383, a result from the worker, and m-384, a task from the chief,
//! delivered, and showed T-3 working).
//!
//! A window opened again on its conversation shows a record shorter than the copy
//! ConsensFlow kept (the fake's does, as a harness's may: it rewrote it). The copy of
//! it was put at the places of the items the copy kept, so the new ones sorted among
//! the old and the last items of the transcript were old ones.

use cf_engine::testing::Context;

use crate::fixtures::Tiers;

/// The first line of each item of the task's transcript, oldest first.
fn lines(context: &Context, tiers: &Tiers<'_>, number: i64, last: Option<usize>) -> Vec<String> {
    context
        .ledger
        .borrow()
        .transcript(tiers.project.id, number, last)
        .unwrap()
        .items
        .iter()
        .map(|item| item.text.lines().next().unwrap_or_default().to_owned())
        .collect()
}

#[test]
fn the_transcript_of_a_task_is_in_the_order_its_window_wrote_through_a_result_and_a_reopen() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();

    context.adapter.answer("zeus", "Parser done");
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "done");
    assert_eq!(
        lines(&context, &tiers, 1, None),
        ["[ConsensFlow m-1 · T-1 · task from @chief]", "Parser done"]
    );

    // The chief sends it back: the session's window comes up again on its
    // conversation and shows a record that holds the new brief alone.
    context
        .ledger
        .borrow_mut()
        .reopen_task(tiers.project.id, 1, "chief", "Handle empty input too")
        .unwrap();
    for _ in 0..4 {
        context.pass().unwrap();
    }
    assert_eq!(tiers.task(1).task.state, "working");
    context.adapter.answer("zeus", "Empty input handled");
    for _ in 0..2 {
        context.pass().unwrap();
    }
    assert_eq!(tiers.task(1).task.state, "done");

    assert_eq!(
        lines(&context, &tiers, 1, None),
        [
            "[ConsensFlow m-1 · T-1 · task from @chief]",
            "Parser done",
            "[ConsensFlow m-3 · T-1 · task from @chief]",
            "Empty input handled",
        ],
        "in the order the window wrote them"
    );
    assert_eq!(
        lines(&context, &tiers, 1, Some(2)),
        [
            "[ConsensFlow m-3 · T-1 · task from @chief]",
            "Empty input handled"
        ],
        "and `--last` shows the newest"
    );
}

//! A line of a window's record torn and passed over (T-223, T-248): the reader
//! goes on past it, and says where in the reading (`Record::lost`). A record lost
//! in a turn may have been part of the turn's answer, so a result taken past one
//! ends with a line for the requester that says so, and the window's activity
//! says it while the turn goes on. A line lost before the message was handed over
//! is no part of its turn. Node had no such lines, so none of these is held to a
//! Node trace.

use cf_engine::testing::Context;
use cf_ledger::NewTask;

use crate::fixtures::{given, Tiers};

/// T-1 given to zeus, whose window has taken it in.
fn working(context: &Context) -> Tiers<'_> {
    let tiers = Tiers::new(context, &["zeus"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working");
    tiers
}

/// The results the chief was given or is to be given.
fn results(context: &Context, tiers: &Tiers<'_>) -> Vec<String> {
    context
        .inbox(tiers.id("chief"))
        .into_iter()
        .filter(|message| message.kind == "result")
        .map(|message| message.body)
        .collect()
}

/// What the window of `handle` says it is doing, if it says why.
fn reason(context: &Context, tiers: &Tiers<'_>, handle: &str) -> Option<String> {
    context.dispatcher.activity(tiers.id(handle)).reason
}

#[test]
fn a_result_taken_past_a_line_lost_in_its_turn_ends_with_a_line_for_the_chief_and_the_window_says_it(
) {
    let context = Context::new();
    let tiers = working(&context);
    let worker = tiers.assignee(1);
    assert_eq!(reason(&context, &tiers, &worker), None);
    context.adapter.loses_a_line("zeus", 5);
    context.pass().unwrap();
    assert_eq!(
        reason(&context, &tiers, &worker),
        Some(format!("a line of @{worker}'s record was lost (record 5)")),
        "said while the turn goes on"
    );
    context.adapter.answer("zeus", "Parser done");
    context.pass().unwrap();
    context.pass().unwrap();

    assert_eq!(tiers.task(1).task.state, "done");
    assert_eq!(
        results(&context, &tiers),
        [format!(
            "Parser done\n\nA line of @{worker}'s record was lost (record 5); this result may miss part of the turn."
        )]
    );
}

#[test]
fn a_line_lost_before_a_message_was_handed_over_is_no_part_of_its_turn() {
    let context = Context::new();
    let tiers = working(&context);
    let worker = tiers.assignee(1);
    context.adapter.loses_a_line("zeus", 5);
    context.adapter.answer("zeus", "Parser done");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "done");

    // T-2 follows T-1 into a window on the same conversation, whose record lost
    // the line before it.
    context.create_task(
        tiers.project.id,
        NewTask {
            from: "chief".to_owned(),
            after: Some(1),
            body: "Now the lexer".to_owned(),
            ..NewTask::default()
        },
    );
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(2).task.state, "working");
    assert_eq!(
        reason(&context, &tiers, &worker),
        None,
        "its turn lost nothing"
    );
    context.adapter.answer("zeus", "Lexer done");
    context.pass().unwrap();
    context.pass().unwrap();

    assert_eq!(tiers.task(2).task.state, "done");
    let results = results(&context, &tiers);
    assert!(results.contains(&"Lexer done".to_owned()), "{results:?}");
}

#[test]
fn a_message_pasted_into_an_open_window_begins_a_turn_that_has_lost_nothing_yet() {
    // The chief's window stays open from message to message: each is handed over
    // into it, after whatever its record lost before.
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    context.pass().unwrap();
    context.pass().unwrap();
    context.adapter.loses_a_line("chief", 3);
    context.pass().unwrap();
    assert_eq!(
        reason(&context, &tiers, "chief"),
        Some("a line of @chief's record was lost (record 3)".to_owned()),
        "lost before any message the engine knows of"
    );
    context.note_from_consensflow(tiers.project.id, "chief", "Word one");
    context.pass().unwrap();
    context.pass().unwrap();
    assert!(
        given(&context, "chief")
            .iter()
            .any(|text| text.contains("Word one")),
        "{:?}",
        given(&context, "chief")
    );
    assert_eq!(
        reason(&context, &tiers, "chief"),
        None,
        "its turn lost nothing"
    );
}

//! The time a launch has for its first message is the engine's limit
//! (`CONSENSFLOW_LAUNCH_TIMEOUT_MS`), and every adapter is told it: what a
//! window waits for before that message can show (the conversation its harness
//! names) ends by it, so the limit cuts that wait short as it does the rest of
//! the launch. None of these tests is held to a Node recording: Node's adapters
//! waited a fixed minute.

use cf_engine::testing::Context;

#[test]
fn every_launch_is_told_how_long_the_engine_gives_its_first_message() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    let waits = context.adapter.first_message_waits();
    assert_eq!(
        waits.len(),
        context.adapter.prepared().len(),
        "one for each launch"
    );
    assert!(waits.len() >= 2, "the chief's window and zeus's: {waits:?}");
    // What the context's limits say (a daemon's say it by the variable), and
    // neither the default of three minutes nor an adapter's own minute.
    assert!(waits.iter().all(|wait| *wait == 120_000), "{waits:?}");
}

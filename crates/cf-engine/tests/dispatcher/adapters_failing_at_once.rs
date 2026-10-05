//! Adapters that fail without waiting (Pi's `prepare` does not wait, a
//! missing executable among its errors): `await adapter.prepare(...)` still
//! took its turn, and the `catch` that writes the failure ran after it, so
//! every launch a pass began had begun its preparation before the first
//! failure was written. The turn is the engine's, not the fake's.

use std::rc::Rc;

use cf_engine::testing::Context;
use serde_json::Value;

use crate::traces::held_to;

const SUITES: &[&str] = &["adapters that fail without waiting"];

/// Where in the recorded effects `wanted` first holds, from `from`.
fn first(events: &[Value], from: usize, wanted: impl Fn(&Value) -> bool) -> usize {
    from + events[from..]
        .iter()
        .position(wanted)
        .expect("the event is there")
}

#[test]
fn begins_the_preparation_of_every_launch_of_a_pass_before_it_writes_the_failure_of_the_first() {
    let context = Context::new();
    let project = context.with_staff(&["zeus", "diana"]);
    context.give(project.id, "zeus", "Parser");
    context.give(project.id, "diana", "Lexer");
    // Nothing is written, nothing is waited for: the failure is the adapter's whole answer.
    *context.adapter.prepare.borrow_mut() = Some(Rc::new(|_| {
        Err("the settings could not be written".to_owned())
    }));
    context.pass().unwrap();
    assert_eq!(
        [1, 2].map(|number| context.task(project.id, number).task.state),
        ["failed", "failed"]
    );
    let events = context.recorder.events();
    let prepared = |launch: &'static str| {
        move |event: &Value| {
            event["method"] == "prepare" && event["args"][0]["launchId"].as_str() == Some(launch)
        }
    };
    let zeus = first(&events, 0, prepared("00000000-0000-4000-8000-000000000002"));
    let diana = first(
        &events,
        zeus,
        prepared("00000000-0000-4000-8000-000000000003"),
    );
    let failed = first(&events, zeus, |event| {
        event["event"]["kind"] == "delivery.failed"
    });
    assert!(
        diana < failed,
        "both launches were prepared before the first failure was written: {zeus}, {diana}, {failed}"
    );
    held_to(
        context.close(),
        SUITES,
        "begins the preparation of every launch of a pass before it writes the failure of the first",
    );
}

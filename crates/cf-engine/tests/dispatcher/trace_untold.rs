//! A window whose trace cannot be told: what the trace says of a window is
//! named by the project the ledger holds it in, and a ledger that cannot be
//! read fails the step or the operation that was telling it, as Node's
//! `#traceWindow` did, instead of a line naming no project and no participant.

use cf_engine::testing::Context;
use cf_engine::ActivityState;

use crate::fixtures::fail_next_read;
use crate::traces::held_to;
use crate::work_in_flight::{entries, looks_at};

const SUITES: &[&str] = &["a window whose trace cannot be told"];

#[test]
fn fails_the_close_of_a_window_the_pane_host_would_not_kill_and_traces_nothing_of_it() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    context.host.refuse_kills.set(true);
    fail_next_read(&context);
    let refused = context.close_project(project.id).unwrap_err();
    assert_eq!(refused.to_string(), "the ledger could not be read");
    assert!(
        context
            .dispatcher
            .pane(context.id(project.id, "chief"))
            .is_some(),
        "its window stays"
    );
    let told = entries(&context);
    assert!(
        told.iter().all(|line| line["kind"] != "window.kill_failed"),
        "no line says the kill failed, naming no project and no participant: {told:?}"
    );
    held_to(
        context.close(),
        SUITES,
        "fails the Close of a window the pane host would not kill, and traces nothing of it",
    );
}

#[test]
fn fails_the_step_of_a_window_whose_activity_changed_and_could_not_be_traced_and_the_activity_changed_all_the_same(
) {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    let chief = context.id(project.id, "chief");
    let launch = context.adapter.agent("chief").launch;
    let release = context.adapter.observe_holds.hold(looks_at(launch));
    let stepping = context.begin_pass();
    fail_next_read(&context);
    release.open();
    context.settle();
    let refused = stepping.take().unwrap().unwrap_err();
    assert_eq!(refused.to_string(), "the ledger could not be read");
    assert_eq!(
        context.dispatcher.activity(chief).state,
        ActivityState::Idle
    );
    held_to(
        context.close(),
        SUITES,
        "fails the step of a window whose activity changed and could not be traced, and the activity changed all the same",
    );
}

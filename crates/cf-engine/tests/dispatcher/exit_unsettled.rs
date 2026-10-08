//! A window whose exit cannot be settled: a Close that made the exit itself
//! (the kill is taken, its event has not come yet) fails with it, as Node's
//! `closeOwn` awaited the exit and rejected with it. The exit the pane host
//! sends has no caller to fail: it is told to the log.

use cf_engine::testing::Context;

use crate::fixtures::{fail_next_read, fail_next_write};
use crate::traces::held_to;

const SUITES: &[&str] = &["a window whose exit cannot be settled"];

#[test]
fn fails_the_close_that_made_its_exit_which_nothing_logs_the_exit_it_settles_is_its_own() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    // The kill is taken and its exit has not come yet: the Close settles the exit itself.
    context.host.hold_exits.set(true);
    fail_next_read(&context);
    let refused = context.close_project(project.id).unwrap_err();
    assert_eq!(refused.to_string(), "the ledger could not be read");
    assert_eq!(context.project(project.id).state, "suspended");
    assert_eq!(
        context.dispatcher.pane(context.id(project.id, "chief")),
        None,
        "its window has gone"
    );
    // The exit was the engine's own, and no longer: the chief's next window,
    // which closes by itself, closes the project as any chief's exit does.
    context.resume_project(project.id).unwrap();
    context.pass().unwrap();
    context.exit("chief");
    assert_eq!(
        context.project(project.id).state,
        "suspended",
        "the chief closed it"
    );
    held_to(
        context.close(),
        SUITES,
        "fails the Close that made its exit, which nothing logs: the exit it settles is its own",
    );
}

#[test]
fn fails_the_close_that_made_the_exit_of_a_members_window_when_the_pause_of_its_task_cannot_be_written(
) {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    context.host.hold_exits.set(true);
    fail_next_write(&context);
    let refused = context.close_project(project.id).unwrap_err();
    assert_eq!(refused.to_string(), "the ledger could not be written");
    assert_eq!(
        context.task(project.id, 1).task.state,
        "working",
        "its pause was not written"
    );
    assert_eq!(
        context.dispatcher.pane(context.id(project.id, "zeus")),
        None,
        "its window has gone"
    );
    held_to(
        context.close(),
        SUITES,
        "fails the Close that made the exit of a member's window when the pause of its task cannot be written",
    );
}

#[test]
fn tells_the_log_of_an_exit_the_pane_host_sent_that_it_could_not_settle() {
    // Nobody awaits an exit that comes from the host: its failure is written
    // down, as the failure of a launch or a delivery is, and the engine goes on.
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    fail_next_read(&context);
    context.exit("zeus");
    let written = std::mem::take(&mut *context.log.failures.borrow_mut());
    assert_eq!(written, ["the ledger could not be read"]);
    assert_eq!(
        context.dispatcher.pane(context.id(project.id, "zeus")),
        None,
        "the window has gone all the same"
    );
    // The next pass finds the window gone with its task in hand, and pauses it.
    context.pass().unwrap();
    assert_eq!(context.task(project.id, 1).task.state, "paused");
    context.close();
}

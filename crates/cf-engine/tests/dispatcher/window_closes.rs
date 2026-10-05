//! A window that closes is killed once, however its close comes about (a
//! launch that never showed its first message, a project closed while it goes
//! with its work, a member who leaves) and though its exit comes late.

use std::rc::Rc;

use cf_engine::testing::Context;
use cf_harness::contract::Pane;

use crate::fixtures::Tiers;
use crate::traces::held_to;

const SUITES: &[&str] = &["a window that closes"];

/// How often a window was killed.
fn kills(context: &Context, window: &Pane) -> usize {
    context
        .host
        .killed()
        .iter()
        .filter(|pane| pane.generation == window.generation)
        .count()
}

#[test]
fn is_killed_once_when_its_launch_never_showed_its_first_message_though_its_exit_comes_late() {
    let context = Context::new();
    context.host.hold_exits.set(true);
    let project = context.with_staff(&["zeus"]);
    // The window opens, but its record never shows the brief.
    let fake = Rc::downgrade(&context.adapter);
    *context.adapter.after_prepare.borrow_mut() = Some(Rc::new(move |handle| {
        if let Some(fake) = fake.upgrade() {
            fake.with(handle, |agent| agent.items.clear());
        }
    }));
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.advance(121_000);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.task(project.id, 1).task.state, "failed");
    assert_eq!(kills(&context, &context.host.last("zeus").unwrap().pane), 1);
    held_to(
        context.close(),
        SUITES,
        "is killed once when its launch never showed its first message, though its exit comes late",
    );
}

#[test]
fn is_killed_once_when_its_project_closes_while_it_already_goes_with_its_work() {
    let context = Context::new();
    context.host.hold_exits.set(true);
    let tiers = Tiers::new(&context, &["zeus"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    context.adapter.answer("zeus", "Parser done");
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "done");
    context.close_project(tiers.project.id).unwrap();
    assert_eq!(kills(&context, &context.host.last("zeus").unwrap().pane), 1);
    held_to(
        context.close(),
        SUITES,
        "is killed once when its project closes while it already goes with its work",
    );
}

#[test]
fn is_killed_once_when_its_member_leaves_the_staff_though_its_exit_comes_late() {
    let context = Context::new();
    context.host.hold_exits.set(true);
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    context.remove_member(project.id, "zeus").unwrap();
    context.pass().unwrap();
    assert_eq!(kills(&context, &context.host.last("zeus").unwrap().pane), 1);
    held_to(
        context.close(),
        SUITES,
        "is killed once when its member leaves the staff, though its exit comes late",
    );
}

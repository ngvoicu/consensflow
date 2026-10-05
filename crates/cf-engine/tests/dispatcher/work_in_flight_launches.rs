//! Launches in flight when their project is deleted and another project is
//! created meanwhile (`work in flight when its participant is forgotten`): a
//! chief's launch, held while it is prepared, its window opens or its window
//! starts, and a removal that waited for a member's launch, do nothing more by
//! the ids they took.

use std::cell::Cell;
use std::rc::Rc;

use cf_engine::testing::Context;
use cf_ledger::ConversationView;

use crate::chiefs::{chief_of, replace_project, replace_project_with, to_human};
use crate::fixtures::participant;
use crate::traces::held_to;

const SUITES: &[&str] = &["work in flight when its participant is forgotten"];

/// The chief's current conversation, as the ledger has it now.
fn conversation(context: &Context, chief: i64) -> ConversationView {
    context
        .ledger
        .borrow()
        .current_conversation(chief)
        .unwrap()
        .expect("the chief's conversation")
}

#[test]
fn opens_no_window_for_a_chief_whose_project_was_deleted_while_its_launch_was_prepared() {
    let context = Context::new();
    let release = context
        .adapter
        .prepare_holds
        .hold(|args| args[0]["directory"] == "/work/app");
    let old = context.with_staff(&["zeus"]);
    let replaced = replace_project(&context, &old);
    let chief = chief_of(&context, replaced.fresh.id).id;
    let own = conversation(&context, chief);
    release.open();
    context.settle();
    replaced.gone();
    context.settle();
    assert!(
        context
            .host
            .opened()
            .iter()
            .all(|open| open.cwd != "/work/app"),
        "nothing opens for the deleted project"
    );
    assert_eq!(
        conversation(&context, chief).id,
        own.id,
        "the new chief keeps its conversation"
    );
    held_to(
        context.close(),
        SUITES,
        "opens no window for a chief whose project was deleted while its launch was prepared",
    );
}

#[test]
fn tells_nobody_of_a_launch_that_failed_once_its_project_was_deleted_not_a_project_created_meanwhile(
) {
    let context = Context::new();
    let release = context.adapter.prepare_holds.hold_instead(
        |args| args[0]["directory"] == "/work/app",
        "the harness would not start".to_owned(),
    );
    let old = context.with_staff(&["zeus"]);
    let replaced = replace_project(&context, &old);
    release.open();
    context.settle();
    replaced.gone();
    context.settle();
    assert!(
        to_human(&context, replaced.fresh.id).is_empty(),
        "the new project's human hears nothing of it"
    );
    held_to(
        context.close(),
        SUITES,
        "tells nobody of a launch that failed once its project was deleted, not a project created meanwhile",
    );
}

#[test]
fn starts_no_conversation_for_a_chief_whose_project_is_deleted_while_its_window_opens_nor_for_a_project_created_meanwhile_and_that_window_closes(
) {
    let context = Context::new();
    let release = context
        .host
        .open_holds
        .hold(|args| args[0]["cwd"] == "/work/app");
    let old = context.with_staff(&["zeus"]);
    let replaced = replace_project(&context, &old);
    let chief = chief_of(&context, replaced.fresh.id).id;
    let own = conversation(&context, chief);
    release.open();
    context.settle();
    replaced.gone();
    context.settle();
    assert_eq!(
        conversation(&context, chief).id,
        own.id,
        "the new chief keeps its conversation"
    );
    let window = context
        .host
        .opened()
        .into_iter()
        .find(|open| open.cwd == "/work/app")
        .expect("the old window opened");
    assert!(
        context
            .host
            .killed()
            .iter()
            .any(|pane| pane.generation == window.pane.generation),
        "the old window closes"
    );
    held_to(
        context.close(),
        SUITES,
        "starts no conversation for a chief whose project is deleted while its window opens, nor for a project created meanwhile, and that window closes",
    );
}

#[test]
fn kills_no_window_of_a_deleted_projects_chief_that_exited_before_its_open_was_answered() {
    let context = Context::new();
    // The host sends the exit first, in the same read as its answer to the open.
    let release = context
        .host
        .open_holds
        .hold_instead(|args| args[0]["cwd"] == "/work/app", "chief".to_owned());
    let old = context.with_staff(&["zeus"]);
    let replaced = replace_project(&context, &old);
    release.open();
    context.settle();
    replaced.gone();
    context.settle();
    let window = context
        .host
        .opened()
        .into_iter()
        .find(|open| open.cwd == "/work/app")
        .expect("the old window opened");
    assert!(
        !context
            .host
            .killed()
            .iter()
            .any(|pane| pane.generation == window.pane.generation),
        "it had gone already"
    );
    held_to(
        context.close(),
        SUITES,
        "kills no window of a deleted project's chief that exited before its open was answered",
    );
}

#[test]
fn binds_no_conversation_of_a_deleted_projects_chief_or_of_a_project_created_meanwhile_to_the_thread_the_old_chiefs_window_named(
) {
    let context = Context::new();
    // The old chief's window takes long to come up, then names its thread.
    let calls = Rc::new(Cell::new(0));
    let counted = Rc::clone(&calls);
    let release = context.adapter.start_holds.hold_instead(
        move |_| {
            counted.set(counted.get() + 1);
            counted.get() == 1
        },
        "native-named".to_owned(),
    );
    let old = context.with_staff(&["zeus"]);
    let replaced = replace_project(&context, &old);
    let chief = chief_of(&context, replaced.fresh.id).id;
    let own = conversation(&context, chief);
    release.open();
    context.settle();
    replaced.gone();
    context.settle();
    assert_eq!(
        conversation(&context, chief).native_session,
        own.native_session,
        "the new chief's conversation keeps its own thread"
    );
    held_to(
        context.close(),
        SUITES,
        "binds no conversation, of a deleted project's chief or of a project created meanwhile, to the thread the old chief's window named",
    );
}

#[test]
fn takes_nobody_off_the_staff_for_a_removal_that_waited_while_its_project_was_deleted_not_a_member_of_a_project_created_meanwhile(
) {
    let context = Context::new();
    let old = context.with_staff(&["zeus"]);
    // zeus's window takes long to come up with its task, and the removal waits for it.
    let release = context
        .adapter
        .prepare_holds
        .hold(|args| args[0]["participant"]["handle"] == "zeus");
    context.give(old.id, "zeus", "Parser");
    context.pass().unwrap();
    let removed = context.begin_remove_member(old.id, "zeus");
    let replaced = replace_project_with(&context, &old, &["zeus"]);
    release.open();
    context.settle();
    replaced.gone();
    context.settle();
    assert!(
        participant(&context, replaced.fresh.id, "zeus").is_some(),
        "its zeus stays on the staff"
    );
    assert_eq!(
        removed.take().unwrap().unwrap_err().to_string(),
        "@zeus left the staff",
        "the removal says its zeus has left"
    );
    held_to(
        context.close(),
        SUITES,
        "takes nobody off the staff for a removal that waited while its project was deleted, not a member of a project created meanwhile",
    );
}

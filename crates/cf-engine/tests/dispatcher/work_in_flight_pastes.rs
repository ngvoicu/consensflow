//! Pastes in flight when their chief's project is deleted and another
//! project is created meanwhile (`work in flight when its participant is
//! forgotten`): the old chief's window, held while it gets ready for a paste
//! or while its harness takes one, settles nothing by what it did, and the
//! trace of the new project stays when the deleted one's windows close.

use std::rc::Rc;

use cf_engine::testing::Context;
use cf_harness::contract::Readiness;

use crate::chiefs::replace_project;
use crate::traces::held_to;
use crate::work_in_flight::{entries, looks_at};

const SUITES: &[&str] = &["work in flight when its participant is forgotten"];

#[test]
fn hands_nothing_to_the_old_chiefs_window_that_was_getting_ready_for_a_paste_once_its_project_is_deleted_nor_anything_of_a_project_created_meanwhile(
) {
    let context = Context::new();
    let old = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    // The old chief's window takes its time to be ready for a paste.
    *context.adapter.ready.borrow_mut() = Some(Rc::new(|| Ok(Readiness::Ready)));
    let launch = context.adapter.agent("chief").launch;
    let release = context.adapter.ready_holds.hold(looks_at(launch));
    let waiting = context.note(old.id, "zeus", "chief", "Waiting");
    context.pass().unwrap();
    let replaced = replace_project(&context, &old);
    let welcome = context.note_from_consensflow(replaced.fresh.id, "chief", "Welcome");
    assert_ne!(
        welcome.id, waiting.id,
        "the ledger never gives the old message's id again"
    );
    release.open();
    context.settle();
    replaced.gone();
    context.settle();
    assert_eq!(
        context.message(welcome.id).state,
        "queued",
        "it waits for its own window"
    );
    held_to(
        context.close(),
        SUITES,
        "hands nothing to the old chief's window that was getting ready for a paste once its project is deleted, nor anything of a project created meanwhile",
    );
}

#[test]
fn settles_nothing_by_what_the_old_chiefs_harness_did_with_a_paste_once_its_project_is_deleted_and_a_project_created_meanwhile_gets_its_message_once(
) {
    let context = Context::new();
    let old = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    // The old chief's harness takes long to refuse a paste.
    let before = context.adapter.agent("chief").launch;
    let release = context.adapter.deliver_holds.hold(looks_at(before.clone()));
    let refused = context.note(old.id, "zeus", "chief", "Refused");
    context.pass().unwrap();
    let replaced = replace_project(&context, &old);
    // Meanwhile the new chief is handed its first message.
    let welcome = context.note_from_consensflow(replaced.fresh.id, "chief", "Welcome");
    assert_ne!(
        welcome.id, refused.id,
        "the ledger never gives the old message's id again"
    );
    context.pass().unwrap();
    assert_eq!(context.message(welcome.id).state, "delivering");
    context
        .adapter
        .with_launch(&before, |agent| agent.admit = false);
    release.open();
    context.settle();
    replaced.gone();
    context.settle();
    assert_eq!(
        context.message(welcome.id).state,
        "delivering",
        "the old harness's refusal does not send it back"
    );
    context.pass().unwrap();
    let arrived = context.message(welcome.id);
    assert_eq!(
        (arrived.state.as_str(), arrived.attempts),
        ("delivered", 1),
        "it arrives, once"
    );
    held_to(
        context.close(),
        SUITES,
        "settles nothing by what the old chief's harness did with a paste once its project is deleted, and a project created meanwhile gets its message once",
    );
}

#[test]
fn keeps_the_trace_of_a_project_created_while_a_deleted_ones_windows_close_and_drops_the_deleted_ones(
) {
    let context = Context::new();
    // As the event file in the home: forgetting a project drops its lines.
    context.trace.forgets.set(true);
    let old = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    // The old chief takes a paste its harness holds: its window closes once that is over.
    let launch = context.adapter.agent("chief").launch;
    let release = context.adapter.deliver_holds.hold(looks_at(launch));
    context.note(old.id, "zeus", "chief", "Held");
    context.pass().unwrap();
    let replaced = replace_project(&context, &old);
    // The new chief's window is looked at meanwhile, and the trace says so.
    context.pass().unwrap();
    release.open();
    context.settle();
    replaced.gone();
    let fresh = replaced.fresh.id;
    let idle: Vec<_> = entries(&context)
        .into_iter()
        .filter(|line| line["kind"] == "window.activity" && line["project"] == fresh)
        .map(|line| line["state"].clone())
        .collect();
    assert_eq!(idle, ["idle"], "the new chief's line stays");
    let of_old: Vec<_> = entries(&context)
        .into_iter()
        .filter(|line| line["project"] == old.id)
        .collect();
    assert!(of_old.is_empty(), "and the deleted project's went");
    held_to(
        context.close(),
        SUITES,
        "keeps the trace of a project created while a deleted one's windows close, and drops the deleted one's",
    );
}

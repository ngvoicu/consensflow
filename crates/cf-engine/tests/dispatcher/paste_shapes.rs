//! The shapes a harness's record keeps a paste in, each of which proves the paste
//! arrived: as it was given, inside the wrapper Claude Code 2.1.296 puts a paste
//! in, and, for a member's fresh window, behind the brief it received, which its
//! launch gives first. Found on 2026-10-16 by the reviews of the stricter proof of
//! that day, which took every paste a real Claude records for lacking: a Claude
//! chief's result was pasted again. Held to no Node trace: Node's dispatcher took
//! any item that held the header.

use cf_engine::delivery_text::header_id;
use cf_engine::testing::Context;
use cf_harness::records::Role;

use crate::fixtures::launches_of;

/// How many messages the chief's window was given that say `words`.
fn given_saying(context: &Context, words: &str) -> usize {
    context
        .adapter
        .agent("chief")
        .items
        .iter()
        .filter(|item| item.role == Role::User && item.text.contains(words))
        .count()
}

#[test]
fn confirms_a_paste_claude_code_recorded_in_its_wrapper_and_pastes_it_once() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    context.adapter.wraps_pastes.set(true);
    let note = context.note(project.id, "zeus", "chief", "Taken");
    context.pass().unwrap();
    let recorded = context
        .adapter
        .agent("chief")
        .items
        .last()
        .unwrap()
        .text
        .clone();
    assert!(
        recorded.starts_with("\n\n<pasted_content id="),
        "{recorded}"
    );
    context.pass().unwrap();
    // The chief answers, and its window is at rest past the arrival window.
    context.adapter.answer("chief", "Noted");
    context.advance(60_000);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(
        given_saying(&context, "Taken"),
        1,
        "the chief was given it once"
    );
    assert_eq!(context.message(note.id).state, "delivered");
}

#[test]
fn confirms_the_first_message_a_fresh_window_was_given_behind_the_brief_it_received() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    let zeus = context.id(project.id, "zeus");
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    // The quit cut zeus's turn, and its harness kept nothing of the conversation: its
    // fresh window is given the brief it received, then the note that the quit cut it.
    context.adapter.holds_nothing("zeus");
    context.ledger.borrow_mut().suspend_for_restart().unwrap();
    let after = context.make();
    after.settle_after_restart().unwrap();
    after.resume_project(project.id).unwrap();
    let first = context
        .ledger
        .borrow()
        .in_flight()
        .unwrap()
        .into_iter()
        .find(|message| message.recipient_id == zeus)
        .unwrap();
    let launch = launches_of(&context, "zeus").pop().unwrap();
    let given = launch["message"].as_str().unwrap();
    assert_ne!(header_id(given), Some(first.id), "the brief first: {given}");

    after.pass().unwrap();
    assert_eq!(
        context.message(first.id).state,
        "delivered",
        "its paste is the one behind the brief"
    );
}

#[test]
fn a_start_confirms_the_first_message_a_fresh_window_was_given_behind_its_brief_and_never_pastes_it_again(
) {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    let zeus = context.id(project.id, "zeus");
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    // Zeus's window goes, and its harness keeps nothing of the conversation; the chief
    // resumes T-1, and a fresh window is given the brief, then the words that resume it.
    context.adapter.holds_nothing("zeus");
    context.exit("zeus");
    assert_eq!(context.task(project.id, 1).task.state, "paused");
    context
        .ledger
        .borrow_mut()
        .resume_task(project.id, 1, Some("chief"), "Go on")
        .unwrap();
    context.pass().unwrap();
    let first = context
        .ledger
        .borrow()
        .in_flight()
        .unwrap()
        .into_iter()
        .find(|message| message.recipient_id == zeus)
        .unwrap();
    let launch = launches_of(&context, "zeus").pop().unwrap();
    let given = launch["message"].as_str().unwrap();
    assert_ne!(header_id(given), Some(first.id), "the brief first: {given}");

    // The daemon stops before a look confirms it, and starts again.
    context.ledger.borrow_mut().suspend_for_restart().unwrap();
    let after = context.make();
    after.settle_after_restart().unwrap();
    assert_eq!(
        context.message(first.id).state,
        "delivered",
        "the record shows it behind the brief"
    );
}

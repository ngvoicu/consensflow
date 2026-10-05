//! What a restart does with a message that was on its way into a window
//! (`describe('a restart while a message is on its way')`): given back to
//! the queue with its attempt, or settled by a record that shows it, and
//! never sent again that the harness took.

use std::rc::Rc;

use cf_engine::delivery_text::delivery_text;
use cf_engine::testing::Context;
use cf_harness::records::Role;
use serde_json::json;

use crate::fixtures::assert_match;
use crate::traces::held_to;

const SUITES: &[&str] = &["a restart while a message is on its way"];

/// The words the window was given, after the header's line: what each
/// message of the chief's said.
fn given(context: &Context, handle: &str) -> Vec<String> {
    let items = context.adapter.agent(handle).items;
    items
        .iter()
        .filter(|item| item.role == Role::User)
        .map(|item| item.text.split('\n').nth(1).unwrap_or_default().to_owned())
        .collect()
}

#[test]
fn gives_a_message_its_window_never_showed_back_to_the_queue_with_its_attempt_and_the_chief_comes_back_to_it_and_to_what_follows(
) {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    context.adapter.with("chief", |agent| agent.arrive = false);
    let one = context.note(project.id, "zeus", "chief", "One");
    context.pass().unwrap();
    assert_eq!(context.message(one.id).state, "delivering");

    // The app quits with it on its way: the daemon stops before the window does.
    context.ledger.borrow_mut().suspend_for_restart().unwrap();
    let after = context.make();
    after.resume_after_restart().unwrap();
    let queued = context.message(one.id);
    assert_eq!(
        (queued.state.as_str(), queued.attempts),
        ("queued", 0),
        "its window died before it landed: the attempt comes back"
    );
    let two = context.note(project.id, "zeus", "chief", "Two");
    after.pass().unwrap();
    after.pass().unwrap();
    context.adapter.answer("chief", "Read one.");
    after.pass().unwrap();
    after.pass().unwrap();
    assert_eq!(
        (context.message(one.id).state, context.message(two.id).state),
        ("delivered".to_owned(), "delivered".to_owned())
    );
    assert_eq!(given(&context, "chief"), ["One", "Two"]);
    held_to(
        context.close(),
        SUITES,
        "gives a message its window never showed back to the queue with its attempt, and the chief comes back to it and to what follows",
    );
}

#[test]
fn sends_nothing_again_that_its_harness_took_though_the_copy_of_its_window_missed_it() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    let chief = context.id(project.id, "chief");
    context.pass().unwrap();
    let note = context.note(project.id, "zeus", "chief", "Taken");
    context.pass().unwrap();
    // The harness took it, and its own record has it; the daemon stopped
    // before its next look copied the window, so the copy does not.
    assert_eq!(context.message(note.id).state, "delivering");
    let copied = context
        .ledger
        .borrow()
        .copied_item_with(chief, &format!("m-{}", note.id))
        .unwrap();
    assert_eq!(copied, None);
    let items = context.adapter.agent("chief").items;
    let taken = items
        .iter()
        .find(|item| item.role == Role::User && item.text.contains("Taken"))
        .unwrap();

    context.ledger.borrow_mut().suspend_for_restart().unwrap();
    let after = context.make();
    after.resume_after_restart().unwrap();
    let settled = context.message(note.id);
    assert_eq!(settled.state, "delivered");
    assert_eq!(settled.receipt, json!({ "item": &*taken.id }));
    after.pass().unwrap();
    after.pass().unwrap();
    let again = context
        .adapter
        .agent("chief")
        .items
        .iter()
        .filter(|item| item.role == Role::User && item.text.contains("Taken"))
        .count();
    assert_eq!(again, 0, "the window that came back is not given it again");
    held_to(
        context.close(),
        SUITES,
        "sends nothing again that its harness took, though the copy of its window missed it",
    );
}

#[test]
fn confirms_a_message_whose_header_the_copy_of_its_window_already_shows() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    let chief = context.id(project.id, "chief");
    context.pass().unwrap();
    context.adapter.with("chief", |agent| agent.arrive = false);
    let note = context.note(project.id, "zeus", "chief", "Shown");
    context.pass().unwrap();
    // The window showed it and the copy has it; the daemon stopped before it confirmed it.
    let shown = context
        .adapter
        .item(Role::User, &delivery_text(&context.message(note.id)));
    let conversation = context
        .ledger
        .borrow()
        .current_conversation(chief)
        .unwrap()
        .unwrap();
    let written = serde_json::to_value(&shown).unwrap();
    context
        .ledger
        .borrow_mut()
        .copy_transcript(conversation.id, &[written], 0)
        .unwrap();

    context.ledger.borrow_mut().suspend_for_restart().unwrap();
    context.make().resume_after_restart().unwrap();
    let settled = context.message(note.id);
    assert_eq!(settled.state, "delivered");
    assert_eq!(settled.receipt, json!({ "item": &*shown.id }));
    held_to(
        context.close(),
        SUITES,
        "confirms a message whose header the copy of its window already shows",
    );
}

#[test]
fn opens_a_members_session_again_on_its_own_conversation_with_the_brief_it_was_opened_for() {
    let context = Context::new();
    let project = context.with_tiers(&["zeus", "diana"]);
    context.pool_task(project.id, "worker", "Write the parser");
    context.pass().unwrap();
    let session = context.id(project.id, "zeus-amber-pine");
    let native = context
        .ledger
        .borrow()
        .current_conversation(session)
        .unwrap()
        .unwrap()
        .native_session
        .unwrap();
    // The window opened, but its record never showed the brief before the stop.
    context.adapter.with("zeus", |agent| agent.items.clear());

    context.ledger.borrow_mut().suspend_for_restart().unwrap();
    let after = context.make();
    after.resume_after_restart().unwrap();
    after.pass().unwrap();
    let launch = context.adapter.prepared().pop().unwrap();
    assert_eq!(
        (
            launch["participant"]["handle"].clone(),
            launch["resume"].clone()
        ),
        (json!("zeus-amber-pine"), json!(native))
    );
    assert_match(
        launch["message"].as_str().unwrap(),
        r"^\[ConsensFlow m-\d+ · T-1 · task from @chief\]\nWrite the parser$",
    );
    after.pass().unwrap();
    assert_eq!(context.task(project.id, 1).task.state, "working");
    held_to(
        context.close(),
        SUITES,
        "opens a member's session again on its own conversation with the brief it was opened for",
    );
}

#[test]
fn gives_a_paste_back_to_the_queue_when_its_window_closes_while_the_harness_takes_it() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    // The window closes after the harness has taken the paste, before it answers.
    let host = Rc::clone(&context.host);
    *context.adapter.deliver.borrow_mut() = Some(Rc::new(move |taking| {
        let host = Rc::clone(&host);
        Box::pin(async move {
            let outcome = taking();
            host.exit("chief").await;
            Ok(outcome)
        })
    }));
    let note = context.note(project.id, "zeus", "chief", "Lost");
    context.pass().unwrap();
    assert_eq!(
        context.message(note.id).state,
        "queued",
        "not left on its way"
    );

    *context.adapter.deliver.borrow_mut() = None;
    context.resume_project(project.id).unwrap();
    context.pass().unwrap();
    assert_eq!(context.message(note.id).state, "delivering");
    context.pass().unwrap();
    assert_eq!(context.message(note.id).state, "delivered");
    held_to(
        context.close(),
        SUITES,
        "gives a paste back to the queue when its window closes while the harness takes it",
    );
}

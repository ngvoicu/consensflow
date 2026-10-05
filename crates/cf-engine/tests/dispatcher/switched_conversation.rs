//! A window the human switches to another conversation (/clear, /new,
//! /resume): the conversation it shows becomes the session's
//! (`describe('a window the human switches to another conversation')`).

use cf_engine::testing::Context;
use cf_ledger::ConversationView;

use crate::fixtures::assert_match;
use crate::traces::held_to;

const SUITES: &[&str] = &["a window the human switches to another conversation"];

/// The conversation a participant is on now.
fn current(context: &Context, participant: i64) -> ConversationView {
    context
        .ledger
        .borrow()
        .current_conversation(participant)
        .unwrap()
        .expect("a conversation")
}

#[test]
fn follows_it_the_conversation_it_shows_becomes_the_sessions_and_deliveries_go_and_count_there() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    let chief = context.id(project.id, "chief");
    context.pass().unwrap();
    let first = current(&context, chief);
    // A note is pasted, and before the next look the human types /clear.
    let one = context.note(project.id, "zeus", "chief", "One");
    context.pass().unwrap();
    context.adapter.switch_to("chief", "native-cleared");
    context.pass().unwrap();
    assert_eq!(
        context.message(one.id).state,
        "delivered",
        "the record it went to showed it"
    );
    let cleared = current(&context, chief);
    assert_eq!(
        (cleared.native_session.as_deref(), cleared.harness.as_str()),
        (Some("native-cleared"), "claude-code")
    );
    let history = context.ledger.borrow().chief_history(project.id).unwrap();
    let earlier = history
        .iter()
        .find(|conversation| conversation.conversation.id == first.id)
        .expect("the first conversation");
    assert_eq!(
        earlier.items.len(),
        1,
        "the first conversation ended with its copy"
    );

    let two = context.note(project.id, "zeus", "chief", "Two");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.message(two.id).state, "delivered");
    assert_match(
        &context.adapter.agent("chief").items.last().unwrap().text,
        r"note from @zeus\]\nTwo$",
    );
    assert_eq!(
        context.adapter.agent("chief").items.len(),
        1,
        "in the conversation it shows"
    );

    // /resume back to the first: it is the chief's conversation again.
    context
        .adapter
        .switch_to("chief", first.native_session.as_deref().unwrap());
    context.pass().unwrap();
    assert_eq!(current(&context, chief).id, first.id);
    held_to(
        context.close(),
        SUITES,
        "follows it: the conversation it shows becomes the session’s, and deliveries go and count there",
    );
}

//! What the engine asks of the queue for the note that says a conversation its
//! role again (`messages/delivery.rs`): it goes ahead of the rest of the queue
//! and is asked of by its id, by every rule the head of the queue goes by; and
//! the write that confirms it records what the conversation was told in the same
//! step, so that neither stands without the other.

use cf_ledger::{MessageView, NewNote};
use serde_json::{json, Value};

use crate::fixture::{world, World};

/// A note about no task, from `from` (ConsensFlow itself where none is named).
fn note(w: &mut World, from: Option<&str>, to: &str, body: &str) -> MessageView {
    w.ledger
        .note(
            w.project,
            &NewNote {
                from: from.map(str::to_owned),
                to: to.to_owned(),
                task: None,
                body: body.to_owned(),
            },
        )
        .expect("a note")
}

/// The message of `recipient`'s queue that `message` is asked of as, if it may go.
fn asked_of(w: &World, recipient: &str, message: i64) -> Option<i64> {
    w.ledger
        .next_delivery_of(w.id(recipient), message)
        .expect("a read")
        .map(|found| found.id)
}

#[test]
fn a_message_behind_others_in_a_chief_s_queue_may_go_ahead_of_them_by_the_rules_the_head_goes_by() {
    let mut w = world();
    let first = note(&mut w, Some("human"), "chief", "Any news?");
    let second = note(&mut w, None, "chief", "Your role has changed");
    assert_eq!(
        w.next("chief"),
        Some(first.id),
        "the queue's head is the older"
    );
    assert_eq!(asked_of(&w, "chief", first.id), Some(first.id));
    assert_eq!(
        asked_of(&w, "chief", second.id),
        Some(second.id),
        "the one behind it may go ahead, as the head would"
    );
    assert_eq!(
        asked_of(&w, "zeus", second.id),
        None,
        "it is the chief's, and no other's"
    );

    // What is on its way blocks every other, as it blocks the head.
    w.begin(first.id);
    assert_eq!(asked_of(&w, "chief", second.id), None);
    w.confirm(first.id);
    assert_eq!(
        asked_of(&w, "chief", first.id),
        None,
        "delivered: nothing to go"
    );
    assert_eq!(asked_of(&w, "chief", second.id), Some(second.id));

    // And one that no longer applies goes no more.
    w.ledger
        .cancel_message(second.id, "withdrawn")
        .expect("cancelled");
    assert_eq!(asked_of(&w, "chief", second.id), None);
}

#[test]
fn a_note_about_no_task_is_never_the_head_of_a_member_s_queue_and_a_brief_of_its_task_is() {
    let mut w = world();
    let given = w.give("zeus", "Parser");
    let brief = given.message.expect("its brief").id;
    let told = note(&mut w, None, "zeus", "Your role has changed");
    assert_eq!(w.next("zeus"), Some(brief));
    assert_eq!(
        asked_of(&w, "zeus", told.id),
        None,
        "a member's queue holds the words of its tasks, so the engine gives this one itself"
    );
    assert_eq!(asked_of(&w, "zeus", brief), Some(brief));

    // A brief behind a task-less note, and the note first by its id: the brief may still go.
    let mut w = world();
    let told = note(&mut w, None, "zeus", "Your role has changed");
    let given = w.give("zeus", "Parser");
    let brief = given.message.expect("its brief").id;
    assert!(told.id < brief);
    assert_eq!(w.next("zeus"), Some(brief), "the note is passed over");
    assert_eq!(asked_of(&w, "zeus", brief), Some(brief));
}

#[test]
fn a_note_that_says_a_conversation_its_role_is_confirmed_and_the_role_recorded_in_one_write() {
    let mut w = world();
    let chief = w.id("chief");
    let conversation = w
        .ledger
        .start_conversation(chief, "codex")
        .expect("a conversation");
    w.ledger
        .set_conversation_role_text(conversation.id, "the role it began with")
        .expect("written");
    let said = note(&mut w, None, "chief", "Your role has changed");
    w.begin(said.id);

    let confirmed = w
        .ledger
        .confirm_delivery_telling(
            said.id,
            Some(&json!({ "item": "i-1" })),
            conversation.id,
            "the role it was told",
        )
        .expect("confirmed");
    assert_eq!(confirmed.state, "delivered");
    assert_eq!(confirmed.receipt, json!({ "item": "i-1" }));
    assert_eq!(
        w.ledger
            .conversation_role_text(conversation.id)
            .expect("read")
            .as_deref(),
        Some("the role it was told")
    );
}

#[test]
fn a_note_is_not_confirmed_where_the_role_cannot_be_recorded_and_the_role_is_not_recorded_where_the_note_cannot_be_confirmed(
) {
    let mut w = world();
    let chief = w.id("chief");
    let conversation = w
        .ledger
        .start_conversation(chief, "codex")
        .expect("a conversation");
    w.ledger
        .set_conversation_role_text(conversation.id, "the role it began with")
        .expect("written");
    let said = note(&mut w, None, "chief", "Your role has changed");
    w.begin(said.id);
    let events = w.ledger.events(w.project, 0, 500).expect("events").len();

    // A conversation that is not there: the confirmation goes with the refusal.
    let refused = w
        .ledger
        .confirm_delivery_telling(said.id, Some(&json!({ "item": "i-1" })), 4242, "the role")
        .expect_err("no such conversation");
    assert_eq!(refused.code(), Some("unknown-conversation"));
    let still = w.message(said.id);
    assert_eq!(still.state, "delivering", "not confirmed either");
    assert_eq!(still.receipt, Value::Null);
    assert_eq!(
        w.ledger.events(w.project, 0, 500).expect("events").len(),
        events,
        "and no event says it was"
    );
    assert_eq!(
        w.ledger
            .conversation_role_text(conversation.id)
            .expect("read")
            .as_deref(),
        Some("the role it began with")
    );

    // A note that is not on its way: the role is not recorded as told.
    let waiting = note(&mut w, None, "chief", "Your role has changed again");
    w.confirm(said.id);
    let refused = w
        .ledger
        .confirm_delivery_telling(
            waiting.id,
            None,
            conversation.id,
            "the role it was not told",
        )
        .expect_err("it was never begun");
    assert_eq!(refused.code(), Some("invalid-transition"));
    assert_eq!(w.message(waiting.id).state, "queued");
    assert_eq!(
        w.ledger
            .conversation_role_text(conversation.id)
            .expect("read")
            .as_deref(),
        Some("the role it began with"),
        "what the conversation was told is what it was"
    );
}

//! What the engine asks of the queue for the note that says a conversation its
//! role again (`messages/delivery.rs`): it goes ahead of the rest of the queue
//! and is asked of by its id, by every rule the head of the queue goes by; and
//! the write that confirms it records what the conversation was told in the same
//! step, so that neither stands without the other. The ledger knows such a note
//! by the role text it holds, and no other message by its words.

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

/// A note from ConsensFlow to `to` that says its conversation `role`.
fn role_note(w: &mut World, to: &str, body: &str, role: &str) -> MessageView {
    w.ledger
        .note_role(w.project, to, body, role)
        .expect("a note that says a role")
}

/// A conversation of `handle`'s window, begun with `text` for its role.
fn conversation(w: &mut World, handle: &str, text: &str) -> i64 {
    let participant = w.id(handle);
    let conversation = w
        .ledger
        .start_conversation(participant, "codex")
        .expect("a conversation");
    w.ledger
        .set_conversation_role_text(conversation.id, text)
        .expect("written");
    conversation.id
}

/// What `conversation` was last told.
fn told(w: &World, conversation: i64) -> Option<String> {
    w.ledger.conversation_role_text(conversation).expect("read")
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
    // An earlier conversation that ended, and the one the window is in now.
    let earlier = conversation(&mut w, "chief", "the role of an earlier conversation");
    w.ledger.end_conversation(earlier).expect("ended");
    let current = conversation(&mut w, "chief", "the role it began with");
    let said = role_note(
        &mut w,
        "chief",
        "Your role has changed",
        "the role it was told",
    );
    w.begin(said.id);

    let confirmed = w
        .ledger
        .confirm_delivery_telling(said.id, Some(&json!({ "item": "i-1" })))
        .expect("confirmed");
    assert_eq!(confirmed.state, "delivered");
    assert_eq!(confirmed.receipt, json!({ "item": "i-1" }));
    assert_eq!(
        told(&w, current).as_deref(),
        Some("the role it was told"),
        "what the window was told, which is its conversation's now"
    );
    assert_eq!(
        told(&w, earlier).as_deref(),
        Some("the role of an earlier conversation"),
        "and no earlier one's"
    );
}

#[test]
fn a_note_is_not_confirmed_where_the_role_cannot_be_recorded_and_the_role_is_not_recorded_where_the_note_cannot_be_confirmed(
) {
    let mut w = world();
    let current = conversation(&mut w, "chief", "the role it began with");
    let said = role_note(
        &mut w,
        "chief",
        "Your role has changed",
        "the role it was told",
    );
    w.begin(said.id);
    let events = w.ledger.events(w.project, 0, 500).expect("events").len();

    // The conversation's role cannot be written (the file refuses it): the
    // confirmation goes with the refusal.
    w.edit(|db| {
        db.execute_batch(
            "CREATE TRIGGER refuses_a_role BEFORE UPDATE OF role_text ON conversation
             BEGIN SELECT RAISE(ABORT, 'no role may be written'); END;",
        )
        .expect("a trigger");
    });
    w.ledger
        .confirm_delivery_telling(said.id, Some(&json!({ "item": "i-1" })))
        .expect_err("the role cannot be recorded");
    let still = w.message(said.id);
    assert_eq!(still.state, "delivering", "not confirmed either");
    assert_eq!(still.receipt, Value::Null);
    assert_eq!(
        w.ledger.events(w.project, 0, 500).expect("events").len(),
        events,
        "and no event says it was"
    );
    assert_eq!(told(&w, current).as_deref(), Some("the role it began with"));

    // A note that is not on its way: the role is not recorded as told.
    let waiting = role_note(
        &mut w,
        "chief",
        "Your role has changed again",
        "the role it was not told",
    );
    let refused = w
        .ledger
        .confirm_delivery_telling(waiting.id, None)
        .expect_err("it was never begun");
    assert_eq!(refused.code(), Some("invalid-transition"));
    assert_eq!(w.message(waiting.id).state, "queued");
    assert_eq!(
        told(&w, current).as_deref(),
        Some("the role it began with"),
        "what the conversation was told is what it was"
    );
}

#[test]
fn a_message_that_says_no_role_is_confirmed_as_any_is_and_a_note_with_no_conversation_to_tell_is_confirmed_alone(
) {
    let mut w = world();
    let current = conversation(&mut w, "chief", "the role it began with");
    let plain = note(&mut w, Some("human"), "chief", "Any news?");
    w.begin(plain.id);
    let confirmed = w
        .ledger
        .confirm_delivery_telling(plain.id, Some(&json!({ "item": "i-1" })))
        .expect("confirmed");
    assert_eq!(confirmed.state, "delivered");
    assert_eq!(
        told(&w, current).as_deref(),
        Some("the role it began with"),
        "it said no role"
    );

    // The window's conversation ended while the note was on its way.
    let said = role_note(
        &mut w,
        "chief",
        "Your role has changed",
        "the role it was told",
    );
    w.begin(said.id);
    w.ledger.end_conversation(current).expect("ended");
    let confirmed = w
        .ledger
        .confirm_delivery_telling(said.id, Some(&json!({ "item": "i-2" })))
        .expect("confirmed");
    assert_eq!(confirmed.state, "delivered");
    assert_eq!(
        told(&w, current).as_deref(),
        Some("the role it began with"),
        "there was no conversation to have told"
    );
}

#[test]
fn the_ledger_knows_a_note_that_says_a_role_by_what_it_holds_and_no_other_by_its_words() {
    let mut w = world();
    let opening = "Your ConsensFlow role instructions have changed since this conversation began.";
    let said = role_note(
        &mut w,
        "chief",
        &format!("{opening}\n\nthe role"),
        "the role",
    );
    // The same words, from ConsensFlow itself and from a participant; and a
    // note of ConsensFlow's that says no role.
    let lookalike = note(&mut w, None, "chief", &format!("{opening}\n\nthe role"));
    let quoted = note(
        &mut w,
        Some("zeus"),
        "chief",
        &format!("Did you mean: {opening}"),
    );
    let hand_off = note(&mut w, None, "chief", "You are the chief now.");
    let chief = w.id("chief");
    assert_eq!(
        w.ledger.role_notes_to(chief).expect("read"),
        [said.id],
        "{:?}",
        [lookalike.id, quoted.id, hand_off.id]
    );
    assert_eq!(
        w.ledger.role_notes_to(w.id("zeus")).expect("read"),
        Vec::<i64>::new()
    );

    // Whatever becomes of it, the note stays one: it is the identity of a
    // message whose words a conversation holds, long after.
    w.ledger
        .cancel_message(said.id, "withdrawn")
        .expect("cancelled");
    let second = role_note(
        &mut w,
        "chief",
        "Your role has changed again",
        "the role again",
    );
    assert_eq!(
        w.ledger.role_notes_to(chief).expect("read"),
        [said.id, second.id]
    );
}

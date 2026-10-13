//! Whether a conversation had a turn as ConsensFlow has seen it
//! (`Ledger::had_a_turn`), which decides if a window is opened on it again or
//! afresh where its harness's own record cannot say: its copy holds a message of
//! the user's. The copy is of the conversation it was made of and no other's, and
//! what was delivered is no turn of it until a look has copied it: a delivery
//! carries no conversation, only a participant and a time, and the time of one
//! conversation's end and the next one's beginning can be the same.

use cf_ledger::{Ledger, NewNote};
use serde_json::json;

use crate::fixture::{frozen_world, world, World};

/// A conversation of `handle`'s, begun now.
fn conversation(w: &mut World, handle: &str) -> i64 {
    let participant = w.id(handle);
    w.ledger
        .start_conversation(participant, "claude-code")
        .expect("a conversation")
        .id
}

/// What the window wrote of `role`, copied.
fn copy(ledger: &mut Ledger, conversation: i64, role: &str) {
    ledger
        .copy_transcript(
            conversation,
            &[json!({ "id": format!("i-{role}"), "role": role, "text": "words" })],
            0,
        )
        .expect("copied");
}

/// Whether the ledger says the conversation had a turn.
fn had_a_turn(w: &World, conversation: i64) -> bool {
    w.ledger.had_a_turn(conversation).expect("asked")
}

#[test]
fn a_conversation_with_nothing_copied_had_no_turn_and_an_unknown_one_is_refused() {
    let mut w = world();
    let chief = conversation(&mut w, "chief");
    assert!(!had_a_turn(&w, chief));
    let refused = w.ledger.had_a_turn(chief + 100).unwrap_err();
    assert_eq!(refused.code(), Some("unknown-conversation"));
}

#[test]
fn a_message_of_the_users_in_the_copy_is_a_turn_whoever_wrote_it_and_what_else_a_harness_writes_is_none(
) {
    let mut w = world();
    let chief = conversation(&mut w, "chief");
    // What a harness writes of itself, and what its tools and its agent say, with no
    // message of the user's before it: no turn.
    for role in ["custom", "tool", "assistant"] {
        copy(&mut w.ledger, chief, role);
        assert!(!had_a_turn(&w, chief), "{role}");
    }
    // What the human typed into the window, or ConsensFlow delivered into it.
    copy(&mut w.ledger, chief, "user");
    assert!(had_a_turn(&w, chief));
}

#[test]
fn the_copy_of_another_conversation_is_no_turn_of_this_one() {
    let mut w = world();
    let first = conversation(&mut w, "chief");
    let diana = conversation(&mut w, "diana");
    copy(&mut w.ledger, first, "user");
    assert!(had_a_turn(&w, first));
    assert!(!had_a_turn(&w, diana), "another participant's");
    // The window is opened afresh, or the human typed /new: a conversation of its own.
    let second = conversation(&mut w, "chief");
    assert!(!had_a_turn(&w, second), "the one before it");
    assert!(had_a_turn(&w, first), "and it stays one of that one's");
}

#[test]
fn what_was_delivered_into_the_conversation_before_is_no_turn_of_the_one_that_began_in_the_same_millisecond(
) {
    // On a clock that never moves, the delivery is confirmed in the very millisecond
    // the next conversation begins: a time cannot say which of them it was for.
    let mut w = frozen_world();
    let first = conversation(&mut w, "chief");
    let note = w
        .ledger
        .note(
            w.project,
            &NewNote {
                from: None,
                to: "chief".into(),
                body: "Any news?".into(),
                task: None,
            },
        )
        .expect("a note")
        .id;
    w.deliver(note);
    copy(&mut w.ledger, first, "user");
    let second = conversation(&mut w, "chief");
    assert!(had_a_turn(&w, first));
    assert!(!had_a_turn(&w, second));
}

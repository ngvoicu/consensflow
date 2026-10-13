//! Whether a conversation had a turn (`Ledger::had_a_turn`), which is what
//! decides if a window is opened on it again or afresh: ConsensFlow's copy of
//! it holds an item, or a message was delivered to its participant since it
//! began. A message that was queued, on its way, given up or withdrawn is no
//! turn, nor is one delivered to anyone else, or before the conversation began.

use cf_ledger::{NewNote, Read};
use serde_json::json;

use crate::fixture::{world, World};

/// A conversation of `handle`'s, begun now.
fn conversation(w: &mut World, handle: &str) -> i64 {
    let participant = w.id(handle);
    w.ledger
        .start_conversation(participant, "claude-code")
        .expect("a conversation")
        .id
}

/// Whether the ledger says the conversation had a turn.
fn had_a_turn(w: &World, conversation: i64) -> bool {
    w.ledger.had_a_turn(conversation).expect("asked")
}

/// A note from ConsensFlow to `to`, queued: its id.
fn note_to(w: &mut World, to: &str) -> i64 {
    w.ledger
        .note(
            w.project,
            &NewNote {
                from: None,
                to: to.into(),
                body: "Any news?".into(),
                task: None,
            },
        )
        .expect("a note")
        .id
}

#[test]
fn a_conversation_with_nothing_copied_and_nothing_delivered_had_no_turn_and_an_unknown_one_is_refused(
) {
    let mut w = world();
    let chief = conversation(&mut w, "chief");
    assert!(!had_a_turn(&w, chief));
    let refused = w.ledger.had_a_turn(chief + 100).unwrap_err();
    assert_eq!(refused.code(), Some("unknown-conversation"));
}

#[test]
fn an_item_in_the_copy_is_a_turn_whoever_wrote_it() {
    let mut w = world();
    let chief = conversation(&mut w, "chief");
    // What the human typed into the window: no message of ConsensFlow's, and the copy holds it.
    w.ledger
        .copy_transcript(
            chief,
            &[json!({ "id": "i-1", "role": "user", "text": "Hello" })],
            0,
        )
        .expect("copied");
    assert!(had_a_turn(&w, chief));
    let other = conversation(&mut w, "diana");
    assert!(
        !had_a_turn(&w, other),
        "the copy of another is no turn of this"
    );
    // Whatever its role: the harness wrote a record of it, which is what a window resumes.
    w.ledger
        .copy_transcript(
            other,
            &[json!({ "id": "i-2", "role": "custom", "text": "" })],
            0,
        )
        .expect("copied");
    assert!(had_a_turn(&w, other));
}

#[test]
fn a_message_delivered_to_its_participant_since_the_conversation_began_is_a_turn() {
    let mut w = world();
    let chief = conversation(&mut w, "chief");
    let note = note_to(&mut w, "chief");
    assert!(!had_a_turn(&w, chief), "queued");
    w.begin(note);
    assert!(!had_a_turn(&w, chief), "on its way");
    w.confirm(note);
    assert!(
        had_a_turn(&w, chief),
        "its record showed it: with no item in the copy"
    );
}

#[test]
fn an_answer_cf_served_whole_is_a_turn_too() {
    let mut w = world();
    let created = w.give("zeus", "Parser");
    let (number, brief) = (created.task.number, created.message.expect("a brief").id);
    w.deliver(brief);
    let question = w.ask("zeus", number, "Which dialect?").id;
    let answer = w.answer(question, "ANSI").id;
    // The window comes back on a conversation begun after the brief was delivered.
    let zeus = conversation(&mut w, "zeus");
    assert!(!had_a_turn(&w, zeus));

    let participant = w.id("zeus");
    w.ledger
        .receive_read(participant, &[answer], Read::Inbox)
        .expect("read");
    assert_eq!(w.message(answer).state, "read");
    assert!(had_a_turn(&w, zeus), "it ran `cf inbox read`");
}

#[test]
fn a_message_that_was_given_up_or_withdrawn_is_no_turn() {
    let mut w = world();
    let chief = conversation(&mut w, "chief");
    let failed = note_to(&mut w, "chief");
    w.begin(failed);
    w.ledger
        .fail_delivery(failed, "the window never showed it")
        .expect("given up");
    let cancelled = note_to(&mut w, "chief");
    w.ledger
        .cancel_message(cancelled, "it no longer applies")
        .expect("withdrawn");
    let back = note_to(&mut w, "chief");
    w.begin(back);
    w.ledger
        .retry_delivery(back, "its window went", true)
        .expect("queued again");
    assert_eq!(
        w.states(&[failed, cancelled, back]),
        ["failed", "cancelled", "queued"]
    );
    assert!(!had_a_turn(&w, chief));
}

#[test]
fn a_message_delivered_to_another_participant_is_none_of_its_turns() {
    let mut w = world();
    let chief = conversation(&mut w, "chief");
    let diana = conversation(&mut w, "diana");
    let note = note_to(&mut w, "diana");
    w.deliver(note);
    assert!(had_a_turn(&w, diana));
    assert!(!had_a_turn(&w, chief));
}

#[test]
fn what_was_delivered_before_a_conversation_began_is_a_turn_of_the_one_before() {
    let mut w = world();
    let first = conversation(&mut w, "chief");
    let note = note_to(&mut w, "chief");
    w.deliver(note);
    assert!(had_a_turn(&w, first));
    // The window is opened afresh (or the human typed /new): a conversation of its own.
    let second = conversation(&mut w, "chief");
    assert!(
        !had_a_turn(&w, second),
        "a delivery to the one before it is no turn of this one"
    );
    assert!(had_a_turn(&w, first), "and it stays one of that one's");
    let later = note_to(&mut w, "chief");
    w.deliver(later);
    assert!(had_a_turn(&w, second));
}

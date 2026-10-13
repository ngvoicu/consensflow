//! What moved since the last wake-up: a wake-up that changed nothing is not
//! repeated until something does, and a quiet board alone never skips one.

use cf_ledger::WakeUpUnit;

use crate::world::{calls, item, world, DAY, MINUTE};

/// A world with the calls on, a minute apart, and the first wake-up delivered.
fn after_the_first() -> crate::world::World {
    let mut w = world();
    w.turn_on(1, WakeUpUnit::Minutes);
    w.advance(MINUTE);
    w.wake_and_deliver();
    w
}

#[test]
fn a_wake_up_that_changed_nothing_is_not_repeated_whatever_the_clock_says() {
    let mut w = after_the_first();
    for _ in 0..5 {
        w.advance(10 * DAY);
        assert!(w.due().is_none(), "a quiet board: nothing to say again");
    }
    assert_eq!(w.queued(), 1);
}

#[test]
fn a_task_that_moves_brings_the_next_wake_up_once_the_interval_has_passed() {
    let mut w = after_the_first();
    w.move_a_task();
    assert!(
        w.due().is_none(),
        "something moved, but the interval has not passed"
    );
    w.advance(MINUTE);
    assert!(w.due().is_some());
}

#[test]
fn a_message_that_is_sent_brings_the_next_wake_up_and_the_wake_ups_own_note_never_does() {
    let mut w = after_the_first();
    w.advance(DAY);
    assert!(
        w.due().is_none(),
        "the wake-up's own note, its delivery and its confirmation moved nothing"
    );
    w.note("zeus", "chief", "The parser is done");
    assert!(w.due().is_some(), "a word from the staff");
}

#[test]
fn what_the_chief_sends_is_its_own_answer_and_brings_no_next_wake_up() {
    let mut w = after_the_first();
    w.advance(DAY);
    w.note("chief", "human", "Nothing left to do");
    assert!(w.due().is_none(), "the chief's own word is no news to it");
}

#[test]
fn a_note_the_human_sends_to_the_chief_counts_too() {
    let mut w = after_the_first();
    w.advance(DAY);
    w.note("human", "chief", "Look at the docs next");
    assert!(w.due().is_some());
}

#[test]
fn the_human_typing_to_the_chief_in_its_window_counts_and_what_a_delivery_made_does_not() {
    let mut w = after_the_first();
    let conversation = w
        .ledger
        .start_conversation(w.id("chief"), "claude-code")
        .unwrap();
    // The wake-up as the window recorded it: its header, as every delivery has one.
    let header = item(
        "i-1",
        "user",
        "[ConsensFlow m-1 · note from @human]\nDo you still have work to do?",
    );
    let reply = item("i-2", "assistant", "No: the board is empty.");
    w.advance(MINUTE);
    w.ledger
        .copy_transcript(conversation.id, &[header.clone(), reply.clone()], 0)
        .unwrap();
    w.advance(DAY);
    assert!(
        w.due().is_none(),
        "the delivery and the chief's own answer are what the wake-up itself made"
    );
    // The human types to the chief, directly.
    w.advance(MINUTE);
    let typed = item("i-3", "user", "Please look at the docs too");
    w.ledger
        .copy_transcript(conversation.id, &[header, reply, typed], 0)
        .unwrap();
    assert!(
        w.due().is_some(),
        "words typed in the chief's window are a message that came"
    );
}

#[test]
fn an_item_copied_again_as_it_was_is_not_new_and_one_a_tool_wrote_is_no_typing() {
    let mut w = world();
    w.turn_on(1, WakeUpUnit::Minutes);
    // The human typed to the chief before the first wake-up.
    w.advance(MINUTE / 2);
    let conversation = w
        .ledger
        .start_conversation(w.id("chief"), "claude-code")
        .unwrap();
    let typed = item("i-1", "user", "Typed before the wake-up");
    w.ledger
        .copy_transcript(conversation.id, std::slice::from_ref(&typed), 0)
        .unwrap();
    w.advance(MINUTE / 2);
    w.wake_and_deliver();
    // A tool's output and the chief's words are not what the human typed.
    w.advance(MINUTE);
    let tool = item("i-2", "tool", "ls: 3 files");
    let said = item("i-3", "assistant", "Three files.");
    w.ledger
        .copy_transcript(conversation.id, &[typed.clone(), tool, said], 0)
        .unwrap();
    assert!(w.due().is_none(), "nothing the human typed is new");
    // The same text once more (a window read again from its first item) is not new.
    w.advance(MINUTE);
    w.ledger
        .copy_transcript(conversation.id, &[typed], 0)
        .unwrap();
    assert!(
        w.due().is_none(),
        "what was typed before the wake-up, copied again, is no news"
    );
}

#[test]
fn the_first_wake_up_after_the_calls_are_turned_on_again_goes_though_the_board_is_quiet() {
    let mut w = after_the_first();
    w.advance(DAY);
    assert!(w.due().is_none(), "nothing moved");
    w.set(&calls(false, 1, WakeUpUnit::Minutes));
    w.advance(DAY);
    w.set(&calls(true, 1, WakeUpUnit::Minutes));
    w.advance(MINUTE);
    assert!(w.due().is_some(), "the first after an enable always goes");
}

#[test]
fn the_first_wake_up_after_a_resume_goes_though_the_board_is_quiet() {
    let mut w = after_the_first();
    w.advance(DAY);
    assert!(w.due().is_none(), "nothing moved");
    w.ledger
        .set_project_state(w.project.id, "suspended")
        .unwrap();
    w.advance(DAY);
    w.ledger.set_project_state(w.project.id, "open").unwrap();
    w.advance(MINUTE);
    let first = w.due().expect("the first after a Resume always goes");
    w.deliver(&first);
    w.advance(DAY);
    assert!(
        w.due().is_none(),
        "and the one after it waits for the board to move again"
    );
}

#[test]
fn a_change_of_the_interval_or_the_words_is_no_news_to_the_board() {
    let mut w = after_the_first();
    w.set(&calls(true, 2, WakeUpUnit::Minutes));
    w.advance(DAY);
    assert!(
        w.due().is_none(),
        "the human changed the setting; the board did not move"
    );
}

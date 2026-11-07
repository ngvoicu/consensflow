//! A chief that waits on the human is not asked, again and again, whether it still
//! has work (3.0.7-alpha.2, T-1113). The field board of 2026-11-02, calls two hours
//! apart: the chief wrote the human a note after each of the first three, the human
//! read none of them for nine hours, and the fourth came sixteen seconds after the
//! human read them and typed. Three rules, each from that board:
//!
//! - a wake-up that something moved before is skipped, and the skip logged, while the
//!   chief's newest note to the human is unread, three intervals in a row at the most;
//! - the human typing to the chief restarts the interval: a nudge never follows it at once;
//! - the interval doubles for each of the newest wake-ups the chief answered only by
//!   writing to the human.

use cf_ledger::WakeUpUnit;
use serde_json::Value;

use crate::world::{item, world, World, HOUR, MINUTE};

/// A world with the calls on, an hour apart, and the first wake-up delivered.
fn after_the_first() -> World {
    let mut w = world();
    w.turn_on(1, WakeUpUnit::Hours);
    w.advance(HOUR);
    w.wake_and_deliver();
    w
}

/// The notes the skips for a chief that waits on the human named, one list for each.
fn waited_on(w: &World) -> Vec<Vec<i64>> {
    w.ledger
        .events(w.project.id, 0, 5000)
        .expect("events")
        .into_iter()
        .filter(|event| event.kind == "project.wake_up_skipped")
        .filter_map(|event| event.data["waitingOnTheHuman"].as_array().cloned())
        .map(|notes| notes.iter().filter_map(Value::as_i64).collect())
        .collect()
}

/// The chief's window, as the copy holds it, and what the human typed to it.
struct Window {
    conversation: i64,
    items: Vec<Value>,
}

impl Window {
    /// The window has recorded the wake-up last queued, as every delivery has a header.
    fn after(w: &mut World) -> Self {
        let delivered = last_nudge(w);
        let conversation = w
            .ledger
            .start_conversation(w.id("chief"), "claude-code")
            .expect("a conversation")
            .id;
        let header = item(
            "i-1",
            "user",
            &format!(
                "[ConsensFlow m-{delivered} · note from @human]\nDo you still have work to do?"
            ),
        );
        let window = Self {
            conversation,
            items: vec![header],
        };
        window.copy(w);
        window
    }

    fn copy(&self, w: &mut World) {
        w.ledger
            .copy_transcript(self.conversation, &self.items, 0)
            .expect("copied");
    }

    /// The human types to the chief, directly, now.
    fn type_to_the_chief(&mut self, w: &mut World, words: &str) {
        let id = format!("i-{}", self.items.len() + 1);
        self.items.push(item(&id, "user", words));
        self.copy(w);
    }
}

/// The message of the wake-up queued last.
fn last_nudge(w: &World) -> i64 {
    w.ledger
        .events(w.project.id, 0, 5000)
        .expect("events")
        .into_iter()
        .rfind(|event| event.kind == "project.wake_up")
        .and_then(|event| event.data["message"].as_i64())
        .expect("a wake-up was queued")
}

/// The chief's note to the human, read or not.
fn tell_the_human(w: &mut World, read: bool) -> i64 {
    let said = w.note(
        "chief",
        "human",
        "Nothing is left but your decisions on A and B",
    );
    if read {
        w.ledger.mark_read(said.id).expect("read");
    }
    said.id
}

/// The same, and a word to a member as well: the chief acts, so the nudge is not one it
/// answered only by writing to the human, and only the wait on the human is at work.
fn wait_on_the_human(w: &mut World, read: bool) -> i64 {
    let said = tell_the_human(w, read);
    w.note("chief", "zeus", "Go on with the parser");
    said
}

// --- a chief whose newest note to the human is unread ---

#[test]
fn a_chief_whose_newest_note_to_the_human_is_unread_is_not_nudged_and_the_skip_is_logged() {
    let mut w = after_the_first();
    let said = wait_on_the_human(&mut w, false);
    w.note("zeus", "chief", "The parser is done");
    w.advance(HOUR);
    assert!(
        w.due().is_none(),
        "the chief waits on the human: something moved, but it has put its questions already"
    );
    assert_eq!(waited_on(&w), [vec![said]]);
    assert_eq!(w.queued(), 1);
}

#[test]
fn a_skip_for_the_human_counts_the_interval_from_itself_and_the_human_reading_the_note_lets_the_next_go(
) {
    let mut w = after_the_first();
    let said = wait_on_the_human(&mut w, false);
    w.note("zeus", "chief", "The parser is done");
    w.advance(HOUR);
    assert!(w.due().is_none());
    w.ledger.mark_read(said).expect("read");
    w.advance(HOUR - MINUTE);
    assert!(
        w.due().is_none(),
        "an interval from the skip, as for work on its way"
    );
    w.advance(MINUTE);
    assert!(w.due().is_some(), "the human has read what the chief said");
}

#[test]
fn a_chief_is_skipped_for_three_intervals_in_a_row_and_nudged_in_the_fourth() {
    let mut w = after_the_first();
    let said = wait_on_the_human(&mut w, false);
    w.note("zeus", "chief", "The parser is done");
    for interval in 1..=3 {
        w.advance(HOUR);
        assert!(w.due().is_none(), "skipped, interval {interval}");
    }
    assert_eq!(waited_on(&w), [vec![said], vec![said], vec![said]]);
    w.advance(HOUR);
    assert!(
        w.due().is_some(),
        "a board nobody reads is not silenced for good"
    );
}

#[test]
fn the_newest_note_read_ends_the_wait_though_an_older_one_is_unread() {
    let mut w = after_the_first();
    wait_on_the_human(&mut w, false);
    wait_on_the_human(&mut w, true);
    w.note("zeus", "chief", "The parser is done");
    w.advance(HOUR);
    assert!(w.due().is_some(), "the human read what the chief said last");
    assert!(waited_on(&w).is_empty());
}

#[test]
fn a_note_the_chief_wrote_to_a_member_is_no_wait_on_the_human() {
    let mut w = after_the_first();
    w.note("chief", "zeus", "Go on with the parser");
    w.note("zeus", "chief", "The parser is done");
    w.advance(HOUR);
    assert!(w.due().is_some());
    assert!(waited_on(&w).is_empty());
}

#[test]
fn the_first_wake_up_after_a_resume_goes_though_the_chief_waits_on_the_human() {
    let mut w = after_the_first();
    tell_the_human(&mut w, false);
    w.ledger
        .set_project_state(w.project.id, "suspended")
        .unwrap();
    w.advance(HOUR);
    w.ledger.set_project_state(w.project.id, "open").unwrap();
    w.advance(HOUR);
    assert!(
        w.due().is_some(),
        "the human resumed the project: they are there"
    );
}

// --- the human typing to the chief ---

#[test]
fn a_wake_up_overdue_since_the_human_was_away_is_not_sent_the_moment_they_type() {
    let mut w = after_the_first();
    let mut window = Window::after(&mut w);
    // The Mac was away for five intervals; something moved meanwhile.
    w.advance(5 * HOUR);
    w.note("zeus", "chief", "The parser is done");
    window.type_to_the_chief(&mut w, "Look at the docs next");
    assert!(
        w.due().is_none(),
        "the chief has the human's words: it is not stopped, and a nudge asks it nothing"
    );
    w.advance(HOUR - MINUTE);
    assert!(w.due().is_none(), "an interval from the human's words");
    w.advance(MINUTE);
    assert!(w.due().is_some());
}

// --- the interval doubles after a nudge answered only by writing to the human ---

#[test]
fn a_nudge_the_chief_answered_only_by_writing_to_the_human_is_followed_two_intervals_later() {
    let mut w = after_the_first();
    tell_the_human(&mut w, true);
    w.note("zeus", "chief", "The parser is done");
    w.advance(HOUR);
    assert!(
        w.due().is_none(),
        "the chief said it waits, the human has read it and not answered: not an hour later"
    );
    w.advance(HOUR - MINUTE);
    assert!(w.due().is_none());
    w.advance(MINUTE);
    assert!(w.due().is_some(), "two intervals after the nudge");
}

#[test]
fn each_such_nudge_in_a_row_doubles_the_interval_again_to_eight_and_no_further() {
    let mut w = after_the_first();
    for wait in [2, 4, 8, 8] {
        tell_the_human(&mut w, true);
        w.note("zeus", "chief", "More is done");
        w.advance((wait - 1) * HOUR + 59 * MINUTE);
        assert!(w.due().is_none(), "{wait} intervals are not over");
        w.advance(MINUTE);
        let nudge = w
            .due()
            .unwrap_or_else(|| panic!("due after {wait} intervals"));
        w.deliver(&nudge);
    }
}

#[test]
fn a_nudge_the_chief_answered_by_telling_a_member_is_followed_after_one_interval() {
    let mut w = after_the_first();
    tell_the_human(&mut w, true);
    w.note("chief", "zeus", "Go on with the parser");
    w.note("zeus", "chief", "The parser is done");
    w.advance(HOUR);
    assert!(w.due().is_some(), "the nudge made the chief act");
}

#[test]
fn a_nudge_the_chief_answered_by_giving_a_task_is_followed_after_one_interval() {
    let mut w = after_the_first();
    tell_the_human(&mut w, true);
    w.move_a_task();
    w.advance(HOUR);
    assert!(w.due().is_some(), "a task given and decided is acting");
}

#[test]
fn a_nudge_after_which_the_human_typed_to_the_chief_is_followed_after_one_interval() {
    let mut w = after_the_first();
    let mut window = Window::after(&mut w);
    tell_the_human(&mut w, true);
    w.advance(MINUTE);
    window.type_to_the_chief(&mut w, "A, and B later");
    w.note("zeus", "chief", "The parser is done");
    w.advance(HOUR);
    assert!(
        w.due().is_some(),
        "the human answered: the wait is over, and the interval counted from their words"
    );
}

#[test]
fn a_nudge_the_chief_did_not_answer_is_followed_after_one_interval() {
    let mut w = after_the_first();
    w.note("zeus", "chief", "The parser is done");
    w.advance(HOUR);
    assert!(w.due().is_some());
}

#[test]
fn a_resume_ends_the_run_of_nudges_answered_only_to_the_human() {
    let mut w = after_the_first();
    tell_the_human(&mut w, true);
    w.ledger
        .set_project_state(w.project.id, "suspended")
        .unwrap();
    w.advance(10 * MINUTE);
    w.ledger.set_project_state(w.project.id, "open").unwrap();
    w.advance(HOUR);
    assert!(
        w.due().is_some(),
        "the first after a Resume, an interval from it"
    );
}

// --- the field board, together ---

#[test]
fn the_field_boards_night_sends_the_chief_one_nudge_where_it_sent_four() {
    // Calls two hours apart. The chief answers the first by writing the human, who is away
    // and reads nothing; the staff's work moves meanwhile. The human comes back after
    // eleven hours (the Mac was away at the end of them), reads the note and types.
    let mut w = world();
    w.turn_on(2, WakeUpUnit::Hours);
    w.advance(2 * HOUR);
    w.wake_and_deliver();
    let mut window = Window::after(&mut w);
    let said = tell_the_human(&mut w, false);
    for hours in [2, 4, 6, 8] {
        w.note("zeus", "chief", "More is done");
        w.advance(2 * HOUR);
        assert!(w.due().is_none(), "{hours} hours after the nudge");
    }
    assert_eq!(
        waited_on(&w),
        [vec![said], vec![said]],
        "the chief waited on the human at the fourth and the eighth hour"
    );
    assert_eq!(w.queued(), 1, "and was nudged once");
    // The human is back at the eleventh hour: reads the note, types.
    w.advance(3 * HOUR);
    w.ledger.mark_read(said).expect("read");
    window.type_to_the_chief(&mut w, "A, and B later");
    assert!(
        w.due().is_none(),
        "overdue since the tenth hour, but never the moment they type"
    );
    w.advance(2 * HOUR - MINUTE);
    assert!(w.due().is_none());
    w.advance(MINUTE);
    assert!(
        w.due().is_some(),
        "two hours after their words, with the note read and the staff's news unanswered"
    );
}

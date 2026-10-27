//! A Devin window opened again on a conversation, with no message to begin a turn of
//! its own (the board's view button; T-722).
//!
//! Devin resumed on a session waits at its prompt and works on nothing, whatever the
//! record says of the turn before. Two records say that turn is not over, and only
//! the old process could end it: a turn cut after a step whose call has no result, and
//! a final answer that Devin stored with no `finish_reason` (every session of the GPT
//! models on this machine ends so, where SWE-2's and Claude's end `stop`), whose end only
//! the old launch's wire log said. The wire log is gone once the daemon has started again,
//! and the window read as at work for good: a field board's tester, finished and accepted,
//! showed "Working" on its row for an hour with Devin at rest.

use std::cell::RefCell;
use std::fs;
use std::rc::Rc;
use std::sync::Arc;

use cf_harness::contract::{Records, Work};
use cf_harness::records::{Item, Options, Reading, Record, Role, Settlement};
use cf_harness::testing::{finished, EPOCH_MS};
use cf_proto::agents::Harness;
use serde_json::json;

use super::fixtures::*;

const SESSION: &str = "wealthy-trick";

fn item(id: &str, role: Role, complete: bool) -> Item {
    Item {
        id: Arc::from(id),
        role,
        text: Arc::from(""),
        complete,
        at: None,
        commentary: false,
        compaction: false,
    }
}

/// A record of `items` whose turn the reader reads as `settlement`.
fn record(items: Vec<Item>, settlement: Settlement, in_flight: bool) -> Record {
    Record {
        items,
        in_flight,
        asking: false,
        failed: false,
        failure: None,
        quota: None,
        settlement,
        pending: Default::default(),
        monitors: Vec::new(),
        lines: Box::default(),
    }
}

/// The record of a finished conversation whose last answer Devin stored with no
/// `finish_reason`, and no wire log says it ended: the reader finds an answer that is not
/// complete, which it cannot tell from one still streaming.
fn an_answer_nothing_marks_as_the_end() -> Record {
    record(
        vec![
            item("u1", Role::User, true),
            item("a1", Role::Assistant, false),
        ],
        Settlement::Unknown,
        true,
    )
}

/// The record of a turn the quit cut in a call: the user's message, and the step that
/// made the call, with no result.
fn cut_in_a_call() -> Record {
    record(
        vec![
            item("u1", Role::User, true),
            item("a1", Role::Assistant, false),
        ],
        Settlement::InFlight,
        true,
    )
}

/// A record that is whatever the test says it is now.
struct Changing(RefCell<Record>);

impl Records for Changing {
    fn look<'a>(
        &'a self,
        _harness: Harness,
        _session: &'a str,
        _options: &'a Options,
    ) -> Work<'a, Arc<Reading>> {
        let reading = Reading::Known(self.0.borrow().clone());
        Box::pin(async { Arc::new(reading) })
    }

    fn has_transcript<'a>(
        &'a self,
        _harness: Harness,
        _session: &'a str,
    ) -> Work<'a, Result<bool, String>> {
        Box::pin(async { Ok(true) })
    }
}

/// A window of Devin opened again on `SESSION`, whose record is `old` then, with `message`
/// as its first message if it has one, and its wire log naming the conversation.
fn reopened(
    home: &Home,
    old: Record,
    message: Option<&str>,
) -> (Rc<Changing>, Rc<dyn cf_harness::contract::Window>) {
    let record = Rc::new(Changing(RefCell::new(old)));
    let adapter = home.adapter_over(Some(Rc::clone(&record) as Rc<dyn Records>));
    let request = Request {
        message: message.map(str::to_owned),
        ..Request::resumed(SESSION)
    };
    let window = window(&adapter, &request);
    fs::create_dir_all(home.folder()).unwrap();
    home.selects(SESSION);
    (record, window)
}

fn at_rest(window: &Rc<dyn cf_harness::contract::Window>) -> bool {
    finished(window.observe()).unwrap().settled
}

#[test]
fn a_window_opened_again_with_no_message_is_at_rest_on_a_turn_nothing_in_the_record_ends() {
    for old in [an_answer_nothing_marks_as_the_end(), cut_in_a_call()] {
        let home = Home::new();
        let (_record, window) = reopened(&home, old, None);
        assert!(
            at_rest(&window),
            "no process is in that turn, and the window has been given nothing"
        );
    }
}

#[test]
fn a_window_opened_with_a_message_of_its_own_is_not_at_rest_on_the_turn_before_it() {
    for old in [an_answer_nothing_marks_as_the_end(), cut_in_a_call()] {
        let home = Home::new();
        let (_record, window) = reopened(&home, old, Some(TASK));
        assert!(
            !at_rest(&window),
            "its message is its turn, and it is not in the record yet"
        );
    }
}

#[test]
fn a_window_whose_record_grew_is_read_by_the_record_and_not_by_the_turn_it_was_opened_on() {
    let home = Home::new();
    let (record, window) = reopened(&home, an_answer_nothing_marks_as_the_end(), None);
    assert!(at_rest(&window));

    // The human typed into the window, or the board gave it a message: its turn is not
    // answered yet.
    *record.0.borrow_mut() = self::record(
        vec![
            item("u1", Role::User, true),
            item("a1", Role::Assistant, false),
            item("u2", Role::User, true),
        ],
        Settlement::InFlight,
        true,
    );
    assert!(!at_rest(&window), "a turn of its own is at work");

    // Its answer ends the turn: the record settles it, as it settles any turn.
    *record.0.borrow_mut() = self::record(
        vec![
            item("u1", Role::User, true),
            item("a1", Role::Assistant, false),
            item("u2", Role::User, true),
            item("a2", Role::Assistant, true),
        ],
        Settlement::Settled,
        false,
    );
    assert!(at_rest(&window));
}

#[test]
fn a_window_that_follows_another_conversation_forgets_the_turn_it_was_opened_on() {
    let home = Home::new();
    let (_record, window) = reopened(&home, an_answer_nothing_marks_as_the_end(), None);
    assert!(at_rest(&window));
    window.follow("another-conversation");
    window.follow(SESSION);
    assert!(
        !at_rest(&window),
        "the turn was a conversation's the window left"
    );
}

#[test]
fn a_conversation_whose_turn_ended_is_read_from_its_record_as_it_always_was() {
    let home = Home::new();
    let (_record, window) = reopened(&home, settled(SESSION), None);
    assert!(at_rest(&window));
}

/// The time a message of the user's was written, as Devin's store has it: just now, by the
/// clock of the test.
fn written_now(mut item: Item) -> Item {
    item.at = Some(json!(cf_base::time::iso(EPOCH_MS)));
    item
}

#[test]
fn a_window_opened_again_on_a_message_the_cut_left_unanswered_is_at_rest_at_once() {
    // Devin reads a lone message of the user's as complete, at rest, and the window reads
    // one written seconds ago as a turn begun for half a minute (T-102). That message is
    // the old process's when the window was opened on it: the window has begun no turn.
    let home = Home::new();
    let old = record(
        vec![written_now(item("u1", Role::User, true))],
        Settlement::Settled,
        false,
    );
    let (_record, window) = reopened(&home, old, None);
    assert!(at_rest(&window));
}

#[test]
fn a_message_given_to_the_window_since_is_a_turn_begun_as_any_for_half_a_minute() {
    let home = Home::new();
    let old = record(
        vec![
            item("u1", Role::User, true),
            item("a1", Role::Assistant, true),
        ],
        Settlement::Settled,
        false,
    );
    let (record, window) = reopened(&home, old, None);
    assert!(at_rest(&window));

    // The human typed into the window: the store has their message, which its agent has
    // not answered.
    *record.0.borrow_mut() = self::record(
        vec![
            item("u1", Role::User, true),
            item("a1", Role::Assistant, true),
            written_now(item("u2", Role::User, true)),
        ],
        Settlement::Settled,
        false,
    );
    assert!(!at_rest(&window), "a turn begun");
    // A turn that says nothing for longer is read as the record reads it.
    home.fakes.time.settle_at(EPOCH_MS + 30_000);
    assert!(at_rest(&window));
}

#[test]
fn rows_the_old_process_wrote_after_the_open_are_no_turn_the_window_began() {
    // Live, Devin 3000.11.205, T-722: the window of a conversation cut in a command was
    // ended, its store was read, the window was opened again, and one row more was in
    // the store: Devin's agent outlives its window by a second or two, and wrote its
    // harness's note of the step it was in. That row is no message given to the window.
    let home = Home::new();
    let (record, window) = reopened(&home, cut_in_a_call(), None);
    assert!(at_rest(&window));
    let mut later = cut_in_a_call();
    later.items.push(Item {
        role: Role::Custom,
        ..item("n1", Role::Custom, true)
    });
    later.items.push(item("t1", Role::Tool, true));
    *record.0.borrow_mut() = later;
    assert!(at_rest(&window), "the old process wrote it");
}

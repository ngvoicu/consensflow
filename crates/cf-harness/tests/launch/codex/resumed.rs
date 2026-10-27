//! A Codex window opened again on a thread the app's quit cut in a turn, with no
//! message to begin one: a role's replacement is given to such a window by paste,
//! once it is up, so the window opens on a record that holds the old process's
//! turn as the quit left it, the user's message with no answer or a tool's call
//! with no result, both of which read as a turn in flight. No process is in that
//! turn any more, so the window is at rest, as Claude's and Pi's are
//! (`LeftOpen`): once its broker names the thread, and while its record holds no
//! more than it held when the window was opened. What Codex itself does with
//! such a thread (`cargo xtask live resume-cut codex`) is what that reading has
//! to agree with: a Codex that goes on with the turn, or closes it, writes to the
//! record, and the record is then read as it always was.

use std::cell::{Cell, RefCell};
use std::sync::Arc;

use cf_harness::contract::{Observed, Window, Work};
use cf_harness::records::{Item, Options, Reading, Record, Role, Settlement};
use cf_proto::agents::Harness;

use super::*;

/// An item of a record, from the user or the assistant.
fn item(id: &str, role: Role, text: &str, complete: bool) -> Item {
    Item {
        id: Arc::from(id),
        role,
        text: Arc::from(text),
        complete,
        at: None,
        commentary: false,
        compaction: false,
    }
}

fn reading(items: Vec<Item>, settlement: Settlement) -> Arc<Reading> {
    Arc::new(Reading::Known(Record {
        items,
        in_flight: false,
        asking: false,
        failed: false,
        failure: None,
        took_back: false,
        model_moved: None,
        model_drift: None,
        quota: None,
        settlement,
        pending: Default::default(),
        monitors: Vec::new(),
        lines: Box::default(),
    }))
}

/// The record as a turn cut in a command leaves it: the user's message, and the
/// assistant's step that made the call, with no result.
fn cut_in_a_call() -> Arc<Reading> {
    reading(
        vec![
            item("u1", Role::User, "Run the command", true),
            item("a1", Role::Assistant, "", false),
        ],
        Settlement::InFlight,
    )
}

/// The record as a turn cut before its answer leaves it: the user's message.
fn cut_before_an_answer() -> Arc<Reading> {
    reading(
        vec![item("u1", Role::User, "Count to forty", true)],
        Settlement::InFlight,
    )
}

/// A record that is whatever the test says it is now, and how often it was read.
struct Changing {
    reading: RefCell<Arc<Reading>>,
    looks: Cell<usize>,
}

impl Records for Changing {
    fn look<'a>(
        &'a self,
        _harness: Harness,
        _session: &'a str,
        _options: &'a Options,
    ) -> Work<'a, Arc<Reading>> {
        self.looks.set(self.looks.get() + 1);
        Box::pin(async { Arc::clone(&self.reading.borrow()) })
    }

    fn has_transcript<'a>(
        &'a self,
        _harness: Harness,
        _session: &'a str,
    ) -> Work<'a, Result<bool, String>> {
        Box::pin(async { Ok(true) })
    }
}

/// A window opened again on the thread `THREAD`, whose record is `cut` then,
/// with `message` as its first message if it has one.
fn resumed(
    home: &Home,
    cut: Arc<Reading>,
    message: Option<&str>,
) -> (Rc<Changing>, Fakes, Rc<dyn Window>) {
    let record = Rc::new(Changing {
        reading: RefCell::new(cut),
        looks: Cell::new(0),
    });
    let (adapter, fakes) =
        adapter_with(home, Some(Rc::clone(&record) as Rc<dyn Records>), "", true);
    let request = Request {
        resume: Some(THREAD.to_owned()),
        message: message.map(str::to_owned),
        ..Request::default()
    };
    let window = prepare(&adapter, &request).unwrap().window;
    (record, fakes, window)
}

/// What the window's look says when its broker says it shows `session`.
fn looks(fakes: &Fakes, window: &Rc<dyn Window>, session: Option<&str>) -> Observed {
    fakes
        .loopback
        .serve("GET /session", [shows(session, session.is_some())]);
    finished(window.observe()).unwrap()
}

#[test]
fn a_window_opened_again_on_a_turn_the_quit_cut_is_at_rest_once_its_broker_names_the_thread() {
    for cut in [cut_in_a_call(), cut_before_an_answer()] {
        let home = Home::new();
        let (record, fakes, window) = resumed(&home, cut, None);
        let waiting = looks(&fakes, &window, None);
        assert!(
            !waiting.settled && waiting.unnamed,
            "its broker has not said which thread it shows"
        );
        assert!(
            looks(&fakes, &window, Some(THREAD)).settled,
            "no process is in the turn it was cut in, and the window has begun none"
        );

        // A message was given to the window, and its turn is not answered yet.
        *record.reading.borrow_mut() = reading(
            vec![
                item("u1", Role::User, "Count to forty", true),
                item("u2", Role::User, "Reply with one line", true),
            ],
            Settlement::InFlight,
        );
        assert!(
            !looks(&fakes, &window, Some(THREAD)).settled,
            "its turn is in flight, whatever the turn the quit cut was"
        );

        // Answered: the record settles it, as it settles any turn.
        *record.reading.borrow_mut() = reading(
            vec![
                item("u1", Role::User, "Count to forty", true),
                item("u2", Role::User, "Reply with one line", true),
                item("a2", Role::Assistant, "One line", true),
            ],
            Settlement::Settled,
        );
        assert!(looks(&fakes, &window, Some(THREAD)).settled);
    }
}

#[test]
fn a_window_that_shows_another_thread_is_not_read_at_rest_by_the_turn_the_quit_cut() {
    let home = Home::new();
    let (_record, fakes, window) = resumed(&home, cut_in_a_call(), None);
    let observed = looks(&fakes, &window, Some(NEXT));
    assert!(!observed.settled);
    assert_eq!(observed.switched.as_deref(), Some(NEXT));
}

#[test]
fn a_window_opened_with_a_message_of_its_own_is_not_read_at_rest_by_the_turn_the_quit_cut() {
    let home = Home::new();
    let (record, fakes, window) = resumed(&home, cut_in_a_call(), Some(TASK));
    assert!(
        !looks(&fakes, &window, Some(THREAD)).settled,
        "its message is its turn, and it is not in the record yet"
    );
    assert_eq!(
        record.looks.get(),
        1,
        "the record was read by the look alone, not for a turn left open"
    );
}

#[test]
fn a_window_that_follows_another_thread_forgets_the_turn_it_was_opened_on() {
    let home = Home::new();
    let (_record, fakes, window) = resumed(&home, cut_in_a_call(), None);
    assert!(looks(&fakes, &window, Some(THREAD)).settled);
    window.follow(NEXT);
    window.follow(THREAD);
    assert!(
        !looks(&fakes, &window, Some(THREAD)).settled,
        "the turn was a thread's the window left"
    );
}

#[test]
fn a_thread_whose_turn_ended_is_read_from_its_record_as_it_always_was() {
    let home = Home::new();
    let settled = reading(
        vec![
            item("u1", Role::User, "hello", true),
            item("a1", Role::Assistant, "hi", true),
        ],
        Settlement::Settled,
    );
    let (_record, fakes, window) = resumed(&home, settled, None);
    assert!(looks(&fakes, &window, Some(THREAD)).settled);
}

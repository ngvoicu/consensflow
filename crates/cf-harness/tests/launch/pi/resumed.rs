//! A Pi window opened again on a conversation the app's quit cut in a turn.
//!
//! Resumed on one, Pi waits at its prompt and writes nothing of the turn: its
//! record stays as the quit left it, the user's message with no answer or a
//! tool's call with no result, both of which read as a turn in flight
//! (`npm run live:resume-cut -- --harness pi`, Pi 1.1.0: the record unchanged
//! after 45 s, and a message pasted after it answered in a second). The
//! extension of ConsensFlow's, inside the window, says which conversation it
//! shows, and writes a marker when a turn of the window begins and removes it
//! when Pi settles: a window whose extension names the conversation and has
//! written no marker has begun no turn.

use super::*;

const SESSION: &str = "cf-1-zeus-0a1b2c3d";

/// What the extension writes when a turn begins: removed, when Pi settles.
fn begins_a_turn(home: &Home, session: &str) {
    let settled = path::join(&[&home.launch_folder(), "settled"]);
    fs::create_dir_all(&settled).unwrap();
    let file = path::join(&[&settled, &format!("{LAUNCH}.working.json")]);
    let said = json!({ "launchId": LAUNCH, "sessionId": session });
    fs::write(file, format!("{said}\n")).unwrap();
}

fn settles(home: &Home) {
    let file = path::join(&[
        &home.launch_folder(),
        "settled",
        &format!("{LAUNCH}.working.json"),
    ]);
    fs::remove_file(file).unwrap();
}

/// An item of a record, from the user or the assistant.
fn item(id: &str, role: Role, text: &str, complete: bool) -> Item {
    Item {
        id: Arc::from(id),
        role,
        text: Arc::from(text),
        complete,
        at: None,
        commentary: false,
        synthetic: false,
        compaction: false,
    }
}

/// The record as a turn cut in a command leaves it: the user's message, and
/// the assistant's step that made the call, with no result.
fn cut_in_a_call() -> Arc<Reading> {
    reading(
        vec![
            item("u1", Role::User, "Run the command", true),
            item("a1", Role::Assistant, "", false),
        ],
        Settlement::InFlight,
        None,
    )
}

/// The record as a turn cut before its answer leaves it: the user's message.
fn cut_before_an_answer() -> Arc<Reading> {
    reading(
        vec![item("u1", Role::User, "Count to forty", true)],
        Settlement::InFlight,
        None,
    )
}

/// The record of a turn that failed (the provider's error ended it): the user's message
/// and an assistant step with no answer. Pi's reader cannot settle a failed turn, so
/// the record says nothing of its end.
fn failed_before_an_answer() -> Arc<Reading> {
    reading(
        vec![
            item("u1", Role::User, "Count to forty", true),
            item("a1", Role::Assistant, "", false),
        ],
        Settlement::Unknown,
        None,
    )
}

/// A record that is whatever the test says it is now.
struct Changing {
    reading: RefCell<Arc<Reading>>,
}

impl Records for Changing {
    fn look<'a>(
        &'a self,
        _harness: Harness,
        _session: &'a str,
        _options: &'a Options,
    ) -> Work<'a, Arc<Reading>> {
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

/// A window opened again on the conversation `SESSION`, whose record is `cut`
/// then, with `message` as its first message if it has one.
fn resumed(
    home: &Home,
    cut: Arc<Reading>,
    message: Option<&str>,
) -> (Rc<Changing>, Rc<dyn cf_harness::contract::Window>) {
    let record = Rc::new(Changing {
        reading: RefCell::new(cut),
    });
    let (adapter, _fakes) = adapter_with(home, Rc::clone(&record) as Rc<dyn Records>);
    let request = Request {
        resume: Some(SESSION.to_owned()),
        message: message.map(str::to_owned),
        ..Request::default()
    };
    let window = prepare(&adapter, &request).unwrap().window;
    (record, window)
}

fn settled(window: &Rc<dyn cf_harness::contract::Window>) -> bool {
    finished(window.observe()).unwrap().settled
}

#[test]
fn a_window_opened_again_on_a_turn_the_quit_cut_is_at_rest_once_its_extension_has_seen_no_turn_of_it(
) {
    for cut in [cut_in_a_call(), cut_before_an_answer()] {
        let home = Home::new();
        let (_record, window) = resumed(&home, cut, None);
        assert!(
            !settled(&window),
            "its extension has not said which conversation it shows"
        );
        shows(&home, SESSION);
        assert!(
            settled(&window),
            "no process is in the turn it was cut in, and the extension has seen none begin"
        );

        // A turn of the window's own begins: the extension says so.
        begins_a_turn(&home, SESSION);
        assert!(!settled(&window), "a turn is at work");
        settles(&home);
        assert!(settled(&window), "Pi settled, with the record as it was");
    }
}

#[test]
fn a_window_opened_again_on_a_turn_its_record_cannot_end_is_at_rest_once_its_extension_has_seen_no_turn_of_it(
) {
    // T-722: a failed turn is no turn in flight, and the record never settles it; the
    // window read as at work for as long as it was open.
    let home = Home::new();
    let (_record, window) = resumed(&home, failed_before_an_answer(), None);
    shows(&home, SESSION);
    assert!(
        settled(&window),
        "no process is in the turn that failed, and the extension has seen none begin"
    );

    begins_a_turn(&home, SESSION);
    assert!(!settled(&window), "a turn is at work");
    settles(&home);
    assert!(settled(&window), "Pi settled, with the record as it was");
}

#[test]
fn a_window_that_shows_another_conversation_is_not_read_at_rest_by_the_turn_the_quit_cut() {
    let home = Home::new();
    let (_record, window) = resumed(&home, cut_in_a_call(), None);
    shows(&home, "another-conversation");
    let observed = finished(window.observe()).unwrap();
    assert!(!observed.settled);
    assert_eq!(observed.switched.as_deref(), Some("another-conversation"));
}

/// A window opened for a message its extension's inbox gives it once it is up
/// (T-432) begins no turn as it opens: the message a launch names is not on its
/// command line, and the window reads at rest by the turn the quit cut, as one
/// opened with none does, so that its message can go in.
#[test]
fn a_window_opened_for_a_message_it_is_given_once_up_is_read_at_rest_by_the_turn_the_quit_cut() {
    let home = Home::new();
    let (_record, window) = resumed(&home, cut_in_a_call(), Some(TASK));
    shows(&home, SESSION);
    assert!(
        settled(&window),
        "its message is not in it yet, and the turn the quit cut is no turn of its own"
    );
}

#[test]
fn a_window_whose_record_grew_is_read_by_the_record_and_not_by_the_turn_the_quit_cut() {
    let home = Home::new();
    let (record, window) = resumed(&home, cut_before_an_answer(), None);
    shows(&home, SESSION);
    assert!(settled(&window));

    // A message was given to the window, and its turn is not answered yet.
    *record.reading.borrow_mut() = reading(
        vec![
            item("u1", Role::User, "Count to forty", true),
            item("u2", Role::User, "Reply with one line", true),
        ],
        Settlement::InFlight,
        None,
    );
    assert!(
        !settled(&window),
        "its turn is in flight, whatever the extension has written"
    );

    // Answered: the record settles it, as it settles any turn.
    *record.reading.borrow_mut() = reading(
        vec![
            item("u1", Role::User, "Count to forty", true),
            item("u2", Role::User, "Reply with one line", true),
            item("a2", Role::Assistant, "One line", true),
        ],
        Settlement::Settled,
        None,
    );
    assert!(settled(&window));
}

#[test]
fn a_window_that_follows_another_conversation_forgets_the_turn_it_was_opened_on() {
    let home = Home::new();
    let (_record, window) = resumed(&home, cut_in_a_call(), None);
    shows(&home, SESSION);
    assert!(settled(&window));
    window.follow("another-conversation");
    window.follow(SESSION);
    assert!(
        !settled(&window),
        "the turn was a conversation's the window left"
    );
}

#[test]
fn a_conversation_whose_turn_ended_is_read_from_its_record_as_it_always_was() {
    let home = Home::new();
    let (_record, window) = resumed(&home, settled_record("hello"), None);
    assert!(
        finished(window.observe()).unwrap().unnamed,
        "its extension has not named it yet"
    );
    shows(&home, SESSION);
    assert!(settled(&window));
}

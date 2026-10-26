//! A Claude window opened again on a conversation the app's quit cut in a turn,
//! and how the look at it reads it: on the records Claude 2.1.296 left in two
//! live runs of `npm run live:resume-cut` (scrubbed by
//! `cargo xtask scrub-transcript`).
//!
//! `quit-before-a-word`: the process was killed as soon as Claude's status read
//! busy, before a word of its answer: the record ends on the user's message.
//! `quit-in-a-command`: killed inside a long command: the record ends on the
//! tool's call and the error result the kill gave it (`Exit code 144`).
//! Resumed on either, Claude waited at its prompt, status idle, for as long as
//! it was watched, and wrote nothing of the turn: only a record of its own
//! bookkeeping. A second message, pasted as the daemon pastes, reached it alone,
//! was answered in a second or two, and Claude closed the turn that was cut with
//! a message of its own, `No response requested.`, written when the second came.
//! The `-resumed` records are the conversation then.

use super::*;

const SESSION: &str = "5c7e3d0a-8f21-4b6c-9a47-0e1d2f3a4b5c";

/// The records of a live run, for `session`.
fn fixture(name: &str) -> Vec<String> {
    let text = match name {
        "quit-before-a-word" => include_str!("../../fixtures/claude/quit-before-a-word.jsonl"),
        "quit-before-a-word-resumed" => {
            include_str!("../../fixtures/claude/quit-before-a-word-resumed.jsonl")
        }
        "quit-in-a-command" => include_str!("../../fixtures/claude/quit-in-a-command.jsonl"),
        "quit-in-a-command-resumed" => {
            include_str!("../../fixtures/claude/quit-in-a-command-resumed.jsonl")
        }
        other => panic!("no fixture {other}"),
    };
    text.replace("$SESSION", SESSION)
        .split_inclusive('\n')
        .map(str::to_owned)
        .collect()
}

/// A window opened again on the conversation of the records `name`.
struct Resumed {
    home: Home,
    window: Rc<dyn Window>,
}

impl Resumed {
    fn new(name: &str) -> Self {
        Self::opened(name, &Request::resumed(SESSION))
    }

    fn opened(name: &str, request: &Request) -> Self {
        let home = Home::new();
        home.transcript(SESSION, &fixture(name));
        let window = window(&home.adapter(), request);
        Self { home, window }
    }

    /// Claude's status of the window, as it writes one.
    fn status(&self, word: &str) {
        self.home.status(SESSION, &json!({ "status": word }), me());
    }

    /// The record as it is by now.
    fn records(&self, name: &str) {
        self.home.transcript(SESSION, &fixture(name));
    }

    /// A look at the window: whether it reads at rest, and its last item.
    fn look(&self) -> (bool, Option<(Role, String)>) {
        let observed = finished(self.window.observe()).unwrap();
        let last = observed
            .items()
            .last()
            .map(|item| (item.role, item.text.to_string()));
        (observed.settled, last)
    }

    fn settled(&self) -> bool {
        self.look().0
    }
}

/// What a window is given: the daemon's own message, with its header.
const MESSAGE: &str = "[ConsensFlow m-2 · T-1 · note from @chief]\nGo on";

#[test]
fn a_window_opened_again_on_a_turn_the_quit_cut_is_at_rest_once_claude_says_it_is_idle() {
    for name in ["quit-before-a-word", "quit-in-a-command"] {
        let window = Resumed::new(name);
        assert!(
            !window.settled(),
            "{name}: Claude has said nothing of the window yet"
        );
        window.status("busy");
        assert!(!window.settled(), "{name}: busy");
        window.home.status(
            SESSION,
            &json!({ "status": "waiting", "waitingFor": "permission prompt" }),
            me(),
        );
        assert!(!window.settled(), "{name}: waiting on its own dialog");

        let before = window.look().1;
        window.status("idle");
        let (settled, last) = window.look();
        assert!(
            settled,
            "{name}: no process is in the turn it was cut in, and Claude is at its prompt"
        );
        assert_eq!(last, before, "{name}: the record is as it was");
        // A worker's window read so has no result to give: the engine takes the
        // words of a turn as its task's result once its last message is complete,
        // and nothing the cut turn wrote is.
        let observed = finished(window.window.observe()).unwrap();
        assert!(
            !observed
                .items()
                .iter()
                .any(|item| item.role == Role::Assistant && item.complete),
            "{name}: the words of a cut turn are no answer"
        );
        assert!(
            !finished(window.window.observe()).unwrap().took_back,
            "{name}: Claude put nothing back in its input box"
        );

        window.status("busy");
        assert!(!window.settled(), "{name}: it went on after all");
    }
}

#[test]
fn what_the_cut_turn_left_is_what_the_look_says_and_a_turn_of_its_own_is_read_as_any() {
    for (name, resumed) in [
        ("quit-before-a-word", "quit-before-a-word-resumed"),
        ("quit-in-a-command", "quit-in-a-command-resumed"),
    ] {
        let window = Resumed::new(name);
        window.status("idle");
        assert!(window.settled(), "{name}");

        // The window is given a message: its record holds the turn that was cut,
        // closed by Claude, and the message's turn begun and not answered. The
        // turn the quit cut says nothing of the window any more.
        let mut in_flight = fixture(resumed);
        let at = in_flight
            .iter()
            .rposition(|line| line.contains("\"role\":\"user\"") && line.contains("AFTER"))
            .unwrap_or_else(|| panic!("{name}: the second message is in the record"));
        in_flight.truncate(at + 1);
        window.home.transcript(SESSION, &in_flight);
        window.status("idle");
        assert!(
            !window.settled(),
            "{name}: its turn has begun, whatever Claude reads while its hooks run"
        );
        window.status("busy");
        assert!(!window.settled(), "{name}: busy");

        // Answered, it is at rest as any window is.
        window.records(resumed);
        window.status("idle");
        let (settled, last) = window.look();
        assert!(settled, "{name}: the turn it began is over");
        assert!(last.is_some_and(|(role, text)| role == Role::Assistant && text.contains("AFTER")));
        window.status("busy");
        assert!(!window.settled(), "{name}: busy again");
    }
}

/// A window opened for a message it is pasted once it is up (T-432) begins no turn
/// as it opens: the message a launch names is not on its command line, and the
/// window reads at rest by the turn the quit cut, as one opened with none does, so
/// that its message can go in.
#[test]
fn a_window_opened_for_a_message_it_is_pasted_once_up_is_read_at_rest_by_the_turn_the_quit_cut() {
    for name in ["quit-before-a-word", "quit-in-a-command"] {
        let request = Request {
            message: Some(MESSAGE.to_owned()),
            ..Request::resumed(SESSION)
        };
        let window = Resumed::opened(name, &request);
        window.status("idle");
        assert!(
            window.settled(),
            "{name}: its message is not in it yet, and the turn the quit cut is no turn of its own"
        );
    }
}

#[test]
fn a_window_that_shows_another_conversation_forgets_the_turn_it_was_opened_on() {
    let window = Resumed::new("quit-before-a-word");
    window.status("idle");
    assert!(window.settled());
    window.window.follow("another-conversation");
    window.window.follow(SESSION);
    assert!(
        !window.settled(),
        "the turn was a conversation's the window left"
    );
}

#[test]
fn a_window_opened_on_a_turn_that_ended_is_read_as_it_always_was() {
    for name in ["quit-before-a-word-resumed", "quit-in-a-command-resumed"] {
        let window = Resumed::new(name);
        assert!(!window.settled(), "{name}: no status yet");
        window.status("busy");
        assert!(!window.settled(), "{name}: busy");
        window.status("idle");
        assert!(window.settled(), "{name}: its turn ended");
    }
}

#[test]
fn a_window_read_at_rest_by_the_turn_the_quit_cut_is_pasted_into_with_no_key_before_it() {
    // Claude resumed did not put the cut message back in its input box
    // (`npm run live:resume-cut`): the second message reached it alone.
    let window = Resumed::new("quit-before-a-word");
    window.status("idle");
    assert!(window.settled());

    let host = AnsweringHost::new(|op| {
        Ok(match op {
            "pane.snapshot" => json!({ "ok": true, "pasteInFlight": false, "unsent": false }),
            _ => json!({ "ok": true }),
        })
    });
    let sent = finished(window.window.deliver(
        &host,
        &Pane {
            id: "p1-chief".to_owned(),
            generation: 1,
        },
        "Go on",
    ));
    assert_eq!(sent, Ok(Admission::Admitted { queued: false }));
    let asked: Vec<(String, Value)> = host
        .asked
        .borrow()
        .iter()
        .filter(|(op, _)| op != "pane.snapshot")
        .map(|(op, body)| (op.clone(), body.get("body").cloned().unwrap_or(Value::Null)))
        .collect();
    assert_eq!(asked, [("pane.write_paste".to_owned(), json!("Go on"))]);
}

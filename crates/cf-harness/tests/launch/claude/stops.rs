//! A Claude window the daemon interrupted, and how the look at it reads it:
//! on the records Claude 2.1.292 wrote in two live runs of
//! `npm run live:stops` (scrubbed by `tests/live/scrub-transcript.mjs`).
//!
//! `stopped-before-a-word`: the daemon paused the task a moment after its
//! brief and pressed Escape; Claude, in the hooks of the prompt, stopped and
//! put the brief back in its input box, wrote no record of it, and read idle
//! (it reads idle through those hooks, before the turn goes busy).
//! `stopped-in-the-stop-hook`: Escape while a Stop hook that ignores signals
//! ran after the answer; Claude's record of the interrupt names no message.

use cf_harness::testing::EPOCH_MS;

use super::*;

const BRIEF: &str = "[ConsensFlow m-1 · T-1 · task from @chief]\nRun exactly this one shell command, then stop: node -e \"setTimeout(() => {}, 300000)\" cf-stops-live-long-command";
const SESSION: &str = "1a8340d5-4fae-474e-a6ff-a3af2182cbc1";

/// The records of a live run, for `session`.
fn fixture(name: &str, session: &str) -> Vec<String> {
    let text = match name {
        "stopped-before-a-word" => {
            include_str!("../../fixtures/claude/stopped-before-a-word.jsonl")
        }
        "stopped-in-the-stop-hook" => {
            include_str!("../../fixtures/claude/stopped-in-the-stop-hook.jsonl")
        }
        other => panic!("no fixture {other}"),
    };
    text.replace("$SESSION", session)
        .split_inclusive('\n')
        .map(str::to_owned)
        .collect()
}

/// A window on a conversation of the records `name`, and what it is made of.
struct Stopped {
    home: Home,
    fakes: Fakes,
    window: Rc<dyn Window>,
}

impl Stopped {
    fn new(name: &str) -> Self {
        let home = Home::new();
        let fakes = Fakes::new(&home.env());
        home.transcript(SESSION, &fixture(name, SESSION));
        let adapter = ClaudeAdapter::new(&fakes.services(&home.env(), Path::new(&home.root)));
        let window = prepare(&adapter, &Request::resumed(SESSION))
            .unwrap()
            .window;
        Self {
            home,
            fakes,
            window,
        }
    }

    /// Claude's status of the window, as it writes one.
    fn status(&self, word: &str) {
        self.home.status(SESSION, &json!({ "status": word }), me());
    }

    /// The clock `ms` after the run began.
    fn at(&self, ms: i64) {
        self.fakes.time.settle_at(EPOCH_MS + ms);
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

/// A host that takes keys and pastes, and says what it was asked.
fn taking() -> AnsweringHost<impl Fn(&str) -> Result<Value, HostError>> {
    AnsweringHost::new(|op| {
        Ok(match op {
            "pane.snapshot" => json!({ "ok": true, "pasteInFlight": false, "unsent": false }),
            _ => json!({ "ok": true }),
        })
    })
}

/// What was asked of the host: its operation, and the keys or the text.
fn asked(host: &AnsweringHost<impl Fn(&str) -> Result<Value, HostError>>) -> Vec<(String, Value)> {
    host.asked
        .borrow()
        .iter()
        .filter(|(op, _)| op != "pane.snapshot")
        .map(|(op, body)| {
            let what = body.get("bytes").or_else(|| body.get("body")).cloned();
            (op.clone(), what.unwrap_or(Value::Null))
        })
        .collect()
}

fn pane() -> Pane {
    Pane {
        id: "s1-zeus".to_owned(),
        generation: 7,
    }
}

#[test]
fn a_turn_the_daemon_interrupted_before_claude_wrote_a_word_is_read_at_rest_a_moment_after_the_press(
) {
    let stopped = Stopped::new("stopped-before-a-word");
    stopped.status("idle");
    // What Claude's record says: the brief, and nothing after it. Its status
    // is idle all through the hooks of the prompt, before the turn goes busy.
    stopped.at(0);
    let (settled, last) = stopped.look();
    assert!(
        !settled,
        "no press: Claude has not begun, and has not stopped"
    );
    assert_eq!(last, Some((Role::User, BRIEF.to_owned())));

    stopped.at(10_000);
    stopped.window.interrupted();
    assert!(!stopped.settled(), "not at the press");
    stopped.at(10_999);
    assert!(
        !stopped.settled(),
        "a turn about to begin is busy within the moment"
    );
    stopped.at(11_000);
    let (settled, last) = stopped.look();
    assert!(
        settled,
        "idle, with nothing written, a moment after the press"
    );
    assert_eq!(
        last,
        Some((Role::User, BRIEF.to_owned())),
        "the record is as it was: it says nothing of the stop"
    );

    stopped.status("busy");
    assert!(!stopped.settled(), "Claude went on after all");
    stopped.status("idle");
    assert!(stopped.settled());
}

#[test]
fn a_press_is_for_the_turn_it_was_pressed_over_and_a_turn_just_pasted_is_never_read_at_rest() {
    let stopped = Stopped::new("stopped-before-a-word");
    stopped.status("idle");
    stopped.at(0);
    assert!(!stopped.settled());
    stopped.window.interrupted();
    stopped.at(5_000);
    assert!(stopped.settled());

    // The words that resume the task are pasted: a user's message of a turn
    // Claude has not begun, in its hooks, idle. The press was not for it.
    let mut records = fixture("stopped-before-a-word", SESSION);
    records.push(format!(
        "{}\n",
        json!({
            "type": "user", "uuid": "resume-1", "parentUuid": null, "isSidechain": false,
            "sessionId": SESSION,
            "message": { "role": "user", "content": "[ConsensFlow m-2 · T-1 · task from @chief]\nResumed: Go on" },
            "timestamp": "2026-10-07T05:07:30.000Z",
        })
    ));
    stopped.home.transcript(SESSION, &records);
    stopped.at(6_000);
    let (settled, last) = stopped.look();
    assert!(
        !settled,
        "no press for it, though one was pressed for the turn before"
    );
    assert!(last.is_some_and(|(role, text)| role == Role::User && text.contains("Resumed: Go on")));
    stopped.at(600_000);
    assert!(!stopped.settled(), "and not later either");
}

#[test]
fn a_window_that_is_busy_or_waiting_is_not_read_at_rest_by_a_press() {
    let stopped = Stopped::new("stopped-before-a-word");
    stopped.status("idle");
    stopped.at(0);
    assert!(!stopped.settled());
    stopped.window.interrupted();
    stopped.at(5_000);
    stopped.status("busy");
    assert!(!stopped.settled());
    stopped.home.status(
        SESSION,
        &json!({ "status": "waiting", "waitingFor": "permission prompt" }),
        me(),
    );
    let observed = finished(stopped.window.observe()).unwrap();
    assert!(!observed.settled);
    assert_eq!(
        observed.waiting,
        Some(Waiting {
            reason: Some("permission prompt".to_owned())
        })
    );
}

#[test]
fn a_turn_claude_has_begun_to_answer_waits_for_its_own_record_of_the_interrupt_whatever_was_pressed(
) {
    // The answer `DONE` is written and a Stop hook runs. The interrupt was
    // pressed and Claude reads idle, but its record of the interrupt is a
    // moment away: the turn is not over until it is written.
    let stopped = Stopped::new("stopped-in-the-stop-hook");
    let mut records = fixture("stopped-in-the-stop-hook", SESSION);
    records.pop();
    stopped.home.transcript(SESSION, &records);
    stopped.status("idle");
    stopped.at(0);
    assert!(!stopped.settled());
    stopped.window.interrupted();
    stopped.at(5_000);
    assert!(
        !stopped.settled(),
        "a word of the answer is written: no press ends such a turn"
    );

    stopped
        .home
        .transcript(SESSION, &fixture("stopped-in-the-stop-hook", SESSION));
    stopped.at(6_000);
    assert!(stopped.settled(), "its record of the interrupt ends it");
}

#[test]
fn a_window_that_shows_another_conversation_forgets_the_press() {
    let stopped = Stopped::new("stopped-before-a-word");
    stopped.status("idle");
    stopped.at(0);
    stopped.settled();
    stopped.window.interrupted();
    stopped.window.follow("another-conversation");
    stopped.window.follow(SESSION);
    stopped.at(5_000);
    assert!(
        !stopped.settled(),
        "what was pressed for was a turn of a conversation the window left"
    );
}

#[test]
fn an_interrupt_record_that_names_no_message_is_the_end_of_the_turn_with_no_press_to_say_so() {
    // Escape in a Stop hook, after the answer was written: Claude's record of
    // the interrupt names no message, and the turn is over for the window.
    let stopped = Stopped::new("stopped-in-the-stop-hook");
    stopped.status("idle");
    stopped.at(0);
    let (settled, last) = stopped.look();
    assert!(settled);
    assert_eq!(
        last,
        Some((Role::User, "[Request interrupted by user]".to_owned()))
    );
}

#[test]
fn the_text_claude_put_back_in_its_input_box_is_cleared_before_the_next_message_is_pasted() {
    let stopped = Stopped::new("stopped-before-a-word");
    stopped.status("idle");
    stopped.at(0);
    stopped.settled();
    stopped.window.interrupted();
    stopped.at(5_000);
    assert!(stopped.settled());

    let host = taking();
    let sent = finished(stopped.window.deliver(&host, &pane(), "Resumed: Go on"));
    assert_eq!(sent, Ok(Admission::Admitted { queued: false }));
    assert_eq!(
        asked(&host),
        [
            ("pane.input".to_owned(), json!([3])),
            ("pane.write_paste".to_owned(), json!("Resumed: Go on")),
        ],
        "Ctrl+C, and then the paste"
    );

    // The paste is in and its record is not yet: the turn it begins is not
    // read at rest by the press for the one before it.
    assert!(!stopped.settled());

    // The turn the paste begins is its own: a second paste has nothing to clear.
    let again = taking();
    finished(stopped.window.deliver(&again, &pane(), "More")).unwrap();
    assert_eq!(
        asked(&again),
        [("pane.write_paste".to_owned(), json!("More"))]
    );
}

#[test]
fn a_window_nobody_interrupted_is_pasted_into_with_no_key_before_it() {
    let stopped = Stopped::new("stopped-in-the-stop-hook");
    stopped.status("idle");
    assert!(stopped.settled());
    let host = taking();
    finished(stopped.window.deliver(&host, &pane(), "Resumed: Go on")).unwrap();
    assert_eq!(
        asked(&host),
        [("pane.write_paste".to_owned(), json!("Resumed: Go on"))],
        "an interrupt that left a record left the input box empty"
    );
}

#[test]
fn an_input_box_that_could_not_be_cleared_is_not_pasted_into_and_is_cleared_again_next_time() {
    let stopped = Stopped::new("stopped-before-a-word");
    stopped.status("idle");
    stopped.at(0);
    stopped.settled();
    stopped.window.interrupted();
    stopped.at(5_000);
    assert!(stopped.settled());

    let refusing = AnsweringHost::new(|op| {
        Ok(match op {
            "pane.input" => json!({ "ok": false, "error": "stale", "cause": "stale pane" }),
            _ => json!({ "ok": true }),
        })
    });
    let refused = finished(stopped.window.deliver(&refusing, &pane(), "Resumed: Go on"));
    assert_eq!(
        refused,
        Ok(Admission::Refused {
            reason: "the window's input box could not be cleared: stale pane".to_owned()
        })
    );
    assert_eq!(
        asked(&refusing),
        [("pane.input".to_owned(), json!([3]))],
        "nothing was pasted after the old text"
    );

    // The next look still reads it so, and the next paste clears first.
    assert!(stopped.settled());
    let host = taking();
    finished(stopped.window.deliver(&host, &pane(), "Resumed: Go on")).unwrap();
    assert_eq!(
        asked(&host).first(),
        Some(&("pane.input".to_owned(), json!([3])))
    );
}

#[test]
fn a_paste_the_window_may_have_taken_in_part_is_cleared_before_the_next_try() {
    let stopped = Stopped::new("stopped-before-a-word");
    stopped.status("idle");
    stopped.at(0);
    stopped.settled();
    stopped.window.interrupted();
    stopped.at(5_000);
    assert!(stopped.settled());

    let breaking = AnsweringHost::new(|op| {
        Ok(match op {
            "pane.write_paste" => json!({ "ok": false, "admitted": null, "error": "uncertain" }),
            _ => json!({ "ok": true }),
        })
    });
    let uncertain = finished(stopped.window.deliver(&breaking, &pane(), "Resumed: Go on")).unwrap();
    assert!(matches!(uncertain, Admission::Uncertain { .. }));

    let host = taking();
    finished(stopped.window.deliver(&host, &pane(), "Resumed: Go on")).unwrap();
    assert_eq!(
        asked(&host).first(),
        Some(&("pane.input".to_owned(), json!([3]))),
        "what the first try left in the box goes before the second"
    );
}

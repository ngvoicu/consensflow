//! Devin's windows, as Node's Devin adapter suite held the tests that need no
//! launch: how a window is interrupted, what its role text says, how it names
//! the conversation it opened, and how a look is told by what its record and
//! its log say. The cases that launch a window are `tests/launch/devin.rs`'s.

use std::fs;
use std::path::Path;
use std::sync::Arc;

use super::*;
use crate::records::{Quota, Reading, Record, Settlement};
use crate::testing::{finished, Driver, Fakes, EPOCH_MS};

mod bypass;

const MINUTE_MS: i64 = 60_000;

/// A window of Devin on `wire`, on `session` if it has one, served by `fakes`.
fn window(fakes: &Fakes, wire: &Path, session: Option<&str>) -> Rc<DevinWindow> {
    Rc::new(DevinWindow {
        env: Env::default(),
        records: Rc::clone(&fakes.records) as Rc<dyn Records>,
        time: Rc::clone(&fakes.time) as Rc<dyn Time>,
        session: RefCell::new(session.map(str::to_owned)),
        wire: WireLog::new(&wire.to_string_lossy(), jiff::tz::TimeZone::UTC),
        discover_for_ms: MINUTE_MS,
        switch: Cell::default(),
        looked: Cell::default(),
        read_empty: Cell::default(),
    })
}

/// The line Devin's log gains when its window configures a conversation it opens.
fn shows(session: &str) -> String {
    let record = serde_json::json!({
        "sessionId": session,
        "update": { "sessionUpdate": "config_option_update", "configOptions": [{ "id": "mode" }] },
    });
    format!("{record}\n")
}

/// A record that says what a test says, whatever conversation it is asked of.
struct Says(Record);

impl Records for Says {
    fn look<'a>(
        &'a self,
        _harness: Harness,
        _session: &'a str,
        _options: &'a Options,
    ) -> Work<'a, Arc<Reading>> {
        let record = self.0.clone();
        Box::pin(async move { Arc::new(Reading::Known(record)) })
    }

    fn has_transcript<'a>(
        &'a self,
        _harness: Harness,
        _session: &'a str,
    ) -> Work<'a, Result<bool, String>> {
        Box::pin(async { Ok(false) })
    }
}

#[test]
fn escape_twice_interrupts_a_turn_and_a_third_a_second_later_closes_the_rewind_it_may_open() {
    // poker-lab, 2026-10-03: two Escapes at a Devin already stopped opened its
    // rewind, and the next message's Enter rewound the conversation.
    let fakes = Fakes::new(&Env::default());
    let services = fakes.services(&Env::default(), Path::new("/nowhere"));
    assert_eq!(
        DevinAdapter::new(&services).interrupt(),
        Interrupt {
            presses: 2,
            close_after: Some(Duration::from_millis(1_000)),
        }
    );
}

#[test]
fn a_windows_machine_tells_devin_how_to_name_files_the_way_its_file_tools_write_them_and_nowhere_else(
) {
    let role = "# ConsensFlow worker\n\nRole text.";
    let windows = role_text(role, &Env::from_vars([("OS", "Windows_NT")]));
    assert!(windows.starts_with(role));
    assert!(windows.contains("never /c/\u{2026} paths"), "{windows}");
    assert_eq!(windows, format!("{role}\n\n{WINDOWS_PATHS}\n"));
    // A Windows machine is Windows whatever its environment says.
    let elsewhere = role_text(role, &Env::from_vars([("HOME", "/h")]));
    assert_eq!(
        elsewhere,
        if cfg!(windows) {
            windows
        } else {
            role.to_owned()
        }
    );
    // None stays none, for the window to be refused as on any machine.
    assert_eq!(role_text("", &Env::from_vars([("OS", "Windows_NT")])), "");
}

#[test]
fn a_window_names_the_conversation_it_opened_after_some_polls() {
    let dir = tempfile::tempdir().unwrap();
    let wire = dir.path().join("wire.jsonl");
    let fakes = Fakes::new(&Env::default());
    let window = window(&fakes, &wire, None);
    let mut driver = Driver::default();
    let started = Rc::clone(&window);
    driver.begin(0, async move { started.started().await });
    assert!(driver.run().is_empty());
    assert_eq!(fakes.time.waits(0), [250]);
    assert!(
        !fakes.time.fire_next(EPOCH_MS + 249),
        "not before it is due"
    );
    assert!(fakes.time.fire_next(EPOCH_MS + 250));
    assert!(driver.run().is_empty());
    assert_eq!(fakes.time.waits(0), [250], "asked again, and waiting again");
    fs::write(&wire, shows("mild-coin")).unwrap();
    assert!(fakes.time.fire_next(EPOCH_MS + 500));
    assert_eq!(driver.run(), [(0, Ok(Some("mild-coin".to_owned())))]);
    assert_eq!(window.session.borrow().as_deref(), Some("mild-coin"));
}

#[test]
fn a_window_that_never_names_one_is_given_up_on_at_its_deadline_and_not_before() {
    let dir = tempfile::tempdir().unwrap();
    let wire = dir.path().join("wire.jsonl");
    fs::write(&wire, "not json\n").unwrap();
    let fakes = Fakes::new(&Env::default());
    let window = window(&fakes, &wire, None);
    let mut driver = Driver::default();
    driver.begin(0, async move { window.started().await });
    assert!(driver.run().is_empty());
    let mut polls = 0;
    while fakes.time.fire_next(EPOCH_MS + 59_999) {
        polls += 1;
        assert!(driver.run().is_empty(), "poll {polls}");
    }
    assert_eq!(polls, 239, "a poll every 250 ms to its last, 59 750 ms in");
    fakes.time.settle_at(EPOCH_MS + 59_999);
    assert_eq!(
        fakes.time.waits(0),
        [1],
        "its last sleep ends at the deadline"
    );
    assert!(fakes.time.fire_next(EPOCH_MS + MINUTE_MS));
    assert_eq!(
        driver.run(),
        [(
            0,
            Err("Devin never said which session it opened (its wire log stayed empty)".to_owned())
        )]
    );
    assert!(fakes.time.waits(0).is_empty());
}

#[test]
fn a_window_that_has_its_conversation_polls_nothing() {
    let fakes = Fakes::new(&Env::default());
    let window = window(&fakes, Path::new("/nowhere/wire.jsonl"), Some("mild-coin"));
    assert_eq!(finished(window.started()), Ok(None));
    assert!(fakes.time.waits(0).is_empty());
}

#[test]
fn a_window_that_has_named_no_conversation_says_nothing_of_one_to_look_at() {
    let fakes = Fakes::new(&Env::default());
    let window = window(&fakes, Path::new("/nowhere/wire.jsonl"), None);
    let observed = finished(window.observe()).unwrap();
    assert_eq!(
        observed,
        Observed {
            reading: None,
            settled: false,
            waiting: None,
            failed: false,
            quota: None,
            switched: None,
            unnamed: false,
            took_back: false,
        }
    );
}

#[test]
fn a_window_is_followed_to_the_conversation_the_human_switched_it_to() {
    let dir = tempfile::tempdir().unwrap();
    let wire = dir.path().join("wire.jsonl");
    fs::write(
        &wire,
        format!("{}{}", shows("mild-coin"), shows("fresh-leaf")),
    )
    .unwrap();
    let fakes = Fakes::new(&Env::default());
    let window = window(&fakes, &wire, Some("mild-coin"));
    let observed = finished(window.observe()).unwrap();
    assert_eq!(observed.switched.as_deref(), Some("fresh-leaf"));
    assert!(!observed.settled);
    window.follow("fresh-leaf");
    let observed = finished(window.observe()).unwrap();
    assert_eq!(observed.switched, None);
    assert!(
        observed.settled,
        "a record that cannot be read has nothing to finish"
    );
}

#[test]
fn a_look_takes_the_wires_word_on_the_quota_never_the_records_and_the_rest_from_the_record() {
    let dir = tempfile::tempdir().unwrap();
    let wire = dir.path().join("wire.jsonl");
    fs::write(&wire, shows("mild-coin")).unwrap();
    let fakes = Fakes::new(&Env::default());
    let mut record = Record::new();
    record.settlement = Settlement::Settled;
    record.failed = true;
    record.quota = Some(Arc::new(Quota::Exhausted {
        at: Some("2026-01-01T00:00:00.000Z".to_owned()),
        resets_at: None,
    }));
    let window = Rc::new(DevinWindow {
        env: Env::default(),
        records: Rc::new(Says(record)),
        time: Rc::clone(&fakes.time) as Rc<dyn Time>,
        session: RefCell::new(Some("mild-coin".to_owned())),
        wire: WireLog::new(&wire.to_string_lossy(), jiff::tz::TimeZone::UTC),
        discover_for_ms: MINUTE_MS,
        switch: Cell::default(),
        looked: Cell::default(),
        read_empty: Cell::default(),
    });
    let observed = finished(window.observe()).unwrap();
    assert_eq!(observed.quota, None, "the log says nothing of the quota");
    assert!(
        observed.settled && observed.failed,
        "the rest is the record's"
    );
}

/// The log gains `text`.
fn append(wire: &Path, text: &str) {
    let mut log = fs::read(wire).unwrap_or_default();
    log.extend_from_slice(text.as_bytes());
    fs::write(wire, log).unwrap();
}

/// A prompt of the TUI to its agent, as its request is in the log.
fn asks(session: &str, text: &str) -> String {
    let record = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "session/prompt",
        "params": { "sessionId": session, "prompt": [{ "type": "text", "text": text }] },
    });
    format!("{record}\n")
}

/// A window on `wire`, conversation `mild-coin`, whose record says `record`.
fn window_reading(fakes: &Fakes, wire: &Path, env: Env, record: Record) -> Rc<DevinWindow> {
    Rc::new(DevinWindow {
        env,
        records: Rc::new(Says(record)),
        time: Rc::clone(&fakes.time) as Rc<dyn Time>,
        session: RefCell::new(Some("mild-coin".to_owned())),
        wire: WireLog::new(&wire.to_string_lossy(), jiff::tz::TimeZone::UTC),
        discover_for_ms: MINUTE_MS,
        switch: Cell::default(),
        looked: Cell::default(),
        read_empty: Cell::default(),
    })
}

/// A window on `wire`, conversation `mild-coin`, whose record reads at rest: its
/// message is the last of it, and its agent has said nothing.
fn window_at_rest(fakes: &Fakes, wire: &Path, env: Env) -> Rc<DevinWindow> {
    let mut record = Record::new();
    record.settlement = Settlement::Settled;
    record.items.push(crate::records::Item {
        id: Arc::from("row-1"),
        role: crate::records::Role::User,
        text: Arc::from("hello"),
        complete: true,
        at: None,
        commentary: false,
        compaction: false,
    });
    window_reading(fakes, wire, env, record)
}

#[test]
fn a_window_its_record_reads_at_rest_is_read_in_the_turn_its_tui_begins_by_a_prompt_for_the_gap() {
    let dir = tempfile::tempdir().unwrap();
    let wire = dir.path().join("wire.jsonl");
    fs::write(&wire, shows("mild-coin")).unwrap();
    let fakes = Fakes::new(&Env::default());
    let window = window_at_rest(&fakes, &wire, Env::default());
    assert!(
        finished(window.observe()).unwrap().settled,
        "nothing asked of its agent: at rest"
    );
    // The TUI puts a prompt to its agent, which has said nothing of it.
    append(&wire, &asks("mild-coin", "hello"));
    assert!(!finished(window.observe()).unwrap().settled);
    fakes.time.settle_at(EPOCH_MS + PROMPT_GAP_MS - 1);
    assert!(!finished(window.observe()).unwrap().settled);
    // An agent that says nothing for the whole gap is read as its record reads it.
    fakes.time.settle_at(EPOCH_MS + PROMPT_GAP_MS);
    assert!(finished(window.observe()).unwrap().settled);
}

#[test]
fn a_prompt_begins_no_turn_of_a_conversation_whose_record_holds_no_message_yet() {
    let dir = tempfile::tempdir().unwrap();
    let wire = dir.path().join("wire.jsonl");
    fs::write(&wire, shows("mild-coin")).unwrap();
    let fakes = Fakes::new(&Env::default());
    let mut record = Record::new();
    record.settlement = Settlement::Settled;
    let window = window_reading(&fakes, &wire, Env::default(), record);
    append(&wire, &asks("mild-coin", "hello"));
    assert!(finished(window.observe()).unwrap().settled);
}

#[test]
fn a_turn_the_agent_ends_or_a_prompt_it_answers_ends_the_turn_its_tui_began_in_the_window() {
    let dir = tempfile::tempdir().unwrap();
    let wire = dir.path().join("wire.jsonl");
    fs::write(&wire, shows("mild-coin")).unwrap();
    let fakes = Fakes::new(&Env::default());
    let window = window_at_rest(&fakes, &wire, Env::default());
    append(&wire, &asks("mild-coin", "hello"));
    assert!(!finished(window.observe()).unwrap().settled);
    append(
        &wire,
        "{\"cause\":\"complete\",\"sessionId\":\"mild-coin\"}\n",
    );
    assert!(finished(window.observe()).unwrap().settled);
    // A prompt the agent refuses ends with its answer, and no turn's end.
    append(&wire, &asks("mild-coin", "again"));
    assert!(!finished(window.observe()).unwrap().settled);
    append(
        &wire,
        "{\"jsonrpc\":\"2.0\",\"id\":2,\"error\":{\"code\":-32011,\"message\":\"Quota exhausted.\"}}\n",
    );
    assert!(finished(window.observe()).unwrap().settled);
    // A prompt of another conversation is none of this window's.
    append(&wire, &asks("another-one", "elsewhere"));
    assert!(finished(window.observe()).unwrap().settled);
}

/// The end of the turn the window's conversation is in, as Devin's agent writes it.
fn turn_ends() -> &'static str {
    "{\"cause\":\"complete\",\"sessionId\":\"mild-coin\"}\n"
}

#[test]
fn a_message_pasted_is_held_in_the_turn_of_the_tuis_prompt_its_line_begins_and_no_longer() {
    let dir = tempfile::tempdir().unwrap();
    let wire = dir.path().join("wire.jsonl");
    fs::write(&wire, shows("mild-coin")).unwrap();
    let fakes = Fakes::new(&Env::default());
    let window = window_at_rest(&fakes, &wire, Env::default());
    let host = crate::testing::AnsweringHost::new(|_| Ok(serde_json::json!({ "ok": true })));
    let pane = Pane {
        id: "s1-zeus".to_owned(),
        generation: 2,
    };
    let text = "[ConsensFlow m-9 · T-1 · note from ConsensFlow]\n@human cancelled T-1";
    assert_eq!(
        finished(window.deliver(&host, &pane, text)),
        Ok(Admission::Admitted { queued: false }),
        "the paste is the window's, and no word of the agent's queue"
    );
    assert!(!window.held(), "pasted at rest");
    // What was pasted, as the TUI has it: on Windows, what the console carried
    // (Windows CI, 2026-10-15: the middle dots).
    let pasted = if Env::default().on_windows() {
        console_text(&window_text(text))
    } else {
        text.to_owned()
    };
    let line = pasted.lines().next().unwrap();
    // The human's prompt that quotes its line is no paste of it (the reviews of
    // 2026-10-16): a turn begun after the paste, which may hold it, while it is on.
    append(
        &wire,
        &asks("mild-coin", &format!("Why is \"{line}\" still queued?")),
    );
    assert!(window.held());
    append(&wire, turn_ends());
    assert!(!window.held(), "its turn is over");
    // None of the TUI's prompts was the paste, and a turn ended since: a prompt asked
    // now is no turn it went into.
    append(&wire, &asks("mild-coin", "and now?"));
    assert!(!window.held());
    append(&wire, turn_ends());
    // The TUI's prompt of what was pasted: the agent runs it in that prompt's turn,
    // and its record shows it when the turn ends.
    append(&wire, &asks("mild-coin", &pasted));
    assert!(window.held());
    append(&wire, turn_ends());
    assert!(!window.held());
    // The next paste is another message, pasted at rest.
    let next = "[ConsensFlow m-10 · T-1 · note from ConsensFlow]\nlater";
    assert!(finished(window.deliver(&host, &pane, next)).is_ok());
    assert!(!window.held());
}

/// Its agent has the paste once the TUI put it to its agent in a prompt its line begins,
/// whatever came of that turn; words that quote the line put nothing to it, and the
/// next paste is another's to say.
#[test]
fn the_agent_has_a_paste_once_the_tuis_prompt_its_line_begins_is_read_and_ever_after() {
    let dir = tempfile::tempdir().unwrap();
    let wire = dir.path().join("wire.jsonl");
    fs::write(&wire, shows("mild-coin")).unwrap();
    let fakes = Fakes::new(&Env::default());
    let window = window_at_rest(&fakes, &wire, Env::default());
    let host = crate::testing::AnsweringHost::new(|_| Ok(serde_json::json!({ "ok": true })));
    let pane = Pane {
        id: "s1-zeus".to_owned(),
        generation: 2,
    };
    let text = "[ConsensFlow m-9 · T-1 · note from ConsensFlow]\n@human cancelled T-1";
    assert!(finished(window.deliver(&host, &pane, text)).is_ok());
    assert!(!window.took(), "pasted, not yet put to the agent");
    let pasted = if Env::default().on_windows() {
        console_text(&window_text(text))
    } else {
        text.to_owned()
    };
    let line = pasted.lines().next().unwrap();
    append(
        &wire,
        &asks("mild-coin", &format!("Why is \"{line}\" still queued?")),
    );
    append(&wire, turn_ends());
    assert!(!window.took(), "words that quote it are no paste of it");
    append(&wire, &asks("mild-coin", &pasted));
    assert!(window.took());
    append(&wire, turn_ends());
    assert!(window.took(), "and its turn's end takes nothing back");
    let next = "[ConsensFlow m-10 · T-1 · note from ConsensFlow]\nlater";
    assert!(finished(window.deliver(&host, &pane, next)).is_ok());
    assert!(!window.took(), "the next paste is not yet put to it");
}

#[test]
fn a_message_pasted_into_a_turn_whose_agent_has_said_nothing_past_the_gap_is_held_while_the_turn_is_on(
) {
    let dir = tempfile::tempdir().unwrap();
    let wire = dir.path().join("wire.jsonl");
    fs::write(&wire, shows("mild-coin")).unwrap();
    let fakes = Fakes::new(&Env::default());
    let window = window_at_rest(&fakes, &wire, Env::default());
    let host = crate::testing::AnsweringHost::new(|_| Ok(serde_json::json!({ "ok": true })));
    let pane = Pane {
        id: "s1-zeus".to_owned(),
        generation: 2,
    };
    // The TUI put a prompt to its agent, which says nothing past the gap: the window
    // reads at rest, as its record does, and a message goes in.
    append(&wire, &asks("mild-coin", "hello"));
    assert!(!finished(window.observe()).unwrap().settled, "a turn begun");
    fakes.time.settle_at(EPOCH_MS + PROMPT_GAP_MS);
    assert!(finished(window.observe()).unwrap().settled);
    assert!(finished(window.ready(&host, &pane)).is_ok());
    let text = "[ConsensFlow m-9 · T-1 · result from @worker]\nRESULT_ONE";
    assert!(finished(window.deliver(&host, &pane, text)).is_ok());
    // The turn is on still: Devin holds the message, and has said nothing of it.
    assert!(window.held());
    // The turn ends: the message is held no longer, and the next paste, at rest, is
    // held by nothing.
    append(
        &wire,
        "{\"cause\":\"complete\",\"sessionId\":\"mild-coin\"}\n",
    );
    assert!(!window.held());
    let next = "[ConsensFlow m-10 · T-2 · result from @worker]\nRESULT_TWO";
    assert!(finished(window.deliver(&host, &pane, next)).is_ok());
    assert!(!window.held());
}

#[test]
fn a_message_pasted_as_a_prompt_begins_while_it_is_handed_over_is_held_in_its_turn() {
    let dir = tempfile::tempdir().unwrap();
    let wire = dir.path().join("wire.jsonl");
    fs::write(&wire, shows("mild-coin")).unwrap();
    let fakes = Fakes::new(&Env::default());
    let window = window_at_rest(&fakes, &wire, Env::default());
    // The human's prompt begins after readiness read the wire at rest, as the paste is
    // handed over (a review of 2026-10-16). One begun while the screen is looked at holds
    // the window (B1's second read of the wire).
    let human = std::cell::Cell::new(false);
    let host = crate::testing::AnsweringHost::new(|op| {
        if op == "pane.write_paste" && !human.replace(true) {
            append(&wire, &asks("mild-coin", "a message of the human's"));
        }
        Ok(serde_json::json!({ "ok": true }))
    });
    let pane = Pane {
        id: "s1-zeus".to_owned(),
        generation: 2,
    };
    assert!(finished(window.observe()).unwrap().settled);
    assert!(matches!(
        finished(window.ready(&host, &pane)),
        Ok(Readiness::Ready)
    ));
    let text = "[ConsensFlow m-9 · T-1 · result from @worker]\nRESULT_ONE";
    assert!(finished(window.deliver(&host, &pane, text)).is_ok());
    assert!(human.get(), "the prompt began as the paste was handed over");
    // The TUI queued the paste behind the human's turn: held, however long its agent
    // says nothing.
    fakes.time.settle_at(EPOCH_MS + PROMPT_GAP_MS);
    assert!(window.held());
}

#[test]
fn on_windows_a_message_is_told_by_its_first_line_as_the_console_carries_it_to_devin() {
    let dir = tempfile::tempdir().unwrap();
    let wire = dir.path().join("wire.jsonl");
    fs::write(&wire, shows("mild-coin")).unwrap();
    let fakes = Fakes::new(&Env::default());
    let window = window_at_rest(&fakes, &wire, Env::from_vars([("OS", "Windows_NT")]));
    let host = crate::testing::AnsweringHost::new(|_| Ok(serde_json::json!({ "ok": true })));
    let pane = Pane {
        id: "s1-zeus".to_owned(),
        generation: 2,
    };
    let text = "[ConsensFlow m-9 · T-1 · note from ConsensFlow]\n@human cancelled T-1";
    assert!(finished(window.deliver(&host, &pane, text)).is_ok());
    // The TUI had what the console carried: the middle dots are bars. A turn begun after
    // the paste may be the one it went into; once it ends, words with the dots are no
    // paste of it, and their turn holds nothing.
    append(&wire, &asks("mild-coin", "a message of the human's"));
    assert!(window.held(), "the turn it may have gone into");
    append(&wire, turn_ends());
    append(&wire, &asks("mild-coin", text));
    assert!(!window.held(), "words that were not pasted");
    append(&wire, turn_ends());
    append(
        &wire,
        &asks(
            "mild-coin",
            "[ConsensFlow m-9 | T-1 | note from ConsensFlow]\n@human cancelled T-1",
        ),
    );
    assert!(window.held(), "in its own prompt's turn");
    append(&wire, turn_ends());
    // The TUI put it to its agent: a prompt after it is no turn it waits behind.
    append(&wire, &asks("mild-coin", "and now?"));
    assert!(!window.held());
}

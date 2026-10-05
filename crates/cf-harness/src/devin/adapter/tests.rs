//! Devin's windows, as `tests/adapter-devin.test.mjs` holds Node's that need
//! no launch: how a window is interrupted, what its role text says, how it
//! names the conversation it opened, and how a look is told by what its
//! record and its log say. The cases that launch a window are
//! `tests/launch/devin.rs`'s.

use std::fs;
use std::path::Path;
use std::sync::Arc;

use super::*;
use crate::records::{Quota, Record, Settlement};
use crate::testing::{finished, Driver, Fakes, EPOCH_MS};

const MINUTE_MS: i64 = 60_000;

/// A window of Devin on `wire`, on `session` if it has one, served by `fakes`.
fn window(fakes: &Fakes, wire: &Path, session: Option<&str>) -> Rc<DevinWindow> {
    Rc::new(DevinWindow {
        env: Env::default(),
        records: Rc::clone(&fakes.records) as Rc<dyn Records>,
        time: Rc::clone(&fakes.time) as Rc<dyn Time>,
        session: RefCell::new(session.map(str::to_owned)),
        wire: WireLog::new(&wire.to_string_lossy()),
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
fn a_dialog_of_its_own_still_open_is_a_window_waiting_and_nothing_else_is() {
    let mut record = Record::new();
    assert_eq!(dialog_waiting(Some(&Reading::Known(record.clone()))), None);
    record.asking = true;
    assert_eq!(
        dialog_waiting(Some(&Reading::Known(record))),
        Some(Waiting {
            reason: Some("its own question dialog is open".to_owned())
        })
    );
    assert_eq!(
        dialog_waiting(Some(&Reading::Unknown("unreadable".to_owned()))),
        None
    );
    assert_eq!(dialog_waiting(None), None);
}

#[test]
fn a_window_switched_away_settles_nothing_and_waits_for_nothing_and_one_unnamed_waits_for_its_name()
{
    let mut record = Record::new();
    record.settlement = Settlement::Settled;
    let observed = record_state(Arc::new(Reading::Known(record)));
    let observed = Observed {
        waiting: Some(Waiting { reason: None }),
        ..observed
    };
    assert!(observed.settled);
    let switched = switched_to(observed.clone(), "fresh-leaf".to_owned());
    assert_eq!(
        (
            switched.settled,
            switched.waiting.is_some(),
            switched.switched.as_deref(),
            switched.unnamed
        ),
        (false, false, Some("fresh-leaf"), false)
    );
    let held = unnamed(observed, HOLD);
    assert_eq!(
        held.waiting,
        Some(Waiting {
            reason: Some(HOLD.to_owned())
        })
    );
    assert!(held.unnamed && held.settled && held.switched.is_none());
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
        wire: WireLog::new(&wire.to_string_lossy()),
    });
    let observed = finished(window.observe()).unwrap();
    assert_eq!(observed.quota, None, "the log says nothing of the quota");
    assert!(
        observed.settled && observed.failed,
        "the rest is the record's"
    );
}

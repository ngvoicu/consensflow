//! Devin's windows, as Node's Devin adapter suite held the tests that need no
//! launch: how a window is interrupted, what its role text says, how it names
//! the conversation it opened, and how a look is told by what its record and
//! its log say. The cases that launch a window are `tests/launch/devin.rs`'s.

use std::fs;
use std::path::Path;
use std::sync::Arc;

use super::*;
use crate::records::{Quota, Reading, Record, Settlement};
use crate::testing::{finished, Driver, Fakes, ScriptedHost, EPOCH_MS};

mod bypass;

const MINUTE_MS: i64 = 60_000;

/// A window of Devin on `wire`, on `session` if it has one, served by `fakes`.
fn window(fakes: &Fakes, wire: &Path, session: Option<&str>) -> Rc<DevinWindow> {
    devin_window(
        Env::default(),
        Rc::clone(&fakes.records) as Rc<dyn Records>,
        fakes,
        wire,
        session,
        true,
    )
}

/// A window of Devin on `wire` and `session`, its store the one `env` names, its record
/// `records`, the clock `fakes`'; `at_enter` where its Devin writes a prompt's history
/// row at the Enter.
fn devin_window(
    env: Env,
    records: Rc<dyn Records>,
    fakes: &Fakes,
    wire: &Path,
    session: Option<&str>,
    at_enter: bool,
) -> Rc<DevinWindow> {
    Rc::new(DevinWindow {
        env,
        records,
        time: Rc::clone(&fakes.time) as Rc<dyn Time>,
        session: RefCell::new(session.map(str::to_owned)),
        wire: WireLog::new(&wire.to_string_lossy(), jiff::tz::TimeZone::UTC),
        discover_for_ms: MINUTE_MS,
        switch: Cell::default(),
        at_enter,
        history: History::default(),
        looked: Cell::default(),
        read_empty: Cell::default(),
        input: RefCell::default(),
        left_open: RefCell::new(None),
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
    driver.begin(
        0,
        async move { started.started(&ScriptedHost::default()).await },
    );
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
    driver.begin(
        0,
        async move { window.started(&ScriptedHost::default()).await },
    );
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
    assert_eq!(finished(window.started(&ScriptedHost::default())), Ok(None));
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
            over: Over::No,
            waiting: None,
            failed: false,
            quota: None,
            switched: None,
            unnamed: false,
            took_back: false,
            at_prompt: false,
            shared: false,
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
fn a_look_keeps_the_records_quota_where_the_log_says_none_and_the_rest_from_the_record() {
    // Devin's reader says no quota of its own (its refusals are the wire log's, until the next
    // prompt), so a record that carries one is a synthetic one: the look's precedence is the one
    // place's (`observed_from`: the move, the live word, the record's), and a silent log takes
    // nothing from it (T-853).
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
    let window = devin_window(
        Env::default(),
        Rc::new(Says(record)),
        &fakes,
        &wire,
        Some("mild-coin"),
        true,
    );
    let observed = finished(window.observe()).unwrap();
    assert_eq!(
        observed.quota,
        Some(Arc::new(Quota::Exhausted {
            at: Some("2026-01-01T00:00:00.000Z".to_owned()),
            resets_at: None,
        })),
        "the log says nothing of the quota: the record's stands"
    );
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

/// Devin's store of a window's sessions, under a root of its own: its env names it.
struct Store {
    _root: tempfile::TempDir,
    env: Env,
    db: rusqlite::Connection,
}

impl Store {
    /// A store whose history has no row yet, where `vars` (more of the env) say so.
    fn new(vars: &[(&str, &str)]) -> Self {
        let root = tempfile::tempdir().unwrap();
        let data = root.path().join("data");
        let under = data.to_string_lossy().into_owned();
        let mut all = vec![
            ("XDG_DATA_HOME", under.as_str()),
            ("APPDATA", under.as_str()),
        ];
        all.extend_from_slice(vars);
        let env = Env::from_vars(all);
        fs::create_dir_all(data.join("devin/cli")).unwrap();
        let db = rusqlite::Connection::open(data.join("devin/cli/sessions.db")).unwrap();
        db.execute_batch(
            "CREATE TABLE prompt_history (id INTEGER PRIMARY KEY AUTOINCREMENT, content TEXT NOT NULL,
               timestamp INTEGER NOT NULL, session_id TEXT NOT NULL, is_shell INTEGER NOT NULL DEFAULT 0)",
        )
        .unwrap();
        Self {
            _root: root,
            env,
            db,
        }
    }

    /// The TUI of `session` submitted `text` at `at_ms` (Devin writes the second).
    fn submit(&self, session: &str, text: &str, at_ms: i64) {
        self.db
            .execute(
                "INSERT INTO prompt_history (content, timestamp, session_id) VALUES (?, ?, ?)",
                (text, at_ms / 1_000, session),
            )
            .unwrap();
    }
}

/// A host that takes every paste and says every screen is drawn and still.
fn host() -> crate::testing::AnsweringHost<
    impl Fn(&str) -> Result<serde_json::Value, crate::contract::HostError>,
> {
    crate::testing::AnsweringHost::new(|_| {
        Ok(serde_json::json!({ "ok": true, "outputQuietMs": 60_000 }))
    })
}

fn pane() -> Pane {
    Pane {
        id: "s1-zeus".to_owned(),
        generation: 2,
    }
}

/// A record whose items are `texts`, each the user's, at rest.
fn record_of(texts: &[&str]) -> Record {
    let mut record = Record::new();
    record.settlement = Settlement::Settled;
    for (at, text) in texts.iter().enumerate() {
        record.items.push(crate::records::Item {
            id: Arc::from(format!("row-{at}")),
            role: crate::records::Role::User,
            text: Arc::from(*text),
            complete: true,
            at: None,
            commentary: false,
            synthetic: false,
            compaction: false,
        });
    }
    record
}

/// T-102 (Devin 3000.11.92, probed live): the TUI writes a prompt's history row, whole,
/// at the Enter, a paste queued behind a turn at work too, and the wire log holds no
/// request of the TUI's. The store's row is the window's word that its TUI took the paste:
/// a row of the conversation shown, written since the hand-over, beginning with the whole
/// paste, and no other.
#[test]
fn the_tui_took_a_paste_once_its_store_has_a_row_of_it_whole_in_the_conversation_shown_since_the_hand_over(
) {
    let dir = tempfile::tempdir().unwrap();
    let wire = dir.path().join("wire.jsonl");
    fs::write(&wire, shows("mild-coin")).unwrap();
    let store = Store::new(&[]);
    let fakes = Fakes::new(&Env::default());
    let window = devin_window(
        store.env.clone(),
        Rc::new(Says(record_of(&[]))),
        &fakes,
        &wire,
        Some("mild-coin"),
        true,
    );
    let text = "[ConsensFlow m-9 \u{b7} T-1 \u{b7} result from @worker]\nRESULT_ONE";
    let pasted = if Env::default().on_windows() {
        console_text(&window_text(text))
    } else {
        window_text(text)
    };
    // An earlier attempt's row, a minute before the hand-over, is no word of this one.
    store.submit("mild-coin", &pasted, EPOCH_MS - MINUTE_MS);
    assert!(finished(window.deliver(&host(), &pane(), text)).is_ok());
    assert!(!window.took(), "nothing submitted since the hand-over");
    // Another conversation's row, and one that begins with its header and goes on in
    // other words, are none of it.
    store.submit("fresh-leaf", &pasted, EPOCH_MS);
    let header = pasted.lines().next().unwrap();
    store.submit("mild-coin", &format!("{header}\nother words"), EPOCH_MS);
    store.submit(
        "mild-coin",
        &format!("Why is this queued? {pasted}"),
        EPOCH_MS,
    );
    assert!(!window.took());
    // Its own row, in the second of the hand-over, behind the space the TUI keeps.
    store.submit("mild-coin", &format!("\n{pasted}\n"), EPOCH_MS + 300);
    assert!(window.took());
    assert!(window.took(), "and ever after");
    // The next paste is another's to say.
    assert!(
        finished(window.deliver(&host(), &pane(), "[ConsensFlow m-10 \u{b7} note]\nlater")).is_ok()
    );
    assert!(!window.took());
}

/// The engine reads `held` only where `took` reads false: a paste Devin's store took is
/// taken whether its record shows it yet or not, and `took` alone keeps it once.
#[test]
fn a_paste_its_store_took_is_taken_whether_its_record_shows_it_yet_or_not() {
    let dir = tempfile::tempdir().unwrap();
    let wire = dir.path().join("wire.jsonl");
    fs::write(&wire, shows("mild-coin")).unwrap();
    let store = Store::new(&[]);
    let fakes = Fakes::new(&Env::default());
    let text = "[ConsensFlow m-9 \u{b7} note]\nwords";
    let pasted = if Env::default().on_windows() {
        console_text(&window_text(text))
    } else {
        window_text(text)
    };
    // The record of the turn the paste is queued behind, without the paste.
    let behind = devin_window(
        store.env.clone(),
        Rc::new(Says(record_of(&["a message of the human's"]))),
        &fakes,
        &wire,
        Some("mild-coin"),
        true,
    );
    assert!(finished(behind.deliver(&host(), &pane(), text)).is_ok());
    assert!(!behind.took(), "not taken yet");
    store.submit("mild-coin", &pasted, EPOCH_MS);
    finished(behind.observe()).unwrap();
    assert!(behind.took(), "taken, queued behind the turn at work");
    // The turn takes it in: the record shows it, and it is taken still.
    let taken = devin_window(
        store.env.clone(),
        Rc::new(Says(record_of(&["a message of the human's", &pasted]))),
        &fakes,
        &wire,
        Some("mild-coin"),
        true,
    );
    assert!(finished(taken.deliver(&host(), &pane(), text)).is_ok());
    finished(taken.observe()).unwrap();
    assert!(taken.took());
    assert!(!behind.held() && !taken.held(), "the contract's own false");
}

/// An older Devin writes the row only as the turn takes the paste in, as late as its
/// record: its window says nothing of it, and the engine's rest-based wait is its path.
#[test]
fn an_older_devins_window_says_nothing_of_its_stores_history() {
    let dir = tempfile::tempdir().unwrap();
    let wire = dir.path().join("wire.jsonl");
    fs::write(&wire, shows("mild-coin")).unwrap();
    let store = Store::new(&[]);
    let fakes = Fakes::new(&Env::default());
    let window = devin_window(
        store.env.clone(),
        Rc::new(Says(record_of(&[]))),
        &fakes,
        &wire,
        Some("mild-coin"),
        false,
    );
    let text = "[ConsensFlow m-9 \u{b7} note]\nwords";
    assert!(finished(window.deliver(&host(), &pane(), text)).is_ok());
    store.submit("mild-coin", &window_text(text), EPOCH_MS);
    store.submit("mild-coin", &console_text(&window_text(text)), EPOCH_MS);
    assert!(!window.took());
    assert!(history::at_enter("devin 3000.11.92 (7f4e64980e22)"));
    assert!(history::at_enter("devin 3000.12.0"));
    assert!(!history::at_enter("devin 3000.11.3 (abc)"));
    assert!(!history::at_enter("devin 3000.10.31"));
    assert!(!history::at_enter("no version"));
}

/// The store is never waited on: a look that meets its writer's lock answers what the
/// last look that read it answered.
#[test]
fn a_locked_store_answers_what_the_last_look_that_read_it_answered() {
    let dir = tempfile::tempdir().unwrap();
    let wire = dir.path().join("wire.jsonl");
    fs::write(&wire, shows("mild-coin")).unwrap();
    let store = Store::new(&[]);
    let fakes = Fakes::new(&Env::default());
    let window = devin_window(
        store.env.clone(),
        Rc::new(Says(record_of(&[]))),
        &fakes,
        &wire,
        Some("mild-coin"),
        true,
    );
    let text = "[ConsensFlow m-9 \u{b7} note]\nwords";
    let pasted = if Env::default().on_windows() {
        console_text(&window_text(text))
    } else {
        window_text(text)
    };
    assert!(finished(window.deliver(&host(), &pane(), text)).is_ok());
    store.submit("mild-coin", &pasted, EPOCH_MS);
    assert!(window.took());
    store.db.execute_batch("BEGIN EXCLUSIVE").unwrap();
    assert!(window.took(), "the lock takes nothing back");
    store.db.execute_batch("COMMIT").unwrap();
    // A store gone between looks is one that cannot be read either.
    let gone = devin_window(
        Env::from_vars([("XDG_DATA_HOME", "/nowhere"), ("APPDATA", "/nowhere")]),
        Rc::new(Says(record_of(&[]))),
        &fakes,
        &wire,
        Some("mild-coin"),
        true,
    );
    assert!(finished(gone.deliver(&host(), &pane(), text)).is_ok());
    assert!(!gone.took());
}

/// A prompt the human submitted after the engine's look found the window at rest begins
/// or queues a turn the record may not show yet: the window is not ready for a paste
/// (Astra on the B1 fix), by its store's history, its wire writing no prompt.
#[test]
fn a_prompt_submitted_since_the_engines_look_holds_the_window_from_a_paste() {
    let dir = tempfile::tempdir().unwrap();
    let wire = dir.path().join("wire.jsonl");
    fs::write(&wire, shows("mild-coin")).unwrap();
    let store = Store::new(&[]);
    store.submit("mild-coin", "an earlier prompt", EPOCH_MS - MINUTE_MS);
    let fakes = Fakes::new(&Env::default());
    let window = devin_window(
        store.env.clone(),
        Rc::new(Says(record_of(&["an earlier prompt"]))),
        &fakes,
        &wire,
        Some("mild-coin"),
        true,
    );
    assert!(finished(window.observe()).unwrap().settled);
    assert!(matches!(
        finished(window.ready(&host(), &pane())),
        Ok(Readiness::Ready)
    ));
    // Another conversation's prompt is none of this window's.
    store.submit("fresh-leaf", "elsewhere", EPOCH_MS);
    assert!(matches!(
        finished(window.ready(&host(), &pane())),
        Ok(Readiness::Ready)
    ));
    store.submit("mild-coin", "a message of the human's", EPOCH_MS);
    assert!(matches!(
        finished(window.ready(&host(), &pane())),
        Ok(Readiness::Held(Held::Unsaid))
    ));
    // The next look sees it: the record says the rest.
    finished(window.observe()).unwrap();
    assert!(matches!(
        finished(window.ready(&host(), &pane())),
        Ok(Readiness::Ready)
    ));
    // An older Devin's window holds nothing for it.
    let older = devin_window(
        store.env.clone(),
        Rc::new(Says(record_of(&[]))),
        &fakes,
        &wire,
        Some("mild-coin"),
        false,
    );
    finished(older.observe()).unwrap();
    store.submit("mild-coin", "and again", EPOCH_MS);
    assert!(matches!(
        finished(older.ready(&host(), &pane())),
        Ok(Readiness::Ready)
    ));
}

#[test]
fn on_windows_a_paste_is_told_by_its_history_row_as_the_console_carries_it_to_devin() {
    let dir = tempfile::tempdir().unwrap();
    let wire = dir.path().join("wire.jsonl");
    fs::write(&wire, shows("mild-coin")).unwrap();
    let store = Store::new(&[("OS", "Windows_NT")]);
    let fakes = Fakes::new(&Env::default());
    let window = devin_window(
        store.env.clone(),
        Rc::new(Says(record_of(&[]))),
        &fakes,
        &wire,
        Some("mild-coin"),
        true,
    );
    let text = "[ConsensFlow m-9 \u{b7} T-1 \u{b7} note from ConsensFlow]\n@human cancelled T-1";
    assert!(finished(window.deliver(&host(), &pane(), text)).is_ok());
    // The TUI had what the console carried: the middle dots are bars.
    store.submit("mild-coin", text, EPOCH_MS);
    assert!(!window.took(), "words that were not pasted");
    store.submit("mild-coin", &console_text(&window_text(text)), EPOCH_MS);
    assert!(window.took());
}

/// What the live tests ask of a window they pasted into themselves: the store's word,
/// by the same rule the engine reads it by.
#[test]
fn the_live_tests_read_a_paste_taken_from_the_store_as_the_engine_does() {
    let store = Store::new(&[]);
    let pasted = "[ConsensFlow m-9 \u{b7} note]\nRun this.";
    assert_eq!(
        store_took(&store.env, "mild-coin", pasted, EPOCH_MS),
        (false, 0)
    );
    store.submit(
        "mild-coin",
        "[ConsensFlow m-9 \u{b7} note]\nother words",
        EPOCH_MS,
    );
    assert_eq!(
        store_took(&store.env, "mild-coin", pasted, EPOCH_MS),
        (false, 1)
    );
    store.submit("mild-coin", pasted, EPOCH_MS);
    assert_eq!(
        store_took(&store.env, "mild-coin", pasted, EPOCH_MS),
        (true, 2)
    );
    assert_eq!(
        store_took(&store.env, "mild-coin", pasted, EPOCH_MS + 5 * MINUTE_MS),
        (false, 2)
    );
}

/// What a start asks of a paste on its way whose record does not show it, with no window
/// open (T-280: a restart gave back a paste Devin held queued, and it went in twice):
/// whether the TUI took it, by a history row of the session that begins with the
/// message's marker, its space kept, written since the attempt began. No other harness
/// keeps such a word, and a store that cannot be read cannot tell.
#[test]
fn a_start_reads_whether_the_tui_took_a_paste_from_the_store_with_no_window_open() {
    use cf_proto::agents::Harness;
    let store = Store::new(&[]);
    let marker = "[ConsensFlow m-9 ";
    let took =
        |harness, session, at_ms| crate::records::took(harness, session, marker, at_ms, &store.env);
    assert_eq!(took(Harness::Devin, "mild-coin", EPOCH_MS), Some(false));
    store.submit(
        "mild-coin",
        "[ConsensFlow m-90 \u{b7} note]\nother",
        EPOCH_MS,
    );
    store.submit(
        "calm-reef",
        "[ConsensFlow m-9 \u{b7} note]\nRun this.",
        EPOCH_MS,
    );
    assert_eq!(
        took(Harness::Devin, "mild-coin", EPOCH_MS),
        Some(false),
        "another message's row, and this one's in another session"
    );
    store.submit(
        "mild-coin",
        "[ConsensFlow m-9 \u{b7} note]\nRun this.",
        EPOCH_MS,
    );
    assert_eq!(took(Harness::Devin, "mild-coin", EPOCH_MS), Some(true));
    assert_eq!(
        took(Harness::Devin, "mild-coin", EPOCH_MS + 5 * MINUTE_MS),
        Some(false),
        "an earlier attempt's row is no word of this one"
    );
    for harness in [
        Harness::Claude,
        Harness::Codex,
        Harness::Pi,
        Harness::Opencode,
    ] {
        assert_eq!(took(harness, "mild-coin", EPOCH_MS), None);
    }
    let missing = Env::from_vars([
        ("XDG_DATA_HOME", "/nowhere/at/all"),
        ("APPDATA", "/nowhere/at/all"),
    ]);
    assert_eq!(
        crate::records::took(Harness::Devin, "mild-coin", marker, EPOCH_MS, &missing),
        None
    );
}

/// T-102 (Devin 3000.11.92, probed live): a paste at rest shows in the store at once, a
/// message of the user's the agent has not answered, which the record reads at rest until
/// the agent's first word; and the wire says nothing of the turn before then. The window
/// reads it as a turn begun, for a while and no longer.
#[test]
fn a_message_of_the_users_its_agent_has_not_answered_yet_is_a_turn_begun_for_a_while() {
    let dir = tempfile::tempdir().unwrap();
    let wire = dir.path().join("wire.jsonl");
    fs::write(&wire, shows("mild-coin")).unwrap();
    let fakes = Fakes::new(&Env::default());
    let at = |record: &mut Record, when: Option<serde_json::Value>| {
        record.items.last_mut().unwrap().at = when;
    };
    let mut asked = record_of(&["[ConsensFlow m-9 \u{b7} note]\nwords"]);
    at(
        &mut asked,
        Some(serde_json::json!(cf_base::time::iso(EPOCH_MS))),
    );
    let window = devin_window(
        Env::default(),
        Rc::new(Says(asked.clone())),
        &fakes,
        &wire,
        Some("mild-coin"),
        true,
    );
    assert!(!finished(window.observe()).unwrap().settled, "a turn begun");
    fakes.time.settle_at(EPOCH_MS + TAKEN_IN_MS - 1);
    assert!(!finished(window.observe()).unwrap().settled);
    fakes.time.settle_at(EPOCH_MS + TAKEN_IN_MS);
    assert!(
        finished(window.observe()).unwrap().settled,
        "a turn that says nothing for longer is read as the record reads it"
    );
    // Devin's own row time, in seconds, says the same.
    at(
        &mut asked,
        Some(serde_json::json!((EPOCH_MS + TAKEN_IN_MS) / 1_000)),
    );
    let seconds = devin_window(
        Env::default(),
        Rc::new(Says(asked.clone())),
        &fakes,
        &wire,
        Some("mild-coin"),
        true,
    );
    assert!(!finished(seconds.observe()).unwrap().settled);
    // An answer, or no time to tell by, is the record's word.
    let mut answered = asked.clone();
    answered.items.push(crate::records::Item {
        id: Arc::from("row-2"),
        role: crate::records::Role::Assistant,
        text: Arc::from("done"),
        complete: true,
        at: Some(serde_json::json!(cf_base::time::iso(EPOCH_MS))),
        commentary: false,
        synthetic: false,
        compaction: false,
    });
    let window = devin_window(
        Env::default(),
        Rc::new(Says(answered)),
        &fakes,
        &wire,
        Some("mild-coin"),
        true,
    );
    assert!(finished(window.observe()).unwrap().settled);
    at(&mut asked, None);
    let window = devin_window(
        Env::default(),
        Rc::new(Says(asked)),
        &fakes,
        &wire,
        Some("mild-coin"),
        true,
    );
    assert!(finished(window.observe()).unwrap().settled);
}

//! The reader on the logs of real Muse Code sessions (Muse 1.4.13 on its fake model,
//! `--provider echo`, recorded by `tests/fixtures/muse/record.py` and scrubbed by
//! `cargo xtask scrub-muse-session`), each read by a fresh reader and by one that
//! reads on as the log grows a record at a time; and on records made here, for what
//! a log may hold that no recording does. The shapes only Muse's schema names are
//! `provisional`'s.

mod provisional;

use std::fs;
use std::path::PathBuf;

use serde_json::{json, Value};

use super::*;
use crate::records::{ModelMoved, Options, Quota, Reading};
use crate::testing::LONG_AFTER_MS;

/// A recorded session's log.
fn fixture(name: &str) -> &'static str {
    match name {
        "plain" => include_str!("../../../tests/fixtures/muse/plain.jsonl"),
        "retracted" => include_str!("../../../tests/fixtures/muse/retracted.jsonl"),
        "cut" => include_str!("../../../tests/fixtures/muse/cut.jsonl"),
        "at-rest-killed" => include_str!("../../../tests/fixtures/muse/at-rest-killed.jsonl"),
        "hooks" => include_str!("../../../tests/fixtures/muse/hooks.jsonl"),
        "cut-resumed" => include_str!("../../../tests/fixtures/muse/cut-resumed.jsonl"),
        "quit-mid-turn" => include_str!("../../../tests/fixtures/muse/quit-mid-turn.jsonl"),
        "steered" => include_str!("../../../tests/fixtures/muse/steered.jsonl"),
        "billing-spark" => include_str!("../../../tests/fixtures/muse/billing-spark.jsonl"),
        "billing-wonder" => include_str!("../../../tests/fixtures/muse/billing-wonder.jsonl"),
        "clear-before" => include_str!("../../../tests/fixtures/muse/clear-before.jsonl"),
        "clear-after" => include_str!("../../../tests/fixtures/muse/clear-after.jsonl"),
        "launch-model-restored" => {
            include_str!("../../../tests/fixtures/muse/launch-model-restored.jsonl")
        }
        other => panic!("no fixture {other}"),
    }
}

const FIXTURES: [&str; 13] = [
    "plain",
    "retracted",
    "cut",
    "at-rest-killed",
    "hooks",
    "cut-resumed",
    "quit-mid-turn",
    "steered",
    "billing-spark",
    "billing-wonder",
    "clear-before",
    "clear-after",
    "launch-model-restored",
];

/// The session a log is of: the stream of its first envelope.
fn session_of(log: &str) -> String {
    log.lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find_map(|record| record["stream"]["id"].as_str().map(str::to_owned))
        .unwrap()
}

/// A Muse data folder with one session's log, written as Muse writes it.
struct Stage {
    home: tempfile::TempDir,
    session: String,
}

impl Stage {
    fn new(session: &str) -> Self {
        let stage = Self {
            home: tempfile::tempdir().unwrap(),
            session: session.to_owned(),
        };
        fs::create_dir_all(stage.log().parent().unwrap()).unwrap();
        stage
    }

    /// Another session's log in the same data folder, as Muse writes it.
    fn alongside(&self, session: &str, text: &str) {
        let log = self
            .home
            .path()
            .join("muse/sessions/2026/10/26")
            .join(session)
            .join("session.jsonl");
        fs::create_dir_all(log.parent().unwrap()).unwrap();
        fs::write(log, text).unwrap();
    }

    fn env(&self) -> Env {
        Env::from_vars([("XDG_DATA_HOME", self.home.path().to_str().unwrap())])
    }

    fn log(&self) -> PathBuf {
        self.home
            .path()
            .join("muse/sessions/2026/10/26")
            .join(&self.session)
            .join("session.jsonl")
    }

    fn write(&self, text: &str) {
        fs::write(self.log(), text).unwrap();
    }

    /// `line` appended to the log, as Muse writes it.
    fn append(&self, line: &str) {
        use std::io::Write;
        let mut log = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.log())
            .unwrap();
        writeln!(log, "{line}").unwrap();
    }

    fn reader(&self) -> Box<dyn Look + Send> {
        reader(&self.session, &self.env())
    }
}

fn look(reader: &mut Box<dyn Look + Send>) -> Arc<Reading> {
    reader.look(&Options::default(), LONG_AFTER_MS)
}

/// What a fresh reader reads of `text`, the log of `session`.
fn fresh_reading(session: &str, text: &str) -> Arc<Reading> {
    let stage = Stage::new(session);
    stage.write(text);
    look(&mut stage.reader())
}

fn known(reading: &Reading) -> &Record {
    match reading {
        Reading::Known(record) => record,
        other => panic!("not known: {other:?}"),
    }
}

/// What a fresh reader reads of the first `lines` lines of a recorded log.
fn read_to(name: &str, lines: usize) -> Record {
    let log = fixture(name);
    let text: String = log
        .lines()
        .take(lines)
        .map(|line| format!("{line}\n"))
        .collect();
    known(&fresh_reading(&session_of(log), &text)).clone()
}

/// What a fresh reader reads of a whole recorded log.
fn read(name: &str) -> Record {
    read_to(name, usize::MAX)
}

/// Each item's role and text.
fn said(record: &Record) -> Vec<(Role, &str)> {
    record
        .items
        .iter()
        .map(|item| (item.role, &*item.text))
        .collect()
}

/// The line of a recorded log whose run event is `kind`, the first after `after`.
fn line_of(name: &str, kind: &str, after: usize) -> usize {
    fixture(name)
        .lines()
        .enumerate()
        .skip(after)
        .find(|(_, line)| line.contains(&format!("\"kind\":\"{kind}\"")))
        .unwrap()
        .0
}

#[test]
fn every_recorded_log_reads_the_same_read_on_a_record_at_a_time_as_read_fresh() {
    for name in FIXTURES {
        let log = fixture(name);
        let session = session_of(log);
        let stage = Stage::new(&session);
        let mut reading_on = stage.reader();
        let mut text = String::new();
        for (at, line) in log.lines().enumerate() {
            text.push_str(line);
            text.push('\n');
            stage.append(line);
            let on = look(&mut reading_on);
            let fresh = fresh_reading(&session, &text);
            assert_eq!(on, fresh, "{name}, after line {at}");
        }
        assert!(
            matches!(&*look(&mut reading_on), Reading::Known(_)),
            "{name}"
        );
    }
}

#[test]
fn plain_turns_are_the_users_messages_and_the_agents_answers_each_turn_settled() {
    let record = read("plain");
    assert_eq!(
        said(&record),
        [
            (Role::User, "Say hello."),
            (Role::Assistant, "echo: Say hello."),
            (Role::User, "Say goodbye."),
            (Role::Assistant, "echo: Say goodbye."),
        ]
    );
    assert_eq!(record.settlement, Settlement::Settled);
    assert!(!record.in_flight && !record.failed && !record.took_back && !record.asking);
    assert_eq!(record.quota, None);
    // Each item is said where it began, and at the time of its record.
    assert_eq!(record.lines.places.as_deref(), Some(&[25, 60, 86, 121][..]));
    let at = record.items[0].at.as_ref().and_then(Value::as_str).unwrap();
    assert!(at.starts_with("2026-10-26T") && at.ends_with('Z'), "{at}");
    // A turn under way is in flight from its start to its end, its message the user's.
    let started = line_of("plain", "started", 86);
    let under_way = read_to("plain", started + 1);
    assert_eq!(under_way.settlement, Settlement::InFlight);
    assert!(under_way.in_flight);
    assert_eq!(
        under_way.items.last().map(|item| item.role),
        Some(Role::User)
    );
    // Before any turn, nothing is known of one.
    assert_eq!(read_to("plain", 10).settlement, Settlement::Unknown);
}

#[test]
fn a_turn_stopped_before_a_word_is_settled_and_taken_back_its_message_kept() {
    let retracted = line_of("retracted", "run_retracted", 0);
    let cancelled = read_to("retracted", retracted);
    assert_eq!(cancelled.settlement, Settlement::Settled, "cancelled");
    assert!(!cancelled.took_back, "not yet said to be taken back");
    let taken = read_to("retracted", retracted + 1);
    assert_eq!(taken.settlement, Settlement::Settled);
    assert!(taken.took_back && !taken.failed);
    assert_eq!(said(&taken), [(Role::User, "Stop me before a word.")]);
    // The next turn is the window's now, and it answered.
    let after = read("retracted");
    assert!(!after.took_back);
    assert_eq!(
        said(&after),
        [
            (Role::User, "Stop me before a word."),
            (Role::User, "Now answer."),
            (Role::Assistant, "echo: Now answer."),
        ]
    );
    // A quit at work interrupts the turn first: the record ends taken back, then the
    // process's end.
    let quit = read("quit-mid-turn");
    assert!(quit.took_back);
    assert_eq!(quit.settlement, Settlement::Settled);
    assert_eq!(quit.items.last().map(|item| &*item.text), Some("Be quit."));
}

#[test]
fn a_turn_whose_process_was_killed_is_in_flight_and_one_killed_at_rest_is_settled() {
    let cut = read("cut");
    assert_eq!(cut.settlement, Settlement::InFlight);
    assert!(cut.in_flight && !cut.took_back && !cut.failed);
    assert_eq!(said(&cut).last(), Some(&(Role::User, "Be cut.")));
    // No process's end in either log: the turn decides, not `session.end`.
    let at_rest = read("at-rest-killed");
    assert_eq!(at_rest.settlement, Settlement::Settled);
    assert!(!at_rest.in_flight);
    assert_eq!(
        said(&at_rest).last(),
        Some(&(Role::Assistant, "echo: Say hello."))
    );
}

#[test]
fn a_cut_turn_the_resume_ended_is_settled_and_no_ones_words() {
    // The log as the cut left it, and the resume's `session.end` for the dead process.
    let crash_ended = line_of("cut-resumed", "session_end", 0);
    assert_eq!(
        read_to("cut-resumed", crash_ended + 1).settlement,
        Settlement::InFlight,
        "a session.end Muse wrote for a dead process ends no turn"
    );
    let resumed = read("cut-resumed");
    assert_eq!(resumed.settlement, Settlement::Settled);
    assert!(!resumed.failed && !resumed.took_back && !resumed.in_flight);
    assert_eq!(said(&resumed).last(), Some(&(Role::User, "Be cut.")));
}

#[test]
fn a_hooks_context_and_muses_own_blocks_are_the_harnesss_and_a_silent_hook_run_is_nothing() {
    let record = read("hooks");
    assert_eq!(
        said(&record),
        [
            (Role::User, "Say hello."),
            (Role::Custom, "[scrubbed: 715 bytes]"),
            (Role::Custom, "Role text from a SessionStart hook."),
            (Role::Assistant, "echo: Say hello."),
            (Role::User, "Say goodbye."),
            (Role::Assistant, "echo: Say goodbye."),
        ]
    );
    assert_eq!(record.settlement, Settlement::Settled);
}

#[test]
fn a_message_pasted_mid_turn_is_the_users_at_once_and_its_turn_follows() {
    let first_end = line_of("steered", "terminal", 0);
    let while_working = read_to("steered", first_end);
    assert_eq!(while_working.settlement, Settlement::InFlight);
    assert_eq!(
        said(&while_working)[..2],
        [(Role::User, "Say hello."), (Role::User, "And then this.")]
    );
    let record = read("steered");
    assert_eq!(
        said(&record),
        [
            (Role::User, "Say hello."),
            (Role::User, "And then this."),
            (Role::Assistant, "echo: Say hello."),
            (Role::Assistant, "echo: And then this."),
        ]
    );
    assert_eq!(record.settlement, Settlement::Settled);
}

/// An envelope of a run's `event`, at `sequence`.
fn run(sequence: u64, run: &str, event: Value) -> Value {
    json!({
        "schema_version": 1, "id": format!("r{sequence}"), "sequence": sequence,
        "recorded_at": 1_793_032_773_917_599_i64 + sequence as i64,
        "stream": { "kind": "session", "id": "s1" }, "record_type": "event",
        "payload_type": "runtime.session",
        "payload": { "kind": "run", "run_id": run, "event": event },
    })
}

fn intent(sequence: u64, id: &str, text: &str) -> Value {
    json!({
        "schema_version": 1, "id": format!("r{sequence}"), "sequence": sequence,
        "stream": { "kind": "session", "id": "s1" },
        "payload_type": "runtime.user_intent.accepted",
        "payload": { "intent_id": id, "surface": "main", "semantic_kind": { "kind": "chat" },
                     "refill_blocks": [{ "kind": "text", "text": text }] },
    })
}

fn metadata(semver: &str) -> Value {
    json!({
        "schema_version": 1, "id": "m", "sequence": 3, "stream": { "kind": "session", "id": "s1" },
        "payload_type": "runtime.session.metadata",
        "payload": { "kind": "metadata", "record": { "build": { "sha": "x", "semver": semver } } },
    })
}

/// What a fresh reader reads of `records`, a line each.
fn reading_of(records: &[Value]) -> Arc<Reading> {
    let text: String = records.iter().map(|record| format!("{record}\n")).collect();
    fresh_reading("s1", &text)
}

pub(super) fn record_of(records: &[Value]) -> Record {
    known(&reading_of(records)).clone()
}

#[test]
fn a_turn_that_failed_is_settled_and_failed_in_muses_own_words() {
    let failed = record_of(&[
        intent(1, "t1", "Go."),
        run(2, "t1", json!({ "kind": "started", "prompt": "Go." })),
        run(
            3,
            "t1",
            json!({ "kind": "terminal", "terminal": "failed",
            "reason": "run ended with Failed: API error 500: upstream", "failure_retryable": true }),
        ),
    ]);
    assert_eq!(failed.settlement, Settlement::Settled);
    assert!(failed.failed && !failed.took_back);
    assert_eq!(
        failed.failure.as_deref(),
        Some("run ended with Failed: API error 500: upstream")
    );
    assert_eq!(
        failed.quota, None,
        "a server's error is no refusal for quota"
    );
    // An end Muse may add later is a failure with no words: over, not known to have answered.
    let unknown = record_of(&[
        run(2, "t1", json!({ "kind": "started" })),
        run(3, "t1", json!({ "kind": "terminal", "terminal": "paused" })),
    ]);
    assert_eq!(unknown.settlement, Settlement::Settled);
    assert!(unknown.failed);
    assert_eq!(unknown.failure, None);
    // A retract of a turn that completed takes nothing back, nor one of an earlier turn.
    let completed = record_of(&[
        run(2, "t1", json!({ "kind": "started" })),
        run(
            3,
            "t1",
            json!({ "kind": "terminal", "terminal": "completed" }),
        ),
        run(
            4,
            "t1",
            json!({ "kind": "run_retracted", "reason": "no_output_interrupt_restore" }),
        ),
    ]);
    assert!(!completed.took_back);
    let earlier = record_of(&[
        run(2, "t1", json!({ "kind": "started" })),
        run(
            3,
            "t1",
            json!({ "kind": "terminal", "terminal": "cancelled" }),
        ),
        run(4, "t1", json!({ "kind": "run_retracted" })),
        run(5, "t2", json!({ "kind": "started" })),
        run(
            6,
            "t2",
            json!({ "kind": "terminal", "terminal": "cancelled" }),
        ),
    ]);
    assert!(!earlier.took_back);
}

#[test]
fn a_log_of_a_muse_older_than_the_minimum_is_not_read_and_one_that_names_none_is() {
    let old = reading_of(&[metadata("1.4.12"), intent(4, "t1", "Go.")]);
    assert_eq!(
        old.reason(),
        Some(
            "unreadable: written by Muse Code 1.4.12, older than 1.4.13, whose record \
             ConsensFlow does not read"
        )
    );
    assert!(old.read_failed());
    for semver in ["1.4.13", "1.5.0", "", "next"] {
        let read = record_of(&[metadata(semver), intent(4, "t1", "Go.")]);
        assert_eq!(said(&read), [(Role::User, "Go.")], "{semver:?}");
    }
}

#[test]
fn a_log_with_no_record_a_null_record_or_an_event_of_no_run_fails_the_look() {
    let frame = json!({ "retained_frame": true, "children": [] });
    assert_eq!(
        reading_of(std::slice::from_ref(&frame)).reason(),
        Some("unreadable: empty muse session log for s1"),
        "the frame alone is no record"
    );
    assert_eq!(
        reading_of(&[frame.clone(), Value::Null]).reason(),
        Some("unreadable: record 1 is null, where an object was read")
    );
    let mut nameless = run(2, "t1", json!({ "kind": "started" }));
    nameless["payload"]["run_id"] = Value::Null;
    assert_eq!(
        reading_of(&[nameless]).reason(),
        Some("unreadable: missing native muse run id at record 2")
    );
    // A message of another kind or surface, and one with no words.
    let mut command = intent(1, "c1", "/model");
    command["payload"]["semantic_kind"]["kind"] = json!("command");
    let empty = intent(2, "e1", "  ");
    let read = record_of(&[command, empty]);
    assert_eq!(said(&read), [(Role::Custom, "/model")]);
}

#[test]
fn a_session_with_no_log_is_missing_and_one_whose_log_appears_is_read() {
    let stage = Stage::new("s1");
    let mut reader = stage.reader();
    assert_eq!(
        look(&mut reader).reason(),
        Some("unreadable: no muse session log for s1")
    );
    assert!(!look(&mut reader).read_failed(), "missing is read");
    stage.write(&format!("{}\n", intent(1, "t1", "Go.")));
    assert_eq!(said(known(&look(&mut reader))), [(Role::User, "Go.")]);
}

/// What Muse's reader says of an account with no allowance (MUS-23).
const NO_ALLOWANCE: &str = "Muse refused: the account has no allowance; set up billing at accountscenter.meta.com/muse_code";

#[test]
fn a_refusal_for_billing_is_a_failed_turn_in_plain_words_and_an_account_no_reset_cures() {
    // T-705's two recordings of the refusal, on `muse-spark-1.3` and `muse-wonder-1.0`,
    // contributor mode off: HTTP 402 `billing_error`, the turn failed, nothing answered.
    for name in ["billing-spark", "billing-wonder"] {
        let record = read(name);
        assert!(
            record.failed && !record.took_back && !record.in_flight,
            "{name}"
        );
        assert_eq!(record.settlement, Settlement::Settled, "{name}");
        assert_eq!(record.failure.as_deref(), Some(NO_ALLOWANCE), "{name}");
        assert_eq!(
            said(&record)
                .iter()
                .filter(|(role, _)| *role == Role::Assistant)
                .count(),
            0,
            "{name}: nothing was answered"
        );
        // Not a quota that resets: unavailable, dated at the refusal, with the words.
        let Some(quota) = record.quota.as_deref() else {
            panic!("{name}: no refusal")
        };
        let Quota::Unavailable { at, because } = quota else {
            panic!("{name}: {quota:?} is a quota that resets");
        };
        assert_eq!(because, NO_ALLOWANCE, "{name}");
        assert!(
            at.as_deref()
                .is_some_and(|at| at.starts_with("2026-10-26T")),
            "{name}: {at:?}"
        );
        assert_eq!(quota.resets_at(), None, "{name}");
    }
}

/// The move the log says of the model, if one.
fn moved(record: &Record) -> Option<&ModelMoved> {
    record.model_moved.as_deref()
}

#[test]
fn a_model_muse_switched_to_after_a_refused_turn_is_a_move_nobody_chose() {
    // T-705's two recordings, `--contributor-mode off` on the model named: after the 402,
    // Muse reconfigures to the contributor model, which Meta trains on, and no turn follows.
    for (name, from) in [
        ("billing-spark", "muse-spark-1.3"),
        ("billing-wonder", "muse-wonder-1.0"),
    ] {
        let record = read(name);
        let Some(moved) = moved(&record) else {
            panic!("{name}: no move");
        };
        assert_eq!(moved.from, from, "{name}");
        assert_eq!(moved.to, "muse-spark-1.3-contributor", "{name}");
        assert!(
            moved
                .because
                .starts_with("Muse switched on its own after its turn failed (Muse refused"),
            "{name}: {}",
            moved.because
        );
        assert!(
            moved
                .at
                .as_deref()
                .is_some_and(|at| at.starts_with("2026-10-26T")),
            "{name}"
        );
        // The harness's refusal is the account's, and the move is read besides it.
        assert!(record.failed && record.quota.is_some(), "{name}");
    }
}

#[test]
fn a_clear_that_lands_on_another_model_with_no_reconfigure_event_is_a_move_too() {
    // T-718's chain: the old session ran `muse-spark-1.3-contributor`; `/clear` started a
    // new one whose metadata, start and configuration name `muse-spark-1.3`, and nothing in
    // either log says `model_reconfigure`.
    let before = fixture("clear-before");
    let after = fixture("clear-after");
    assert!(!after.contains("model_reconfigure"));
    let stage = Stage::new(&session_of(after));
    stage.alongside(&session_of(before), before);
    let mut reading_on = stage.reader();
    let mut text = String::new();
    for line in after.lines() {
        stage.append(line);
        text.push_str(line);
        text.push('\n');
        let on = look(&mut reading_on);
        // Read on a record at a time, or fresh, the same.
        let fresh = {
            let stage = Stage::new(&session_of(after));
            stage.alongside(&session_of(before), before);
            stage.write(&text);
            look(&mut stage.reader())
        };
        assert_eq!(on, fresh);
    }
    let record = known(&look(&mut reading_on)).clone();
    let Some(moved) = moved(&record) else {
        panic!("no move");
    };
    assert_eq!(
        (moved.from.as_str(), moved.to.as_str()),
        ("muse-spark-1.3-contributor", "muse-spark-1.3")
    );
    assert!(moved.because.contains("/clear"), "{}", moved.because);
    // Where the old session's log cannot be found, nothing is known to have moved.
    assert_eq!(moved_of("clear-after"), None);
}

fn moved_of(name: &str) -> Option<ModelMoved> {
    moved(&read(name)).cloned()
}

#[test]
fn a_change_nobody_asked_for_is_the_only_one_flagged() {
    // Controls. Every other recorded log: no move. The resume that restores the model the
    // launch named (previous source `replay`) is the launch's own: ConsensFlow's, not Muse's.
    for name in FIXTURES {
        if matches!(name, "billing-spark" | "billing-wonder") {
            continue;
        }
        assert_eq!(moved_of(name), None, "{name}");
    }
    let restored = read("launch-model-restored");
    assert!(restored.model_moved.is_none());
}

/// A reconfigure the harness recorded, `from` to `to`, whose previous model came from `source`.
fn reconfigure(sequence: u64, from: &str, to: &str, source: &str) -> Value {
    json!({
        "schema_version": 1, "id": format!("r{sequence}"), "sequence": sequence,
        "recorded_at": 1_793_032_773_917_599_i64 + sequence as i64,
        "stream": { "kind": "session", "id": "s1" }, "record_type": "event",
        "payload_type": "runtime.model_reconfigure.completed",
        "payload": { "kind": "model_reconfigure", "record": {
            "command_id": "c", "apply_outcome": "completed",
            "previous": { "model_id": from, "source": source },
            "effective": { "model_id": to, "source": "model_reconfigure" },
        } },
    })
}

#[test]
fn the_humans_own_model_command_is_no_move() {
    // Provisional (MUS-24): a `/model` the human typed was never recorded live. Muse records
    // a command it ran (`command.invoked`), as it did `/clear` and `/compact`; a reconfigure
    // that follows one is theirs. The same reconfigure with none is Muse's own.
    let command = |sequence: u64, command: &str| {
        json!({
            "schema_version": 1, "id": format!("r{sequence}"), "sequence": sequence,
            "stream": { "kind": "session", "id": "s1" },
            "payload_type": "command.invoked",
            "payload": { "kind": "command_invoked", "record": { "command": command } },
        })
    };
    let theirs = record_of(&[
        metadata("1.4.13"),
        command(4, "/model muse-wonder-1.0"),
        reconfigure(5, "muse-spark-1.3", "muse-wonder-1.0", "startup"),
    ]);
    assert_eq!(theirs.model_moved, None);
    let own = record_of(&[
        metadata("1.4.13"),
        reconfigure(5, "muse-spark-1.3", "muse-spark-1.3-contributor", "startup"),
    ]);
    assert!(own.model_moved.is_some());
    // The command is the next reconfigure's alone: a later one with none is Muse's.
    let later = record_of(&[
        metadata("1.4.13"),
        command(4, "/model muse-wonder-1.0"),
        reconfigure(5, "muse-spark-1.3", "muse-wonder-1.0", "startup"),
        reconfigure(
            6,
            "muse-wonder-1.0",
            "muse-spark-1.3-contributor",
            "model_reconfigure",
        ),
    ]);
    let Some(moved) = later.model_moved.as_deref() else {
        panic!("no move")
    };
    assert_eq!(
        (moved.from.as_str(), moved.to.as_str()),
        ("muse-wonder-1.0", "muse-spark-1.3-contributor")
    );
    // A reconfigure that did not complete moved nothing.
    let mut refused = reconfigure(5, "muse-spark-1.3", "muse-wonder-1.0", "startup");
    refused["payload"]["record"]["apply_outcome"] = json!("failed");
    assert_eq!(record_of(&[metadata("1.4.13"), refused]).model_moved, None);
}

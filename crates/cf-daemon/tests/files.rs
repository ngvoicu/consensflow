//! The daemon's two files, held line for line to what Node writes (recorded
//! from Node, at a clock fixed at 2026-10-05T10:00:00.123Z, and fixed since):
//! the lines of `daemon.log` and of `events.jsonl`, how each is moved aside
//! past its limit, and what `forget` leaves of a trace.

// The goldens' own reading: a failure in it is the test's.
#![allow(clippy::unwrap_used)]

use std::path::Path;

use cf_base::time::Clock;
use cf_daemon::files::{Log, Trace};
use cf_ledger::{DeletedProject, Event};
use cf_proto::trace::{TraceLine, Traced, WindowEvent};
use serde_json::Value;

const GOLDENS: &str = include_str!("goldens/files.json");

/// The moment the recorder fixed, which every time Node read was.
const FIXED_MS: i64 = 1_791_194_400_123;
const FIXED: &str = "2026-10-05T10:00:00.123Z";

/// A clock that says one moment, or moves a second at each reading.
struct Clock3(i64, i64);

impl Clock for Clock3 {
    fn now_ms(&mut self) -> i64 {
        let now = self.0;
        self.0 += self.1;
        now
    }
}

fn goldens() -> Value {
    serde_json::from_str(GOLDENS).unwrap()
}

fn read(file: &Path) -> Option<String> {
    std::fs::read_to_string(file).ok()
}

fn text(value: &Value) -> &str {
    value.as_str().unwrap()
}

#[test]
fn the_recorder_s_clock_is_the_one_these_tests_use() {
    assert_eq!(goldens()["clock"], FIXED);
    assert_eq!(cf_base::time::iso(FIXED_MS), FIXED);
}

#[test]
fn every_log_line_is_what_node_wrote() {
    let goldens = goldens();
    let cases = goldens["log"]["cases"].as_array().unwrap();
    assert!(cases.len() >= 8);
    for case in cases {
        let home = tempfile::tempdir().unwrap();
        let log = Log::with_clock(
            home.path().join("daemon.log"),
            5_000_000,
            Box::new(Clock3(FIXED_MS, 0)),
        );
        let (message, cause) = (text(&case["message"]), case["cause"].as_str());
        match text(&case["level"]) {
            "info" => log.info(message),
            "warn" => log.warn(message, cause),
            "error" => log.error(message, cause),
            other => panic!("a level of {other}"),
        }
        assert_eq!(
            read(log.file()).as_deref(),
            case["expected"].as_str(),
            "{}",
            case["name"]
        );
    }
}

#[test]
fn the_log_is_moved_aside_past_its_limit_as_node_moved_it() {
    let goldens = goldens();
    let rotation = &goldens["log"]["rotation"];
    let home = tempfile::tempdir().unwrap();
    let log = Log::with_clock(
        home.path().join("daemon.log"),
        rotation["limit"].as_u64().unwrap(),
        // A second later at each line, as the recorder's clock moved.
        Box::new(Clock3(FIXED_MS, 1000)),
    );
    for line in 0..rotation["lines"].as_u64().unwrap() {
        log.info(&format!("line {line}"));
    }
    assert_eq!(read(log.file()).as_deref(), rotation["current"].as_str());
    assert_eq!(
        read(&cf_base::file::aside(log.file())).as_deref(),
        rotation["aside"].as_str()
    );
}

/// `entry`, as the trace writes it: the ledger's event, the engine's line, or the daemon's error.
fn write(trace: &Trace, entry: &Value) {
    let kind = text(&entry["kind"]);
    let project = entry["project"].as_i64();
    let participant = entry["participant"].as_str().map(str::to_owned);
    let window = |event: WindowEvent| {
        trace.write(&TraceLine {
            at: FIXED.to_owned(),
            what: Traced::Window {
                project,
                participant: participant.clone(),
                event,
            },
        });
    };
    match kind {
        "daemon.error" => trace.error(text(&entry["reason"])),
        "window.activity" => window(WindowEvent::Activity {
            state: text(&entry["state"]).to_owned(),
            reason: entry["reason"].as_str().map(str::to_owned),
        }),
        "window.kill_failed" => window(WindowEvent::KillFailed {
            error: entry["error"].as_str().map(str::to_owned),
        }),
        "delivery.held" => window(WindowEvent::DeliveryHeld {
            message: entry["message"].as_i64().unwrap(),
            reason: text(&entry["reason"]).to_owned(),
        }),
        "delivery.enter_again" => window(WindowEvent::EnterAgain {
            message: entry["message"].as_i64().unwrap(),
        }),
        "project.deleted" => {
            let data = &entry["data"];
            trace.write(&TraceLine {
                at: FIXED.to_owned(),
                what: Traced::ProjectDeleted(DeletedProject {
                    id: data["id"].as_i64().unwrap(),
                    name: text(&data["name"]).to_owned(),
                    directory: text(&data["directory"]).to_owned(),
                    created_at: text(&data["createdAt"]).to_owned(),
                    members: data["members"].as_i64().unwrap(),
                    sessions: data["sessions"].as_i64().unwrap(),
                    tasks: data["tasks"].as_i64().unwrap(),
                    messages: data["messages"].as_i64().unwrap(),
                }),
            });
        }
        _ => trace.event(&Event {
            at: text(&entry["at"]).to_owned(),
            project: entry["project"].as_i64().unwrap(),
            kind: kind.to_owned(),
            data: entry["data"].clone(),
        }),
    }
}

fn trace_in(home: &Path, limit: u64) -> Trace {
    Trace::with_clock(
        home.join("events.jsonl"),
        limit,
        Box::new(Clock3(FIXED_MS, 0)),
    )
}

#[test]
fn every_trace_line_is_what_node_wrote() {
    let goldens = goldens();
    let cases = goldens["trace"]["cases"].as_array().unwrap();
    assert!(cases.len() >= 10);
    for case in cases {
        let home = tempfile::tempdir().unwrap();
        let trace = trace_in(home.path(), 5_000_000);
        write(&trace, &case["entry"]);
        assert_eq!(
            read(trace.file()).as_deref(),
            case["expected"].as_str(),
            "{}",
            case["name"]
        );
    }
}

#[test]
fn the_trace_is_moved_aside_past_its_limit_as_node_moved_it() {
    let goldens = goldens();
    let rotation = &goldens["trace"]["rotation"];
    let home = tempfile::tempdir().unwrap();
    let trace = trace_in(home.path(), rotation["limit"].as_u64().unwrap());
    for entry in rotation["entries"].as_array().unwrap() {
        write(&trace, entry);
    }
    assert_eq!(read(trace.file()).as_deref(), rotation["current"].as_str());
    assert_eq!(
        read(&cf_base::file::aside(trace.file())).as_deref(),
        rotation["aside"].as_str()
    );
}

#[test]
fn forgetting_a_project_leaves_what_node_left() {
    let goldens = goldens();
    let forget = &goldens["trace"]["forget"];
    let home = tempfile::tempdir().unwrap();
    let trace = trace_in(home.path(), 5_000_000);
    std::fs::write(trace.file(), text(&forget["before"])).unwrap();
    let aside = cf_base::file::aside(trace.file());
    std::fs::write(&aside, text(&forget["beforeAside"])).unwrap();
    trace.forget(forget["project"].as_i64().unwrap());
    assert_eq!(read(trace.file()).as_deref(), forget["after"].as_str());
    assert_eq!(read(&aside).as_deref(), forget["afterAside"].as_str());
}

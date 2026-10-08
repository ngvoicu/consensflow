//! Each ported test held to the Node trace of the same test
//! (`crates/cf-engine/tests/traces/`, recorded from Node and fixed since): what
//! the engine did at its seams, in order, and the database it left.
//!
//! Both sides are read through one projection, which keeps what is the
//! engine's behaviour and drops what is how it got there:
//! - kept: where each operation the test calls begins (by name; the
//!   engine's calls of its own operations are its insides, unmarked), with
//!   what it answered, whole (the project as the ledger had it when it
//!   answered, or none), or the words it failed with; each
//!   event the ledger logs, whole; each call of the pane host, with what it
//!   was given and answered; each call of an adapter by its method and what
//!   names it (the launch, the message, the text, the conversation resumed or
//!   read, and the pane a `ready` or a delivery is for), and each launch with
//!   what it was given (the participant's
//!   handle, its project, role, folder and role text, its agent's
//!   settings); each
//!   token issued, with whom for, and revoked; each launch's files
//!   forgotten; each line of the trace, each project it forgot, and each
//!   line of the log, a failure by its words;
//! - dropped: the ledger's own calls (its events and the final database say
//!   what they changed, and reads are no behaviour), the roster, the role
//!   texts and the pane environment (lookups), a revoke or a forget of
//!   nothing (JavaScript made them for a window that had no token yet);
//! - read as absent: a field JavaScript held `undefined`, as `JSON.stringify`
//!   leaves it out.
//!
//! Both kits number a test's conversation items from `i-1`, so the ids are
//! held as they are.
//!
//! A limit: a transcript's copy logs no event, so where its writes fall
//! among the other effects is held only by the database each side left.
//!
//! The effects are held in order, but for the tests named in [`INTERLEAVED`],
//! which may differ by how independent windows interleave, and only so
//! ([`lanes`]). The database each side left is held equal whole, table by
//! table, each value as SQLite quotes it, the test's temporary folder
//! written «dir», but for the four columns of migration 0011 that only this
//! ledger writes (`cf_ledger::testing`), held apart on both sides.
//!
//! A test of the receipt and stop redesign has no Node trace: its rule is Node's
//! no more, so it is not held to one (`held_to` panics for a test with none)
//! and asserts what it holds directly.
//!
//! A test whose rule is Node's still, but whose trace the engine departs from
//! on purpose (the notes that tell a requester its task is paused go once it is
//! resumed; a window that did not come up is asked what it shows before it is
//! closed), is named in [`DEPARTED`], with what it does that Node does not. It
//! is held to a trace of its own, recorded from the engine
//! (`tests/departures/`, `npm run departures`), and fails when Node's trace is
//! the engine's again, so the departure is taken off once Node does it too.

use std::collections::HashMap;
use std::ffi::OsString;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use cf_base::env::Env;
use cf_engine::testing::Closed;
use cf_ledger::testing::{hold_apart_what_node_never_logs, hold_apart_what_node_never_writes};

use crate::lanes;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use rusqlite::{Connection, OpenFlags};
use serde_json::{json, Map, Value};

/// A test's suites and sentence: what names its trace.
type Key = (Vec<String>, String);

/// A recorded trace, and the file it is in.
struct Recorded {
    file: OsString,
    trace: Value,
}

/// The traces in `folder`, by each test's suites and sentence; none when
/// there is no folder.
fn load(folder: &Path) -> HashMap<Key, Recorded> {
    let mut traces = HashMap::new();
    let Ok(entries) = fs::read_dir(folder) else {
        return traces;
    };
    for entry in entries {
        let path = entry.expect("a trace").path();
        // The folder's README says what the traces are; it is no trace.
        if path.extension().is_none_or(|extension| extension != "gz") {
            continue;
        }
        let mut text = String::new();
        GzDecoder::new(fs::File::open(&path).expect("a trace's file"))
            .read_to_string(&mut text)
            .expect("a trace gunzipped");
        let trace: Value = serde_json::from_str(&text).expect("a trace's JSON");
        let suites = trace["test"]["suites"]
            .as_array()
            .expect("a test's suites")
            .iter()
            .map(|suite| suite.as_str().expect("a suite's name").to_owned())
            .collect();
        let name = trace["test"]["name"]
            .as_str()
            .expect("a test's name")
            .to_owned();
        let file = path.file_name().expect("a trace's name").to_owned();
        traces.insert((suites, name), Recorded { file, trace });
    }
    traces
}

fn folder(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join(name)
}

/// The Node traces, by each test's suites and sentence.
static TRACES: LazyLock<HashMap<Key, Recorded>> = LazyLock::new(|| {
    let traces = load(&folder("traces"));
    assert!(
        !traces.is_empty(),
        "no traces: they are fixed recordings (tests/traces/README.md)"
    );
    traces
});

/// The engine's own traces of the tests it departs from Node's in.
static DEPARTURES: LazyLock<HashMap<Key, Recorded>> = LazyLock::new(|| load(&folder("departures")));

/// The tests whose effects may come in another order than Node's where two
/// windows' interleave, and only there: JavaScript's microtask hops through
/// nested async functions let a worker's launch overtake the chief's look.
/// Each is one an exception is counted for. None is now: the kit's fakes and
/// the engine wait the turns JavaScript's awaits waited (`runtime::returning`,
/// the fakes' own), so each ported test's effects come in Node's order.
const INTERLEAVED: &[&str] = &[];

/// The tests the engine departs from Node's trace in on purpose, each with
/// what the engine does that Node does not. Node never withdraws a note that
/// told a requester a task was paused, so the notes the engine withdraws when
/// the task is resumed are in Node's traces still queued, and pasted.
const DEPARTED: &[(&str, &str)] = &[
    (
        "closes a project: its windows go, work in them pauses, and Resume brings the chief back",
        "the chief resumes T-1 before its window came back to take the note that T-1 is paused: the note is withdrawn, where Node pastes it into the window",
    ),
    (
        "keeps a held task paused, its hold cleared, when its session was deleted before the hold ended, and tells its requester once while every other task goes on",
        "the daemon resumes T-3 when its hold ends, and the note that said T-3 waits, which the chief had not been given, is withdrawn, where Node leaves it queued",
    ),
    (
        "fails a launch whose first message never arrives",
        "before it closes a window whose first message never showed, the engine asks the pane host what the window shows (`pane.snapshot` with `tail`), to tell it in the failure, where Node closes it unasked",
    ),
    (
        "is killed once when its launch never showed its first message, though its exit comes late",
        "before it closes a window whose first message never showed, the engine asks the pane host what the window shows (`pane.snapshot` with `tail`), to tell it in the failure, where Node closes it unasked",
    ),
    (
        "fails the first message at once when the harness cannot take it after the window opens",
        "before it closes a window whose harness could not take its first message, the engine asks the pane host what the window shows (`pane.snapshot` with `tail`), to tell it in the failure, where Node closes it unasked",
    ),
    (
        "keeps the project open when the new chief cannot take its handoff, and opens it again with it",
        "before it closes the chief's window, whose harness could not take its handoff, the engine asks the pane host what the window shows (`pane.snapshot` with `tail`), to tell it in the failure, where Node closes it unasked",
    ),
    (
        "a note written 5 turns after a pass whose launch cannot take its first message",
        "the engine asks the pane host what the window shows (`pane.snapshot` with `tail`) before it closes a window whose harness could not take its first message, which takes the turns the host's answer takes: the note falls among other effects than in Node's trace",
    ),
    (
        "a note written 6 turns after a pass whose launch cannot take its first message",
        "the engine asks the pane host what the window shows (`pane.snapshot` with `tail`) before it closes a window whose harness could not take its first message, which takes the turns the host's answer takes: the note falls among other effects than in Node's trace",
    ),
];

/// The variable that asks for the departures to be recorded again, which `npm
/// run departures` sets.
const RERECORD: &str = "CF_RERECORD_DEPARTED";

/// Holds a closed test to the Node trace of the test named `name` in `suites`,
/// or, if the engine departs from Node's in it, to the trace of its own.
pub fn held_to(closed: Closed, suites: &[&str], name: &str) {
    let key = (
        suites.iter().map(|suite| (*suite).to_owned()).collect(),
        name.to_owned(),
    );
    let node = TRACES.get(&key).unwrap_or_else(|| {
        panic!("no Node trace of {suites:?} › {name}: the traces are fixed recordings (tests/traces/README.md)")
    });
    let Some((_, departure)) = DEPARTED.iter().find(|(departed, _)| *departed == name) else {
        if let Some(difference) = first_difference(&closed, &node.trace, name) {
            panic!("{difference}");
        }
        return;
    };
    if Env::from_process().text(RERECORD).is_some() {
        return rerecord(&closed, node);
    }
    let kept = DEPARTURES
        .get(&key)
        .unwrap_or_else(|| panic!("{name}: no trace of its departure: npm run departures"));
    if let Some(difference) = first_difference(&closed, &kept.trace, name) {
        panic!("{difference}");
    }
    assert!(
        first_difference(&closed, &node.trace, name).is_some(),
        "{name}: the engine does what Node's trace has again (it departed: {departure}): \
         take it off DEPARTED and delete its departure"
    );
}

/// Where the test's effects or the database it left first differ from
/// `trace`'s, or none when they are the same.
fn first_difference(closed: &Closed, trace: &Value, name: &str) -> Option<String> {
    let node = projected(trace["events"].as_array().expect("its events"));
    let rust = projected(&logged(closed));
    if INTERLEAVED.contains(&name) {
        if let Some(difference) = lanes::first_difference(&node, &rust) {
            return Some(format!("{name}: the engine's effects differ {difference}"));
        }
    } else if let Some(at) =
        (0..node.len().max(rust.len())).find(|&at| node.get(at) != rust.get(at))
    {
        let around = |events: &[Value]| {
            events[at.saturating_sub(3)..(at + 3).min(events.len())]
                .iter()
                .map(|event| format!("    {event}"))
                .collect::<Vec<_>>()
                .join("\n")
        };
        return Some(format!(
            "{name}: the engine's effects differ at {at}\n  node:\n{}\n  rust:\n{}",
            around(&node),
            around(&rust)
        ));
    }
    let left = dump(&closed.file, closed.dir.path());
    // What Node's ledger never writes is held apart on both sides, so a
    // recording made before the migration and one made after it are the same.
    let held = trace["finals"]
        .as_array()
        .and_then(|finals| finals.last())
        .cloned()
        .map(|mut held| {
            if let Value::Object(tables) = &mut held {
                hold_apart_what_node_never_writes(tables);
            }
            held
        });
    let held = held.as_ref();
    (Some(&left) != held).then(|| {
        let table = left
            .as_object()
            .into_iter()
            .flatten()
            .find(|(table, rows)| held.and_then(|held| held.get(table.as_str())) != Some(*rows));
        let words = table.map_or_else(String::new, |(table, rows)| {
            format!(
                " in {table}:\n  engine: {rows}\n  trace: {}",
                held.and_then(|held| held.get(table.as_str()))
                    .map_or_else(|| "none".to_owned(), ToString::to_string)
            )
        });
        format!("{name}: the database the engine left differs{words}")
    })
}

/// What the engine did at its seams in a closed test, as Node's trace says
/// it: the stop a pause counted is in the ledger's event, and Node's never
/// says it.
fn logged(closed: &Closed) -> Vec<Value> {
    let mut logged = closed.events.clone();
    for line in &mut logged {
        if let Some(event) = line.get_mut("event") {
            hold_apart_what_node_never_logs(std::slice::from_mut(event));
        }
    }
    logged
}

/// Writes the trace of a departed test from this run of the engine, in the
/// shape of Node's: its `test`, what the engine did at its seams, and the
/// database it left.
fn rerecord(closed: &Closed, node: &Recorded) {
    let trace = json!({
        "test": node.trace["test"],
        "events": logged(closed),
        "finals": [dump(&closed.file, closed.dir.path())],
    });
    let mut gzip = GzEncoder::new(Vec::new(), Compression::best());
    gzip.write_all(trace.to_string().as_bytes())
        .expect("the trace gzipped");
    let departures = folder("departures");
    fs::create_dir_all(&departures).expect("the departures' folder");
    fs::write(
        departures.join(&node.file),
        gzip.finish().expect("the trace gzipped"),
    )
    .expect("the departure written");
}

/// What a trace's events are the engine's behaviour, as both sides write it.
fn projected(events: &[Value]) -> Vec<Value> {
    events.iter().filter_map(project).map(defined).collect()
}

fn project(event: &Value) -> Option<Value> {
    if let Some(op) = event.get("op") {
        let mut kept = Map::new();
        kept.insert("op".to_owned(), op.clone());
        if let Some(answer) = event.get("answer") {
            kept.insert("answer".to_owned(), answer.clone());
        }
        if let Some(threw) = event.get("threw") {
            kept.insert("threw".to_owned(), said(threw));
        }
        return Some(Value::Object(kept));
    }
    let seam = event["seam"].as_str()?;
    let method = event.get("method").and_then(Value::as_str);
    let args = &event["args"];
    match seam {
        "event" => Some(json!({ "event": event["event"] })),
        "host" => Some(json!({ "host": method, "args": args, "answer": event["answer"] })),
        "credentials" => match method {
            Some("issue") => Some(json!({
                "issue": event["answer"],
                "participant": args[0]["participant"]["id"],
                "handle": args[0]["participant"]["handle"],
                "project": args[0]["project"]["id"],
            })),
            _ => (!args[0].is_null()).then(|| json!({ "revoke": args[0] })),
        },
        "launchFiles" => (!args[0].is_null()).then(|| json!({ "forget": args[0] })),
        "trace" => Some(match method {
            Some("forget") => json!({ "trace forgets": args[0] }),
            _ => json!({ "trace": args[0] }),
        }),
        "log" => Some(json!({ "log": args[0], "cause": said(&args[1]) })),
        adapter if adapter.starts_with("adapter:") => Some(adapter_call(adapter, method, &args[0])),
        _ => None,
    }
}

/// An adapter's call by what names it; a launch with what it was given.
fn adapter_call(adapter: &str, method: Option<&str>, given: &Value) -> Value {
    let launch = given.get("launchId").or_else(|| {
        given
            .get("launch")
            .and_then(|launch| launch.get("launchId"))
    });
    let mut named = Map::new();
    named.insert("adapter".to_owned(), json!(adapter));
    named.insert("method".to_owned(), json!(method));
    for (key, value) in [
        ("launch", launch),
        ("message", given.get("message")),
        ("text", given.get("text")),
        ("resume", given.get("resume")),
        (
            "session",
            given
                .get("conversation")
                .and_then(|conversation| conversation.get("nativeSession")),
        ),
    ] {
        if let Some(value) = value {
            named.insert(key.to_owned(), value.clone());
        }
    }
    // The pane a window is asked about or given a message in: the window's own.
    if matches!(method, Some("ready" | "deliver")) {
        named.insert("pane".to_owned(), given["pane"].clone());
    }
    if method == Some("prepare") {
        named.insert("handle".to_owned(), given["participant"]["handle"].clone());
        named.insert("project".to_owned(), given["project"]["id"].clone());
        for key in ["role", "directory", "instructions"] {
            named.insert(key.to_owned(), given[key].clone());
        }
        named.insert("agent".to_owned(), settings(&given["agent"]));
    }
    Value::Object(named)
}

/// A saved agent as the adapters read it: its model, effort, thinking and
/// whether it is an image agent; none for a chief from before every chief
/// had one.
fn settings(agent: &Value) -> Value {
    if agent.is_null() {
        return Value::Null;
    }
    let read = |key: &str| match &agent[key] {
        Value::Object(tagged) if tagged.contains_key("$undefined") => Value::Null,
        value => value.clone(),
    };
    json!({
        "model": read("model"),
        "effort": read("effort"),
        "thinking": read("thinking"),
        "designer": agent["designer"] == json!(true),
    })
}

/// A failure by its words: JavaScript logged an `Error`, Rust its message.
fn said(cause: &Value) -> Value {
    cause
        .get("$error")
        .and_then(|error| error.get("message"))
        .unwrap_or(cause)
        .clone()
}

/// `value` with each field JavaScript held `undefined` left out.
fn defined(value: Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.into_iter().map(defined).collect()),
        Value::Object(fields) => Value::Object(
            fields
                .into_iter()
                .filter(|(_, item)| *item != json!({ "$undefined": true }))
                .map(|(key, item)| (key, defined(item)))
                .collect(),
        ),
        other => other,
    }
}

/// The database `file` holds, as the Node recorder dumped its own: each
/// table by name, its columns, and its rows in rowid order, each value as
/// SQLite quotes it; `folder` written «dir».
fn dump(file: &Path, folder: &Path) -> Value {
    let db = Connection::open_with_flags(file, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .expect("the ledger's file");
    let mut tables = Map::new();
    let names: Vec<String> = db
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
        .expect("the tables")
        .query_map([], |row| row.get(0))
        .expect("the tables read")
        .collect::<Result<_, _>>()
        .expect("each table");
    for name in names {
        let columns: Vec<String> = db
            .prepare(&format!("PRAGMA table_info(\"{name}\")"))
            .expect("the columns")
            .query_map([], |row| row.get(1))
            .expect("the columns read")
            .collect::<Result<_, _>>()
            .expect("each column");
        let quoted: Vec<String> = columns
            .iter()
            .map(|column| format!("quote(\"{column}\")"))
            .collect();
        let rows: Vec<Value> = db
            .prepare(&format!(
                "SELECT {} FROM \"{name}\" ORDER BY rowid",
                quoted.join(", ")
            ))
            .expect("the rows")
            .query_map([], |row| {
                (0..columns.len())
                    .map(|at| row.get::<_, String>(at).map(Value::String))
                    .collect::<Result<Vec<_>, _>>()
                    .map(Value::Array)
            })
            .expect("the rows read")
            .collect::<Result<_, _>>()
            .expect("each row");
        tables.insert(name, json!({ "columns": columns, "rows": rows }));
    }
    hold_apart_what_node_never_writes(&mut tables);
    let text = Value::Object(tables).to_string();
    let folder = serde_json::to_string(&folder.to_string_lossy()).expect("the folder as JSON");
    serde_json::from_str(&text.replace(&folder[1..folder.len() - 1], "«dir»"))
        .expect("the dump read again")
}

#[test]
fn a_field_javascript_held_undefined_is_absent_and_a_failure_is_its_words() {
    let node = [
        json!({ "seam": "host", "method": "open", "args": [{ "id": "p1-chief" }], "answer": { "ok": true, "pid": { "$undefined": true } } }),
        json!({ "seam": "log", "method": "error", "args": ["a launch failed", { "$error": { "name": "Error", "code": null, "status": null, "message": "refused" } }] }),
    ];
    let rust = [
        json!({ "seam": "host", "method": "open", "args": [{ "id": "p1-chief" }], "answer": { "ok": true } }),
        json!({ "seam": "log", "method": "error", "args": ["a launch failed", "refused"] }),
    ];
    assert_eq!(projected(&node), projected(&rust));
    let kept = [
        json!({ "seam": "host", "method": "open", "args": [{ "id": "p1-chief" }], "answer": { "ok": true, "pid": 4242 } }),
    ];
    assert_ne!(projected(&kept), projected(&rust), "a pid is kept");
}

#[test]
fn a_launch_is_held_with_what_it_was_given() {
    let node = json!({ "seam": "adapter:claude-code", "method": "prepare", "args": [{
        "launchId": "l", "participant": { "id": 2, "handle": "chief", "createdAt": "then" },
        "role": "chief", "project": { "id": 1, "name": "app" }, "directory": "/work/app",
        "resume": null, "message": null,
        "agent": { "id": "apollo", "model": "claude-opus-5", "profile": { "modelKey": "claude-opus-5" } },
        "instructions": "instructions for chief",
    }] });
    let rust = json!({ "seam": "adapter:claude-code", "method": "prepare", "args": [{
        "launchId": "l", "participant": { "handle": "chief" }, "role": "chief",
        "project": { "id": 1 }, "directory": "/work/app", "resume": null, "message": null,
        "agent": { "model": "claude-opus-5", "effort": null, "thinking": null, "designer": false },
        "instructions": "instructions for chief",
    }] });
    assert_eq!(
        projected(std::slice::from_ref(&node)),
        projected(std::slice::from_ref(&rust))
    );
    let mut other = rust.clone();
    other["args"][0]["agent"]["model"] = json!("gpt-6-astra");
    assert_ne!(
        projected(std::slice::from_ref(&node)),
        projected(std::slice::from_ref(&other)),
        "the model is held"
    );
    let mut other = rust.clone();
    other["args"][0]["project"]["id"] = json!(2);
    assert_ne!(
        projected(std::slice::from_ref(&node)),
        projected(std::slice::from_ref(&other)),
        "the project is held"
    );
    let mut other = rust;
    other["args"][0]["instructions"] = json!("instructions for worker");
    assert_ne!(
        projected(std::slice::from_ref(&node)),
        projected(std::slice::from_ref(&other)),
        "the role text is held"
    );
}

#[test]
fn a_lookup_and_a_ledger_call_are_no_behaviour_and_a_revoke_of_nothing_is_none() {
    let events = [
        json!({ "op": "pass", "args": [] }),
        json!({ "seam": "roster", "args": ["zeus"], "answer": {} }),
        json!({ "seam": "ledger", "method": "project", "args": [1], "answer": null }),
        json!({ "seam": "credentials", "method": "revoke", "args": [null] }),
        json!({ "seam": "credentials", "method": "revoke", "args": ["token-zeus"] }),
        json!({ "seam": "launchFiles", "method": "forget", "args": [null] }),
        json!({ "seam": "adapter:codex", "method": "observe", "args": [{ "launch": { "launchId": "l" }, "host": "$host" }] }),
        json!({ "seam": "trace", "method": "forget", "args": [1], "answer": 1 }),
    ];
    assert_eq!(
        projected(&events),
        [
            json!({ "op": "pass" }),
            json!({ "revoke": "token-zeus" }),
            json!({ "adapter": "adapter:codex", "method": "observe", "launch": "l" }),
            json!({ "trace forgets": 1 }),
        ]
    );
}

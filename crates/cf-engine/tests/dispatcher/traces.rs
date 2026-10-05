//! Each ported test held to the Node trace of the same test
//! (`crates/cf-engine/tests/traces/`, `npm run goldens:dispatcher`): what
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
//! written «dir».

use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::Path;
use std::sync::LazyLock;

use cf_engine::testing::Closed;

use crate::lanes;
use flate2::read::GzDecoder;
use rusqlite::{Connection, OpenFlags};
use serde_json::{json, Map, Value};

/// The Node traces, by each test's suites and sentence.
static TRACES: LazyLock<HashMap<(Vec<String>, String), Value>> = LazyLock::new(|| {
    let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/traces");
    let mut traces = HashMap::new();
    for entry in fs::read_dir(&folder).expect("the traces' folder") {
        let path = entry.expect("a trace").path();
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
        traces.insert((suites, name), trace);
    }
    traces
});

/// The tests whose effects may come in another order than Node's where two
/// windows' interleave, and only there: JavaScript's microtask hops through
/// nested async functions let a worker's launch overtake the chief's look.
/// Each is one an exception is counted for. None is now: the kit's fakes and
/// the engine wait the turns JavaScript's awaits waited (`runtime::returning`,
/// the fakes' own), so each ported test's effects come in Node's order.
const INTERLEAVED: &[&str] = &[];

/// Holds a closed test to the Node trace of the test named `name` in `suites`.
pub fn held_to(closed: Closed, suites: &[&str], name: &str) {
    let key = (
        suites.iter().map(|suite| (*suite).to_owned()).collect(),
        name.to_owned(),
    );
    let trace = TRACES.get(&key).unwrap_or_else(|| {
        panic!("no Node trace of {suites:?} › {name}: npm run goldens:dispatcher")
    });
    let node = projected(trace["events"].as_array().expect("its events"));
    let rust = projected(&closed.events);
    if INTERLEAVED.contains(&name) {
        if let Some(difference) = lanes::first_difference(&node, &rust) {
            panic!("{name}: the engine's effects differ {difference}");
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
        panic!(
            "{name}: the engine's effects differ at {at}\n  node:\n{}\n  rust:\n{}",
            around(&node),
            around(&rust)
        );
    }
    let left = dump(&closed.file, closed.dir.path());
    let node_left = trace["finals"].as_array().and_then(|finals| finals.last());
    assert_eq!(
        Some(&left),
        node_left,
        "{name}: the database the engine left differs"
    );
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

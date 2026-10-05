//! Each ported test held to the Node trace of the same test
//! (`crates/cf-engine/tests/traces/`, `npm run goldens:dispatcher`): what
//! the engine did at its seams, in order, and the database it left.
//!
//! Both sides are read through one projection, which keeps what is the
//! engine's behaviour and drops what is how it got there:
//! - kept: where each of the engine's operations begins (by name); each
//!   event the ledger logs, whole; each call of the pane host, with what it
//!   was given and answered; each call of an adapter by its method and what
//!   names it (the launch, the message, the text, the conversation resumed or
//!   read); each token issued and revoked; each launch's files forgotten;
//!   each line of the trace and of the log;
//! - dropped: the ledger's own calls (its events and the final database say
//!   what they changed, and reads are no behaviour), the roster, the role
//!   texts and the pane environment (lookups), a revoke or a forget of
//!   nothing (JavaScript made them for a window that had no token yet);
//! - renamed: the conversations' item ids, numbered in the test file by
//!   JavaScript and in the test by Rust, each by its first appearance.
//!
//! The database each side left is held equal whole, table by table, each
//! value as SQLite quotes it, the test's temporary folder written «dir».

use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::Path;
use std::sync::LazyLock;

use cf_engine::testing::Closed;
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

/// Holds a closed test to the Node trace of the test named `name` in `suites`.
pub fn held_to(closed: Closed, suites: &[&str], name: &str) {
    let key = (
        suites.iter().map(|suite| (*suite).to_owned()).collect(),
        name.to_owned(),
    );
    let trace = TRACES.get(&key).unwrap_or_else(|| {
        panic!("no Node trace of {suites:?} › {name}: npm run goldens:dispatcher")
    });
    let node = renumbered(projected(trace["events"].as_array().expect("its events")));
    let rust = renumbered(projected(&closed.events));
    if let Some(at) = (0..node.len().max(rust.len())).find(|&at| node.get(at) != rust.get(at)) {
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
    events.iter().filter_map(project).collect()
}

fn project(event: &Value) -> Option<Value> {
    if let Some(op) = event.get("op") {
        return Some(json!({ "op": op }));
    }
    let seam = event["seam"].as_str()?;
    let method = event.get("method").and_then(Value::as_str);
    let args = &event["args"];
    match seam {
        "event" => Some(json!({ "event": event["event"] })),
        "host" => Some(json!({ "host": method, "args": args, "answer": event["answer"] })),
        "credentials" => match method {
            Some("issue") => Some(json!({ "issue": event["answer"] })),
            _ => (!args[0].is_null()).then(|| json!({ "revoke": args[0] })),
        },
        "launchFiles" => (!args[0].is_null()).then(|| json!({ "forget": args[0] })),
        "trace" => Some(json!({ "trace": args[0] })),
        "log" => Some(json!({ "log": args })),
        adapter if adapter.starts_with("adapter:") => {
            let given = &args[0];
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
            Some(Value::Object(named))
        }
        _ => None,
    }
}

/// `events` with each conversation item's id (`i-<n>`) numbered by its first
/// appearance.
fn renumbered(events: Vec<Value>) -> Vec<Value> {
    let mut names: HashMap<String, String> = HashMap::new();
    events
        .into_iter()
        .map(|event| rename(event, &mut names))
        .collect()
}

fn rename(value: Value, names: &mut HashMap<String, String>) -> Value {
    match value {
        Value::String(text) if is_item_id(&text) => {
            let next = format!("i-{}", names.len() + 1);
            Value::String(names.entry(text).or_insert(next).clone())
        }
        Value::Array(items) => {
            Value::Array(items.into_iter().map(|item| rename(item, names)).collect())
        }
        Value::Object(fields) => Value::Object(
            fields
                .into_iter()
                .map(|(key, item)| (key, rename(item, names)))
                .collect(),
        ),
        other => other,
    }
}

fn is_item_id(text: &str) -> bool {
    text.strip_prefix("i-").is_some_and(|digits| {
        !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
    })
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
fn an_item_id_is_named_by_its_first_appearance_and_nothing_else_is() {
    let events = vec![
        json!({ "items": [{ "id": "i-7" }, { "id": "i-3" }], "text": "i-7 said" }),
        json!({ "id": "i-3", "other": "i-", "launch": "i-x" }),
    ];
    assert_eq!(
        renumbered(events),
        [
            json!({ "items": [{ "id": "i-1" }, { "id": "i-2" }], "text": "i-7 said" }),
            json!({ "id": "i-2", "other": "i-", "launch": "i-x" }),
        ]
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
    ];
    assert_eq!(
        projected(&events),
        [
            json!({ "op": "pass" }),
            json!({ "revoke": "token-zeus" }),
            json!({ "adapter": "adapter:codex", "method": "observe", "launch": "l" }),
        ]
    );
}

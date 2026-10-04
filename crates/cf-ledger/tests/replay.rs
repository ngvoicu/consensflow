//! Every ledger the Node suite opened, replayed against this one
//! (`tests/goldens/ledger/`; `npm run goldens:ledger` records them into
//! `tests/traces/`): the same file to start from, the same clock readings and
//! the same calls, and each call's answer or refusal, the events it logged and
//! the clock readings it took, then the database it left, compared exactly.
//! A trace that calls what this crate does not do yet is skipped and counted.

// The replay's own scaffolding: a failure in it is the test's.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, VecDeque};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use base64::Engine;
use cf_base::js;
use cf_base::time::{parse, Clock};
use cf_ledger::model::parse_gate;
use cf_ledger::{open_ledger, Event, Ledger, LedgerError, NewProject, Options};
use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
use serde_json::{json, Value};

/// What a replay found.
enum Outcome {
    Replayed,
    /// Calls something this crate does not do yet.
    Skipped(String),
    Failed(String),
}

/// A clock that answers the readings Node recorded, in order.
struct Recorded {
    readings: Rc<RefCell<VecDeque<i64>>>,
    overdrawn: Rc<Cell<bool>>,
}

impl Clock for Recorded {
    fn now_ms(&mut self) -> i64 {
        self.readings.borrow_mut().pop_front().unwrap_or_else(|| {
            self.overdrawn.set(true);
            0
        })
    }
}

fn traces() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("traces")
}

#[test]
fn every_ledger_the_node_suite_opened_answers_here_as_it_answered_there() {
    let mut names: Vec<PathBuf> = std::fs::read_dir(traces())
        .expect("the traces: npm run goldens:ledger")
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "gz"))
        .collect();
    names.sort();
    assert!(!names.is_empty(), "no traces: npm run goldens:ledger");
    let (mut replayed, mut skipped, mut failed) = (0, BTreeMap::<String, usize>::new(), Vec::new());
    for name in &names {
        let mut text = String::new();
        flate2::read::GzDecoder::new(std::fs::File::open(name).unwrap())
            .read_to_string(&mut text)
            .unwrap();
        let trace = name.file_name().unwrap().to_string_lossy().to_string();
        match replay(&text) {
            Outcome::Replayed => replayed += 1,
            Outcome::Skipped(why) => *skipped.entry(why).or_default() += 1,
            Outcome::Failed(why) => failed.push(format!("{trace}: {why}")),
        }
    }
    let skipped_count: usize = skipped.values().sum();
    println!(
        "{replayed} traces replayed, {skipped_count} skipped, {} failed of {}",
        failed.len(),
        names.len()
    );
    for (why, count) in &skipped {
        println!("  skipped {count}: {why}");
    }
    assert!(
        failed.is_empty(),
        "{} traces answered otherwise:\n{}",
        failed.len(),
        failed.join("\n")
    );
    assert!(replayed > 0, "nothing replayed");
}

fn replay(text: &str) -> Outcome {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("consensflow.db");
    // The recorder wrote the ledger's path as «ledger»: this replay's goes back.
    let path = serde_json::to_string(&file.display().to_string()).unwrap();
    let trace: Value =
        serde_json::from_str(&text.replace("«ledger»", &path[1..path.len() - 1])).unwrap();
    let calls = trace["calls"].as_array().cloned().unwrap_or_default();
    // A trace is replayed only whole: anything it calls that this crate does not do skips it.
    if let Some(unknown) = calls.iter().find_map(unsupported) {
        return Outcome::Skipped(unknown);
    }
    let held = start_from(&trace["initial"], &file);
    let readings = Rc::new(RefCell::new(VecDeque::new()));
    let overdrawn = Rc::new(Cell::new(false));
    let told = Rc::new(RefCell::new(Vec::<Event>::new()));
    let events = Rc::clone(&told);
    let opened = open_ledger(
        &file,
        Options {
            clock: Box::new(Recorded {
                readings: Rc::clone(&readings),
                overdrawn: Rc::clone(&overdrawn),
            }),
            trace: Box::new(move |event| events.borrow_mut().push(event.clone())),
        },
    );
    drop(held);
    let mut ledger = match (opened, trace.get("openError")) {
        (Err(error), Some(expected)) => {
            return compare(
                "the opening",
                &failure(&error),
                &json!({ "$error": expected }),
            )
            .map_or(Outcome::Replayed, Outcome::Failed);
        }
        (Ok(_), Some(expected)) => {
            return Outcome::Failed(format!("opened, where Node was refused: {expected}"))
        }
        (Err(error), None) => {
            return Outcome::Failed(format!("refused, where Node opened: {error}"))
        }
        (Ok(ledger), None) => Some(ledger),
    };
    for (at, call) in calls.iter().enumerate() {
        let method = call["method"].as_str().unwrap_or_default();
        let recorded = call["clock"].as_array().unwrap();
        readings.borrow_mut().extend(
            recorded
                .iter()
                .map(|at| parse(at.as_str().unwrap()).expect("a clock reading Node wrote")),
        );
        let answer = match (method, ledger.take()) {
            (_, None) => {
                return Outcome::Failed(format!("call {at} ({method}) after the ledger closed"))
            }
            ("close", Some(open)) => open
                .close()
                .map_or_else(|error| failure(&error), |()| undefined()),
            (_, Some(mut open)) => {
                let args: Vec<Option<Value>> = call["args"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(revive)
                    .collect();
                let answered = answer(&mut open, method, &args);
                ledger = Some(open);
                answered
            }
        };
        let left = readings.borrow_mut().drain(..).count();
        if overdrawn.replace(false) {
            return Outcome::Failed(format!(
                "call {at} ({method}) read the clock more than Node's {} times",
                recorded.len()
            ));
        }
        if left > 0 {
            return Outcome::Failed(format!(
                "call {at} ({method}) read the clock {} times, Node {}",
                recorded.len() - left,
                recorded.len()
            ));
        }
        let logged: Vec<Value> = told
            .borrow_mut()
            .drain(..)
            .map(|event| json!({ "at": event.at, "project": event.project, "kind": event.kind, "data": event.data }))
            .collect();
        if let Some(why) = compare(&format!("call {at} ({method})"), &answer, &call["result"]) {
            return Outcome::Failed(why);
        }
        if let Some(why) = compare(
            &format!("call {at} ({method}) logged"),
            &json!(logged),
            &call["events"],
        ) {
            return Outcome::Failed(why);
        }
        if method == "close" {
            if let Some(why) = compare("the database it left", &dump(&file), &trace["final"]) {
                return Outcome::Failed(why);
            }
        }
    }
    Outcome::Replayed
}

/// What this crate does not do yet, in one call; none when it does all of it.
fn unsupported(call: &Value) -> Option<String> {
    let method = call["method"].as_str().unwrap_or_default();
    const DONE: [&str; 11] = [
        "createProject",
        "project",
        "projects",
        "setProjectState",
        "deleteProject",
        "setGate",
        "suspendForRestart",
        "forgetResume",
        "events",
        "integrity",
        "close",
    ];
    if !DONE.contains(&method) {
        return Some(format!("calls {method}"));
    }
    if call["names"]
        .as_array()
        .is_some_and(|names| !names.is_empty())
    {
        return Some(format!("{method} draws session names"));
    }
    None
}

/// A value a call passed, as the recorder wrote it: `undefined` is no value,
/// so an argument holding it is missing and an object leaves its key out.
fn revive(value: &Value) -> Option<Value> {
    match value {
        Value::Object(fields) if fields.contains_key("$undefined") => None,
        Value::Object(fields) => Some(Value::Object(
            fields
                .iter()
                .filter_map(|(key, item)| Some((key.clone(), revive(item)?)))
                .collect(),
        )),
        other => Some(other.clone()),
    }
}

/// One call made here, and its answer as the recorder wrote Node's.
fn answer(ledger: &mut Ledger, method: &str, args: &[Option<Value>]) -> Value {
    let arg = |at: usize| args.get(at).and_then(Option::as_ref);
    let id = || arg(0).and_then(Value::as_i64).expect("an id");
    let answered: Result<Value, LedgerError> = match method {
        "createProject" => NewProject::from_json(arg(0).expect("a request"))
            .and_then(|request| encode(ledger.create_project(&request))),
        "project" => encode(ledger.project(id())),
        "projects" => encode(ledger.projects()),
        "setProjectState" => encode(ledger.set_project_state(id(), &js::text(arg(1)))),
        "deleteProject" => encode(ledger.delete_project(id())),
        "setGate" => parse_gate(arg(1)).and_then(|gate| encode(ledger.set_gate(id(), gate))),
        "suspendForRestart" => encode(ledger.suspend_for_restart()),
        "forgetResume" => ledger.forget_resume(id()).map(|()| undefined()),
        "events" => {
            let options = arg(1);
            let after = options
                .and_then(|o| o.get("after"))
                .and_then(Value::as_i64)
                .unwrap_or(0);
            let limit = options
                .and_then(|o| o.get("limit"))
                .and_then(Value::as_i64)
                .unwrap_or(500);
            encode(ledger.events(id(), after, limit))
        }
        "integrity" => encode(ledger.integrity()),
        other => unreachable!("{other} is checked before the replay"),
    };
    answered.unwrap_or_else(|error| failure(&error))
}

fn encode<T: Serialize>(answered: Result<T, LedgerError>) -> Result<Value, LedgerError> {
    answered.map(|value| serde_json::to_value(value).unwrap())
}

fn undefined() -> Value {
    json!({ "$undefined": true })
}

/// A refusal as the recorder wrote one: what the ledger refused, or what SQLite said.
fn failure(error: &LedgerError) -> Value {
    match error {
        LedgerError::Refused(refusal) => json!({ "$error": {
            "name": "LedgerError", "code": refusal.code, "status": refusal.status, "message": refusal.message,
        }}),
        other => json!({ "$error": { "name": "Error", "message": other.to_string() } }),
    }
}

/// `actual` against what Node answered: exactly, key order and all; for an
/// error SQLite raised, its message.
fn compare(what: &str, actual: &Value, expected: &Value) -> Option<String> {
    let (actual, expected) = match (actual.get("$error"), expected.get("$error")) {
        (Some(ours), Some(theirs)) if theirs["name"] != "LedgerError" => (
            json!({ "error": ours["message"] }),
            json!({ "error": theirs["message"] }),
        ),
        _ => (actual.clone(), expected.clone()),
    };
    let (ours, theirs) = (actual.to_string(), expected.to_string());
    (ours != theirs).then(|| {
        let at = ours
            .bytes()
            .zip(theirs.bytes())
            .take_while(|(a, b)| a == b)
            .count();
        let from = at.saturating_sub(80);
        let near = |text: &str| {
            text.get(from..(at + 160).min(text.len()))
                .unwrap_or_default()
                .to_string()
        };
        format!(
            "{what} differs at byte {at}:\n    here: …{}…\n    node: …{}…",
            near(&ours),
            near(&theirs)
        )
    })
}

/// The file a trace's ledger found, made again: a database from its dump,
/// bytes as they were, or one another holder keeps (returned, held until the
/// ledger has tried to open it).
fn start_from(initial: &Value, file: &Path) -> Option<Connection> {
    if let Some(database) = initial.get("database") {
        restore(database, file);
    } else if let Some(bytes) = initial.get("bytes").and_then(Value::as_str) {
        std::fs::write(
            file,
            base64::engine::general_purpose::STANDARD
                .decode(bytes)
                .unwrap(),
        )
        .unwrap();
    } else if initial.get("heldHere").is_some() {
        let holder = Connection::open(file).unwrap();
        holder
            .execute_batch("PRAGMA locking_mode = EXCLUSIVE; BEGIN EXCLUSIVE; COMMIT")
            .unwrap();
        return Some(holder);
    }
    None
}

/// A database made again from a recorded dump: its tables, rows (each value
/// as SQLite quoted it, so of its own storage class), indexes, the sequence
/// of its AUTOINCREMENT ids, and its version.
fn restore(database: &Value, file: &Path) {
    let db = Connection::open(file).unwrap();
    // Tables come back in name order, so a row may come before the one it
    // references; and a test may have written rows past the checks on purpose.
    db.execute_batch("PRAGMA foreign_keys = OFF; PRAGMA ignore_check_constraints = ON")
        .unwrap();
    let schema = database["schema"].as_array().unwrap();
    let sql = |entry: &Value| entry["sql"].as_str().unwrap().to_string();
    for entry in schema.iter().filter(|entry| {
        sql(entry).starts_with("CREATE TABLE") && entry["name"] != "sqlite_sequence"
    }) {
        db.execute_batch(&sql(entry)).unwrap();
    }
    let tables = database["tables"].as_object().unwrap();
    for (table, contents) in tables
        .iter()
        .filter(|(table, _)| *table != "sqlite_sequence")
    {
        insert(&db, table, contents);
    }
    for entry in schema
        .iter()
        .filter(|entry| sql(entry).starts_with("CREATE") && !sql(entry).starts_with("CREATE TABLE"))
    {
        db.execute_batch(&sql(entry)).unwrap();
    }
    if let Some(sequence) = tables.get("sqlite_sequence") {
        db.execute_batch("DELETE FROM sqlite_sequence").unwrap();
        insert(&db, "sqlite_sequence", sequence);
    }
    db.execute_batch(&format!(
        "PRAGMA user_version = {}",
        database["userVersion"]
    ))
    .unwrap();
}

fn insert(db: &Connection, table: &str, contents: &Value) {
    let columns: Vec<String> = contents["columns"]
        .as_array()
        .unwrap()
        .iter()
        .map(|column| format!("\"{}\"", column.as_str().unwrap()))
        .collect();
    for row in contents["rows"].as_array().unwrap() {
        let values: Vec<&str> = row
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap())
            .collect();
        db.execute_batch(&format!(
            "INSERT INTO \"{table}\" ({}) VALUES ({})",
            columns.join(", "),
            values.join(", ")
        ))
        .unwrap();
    }
}

/// The database a ledger left, as the recorder dumps it: its version, its
/// schema, and each table's rows in rowid order, each value as SQLite quotes it.
fn dump(file: &Path) -> Value {
    let db = Connection::open_with_flags(file, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let schema: Vec<Value> = db
        .prepare("SELECT name, sql FROM sqlite_master WHERE type IN ('table', 'index') AND sql IS NOT NULL ORDER BY name")
        .unwrap()
        .query_map([], |row| Ok(json!({ "name": row.get::<_, String>(0)?, "sql": row.get::<_, String>(1)? })))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let names: Vec<String> = db
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let mut tables = serde_json::Map::new();
    for name in names {
        let columns: Vec<String> = db
            .prepare(&format!("PRAGMA table_info(\"{name}\")"))
            .unwrap()
            .query_map([], |row| row.get(1))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        let quoted = columns
            .iter()
            .map(|column| format!("quote(\"{column}\")"))
            .collect::<Vec<_>>()
            .join(", ");
        let rows: Vec<Value> = db
            .prepare(&format!("SELECT {quoted} FROM \"{name}\" ORDER BY rowid"))
            .unwrap()
            .query_map([], |row| {
                (0..columns.len())
                    .map(|at| row.get::<_, String>(at))
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .unwrap()
            .map(|row| json!(row.unwrap()))
            .collect();
        tables.insert(name, json!({ "columns": columns, "rows": rows }));
    }
    let version: i64 = db
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    json!({ "userVersion": version, "schema": schema, "tables": tables })
}

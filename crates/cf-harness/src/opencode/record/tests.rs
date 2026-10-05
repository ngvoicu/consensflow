//! What no scenario of the goldens reaches. Each expected reading is what
//! Node's `opencodeReader` read of the same store (Node 26, `TZ` set to
//! `America/Los_Angeles`), written as `show` writes one; a failure V8 threw
//! is said here in words of its own.

mod answers;
mod reads;

use std::path::PathBuf;

use jiff::tz::TimeZoneDatabase;
use rusqlite::Connection;
use serde_json::{json, Value};

use super::*;
use crate::shared::record::reading::Settlement;

const SESSION: &str = "ses_1";

/// A store of OpenCode's tables, under a root of its own, holding the
/// session `ses_1`.
struct Staged {
    root: tempfile::TempDir,
    env: Env,
    store: Connection,
}

impl Staged {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("opencode")).unwrap();
        let store = Connection::open(root.path().join("opencode").join("opencode.db")).unwrap();
        store
            .execute_batch(
                "pragma journal_mode = WAL;
                 create table session (id text primary key, title text);
                 create table message (id text primary key, session_id text not null,
                   time_created integer not null, time_updated integer not null, data text not null);
                 create table part (id text primary key, message_id text not null,
                   session_id text not null, time_created integer not null,
                   time_updated integer not null, data text not null);
                 create table event (id text primary key, aggregate_id text not null,
                   seq integer not null, type text not null, data text not null);",
            )
            .unwrap();
        store
            .execute("insert into session values (?, 't')", [SESSION])
            .unwrap();
        let env = Env::from_vars([("XDG_DATA_HOME", root.path().as_os_str())]);
        Self { root, env, store }
    }

    fn file(&self) -> PathBuf {
        self.root.path().join("opencode").join("opencode.db")
    }

    /// `data` as a row holds it: text as it is, any other value as its JSON.
    fn data(data: &Value) -> String {
        match data {
            Value::String(text) => text.clone(),
            other => other.to_string(),
        }
    }

    fn message(&self, id: &str, time: i64, data: &Value) {
        self.store
            .execute(
                "insert or replace into message values (?, ?, ?, ?, ?)",
                (id, SESSION, time, time, Self::data(data)),
            )
            .unwrap();
    }

    fn part(&self, id: &str, message: &str, time: i64, data: &Value) {
        self.store
            .execute(
                "insert or replace into part values (?, ?, ?, ?, ?, ?)",
                (id, message, SESSION, time, time, Self::data(data)),
            )
            .unwrap();
    }

    fn event(&self, seq: i64, kind: &str, data: &Value) {
        self.store
            .execute(
                "insert into event values (?, ?, ?, ?, ?)",
                (format!("e{seq}"), SESSION, seq, kind, Self::data(data)),
            )
            .unwrap();
    }

    /// A user's message and a reply, each with a text part, and their
    /// events: the reply's completion its own, when it has one.
    fn conversation(&self, reply: &Value) {
        self.message(
            "m1",
            1,
            &json!({ "role": "user", "time": { "created": 1 } }),
        );
        self.part(
            "p1",
            "m1",
            2,
            &json!({ "type": "text", "text": "Review T-1" }),
        );
        self.message("m2", 3, reply);
        self.part("p2", "m2", 4, &json!({ "type": "text", "text": "Done." }));
        self.event(1, "message.updated.1", &info("m1", None));
        self.event(2, "message.part.updated.1", &part_of("p1", "m1"));
        self.event(3, "message.updated.1", &info("m2", None));
        self.event(4, "message.part.updated.1", &part_of("p2", "m2"));
        if let Some(time) = reply
            .get("time")
            .filter(|time| time.get("completed").is_some())
        {
            self.event(5, "message.updated.1", &info("m2", Some(time)));
        }
    }

    /// The conversation with a stopped reply.
    fn stopped(&self) {
        self.conversation(&json!({
            "role": "assistant",
            "time": { "created": 3, "completed": 9 },
            "finish": "stop",
        }));
    }

    fn reader(&self) -> Box<dyn Look + Send> {
        reader(SESSION, &self.env, &local())
    }
}

/// The zone the readings were made in.
fn local() -> TimeZone {
    TimeZoneDatabase::bundled()
        .get("America/Los_Angeles")
        .unwrap()
}

fn info(id: &str, time: Option<&Value>) -> Value {
    let mut info = json!({ "id": id, "sessionID": SESSION });
    if let Some(time) = time {
        info["time"] = time.clone();
    }
    json!({ "info": info })
}

fn part_of(id: &str, message: &str) -> Value {
    json!({ "part": { "id": id, "messageID": message, "sessionID": SESSION } })
}

fn look(reader: &mut Box<dyn Look + Send>) -> Arc<Reading> {
    reader.look(&Options::default(), 0)
}

/// A reading as the Node probe printed it.
fn show(reading: &Reading) -> String {
    let record = match reading {
        Reading::Unknown(reason) => return format!("unknown: {reason}"),
        Reading::Known(record) => record,
    };
    let items: Vec<Value> = record
        .items
        .iter()
        .map(|item| json!([&*item.id, item.role, &*item.text, item.complete, item.at]))
        .collect();
    let state = match record.settlement {
        Settlement::Unknown => "unknown",
        Settlement::InFlight => "in-flight",
        Settlement::Settled => "settled",
    };
    format!(
        r#"{{"items":{},"inFlight":{},"asking":{},"failed":{},"quota":{},"settlement":"{state}"}}"#,
        Value::from(items),
        record.in_flight,
        record.asking,
        record.failed,
        serde_json::to_string(&record.quota).unwrap()
    )
}

fn read_once(staged: &Staged) -> String {
    show(&look(&mut staged.reader()))
}

/// The items of the conversation, its reply complete or not, at `at`.
fn conversation_items(complete: bool, at: i64) -> String {
    format!(r#"[["m1","user","Review T-1",true,1],["m2","assistant","Done.",{complete},{at}]]"#)
}

/// A reading of `items`, its turn settled.
fn settled(items: &str) -> String {
    format!(
        r#"{{"items":{items},"inFlight":false,"asking":false,"failed":false,"quota":null,"settlement":"settled"}}"#
    )
}

/// A reading of `items`, its turn in flight.
fn in_flight(items: &str, asking: bool) -> String {
    format!(
        r#"{{"items":{items},"inFlight":true,"asking":{asking},"failed":false,"quota":null,"settlement":"in-flight"}}"#
    )
}

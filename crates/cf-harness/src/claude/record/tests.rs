//! What the goldens' scenarios never reach, played against a transcript in a
//! Claude config folder of its own. Each expected reading is what Node's
//! `claudeReader` read of the same records (Node 26.8.1, `TZ` set to
//! `America/Los_Angeles`), written as `show` writes one. A failure V8 threw
//! is said here in words of its own, and the test says what Node's was.
//!
//! The records and the helpers that make them are here; the tests are by what
//! they look at: the ids and texts of the records (`texts`), what ends a turn
//! (`boundaries`), the tool calls and how the items are ordered (`items`),
//! the messages queued (`queues`), the API's refusals (`refusals`), the
//! output of a `/clear` (`clear`), an interrupt (`interrupts`), and the
//! records read late (`ancestry`). What a record waits as, and the line it is
//! built from, are held to what replay reads of a record: what nothing reads
//! changes nothing (`projection`), and each field replay reads changes the
//! reading (`fields`). A long transcript, made like a big one (`synthetic`),
//! is read (`scale`) and measured (`memory`).

mod ancestry;
mod boundaries;
mod clear;
mod fields;
mod interrupts;
mod items;
mod memory;
mod projection;
mod queues;
mod refusals;
mod scale;
mod synthetic;
mod texts;

use std::fs;
use std::path::PathBuf;

use cf_base::js;
use jiff::tz::TimeZoneDatabase;
use serde_json::{json, Map, Value};
use tempfile::TempDir;

use super::*;
use crate::shared::record::cache::Options;
use crate::shared::record::reading::Reading;

const SESSION: &str = "s1";

/// The zone the readings were made in, which a reset that names none is read in.
fn local() -> TimeZone {
    TimeZoneDatabase::bundled()
        .get("America/Los_Angeles")
        .unwrap()
}

/// A transcript in a config folder of its own, and the reader of its
/// session, which is the same reader look after look.
struct Stage {
    root: TempDir,
    reader: Box<dyn Look + Send>,
}

impl Stage {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let env = Env::from_vars([("CLAUDE_CONFIG_DIR", root.path().as_os_str())]);
        let reader = reader(SESSION, &env, &local());
        Self { root, reader }
    }

    fn file(&self) -> PathBuf {
        self.root
            .path()
            .join("projects")
            .join(format!("{SESSION}.jsonl"))
    }

    /// The records as the transcript's text: a line each.
    fn lines(records: &[Value]) -> String {
        let lines: Vec<String> = records.iter().map(Value::to_string).collect();
        format!("{}\n", lines.join("\n"))
    }

    /// The transcript, written anew.
    fn write(&self, records: &[Value]) {
        fs::create_dir_all(self.file().parent().unwrap()).unwrap();
        fs::write(self.file(), Self::lines(records)).unwrap();
    }

    /// `records` added to the transcript.
    fn append(&self, records: &[Value]) {
        let mut file = fs::OpenOptions::new()
            .append(true)
            .open(self.file())
            .unwrap();
        std::io::Write::write_all(&mut file, Self::lines(records).as_bytes()).unwrap();
    }

    /// A look, as `show` writes it.
    fn look(&mut self) -> String {
        show(&self.reader.look(&Options::default(), 0))
    }
}

/// What one look at a transcript of `records` reads.
fn once(records: &[Value]) -> String {
    let mut stage = Stage::new();
    stage.write(records);
    stage.look()
}

/// What one look at a transcript of `records` says of the turn.
fn settlement_of(records: &[Value]) -> Settlement {
    let mut stage = Stage::new();
    stage.write(records);
    match &*stage.reader.look(&Options::default(), 0) {
        Reading::Known(record) => record.settlement,
        Reading::Unknown(reason) => panic!("unknown: {reason}"),
    }
}

/// What a look after each batch of records reads, the batches written one
/// after the other and read by one reader.
fn looks(batches: &[&[Value]]) -> Vec<String> {
    let mut stage = Stage::new();
    batches
        .iter()
        .enumerate()
        .map(|(at, records)| {
            if at == 0 {
                stage.write(records);
            } else {
                stage.append(records);
            }
            stage.look()
        })
        .collect()
}

/// A reading as the Node probe printed it: its reason, or its items and what
/// they say of the turn.
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
        js::stringify(&Value::from(items)),
        record.in_flight,
        record.asking,
        record.failed,
        serde_json::to_string(&record.quota).unwrap()
    )
}

/// A reading of `items`, its turn settled, as Node printed it.
fn settled(items: &str) -> String {
    format!(
        r#"{{"items":{items},"inFlight":false,"asking":false,"failed":false,"quota":null,"settlement":"settled"}}"#
    )
}

/// A reading of `items`, its turn in flight.
fn in_flight(items: &str) -> String {
    format!(
        r#"{{"items":{items},"inFlight":true,"asking":false,"failed":false,"quota":null,"settlement":"in-flight"}}"#
    )
}

/// The base of every record of the session: its id, and the main conversation's.
fn of_session(mut record: Value) -> Value {
    record["sessionId"] = json!(SESSION);
    record["isSidechain"] = json!(false);
    record
}

fn text(value: Value) -> Value {
    json!({ "type": "text", "text": value })
}

fn user(uuid: &str, parent: Value, content: Value) -> Value {
    of_session(json!({
        "type": "user", "uuid": uuid, "parentUuid": parent,
        "message": { "role": "user", "content": content },
    }))
}

/// An assistant's record of the message `id`, which stopped as `stop` says.
fn assistant(uuid: &str, parent: &str, id: Value, content: Value, stop: Value) -> Value {
    of_session(json!({
        "type": "assistant", "uuid": uuid, "parentUuid": parent,
        "message": { "id": id, "role": "assistant", "content": content, "stop_reason": stop },
    }))
}

/// The assistant's answer: message `m1`, which ended its turn.
fn answer(uuid: &str, parent: &str) -> Value {
    assistant(
        uuid,
        parent,
        json!("m1"),
        json!([text(json!("Hi"))]),
        json!("end_turn"),
    )
}

/// A refusal of the API, which says when its limit resets: the assistant's
/// record `uuid` of the message `m1`, an error at a time of its own, with
/// `fields` besides.
fn refused(uuid: &str, fields: Value) -> Value {
    let said = text(json!("You've hit your limit. Resets in 2 hours."));
    let record = assistant(uuid, "u1", json!("m1"), json!([said]), Value::Null);
    having(
        having(
            record,
            json!({ "isApiErrorMessage": true, "timestamp": "2026-09-19T10:00:00.000Z" }),
        ),
        fields,
    )
}

fn duration(uuid: &str, parent: &str) -> Value {
    of_session(json!({
        "type": "system", "subtype": "turn_duration", "uuid": uuid, "parentUuid": parent,
        "durationMs": 5, "messageCount": 2,
    }))
}

fn summary(uuid: &str, parent: &str) -> Value {
    of_session(json!({
        "type": "system", "subtype": "stop_hook_summary", "uuid": uuid, "parentUuid": parent,
        "preventedContinuation": false,
    }))
}

fn attachment(uuid: &str, parent: &str) -> Value {
    of_session(json!({
        "type": "attachment", "uuid": uuid, "parentUuid": parent,
        "attachment": { "type": "environment" },
    }))
}

/// What a `UserPromptSubmit` hook added to the prompt.
fn hook(content: Value, uuid: Option<&str>) -> Value {
    let record = of_session(json!({
        "type": "attachment",
        "attachment": { "type": "hook_additional_context", "hookEvent": "UserPromptSubmit", "content": content },
    }));
    match uuid {
        Some(uuid) => having(record, json!({ "uuid": uuid })),
        None => record,
    }
}

/// What Claude Code writes as the user's turn for a `/clear`, `between` its tags.
fn command(between: &str, args: &str) -> String {
    format!(
        "<command-name>/clear</command-name>{between}<command-message>clear</command-message>{between}<command-args>{args}</command-args>"
    )
}

/// The output of a `/clear`, a record of the turn `parent`.
fn output(parent: &str) -> Value {
    of_session(json!({
        "type": "system", "subtype": "local_command", "isMeta": false, "level": "info",
        "content": "<local-command-stdout></local-command-stdout>",
        "uuid": "c1", "parentUuid": parent,
    }))
}

fn queue(operation: &str, content: Value) -> Value {
    json!({ "type": "queue-operation", "operation": operation, "content": content })
}

fn tool_use(id: Value) -> Value {
    json!({ "type": "tool_use", "id": id, "name": "Read" })
}

fn tool_result(id: Value, content: Value) -> Value {
    json!({ "type": "tool_result", "tool_use_id": id, "content": content })
}

/// The first user turn of every transcript: `u1`, saying `Hello`.
fn hello() -> Value {
    user("u1", Value::Null, json!("Hello"))
}

/// `record` with `fields` set.
fn having(mut record: Value, fields: Value) -> Value {
    let Value::Object(set) = fields else {
        unreachable!("fields are an object");
    };
    let Value::Object(into) = &mut record else {
        unreachable!("a record is an object");
    };
    into.extend(set);
    record
}

/// `record` without the field `name`.
fn lacking(mut record: Value, name: &str) -> Value {
    let Value::Object(fields) = &mut record else {
        unreachable!("a record is an object");
    };
    fields.shift_remove(name);
    record
}

/// `record` with the `name` of its field `parent` set to `value`, or left
/// out when none.
fn within(mut record: Value, parent: &str, name: &str, value: Option<Value>) -> Value {
    let fields: &mut Map<String, Value> = record[parent]
        .as_object_mut()
        .expect("a record with that field");
    match value {
        Some(value) => {
            fields.insert(name.to_owned(), value);
        }
        None => {
            fields.shift_remove(name);
        }
    }
    record
}

/// `record` with its message's `name` set to `value`, or left out when none.
fn in_message(record: Value, name: &str, value: Option<Value>) -> Value {
    within(record, "message", name, value)
}

/// `record` with its attachment's `name` set to `value`, or left out when none.
fn in_attachment(record: Value, name: &str, value: Option<Value>) -> Value {
    within(record, "attachment", name, value)
}

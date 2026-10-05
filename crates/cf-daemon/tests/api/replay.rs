//! The calls a test made on its ledger, replayed as step 3.1's replay replays
//! them (`crates/cf-ledger/tests/replay.rs`, from which the arms are taken):
//! the clock's readings and the session names Node's call drew are put in the
//! queues before it, and found taken after; its answer or refusal and the
//! events it logged are compared exactly.
//!
//! Only the calls the API's traces make are here. Another is a failure of the
//! player, not a skip: a trace it cannot replay whole proves nothing.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;

use cf_base::js;
use cf_base::time::{parse, Clock};
use cf_ledger::model::{parse_gate, parse_roles};
use cf_ledger::{
    ChiefSwitch, Event, Ledger, LedgerError, NewMember, NewNote, NewProject, NewQuestion, NewTask,
};
use serde::Serialize;
use serde_json::{json, Value};

/// What Node's ledger drew (the clock's readings, a session's name), answered
/// here in the same order; drawing past what was given is noted.
pub struct Draws<T> {
    left: Rc<RefCell<VecDeque<T>>>,
    overdrawn: Rc<Cell<bool>>,
}

impl<T> Draws<T> {
    pub fn new() -> Self {
        Self {
            left: Rc::new(RefCell::new(VecDeque::new())),
            overdrawn: Rc::new(Cell::new(false)),
        }
    }

    pub fn share(&self) -> Self {
        Self {
            left: Rc::clone(&self.left),
            overdrawn: Rc::clone(&self.overdrawn),
        }
    }

    pub fn give(&self, drawn: impl IntoIterator<Item = T>) {
        self.left.borrow_mut().extend(drawn);
    }

    pub fn draw(&self) -> Option<T> {
        let next = self.left.borrow_mut().pop_front();
        if next.is_none() {
            self.overdrawn.set(true);
        }
        next
    }

    /// After a step: why not, when it did not draw what Node's did.
    fn settle(&self, what: &str, recorded: usize) -> Option<String> {
        let left = self.left.borrow_mut().drain(..).count();
        if self.overdrawn.replace(false) {
            return Some(format!("{what} more than Node's {recorded} times"));
        }
        (left > 0).then(|| format!("{what} {} times, Node {recorded}", recorded - left))
    }
}

/// A clock that answers the readings Node recorded, in order.
pub struct Recorded(pub Draws<i64>);

impl Clock for Recorded {
    fn now_ms(&mut self) -> i64 {
        self.0.draw().unwrap_or_default()
    }
}

/// The readings and names a step is to draw: put in before it, and found taken
/// after.
pub struct Queues {
    pub readings: Draws<i64>,
    pub names: Draws<String>,
}

impl Queues {
    pub fn new() -> Self {
        Self {
            readings: Draws::new(),
            names: Draws::new(),
        }
    }

    /// What a step recorded as drawn: its `clock` and `names`.
    pub fn give(&self, step: &Value) {
        let readings = step["clock"].as_array().cloned().unwrap_or_default();
        self.readings.give(
            readings
                .iter()
                .map(|at| parse(at.as_str().expect("a reading")).expect("a time Node wrote")),
        );
        let names = step["names"].as_array().cloned().unwrap_or_default();
        self.names.give(
            names
                .iter()
                .map(|name| name.as_str().expect("a name").to_owned()),
        );
    }

    /// After the steps that were given `steps`: why they did not draw what
    /// Node's drew, if they did not.
    pub fn settle(&self, steps: &[&Value]) -> Option<String> {
        let count = |key: &str| {
            steps
                .iter()
                .map(|step| step[key].as_array().map_or(0, Vec::len))
                .sum()
        };
        [
            self.readings.settle("read the clock", count("clock")),
            self.names.settle("drew a session name", count("names")),
        ]
        .into_iter()
        .flatten()
        .next()
    }
}

/// An event as the recorder wrote one.
pub fn event_json(event: &Event) -> Value {
    json!({ "at": event.at, "project": event.project, "kind": event.kind, "data": event.data })
}

/// A value a call passed, as the recorder wrote it: `undefined` is no value,
/// so an argument holding it is missing, an object leaves its key out, and a
/// list holds null in its place, as `JSON.stringify` writes it.
fn revive(value: &Value) -> Option<Value> {
    match value {
        Value::Object(fields) if fields.contains_key("$undefined") => None,
        Value::Object(fields) => Some(Value::Object(
            fields
                .iter()
                .filter_map(|(key, item)| Some((key.clone(), revive(item)?)))
                .collect(),
        )),
        Value::Array(items) => Some(Value::Array(
            items
                .iter()
                .map(|item| revive(item).unwrap_or(Value::Null))
                .collect(),
        )),
        other => Some(other.clone()),
    }
}

/// Argument `at`, when the call passed one.
fn arg(args: &[Option<Value>], at: usize) -> Option<&Value> {
    args.get(at).and_then(Option::as_ref)
}

/// Field `name` of argument `at`, an object.
fn field<'a>(args: &'a [Option<Value>], at: usize, name: &str) -> Option<&'a Value> {
    arg(args, at).and_then(|value| value.get(name))
}

fn text(value: Option<&Value>) -> &str {
    value.and_then(Value::as_str).expect("text")
}

fn integer(value: Option<&Value>) -> i64 {
    value.and_then(Value::as_i64).expect("an integer")
}

fn encode<T: Serialize>(answered: Result<T, LedgerError>) -> Result<Value, LedgerError> {
    answered.map(|value| serde_json::to_value(value).expect("a view that is JSON"))
}

pub fn undefined() -> Value {
    json!({ "$undefined": true })
}

/// A refusal as the recorder wrote one: what the ledger refused, or what
/// SQLite said.
pub fn failure(error: &LedgerError) -> Value {
    match error {
        LedgerError::Refused(refusal) => json!({ "$error": {
            "name": "LedgerError", "code": refusal.code, "status": refusal.status, "message": refusal.message,
        }}),
        other => json!({ "$error": { "name": "Error", "message": other.to_string() } }),
    }
}

/// `actual` against what Node answered: exactly, key order and all; for an
/// error SQLite raised, its message.
pub fn compare(what: &str, actual: &Value, expected: &Value) -> Option<String> {
    let (actual, expected) = match (actual.get("$error"), expected.get("$error")) {
        (Some(ours), Some(theirs)) if theirs["name"] != "LedgerError" => (
            json!({ "error": ours["message"] }),
            json!({ "error": theirs["message"] }),
        ),
        _ => (actual.clone(), expected.clone()),
    };
    let (ours, theirs) = (actual.to_string(), expected.to_string());
    (ours != theirs).then(|| differs(what, &ours, &theirs))
}

/// Where two texts first differ, and what is near.
pub fn differs(what: &str, ours: &str, theirs: &str) -> String {
    let at = ours
        .bytes()
        .zip(theirs.bytes())
        .take_while(|(a, b)| a == b)
        .count();
    let from = at.saturating_sub(80);
    let near = |text: &str| {
        text.get(from..(at + 160).min(text.len()))
            .unwrap_or_default()
            .to_owned()
    };
    format!(
        "{what} differs at byte {at}:\n    here: …{}…\n    node: …{}…",
        near(ours),
        near(theirs)
    )
}

/// One call of a trace made here, and its answer as the recorder wrote
/// Node's.
pub fn answer(ledger: &mut Ledger, call: &Value) -> Value {
    let method = call["method"].as_str().unwrap_or_default();
    let args: Vec<Option<Value>> = call["args"]
        .as_array()
        .expect("the call's arguments")
        .iter()
        .map(revive)
        .collect();
    let args = args.as_slice();
    let id = || integer(arg(args, 0));
    let optional_text = |at: usize, name: &str| {
        field(args, at, name)
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    let answered: Result<Value, LedgerError> = match method {
        "createProject" => NewProject::from_json(arg(args, 0).expect("a request"))
            .and_then(|request| encode(ledger.create_project(&request))),
        "setProjectState" => encode(ledger.set_project_state(id(), &js::text(arg(args, 1)))),
        "deleteProject" => encode(ledger.delete_project(id())),
        "setGate" => parse_gate(arg(args, 1)).and_then(|gate| encode(ledger.set_gate(id(), gate))),
        "addMember" => NewMember::from_json(arg(args, 1).expect("a member"))
            .and_then(|member| encode(ledger.add_member(id(), &member))),
        "setRoles" => parse_roles(arg(args, 2))
            .and_then(|roles| encode(ledger.set_roles(id(), text(arg(args, 1)), &roles))),
        "endSession" => {
            encode(ledger.end_session(id(), text(arg(args, 1)), text(field(args, 2, "by"))))
        }
        "startConversation" => {
            encode(ledger.start_conversation(id(), text(field(args, 1, "harness"))))
        }
        "bindConversation" => encode(ledger.bind_conversation(id(), text(arg(args, 1)))),
        "copyTranscript" => {
            let items = arg(args, 1).and_then(Value::as_array).expect("a list");
            let from = field(args, 2, "from").and_then(Value::as_i64);
            encode(ledger.copy_transcript(id(), items, from.unwrap_or(0)))
        }
        "switchChief" => ChiefSwitch::from_json(arg(args, 1).expect("a switch"))
            .and_then(|switch| encode(ledger.switch_chief(id(), &switch))),
        "createTask" => NewTask::from_json(arg(args, 1).expect("a task"))
            .and_then(|task| encode(ledger.create_task(id(), &task))),
        "assignTask" => {
            encode(ledger.assign_task(id(), integer(arg(args, 1)), integer(arg(args, 2))))
        }
        "recordResult" => {
            encode(ledger.record_result(id(), integer(arg(args, 1)), text(field(args, 2, "body"))))
        }
        "acceptTask" => {
            encode(ledger.accept_task(id(), integer(arg(args, 1)), text(field(args, 2, "by"))))
        }
        "cancelTask" => {
            encode(ledger.cancel_task(id(), integer(arg(args, 1)), text(field(args, 2, "by"))))
        }
        "deleteTasks" => {
            let numbers: Vec<i64> = arg(args, 1)
                .and_then(Value::as_array)
                .expect("task numbers")
                .iter()
                .map(|number| integer(Some(number)))
                .collect();
            encode(ledger.delete_tasks(id(), &numbers))
        }
        "note" => encode(ledger.note(
            id(),
            &NewNote {
                from: optional_text(1, "from"),
                to: text(field(args, 1, "to")).to_owned(),
                body: text(field(args, 1, "body")).to_owned(),
                task: field(args, 1, "task").and_then(Value::as_i64),
            },
        )),
        "ask" => encode(
            ledger.ask(
                id(),
                &NewQuestion {
                    from: optional_text(1, "from"),
                    to: text(field(args, 1, "to")).to_owned(),
                    body: optional_text(1, "body"),
                    task: field(args, 1, "task").and_then(Value::as_i64),
                    questions: field(args, 1, "questions").cloned(),
                    urgent: field(args, 1, "urgent")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                },
            ),
        ),
        "beginDelivery" => encode(ledger.begin_delivery(id())),
        "confirmDelivery" => encode(ledger.confirm_delivery(id(), arg(args, 1))),
        "approveMessage" => encode(ledger.approve_message(id(), text(field(args, 1, "by")))),
        other => panic!("the player does not replay the ledger's {other} yet"),
    };
    answered.unwrap_or_else(|error| failure(&error))
}

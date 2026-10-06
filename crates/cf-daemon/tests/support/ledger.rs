//! The ledger a trace's own calls are made again on (as step 3.1's replay makes
//! them, `crates/cf-ledger/tests/replay.rs`, from which the arms are taken):
//! opened on a file of its own, its clock and the session names it draws taken
//! from what Node's call drew, every event it logs kept. A call is made again
//! with [`Rig::around`], and the answer, the readings and names, and the events
//! it left are held to what Node's call had; the database the ledger leaves is
//! held to `ledger.final` by [`Rig::close`].
//!
//! Where the ledger does not answer as Node's did, the why is returned, not
//! recorded and not panicked over: a player with an operation running must see
//! it through to compare its reply, and one with none stops at the step.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use cf_base::js;
use cf_base::time::{parse, Clock};
use cf_ledger::model::{parse_gate, parse_roles};
use cf_ledger::{
    open_ledger, ChiefSwitch, Event, Ledger, LedgerError, NewMember, NewNote, NewProject,
    NewQuestion, NewTask, Options,
};
use serde::Serialize;
use serde_json::{json, Value};

use crate::support::compare::{compare, left};

/// What Node's ledger drew (the clock's readings, a session's name), answered
/// here in the same order; drawing past what was given is noted.
struct Draws<T> {
    left: Rc<RefCell<VecDeque<T>>>,
    overdrawn: Rc<Cell<bool>>,
}

impl<T> Draws<T> {
    fn new() -> Self {
        Self {
            left: Rc::new(RefCell::new(VecDeque::new())),
            overdrawn: Rc::new(Cell::new(false)),
        }
    }

    fn share(&self) -> Self {
        Self {
            left: Rc::clone(&self.left),
            overdrawn: Rc::clone(&self.overdrawn),
        }
    }

    fn give(&self, drawn: impl IntoIterator<Item = T>) {
        self.left.borrow_mut().extend(drawn);
    }

    fn draw(&self) -> Option<T> {
        let next = self.left.borrow_mut().pop_front();
        if next.is_none() {
            self.overdrawn.set(true);
        }
        next
    }

    /// What was waiting to be drawn, taken.
    fn hold(&self) -> VecDeque<T> {
        std::mem::take(&mut *self.left.borrow_mut())
    }

    /// What was held, waiting again.
    fn restore(&self, held: VecDeque<T>) {
        *self.left.borrow_mut() = held;
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
struct Recorded(Draws<i64>);

impl Clock for Recorded {
    fn now_ms(&mut self) -> i64 {
        self.0.draw().unwrap_or_default()
    }
}

/// The readings and names a step is to draw: put in before it, and found taken
/// after.
struct Queues {
    readings: Draws<i64>,
    names: Draws<String>,
}

impl Queues {
    fn new() -> Self {
        Self {
            readings: Draws::new(),
            names: Draws::new(),
        }
    }

    /// What a step recorded as drawn: its `clock` and `names`.
    fn give(&self, step: &Value) {
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
    fn settle(&self, steps: &[&Value]) -> Option<String> {
        let count = |key: &str| {
            steps
                .iter()
                .map(|step| step[key].as_array().map_or(0, Vec::len))
                .sum()
        };
        let readings = self.readings.settle("read the clock", count("clock"));
        let names = self.names.settle("drew a session name", count("names"));
        readings.or(names)
    }
}

/// A ledger open on a file, and what it was told.
pub struct Rig {
    file: PathBuf,
    pub ledger: Rc<RefCell<Ledger>>,
    queues: Queues,
    told: Rc<RefCell<Vec<Event>>>,
}

impl Rig {
    /// A ledger on a file of its own at `file`.
    pub fn open(file: &Path) -> Self {
        let queues = Queues::new();
        let told = Rc::new(RefCell::new(Vec::new()));
        let (events, drawn) = (Rc::clone(&told), queues.names.share());
        let ledger = open_ledger(
            file,
            Options {
                clock: Box::new(Recorded(queues.readings.share())),
                names: Box::new(move || drawn.draw().unwrap_or_default()),
                trace: Box::new(move |event| events.borrow_mut().push(event.clone())),
            },
        )
        .expect("a ledger");
        Self {
            file: file.to_owned(),
            ledger: Rc::new(RefCell::new(ledger)),
            queues,
            told,
        }
    }

    /// The readings and names an operation or an exchange of the trace is to
    /// draw, put in before it runs: `step` is its record.
    pub fn give(&self, step: &Value) {
        self.queues.give(step);
    }

    /// After the operation or exchanges that were given `steps`: why they did
    /// not draw what Node's drew, if they did not.
    pub fn settle(&self, steps: &[&Value]) -> Option<String> {
        self.queues.settle(steps)
    }

    /// The events logged after the first `before` of those not yet taken,
    /// taken, as the recorder wrote them.
    fn events_after(&self, before: usize) -> Vec<Value> {
        let mut waiting = self.told.borrow_mut();
        let from = before.min(waiting.len());
        let mut taken: Vec<Value> = waiting.split_off(from).iter().map(event_json).collect();
        // The stop a pause counted is in the event, and Node's never says it.
        cf_ledger::testing::hold_apart_what_node_never_logs(&mut taken);
        taken
    }

    /// The events logged since they were last taken.
    pub fn take_events(&self) -> Vec<Value> {
        self.events_after(0)
    }

    /// A call of the trace, made by `make`: what was waiting to be drawn is
    /// held for the while (an operation's own readings, which the stand-ins it
    /// calls do not take), the call's own are put in and found taken after, and
    /// its answer and the events it logged meanwhile are held to what Node's
    /// call had. What it made, and why it is not Node's, if it is not.
    pub fn around<T: Serialize>(
        &self,
        recorded: &Value,
        make: impl FnOnce(&mut Ledger) -> Result<T, LedgerError>,
    ) -> (Result<T, LedgerError>, Option<String>) {
        let held = (self.queues.readings.hold(), self.queues.names.hold());
        let before = self.told.borrow().len();
        self.queues.give(recorded);
        let made = make(&mut self.ledger.borrow_mut());
        let answered = compare("its answer", &answer_of(&made), &recorded["result"]);
        let drew = self.queues.settle(&[recorded]);
        let logged = compare(
            "the events it logged",
            &json!(self.events_after(before)),
            &recorded["events"],
        );
        self.queues.readings.restore(held.0);
        self.queues.names.restore(held.1);
        (made, answered.or(drew).or(logged))
    }

    /// A call the test made on its ledger, made again; why it is not Node's.
    pub fn apply(&self, call: &Value) -> Option<String> {
        let method = call["method"].as_str().unwrap_or_default();
        let (_, problem) = self.around(call, |ledger| answer(ledger, call));
        problem.map(|why| format!("ledger {method}: {why}"))
    }

    /// The `close` that ends a trace: made again like any call, and the
    /// database it leaves held to `expected`, `ledger.final`.
    pub fn close(&self, step: &Value, expected: &Value) -> Option<String> {
        let (_, problem) =
            self.around(step, |ledger| ledger.close_in_place().map(|()| undefined()));
        problem
            .map(|why| format!("ledger close: {why}"))
            .or_else(|| left(&self.file, expected))
    }
}

/// An event as the recorder wrote one.
fn event_json(event: &Event) -> Value {
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

/// What a call answered, as the recorder wrote an answer: the value, or the
/// refusal as `{$error}`.
fn answer_of<T: Serialize>(made: &Result<T, LedgerError>) -> Value {
    match made {
        Ok(value) => serde_json::to_value(value).expect("a view that is JSON"),
        Err(error) => failure(error),
    }
}

fn undefined() -> Value {
    json!({ "$undefined": true })
}

/// A refusal as the recorder wrote one: what the ledger refused, or what
/// SQLite said.
fn failure(error: &LedgerError) -> Value {
    match error {
        LedgerError::Refused(refusal) => json!({ "$error": {
            "name": "LedgerError", "code": refusal.code, "status": refusal.status, "message": refusal.message,
        }}),
        other => json!({ "$error": { "name": "Error", "message": other.to_string() } }),
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

/// One call of a trace made on `ledger`: the answer, as the recorder wrote
/// Node's. A call the player does not make is a failure of the player, not a
/// skip: a trace it cannot replay whole proves nothing.
fn answer(ledger: &mut Ledger, call: &Value) -> Result<Value, LedgerError> {
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
    match method {
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
        "markOut" => encode(ledger.mark_out(
            id(),
            text(field(args, 1, "until")),
            text(field(args, 1, "reason")),
        )),
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
        "beginDelivery" => encode(ledger.begin_delivery(id()).map(|begun| begun.message)),
        "confirmDelivery" => encode(ledger.confirm_delivery(id(), arg(args, 1))),
        "approveMessage" => encode(ledger.approve_message(id(), text(field(args, 1, "by")))),
        other => panic!("the player does not replay the ledger's {other} yet"),
    }
}

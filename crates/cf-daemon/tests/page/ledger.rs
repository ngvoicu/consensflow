//! The ledger as a trace's calls make it: opened on a file of its own, its
//! clock and the session names it draws taken from what Node's call drew, every
//! event it logs kept, and the database it leaves read back, to be compared
//! with the one Node left. Where the ledger does not answer as Node's did, the
//! why is noted, not panicked over: an operation that is running must finish
//! for the reply to be compared.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use cf_base::time::{parse, Clock};
use cf_ledger::{open_ledger, Event, Ledger, LedgerError, NewNote, NewTask, Options};
use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
use serde_json::{json, Value};

/// What Node's ledger drew in a call (the clock's readings, session names),
/// answered here in the same order; drawing past them is noted.
struct Draws<T> {
    left: RefCell<VecDeque<T>>,
    overdrawn: Cell<bool>,
}

impl<T> Draws<T> {
    fn new() -> Rc<Self> {
        Rc::new(Self {
            left: RefCell::new(VecDeque::new()),
            overdrawn: Cell::new(false),
        })
    }

    fn draw(&self) -> Option<T> {
        let next = self.left.borrow_mut().pop_front();
        if next.is_none() {
            self.overdrawn.set(true);
        }
        next
    }

    /// Why not, when a call did not draw what Node's call drew.
    fn settle(&self, what: &str, recorded: usize) -> Option<String> {
        let left = self.left.borrow_mut().drain(..).count();
        if self.overdrawn.replace(false) {
            return Some(format!("{what} more than Node's {recorded} times"));
        }
        (left > 0).then(|| format!("{what} {} times, Node {recorded}", recorded - left))
    }
}

/// A clock that answers the readings Node recorded, in order.
struct Recorded(Rc<Draws<i64>>);

impl Clock for Recorded {
    fn now_ms(&mut self) -> i64 {
        self.0.draw().unwrap_or_default()
    }
}

/// A ledger open on a file, and what it was told.
pub struct Rig {
    file: PathBuf,
    pub ledger: Rc<RefCell<Ledger>>,
    readings: Rc<Draws<i64>>,
    names: Rc<Draws<String>>,
    told: Rc<RefCell<Vec<Event>>>,
    problems: RefCell<Vec<String>>,
}

impl Rig {
    /// A ledger on a file of its own at `file`.
    pub fn open(file: &Path) -> Self {
        let (readings, names) = (Draws::new(), Draws::new());
        let told = Rc::new(RefCell::new(Vec::new()));
        let (events, drawn) = (Rc::clone(&told), Rc::clone(&names));
        let ledger = open_ledger(
            file,
            Options {
                clock: Box::new(Recorded(Rc::clone(&readings))),
                names: Box::new(move || drawn.draw().unwrap_or_default()),
                trace: Box::new(move |event| events.borrow_mut().push(event.clone())),
            },
        )
        .unwrap();
        Self {
            file: file.to_owned(),
            ledger: Rc::new(RefCell::new(ledger)),
            readings,
            names,
            told,
            problems: RefCell::new(Vec::new()),
        }
    }

    /// Notes that something did not answer as Node's did.
    pub fn problem(&self, why: impl Into<String>) {
        self.problems.borrow_mut().push(why.into());
    }

    /// What was noted since the last asking.
    pub fn problems(&self) -> Vec<String> {
        std::mem::take(&mut *self.problems.borrow_mut())
    }

    /// The clock's readings and the names a step of the trace drew, to be drawn again.
    pub fn queue(&self, step: &Value) {
        self.readings.left.borrow_mut().extend(
            step["clock"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|at| parse(at.as_str().unwrap()).expect("a clock reading Node wrote")),
        );
        self.names.left.borrow_mut().extend(
            step["names"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|name| name.as_str().unwrap().to_owned()),
        );
    }

    /// After a step: the readings and names it drew are those of Node's.
    pub fn settle(&self, what: &str, step: &Value) {
        let drew = |field: &str| step[field].as_array().map_or(0, Vec::len);
        let settled = [
            self.readings.settle("read the clock", drew("clock")),
            self.names.settle("drew a session name", drew("names")),
        ];
        for why in settled.into_iter().flatten() {
            self.problem(format!("{what} {why}"));
        }
    }

    /// The events logged after the first `before` of those still waiting,
    /// taken, as the recorder wrote them.
    fn logged_after(&self, before: usize) -> Vec<Value> {
        let mut waiting = self.told.borrow_mut();
        let from = before.min(waiting.len());
        waiting
            .split_off(from)
            .into_iter()
            .map(|event| {
                json!({ "at": event.at, "project": event.project, "kind": event.kind, "data": event.data })
            })
            .collect()
    }

    /// The events logged since they were last taken.
    pub fn logged(&self) -> Vec<Value> {
        self.logged_after(0)
    }

    /// A call the test made on the ledger: made again, its answer, events and
    /// readings compared with Node's.
    pub fn apply(&self, call: &Value) {
        let method = call["method"].as_str().unwrap_or_default();
        let what = format!("ledger {method}");
        self.queue(call);
        let args = call["args"].as_array().map_or(&[][..], Vec::as_slice);
        let answered = self.make(method, args);
        self.compare(&what, &answered, &call["result"]);
        self.settle(&what, call);
        let events = Value::Array(self.logged());
        self.compare(&format!("{what} logged"), &events, &call["events"]);
    }

    /// A call the test's stand-in made on the ledger, where it stood in for the
    /// dispatcher: `make` is that call as Rust's stand-in makes it. The
    /// operation's own readings wait while it draws its own.
    pub fn call<T: Serialize>(
        &self,
        recorded: &Value,
        method: &str,
        make: impl FnOnce(&mut Ledger) -> Result<T, LedgerError>,
    ) -> Result<T, LedgerError> {
        let what = format!("the stand-in's ledger {method}");
        if recorded["method"] != method {
            self.problem(format!("{what}: Node's call was {}", recorded["method"]));
        }
        let held = (
            self.readings.left.take(),
            self.names.left.take(),
            self.told.borrow().len(),
        );
        self.queue(recorded);
        let made = make(&mut self.ledger.borrow_mut());
        self.compare(&what, &answer_of(&made), &recorded["result"]);
        self.settle(&what, recorded);
        let events = Value::Array(self.logged_after(held.2));
        self.compare(&format!("{what} logged"), &events, &recorded["events"]);
        self.readings.left.replace(held.0);
        self.names.left.replace(held.1);
        made
    }

    /// The call of the test's, made.
    fn make(&self, method: &str, args: &[Value]) -> Value {
        let mut ledger = self.ledger.borrow_mut();
        let arg = |at: usize| args.get(at);
        let id = || arg(0).and_then(Value::as_i64).expect("an id");
        let number = |at: usize| arg(at).and_then(Value::as_i64).expect("a number");
        let field = |at: usize, name: &str| arg(at).and_then(|value| value.get(name));
        let text = |at: usize, name: &str| {
            field(at, name)
                .and_then(Value::as_str)
                .expect("text")
                .to_owned()
        };
        let optional =
            |at: usize, name: &str| field(at, name).and_then(Value::as_str).map(str::to_owned);
        let made: Result<Value, LedgerError> = match method {
            "assignTask" => encode(ledger.assign_task(id(), number(1), number(2))),
            "beginDelivery" => encode(ledger.begin_delivery(id())),
            "cancelTask" => encode(ledger.cancel_task(id(), number(1), &text(2, "by"))),
            "confirmDelivery" => encode(ledger.confirm_delivery(id(), arg(1))),
            "copyTranscript" => {
                let items = arg(1).and_then(Value::as_array).expect("a list");
                let from = field(2, "from").and_then(Value::as_i64).unwrap_or(0);
                encode(ledger.copy_transcript(id(), items, from))
            }
            "createTask" => NewTask::from_json(arg(1).expect("a task"))
                .and_then(|task| encode(ledger.create_task(id(), &task))),
            "markOut" => encode(ledger.mark_out(id(), &text(1, "until"), &text(1, "reason"))),
            "note" => encode(ledger.note(
                id(),
                &NewNote {
                    from: optional(1, "from"),
                    to: text(1, "to"),
                    body: text(1, "body"),
                    task: field(1, "task").and_then(Value::as_i64),
                },
            )),
            "recordResult" => encode(ledger.record_result(id(), number(1), &text(2, "body"))),
            "startConversation" => encode(ledger.start_conversation(id(), &text(1, "harness"))),
            other => {
                self.problem(format!(
                    "a ledger call the page's player does not make: {other}"
                ));
                return json!({ "$undefined": true });
            }
        };
        answer_of(&made)
    }

    /// The ledger closed, and the database it leaves compared with the one
    /// Node left (`ledger.final` of the trace).
    pub fn close_into(&self, expected: &Value) {
        let closed = self.ledger.borrow_mut().close_in_place();
        if let Err(error) = closed {
            self.problem(format!("the ledger did not close: {error}"));
        }
        self.compare("the database it left", &dump(&self.file), expected);
    }

    /// `actual` against what Node answered: exactly, key order and all; for an
    /// error SQLite raised, its message.
    pub fn compare(&self, what: &str, actual: &Value, expected: &Value) {
        let (actual, expected) = match (actual.get("$error"), expected.get("$error")) {
            (Some(ours), Some(theirs)) if theirs["name"] != "LedgerError" => (
                json!({ "error": ours["message"] }),
                json!({ "error": theirs["message"] }),
            ),
            _ => (actual.clone(), expected.clone()),
        };
        if let Some(why) = differs(what, &actual.to_string(), &expected.to_string()) {
            self.problem(why);
        }
    }
}

/// Where two texts first differ, with what is around it.
pub fn differs(what: &str, ours: &str, theirs: &str) -> Option<String> {
    (ours != theirs).then(|| {
        let at = ours
            .bytes()
            .zip(theirs.bytes())
            .take_while(|(a, b)| a == b)
            .count();
        let from = at.saturating_sub(80);
        let near = |text: &str| {
            let mut start = from;
            while !text.is_char_boundary(start) {
                start -= 1;
            }
            let mut end = (at + 160).min(text.len());
            while !text.is_char_boundary(end) {
                end += 1;
            }
            text[start..end].to_owned()
        };
        format!(
            "{what} differs at byte {at}:\n    here: …{}…\n    node: …{}…",
            near(ours),
            near(theirs)
        )
    })
}

fn encode<T: Serialize>(answered: Result<T, LedgerError>) -> Result<Value, LedgerError> {
    answered.map(|value| serde_json::to_value(value).unwrap())
}

/// What a call answered, as the recorder wrote an answer: the value, or the
/// refusal as `{$error}`.
pub fn answer_of<T: Serialize>(made: &Result<T, LedgerError>) -> Value {
    match made {
        Ok(value) => serde_json::to_value(value).unwrap(),
        Err(error) => failure(error),
    }
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

//! What the engine asked of its seams, in order, written as the Node traces
//! write it (`tests/goldens/engine/dispatcher/`), so that one projection
//! reads both sides: `{op, args, answer}` where one of the engine's
//! operations begins (its answer, or `threw`, once it ends),
//! `{seam, method?, args, answer}` for each call of a seam (a call
//! that waits gets its answer once it has one), and `{seam: "event", event}`
//! for each event the ledger logs. The engine's sleeps are written down too,
//! as `{seam: "time", method: "sleep", args: [ms]}`, which the Node traces
//! have none of, and no projection keeps.

use std::cell::RefCell;
use std::rc::Rc;

use serde_json::{json, Value};

/// The engine's calls of its seams, in order. Cloned, it is the same record.
#[derive(Clone, Default)]
pub struct Recorder(Rc<RefCell<Vec<Value>>>);

impl Recorder {
    /// Writes down where one of the engine's operations begins: its place,
    /// for its answer to go to.
    pub fn op(&self, name: &str, args: Value) -> usize {
        let mut events = self.0.borrow_mut();
        events.push(json!({ "op": name, "args": args }));
        events.len() - 1
    }

    /// What the operation written down at `at` failed with, in its own words.
    pub fn threw(&self, at: usize, message: &str) {
        if let Some(call) = self.0.borrow_mut().get_mut(at) {
            call["threw"] = json!({ "$error": { "message": message } });
        }
    }

    /// Writes down a call of `seam` (its `method`, when it is an object's),
    /// with what it was given: its place, for the answer to go to.
    pub fn call(&self, seam: &str, method: Option<&str>, args: Value) -> usize {
        let mut call = json!({ "seam": seam });
        if let Some(method) = method {
            call["method"] = json!(method);
        }
        call["args"] = args;
        let mut events = self.0.borrow_mut();
        events.push(call);
        events.len() - 1
    }

    /// The answer of the call written down at `at`.
    pub fn answered(&self, at: usize, answer: Value) {
        if let Some(call) = self.0.borrow_mut().get_mut(at) {
            call["answer"] = answer;
        }
    }

    /// A call answered as soon as it was made.
    pub fn called(&self, seam: &str, method: Option<&str>, args: Value, answer: Value) {
        let at = self.call(seam, method, args);
        self.answered(at, answer);
    }

    /// An event the ledger logged.
    pub fn event(&self, event: Value) {
        self.0
            .borrow_mut()
            .push(json!({ "seam": "event", "event": event }));
    }

    /// Everything written down so far.
    pub fn events(&self) -> Vec<Value> {
        self.0.borrow().clone()
    }

    /// Each call of `seam`'s `methods`, in order: the method and what it
    /// was given.
    pub fn calls(&self, seam: &str, methods: &[&str]) -> Vec<(String, Value)> {
        self.0
            .borrow()
            .iter()
            .filter(|event| event["seam"] == seam)
            .filter_map(|event| {
                let method = event["method"].as_str()?;
                methods
                    .contains(&method)
                    .then(|| (method.to_owned(), event["args"].clone()))
            })
            .collect()
    }
}

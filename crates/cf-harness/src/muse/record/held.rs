//! What holds a Muse turn open while it runs: a tool call no result has answered, a
//! question to the human (`request_user_input`) or an approval not yet decided. Each
//! as Muse 1.4.13 writes it live (T-705's contributor run, T-718's paid runs):
//! - `assistant_tool_calls_committed`, `tool_calls[{name, args, call_id | id}]`, each
//!   answered by `tool_result_batch_committed`, `results[{tool_call_id, text}]`, which is
//!   the tool's item;
//! - `user_input_prompt_requested` and `user_input_prompt_settled`, by `prompt_id`;
//! - `payload.kind: approval`, its `event.kind` `requested`, then
//!   `stage_requirement_resolved` and `decision_applied`, by `pending_action_id`: Muse
//!   waits for the human from the first to the last.
//!
//! The other names Muse's binary has for a tool's results (`tool_result`,
//! `tool_results_committed`, `tool_result_committed`, which its own session reader
//! reads alike) were never seen live, and are read as the one that was.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use serde_json::Value;

use super::field;
use crate::shared::record::jsonl::Stop;
use crate::shared::record::reading::{native_id, Role};

/// An item a record says: its id, role and text.
pub(super) type Said = (Arc<str>, Role, String);

/// What each run holds open.
#[derive(Default)]
pub(super) struct Held {
    /// The tool calls each run made that no result has answered.
    calls: HashMap<Arc<str>, HashSet<Arc<str>>>,
    /// The questions and approvals each run asked that are not settled.
    asked: HashMap<Arc<str>, HashSet<Arc<str>>>,
}

impl Held {
    /// A run event of `kind`: the items it says, where it is a call, a result or a
    /// question; none of any other.
    pub(super) fn event(
        &mut self,
        kind: &str,
        run: &Arc<str>,
        record: &Value,
        event: Option<&Value>,
        seq: &Value,
    ) -> Result<Vec<Said>, Stop> {
        let record_id = || native_id(record.get("id"), "muse record", seq).map_err(Stop::Failed);
        Ok(match kind {
            "assistant_tool_calls_committed" => {
                for call in list(field(event, "tool_calls")) {
                    if let Some(id) = id_of(call, &["call_id", "id"]) {
                        self.calls.entry(Arc::clone(run)).or_default().insert(id);
                    }
                }
                Vec::new()
            }
            "tool_result_batch_committed"
            | "tool_result"
            | "tool_results_committed"
            | "tool_result_committed" => {
                let results = match field(event, "results").and_then(Value::as_array) {
                    Some(results) => results.iter().collect(),
                    None => event.into_iter().collect::<Vec<_>>(),
                };
                let mut said = Vec::new();
                for (at, result) in results.into_iter().enumerate() {
                    let call = id_of(result, &["tool_call_id", "call_id"]);
                    if let Some(call) = &call {
                        for open in self.calls.values_mut() {
                            open.remove(call);
                        }
                    }
                    let id = match call {
                        Some(call) => call,
                        None => Arc::from(format!("{}:{at}", record_id()?)),
                    };
                    let output = ["text", "output", "result"].iter().find_map(|name| {
                        field(Some(result), name).filter(|found| !found.is_null())
                    });
                    said.push((id, Role::Tool, words(output)));
                }
                said
            }
            "user_input_prompt_requested" => {
                let id =
                    id_of(event.unwrap_or(&Value::Null), &["prompt_id"]).unwrap_or(record_id()?);
                self.asked.entry(Arc::clone(run)).or_default().insert(id);
                Vec::new()
            }
            "user_input_prompt_settled" => {
                self.settled(run, event, "prompt_id");
                Vec::new()
            }
            _ => Vec::new(),
        })
    }

    /// An approval's event (`payload.kind: approval`) of the run `run`.
    pub(super) fn approval(&mut self, run: &Arc<str>, event: Option<&Value>) {
        match field(event, "kind").and_then(Value::as_str) {
            Some("requested") => {
                if let Some(id) = id_of(event.unwrap_or(&Value::Null), &["pending_action_id"]) {
                    self.asked.entry(Arc::clone(run)).or_default().insert(id);
                }
            }
            Some("decision_applied") => self.settled(run, event, "pending_action_id"),
            _ => {}
        }
    }

    /// The question or approval `event` names by `name` is settled; all of the run's
    /// where it names none.
    fn settled(&mut self, run: &Arc<str>, event: Option<&Value>, name: &str) {
        match id_of(event.unwrap_or(&Value::Null), &[name]) {
            Some(id) => {
                for open in self.asked.values_mut() {
                    open.remove(&id);
                }
            }
            None => {
                self.asked.remove(run);
            }
        }
    }

    /// Whether the run `run` holds a tool call or a question open.
    pub(super) fn holds_open(&self, run: &str) -> bool {
        self.calls.get(run).is_some_and(|open| !open.is_empty()) || self.asking(run)
    }

    /// Whether the run `run` waits on the human's answer.
    pub(super) fn asking(&self, run: &str) -> bool {
        self.asked.get(run).is_some_and(|open| !open.is_empty())
    }
}

/// The elements of a list; none of anything else.
fn list(value: Option<&Value>) -> impl Iterator<Item = &Value> {
    value.and_then(Value::as_array).into_iter().flatten()
}

/// The first of `names` that `value` holds as a text, not empty.
fn id_of(value: &Value, names: &[&str]) -> Option<Arc<str>> {
    names
        .iter()
        .find_map(|name| value.get(name).and_then(Value::as_str))
        .filter(|id| !id.is_empty())
        .map(Arc::from)
}

/// A tool's output as text: text is itself, nothing is nothing, anything else its JSON.
fn words(output: Option<&Value>) -> String {
    match output {
        Some(Value::String(text)) => text.clone(),
        None | Some(Value::Null) => String::new(),
        Some(other) => other.to_string(),
    }
}

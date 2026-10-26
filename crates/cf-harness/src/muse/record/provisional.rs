//! What a Muse log says of a turn's tools, questions, approvals, compaction, background
//! work and refusals for quota. **Provisional (schema only, MUS-02):** no turn of these
//! was ever seen (the Mac's Muse login runs no model turn, 2026-10-26), so each shape
//! below is read from what Muse 1.4.13 names: its exported wire schema
//! (`muse schema generate-json-schema`), the event names in its binary, and the
//! session reader it carries for its own agent (which reads `tool_calls[{name, args,
//! call_id | id}]` and `results[{text | output | result, tool_call_id | call_id}]`).
//! The live fixtures of MUS-02 replace them, and their tests are apart from the
//! recorded ones (`tests/provisional.rs`).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use serde_json::Value;

use super::field;
use crate::shared::quota::Quota;
use crate::shared::record::jsonl::Stop;
use crate::shared::record::reading::{native_id, Pending, Role};

/// An item an event says: its id, role, text, and whether it is a compaction's summary.
pub(super) type Said = (Arc<str>, Role, String, bool);

/// What the provisional shapes said so far.
#[derive(Default)]
pub(super) struct Provisional {
    /// The tool calls each run made that no result has answered.
    calls: HashMap<Arc<str>, HashSet<Arc<str>>>,
    /// The questions and approvals each run asked that are not settled.
    asked: HashMap<Arc<str>, HashSet<Arc<str>>>,
    /// The tasks put in the background that have not ended.
    background: HashSet<Arc<str>>,
    /// The refusal for quota each run ended with.
    refusals: HashMap<Arc<str>, Arc<Quota>>,
}

impl Provisional {
    /// A run event of `kind` that the recorded shapes do not read: the items it says.
    pub(super) fn event(
        &mut self,
        kind: &str,
        run: &Arc<str>,
        record: &Value,
        event: Option<&Value>,
        _at: &Value,
        seq: &Value,
    ) -> Result<Vec<Said>, Stop> {
        let record_id = || native_id(record.get("id"), "muse record", seq).map_err(Stop::Failed);
        Ok(match kind {
            // Provisional (schema only, MUS-02): a model's tool calls hold the turn open
            // until each is answered.
            "assistant_tool_calls_committed" => {
                for call in list(field(event, "tool_calls")) {
                    if let Some(id) = call_id(call, &["call_id", "id"]) {
                        self.calls.entry(Arc::clone(run)).or_default().insert(id);
                    }
                }
                Vec::new()
            }
            // Provisional (schema only, MUS-02): a tool's results, a tool item each.
            "tool_result"
            | "tool_result_batch_committed"
            | "tool_results_committed"
            | "tool_result_committed" => {
                let results = match field(event, "results").and_then(Value::as_array) {
                    Some(results) => results.iter().collect(),
                    None => event.into_iter().collect::<Vec<_>>(),
                };
                let mut said = Vec::new();
                for (at, result) in results.into_iter().enumerate() {
                    let call = call_id(result, &["tool_call_id", "call_id"]);
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
                    said.push((id, Role::Tool, words(output), false));
                }
                said
            }
            // Provisional (schema only, MUS-02): a question to the human
            // (`request_user_input`) or an approval, open until settled.
            "user_input_prompt_requested" | "approval_requested" => {
                let id = asked_id(event).unwrap_or(record_id()?);
                self.asked.entry(Arc::clone(run)).or_default().insert(id);
                Vec::new()
            }
            "user_input_prompt_settled" | "approval_resolved" | "approval_decision_applied" => {
                match asked_id(event) {
                    Some(id) => {
                        for open in self.asked.values_mut() {
                            open.remove(&id);
                        }
                    }
                    None => {
                        self.asked.remove(run);
                    }
                }
                Vec::new()
            }
            // Provisional (schema only, MUS-02): the summary a compaction installs in
            // the context, the harness's own words.
            "context_compaction_installed" => {
                let summary = ["summary", "text"]
                    .iter()
                    .find_map(|name| field(event, name).and_then(Value::as_str))
                    .unwrap_or("");
                if summary.trim().is_empty() {
                    Vec::new()
                } else {
                    vec![(record_id()?, Role::Custom, summary.to_owned(), true)]
                }
            }
            // Provisional (schema only, MUS-02): a task a tool call put in the
            // background, pending until its task ends (`task_event`).
            "task_backgrounded" => {
                if let Some(task) = call_id(event.unwrap_or(&Value::Null), &["task_id"]) {
                    self.background.insert(task);
                }
                Vec::new()
            }
            // Provisional (schema only, MUS-02): what Muse delivers into a session that
            // is not the user's (a background command's end, a monitor's line, a
            // scheduled fire): the harness's own. The user's message pasted mid-turn
            // (`user_steer`) is the user's intent, read where it is accepted.
            "inbox_item_queued" => {
                let source = field(field(event, "source"), "source").and_then(Value::as_str);
                let body = field(event, "body").and_then(Value::as_str).unwrap_or("");
                if source == Some("user_steer") || body.trim().is_empty() {
                    Vec::new()
                } else {
                    vec![(record_id()?, Role::Custom, body.to_owned(), false)]
                }
            }
            _ => Vec::new(),
        })
    }

    /// A task's own event (`payload.kind: task`): one that ends it ends its background
    /// work. Provisional (schema only, MUS-02).
    pub(super) fn task_event(&mut self, task: Option<&Value>, kind: Option<&str>) {
        if matches!(kind, Some("completed" | "failed" | "cancelled")) {
            if let Some(Value::String(task)) = task {
                self.background.remove(task.as_str());
            }
        }
    }

    /// A run that failed for `reason`, at `at`: a refusal for quota where its words are
    /// one of Muse's limits. Provisional (MUS-14): the 402 below is the one refusal
    /// seen (the Mac's billing, T-700); a spent allowance's words were never seen.
    pub(super) fn failed(&mut self, run: &Arc<str>, reason: Option<&str>, at: &Value) {
        if reason.is_some_and(refused_for_quota) {
            let refusal = Quota::Exhausted {
                at: at.as_str().map(str::to_owned),
                resets_at: None,
            };
            self.refusals.insert(Arc::clone(run), Arc::new(refusal));
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

    /// The refusal for quota the run `run` ended with.
    pub(super) fn quota(&self, run: &str) -> Option<Arc<Quota>> {
        self.refusals.get(run).cloned()
    }

    /// What runs in the background.
    pub(super) fn pending(&self) -> Pending {
        Pending {
            jobs: self.background.len(),
            ..Pending::default()
        }
    }
}

/// Muse's words for a refused request that its account's limits refused: an API error
/// 429 (a rate or usage limit) or 402 (credit or billing), or its own "usage limit
/// reached". Provisional (MUS-14).
fn refused_for_quota(reason: &str) -> bool {
    let reason = reason.to_ascii_lowercase();
    ["api error 429", "api error 402", "usage limit reached"]
        .iter()
        .any(|words| reason.contains(words))
}

/// The elements of a list; none of anything else.
fn list(value: Option<&Value>) -> impl Iterator<Item = &Value> {
    value.and_then(Value::as_array).into_iter().flatten()
}

/// The first of `names` that `value` holds as a text, not empty.
fn call_id(value: &Value, names: &[&str]) -> Option<Arc<str>> {
    names
        .iter()
        .find_map(|name| value.get(name).and_then(Value::as_str))
        .filter(|id| !id.is_empty())
        .map(Arc::from)
}

/// The id a question or an approval is named by.
fn asked_id(event: Option<&Value>) -> Option<Arc<str>> {
    event.and_then(|event| call_id(event, &["user_input_id", "prompt_id", "approval_id", "id"]))
}

/// A tool's output as text: text is itself, nothing is nothing, anything else its JSON.
fn words(output: Option<&Value>) -> String {
    match output {
        Some(Value::String(text)) => text.clone(),
        None | Some(Value::Null) => String::new(),
        Some(other) => other.to_string(),
    }
}

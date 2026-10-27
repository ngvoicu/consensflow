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

use super::super::usage::wire_quota;
use super::field;
use crate::shared::quota::{account_refusal, Quota, Refusal};
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

    /// A run that failed for `reason`, at `at`: what the account refused for, where its
    /// words say it, and the plain words for the failure it is. A usage limit is a quota
    /// that resets (provisional, MUS-14: only the 429 shape and Muse's own "usage limit
    /// reached" are known, a subscribed account has not shown one); a refusal for billing
    /// or for the login is the account unavailable, with no reset, only the human's fix
    /// (MUS-23; the 402 is the refusal seen, T-705).
    pub(super) fn failed(
        &mut self,
        run: &Arc<str>,
        event: Option<&Value>,
        reason: Option<&str>,
        at: &Value,
    ) -> Option<&'static str> {
        let at = at.as_str().map(str::to_owned);
        // The wire's own word on a limit (`error.quota`, `{limitKind, resetsAtMs}`) names its
        // reset, which no words do. Provisional: never seen in a log, only in the schema.
        let detail = field(event, "quota").or_else(|| field(field(event, "error"), "quota"));
        if let Some(refusal) =
            detail.and_then(|detail| wire_quota(detail, at.as_deref()).ok().flatten())
        {
            self.refusals.insert(Arc::clone(run), Arc::new(refusal));
            return None;
        }
        let (refusal, plain) = match refused_for(reason?)? {
            Refused::Limit => (
                Quota::Exhausted {
                    at,
                    resets_at: None,
                },
                None,
            ),
            Refused::Account(Refusal::Billing) => {
                (unavailable(at, NO_ALLOWANCE), Some(NO_ALLOWANCE))
            }
            Refused::Account(Refusal::Credentials) => {
                (unavailable(at, LOGIN_REFUSED), Some(LOGIN_REFUSED))
            }
        };
        self.refusals.insert(Arc::clone(run), Arc::new(refusal));
        plain
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

/// A failure of a turn that the account refused: by a limit that resets, or for what
/// only its human cures.
enum Refused {
    Limit,
    Account(Refusal),
}

/// The plain words of a failure for billing (MUS-23): what `muse usage` calls
/// `no_allowance`, where the human fixes it.
const NO_ALLOWANCE: &str = "Muse refused: the account has no allowance; set up billing at accountscenter.meta.com/muse_code";

/// The plain words of a failure for the login.
const LOGIN_REFUSED: &str = "Muse refused: its login is not valid; run `muse login`";

fn unavailable(at: Option<String>, because: &str) -> Quota {
    Quota::Unavailable {
        at,
        because: because.to_owned(),
    }
}

/// What Muse's words for a failed request say the account refused for. A turn's reason
/// is `API error <status> [request_id=…]: <message> (<kind>)` for a refused request
/// (`billing_error`, T-705), or Muse's own words for no login. A 429 or Muse's "usage
/// limit reached" is a limit that resets; a 402 or 401 and the words the other harnesses
/// share (`account_refusal`) are the account's own. Provisional (MUS-14): a spent
/// allowance's words were never seen.
fn refused_for(reason: &str) -> Option<Refused> {
    let lower = reason.to_ascii_lowercase();
    if lower.contains("no meta credentials") || lower.contains("not logged in") {
        return Some(Refused::Account(Refusal::Credentials));
    }
    if let Some(refusal) = account_refusal(reason, api_status(&lower)) {
        return Some(Refused::Account(refusal));
    }
    [
        "api error 429",
        "usage limit reached",
        "usage limit was reached",
    ]
    .iter()
    .any(|words| lower.contains(words))
    .then_some(Refused::Limit)
}

/// The status of `API error <status>`, in a lowercased reason.
fn api_status(lower: &str) -> Option<f64> {
    let after = lower.split("api error ").nth(1)?;
    let digits: String = after.chars().take_while(char::is_ascii_digit).collect();
    (digits.len() == 3).then(|| digits.parse().ok()).flatten()
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

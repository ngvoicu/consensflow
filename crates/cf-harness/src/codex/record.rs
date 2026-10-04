//! Codex's record of a thread (`codexReader` and `codexParser`,
//! `hosts/lib/completion/codex.js`): its rollout, a JSONL file in its
//! `sessions` folder. A look's answer is the rollout's alone.
//!
//! The rollout says what the thread did in two kinds of record:
//! - `response_item`: the model's messages, its tool calls, their output;
//! - `event_msg`: a turn's start and end (`task_started`, `task_complete`,
//!   `turn_aborted`), the items Codex itself started and completed, and
//!   the rate limits (`token_count`).
//!
//! The latest turn decides. It is in flight while it has started and not
//! ended, or holds a tool call or a subagent open. It is settled once it
//! ended: cancelled or failed, or complete with a final answer the rollout
//! holds, as `task_complete` names it.
//!
//! The rollout's ids key the reader's maps as they keyed Node's (see
//! [`Key`]), so a record that says something odd is read as Node read it.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use cf_base::env::Env;
use cf_base::js;
use serde_json::{Number, Value};

use super::paths::transcript;
use super::quota::codex_quota;
use crate::shared::quota::Quota;
use crate::shared::record::cache::Look;
use crate::shared::record::followed::{Answer, Followed, Parser, TranscriptReader};
use crate::shared::record::jsonl::Stop;
use crate::shared::record::key::{Key, Keys};
use crate::shared::record::reading::{
    native_id, null_record, visible_text, Item, Record, Role, Settlement,
};

/// The reader of the thread `session`, in the Codex home `env` names.
pub fn reader(session: &str, env: &Env) -> Box<dyn Look + Send> {
    let locating = session.to_owned();
    let env = env.clone();
    let reading: Arc<str> = Arc::from(session);
    Box::new(TranscriptReader::new(
        Followed::new(
            Box::new(move || transcript(&locating, &env)),
            Box::new(move || Rollout::new(Arc::clone(&reading))),
        ),
        format!("codex rollout for {session}"),
    ))
}

/// What a rollout said so far.
struct Rollout {
    session: Arc<str>,
    /// Every item, in the order the rollout first named it (`list`).
    items: Vec<Kept>,
    /// Where each item is among them, by its native id (`items`).
    places: HashMap<Arc<str>, usize>,
    turns: HashMap<Key, Turn>,
    /// The turn each tool call was made in.
    calls: HashMap<Key, Key>,
    /// The turn each subagent was started in.
    subagents: HashMap<Key, Key>,
    /// The turn the last `task_started` began, as it named it.
    current_turn: Key,
    latest_turn: Key,
    quota: Option<Arc<Quota>>,
    records: usize,
    keys: Keys,
}

/// An item as the rollout left it, and the text Codex marked its final
/// answer (`_nativeFinalText`), which no later message changes.
struct Kept {
    item: Item,
    final_text: Option<Arc<str>>,
}

/// A turn as the rollout told it.
#[derive(Default)]
struct Turn {
    started: bool,
    end: Option<End>,
    /// Its assistant's items, each once, by native id (`assistantIds`).
    answers: Vec<Arc<str>>,
    tools: HashSet<Key>,
    subagents: HashSet<Key>,
}

/// How a turn ended (`terminal`).
enum End {
    /// `task_complete`, and the final answer it names (`last_agent_message`,
    /// none when it names none).
    Complete(Option<Value>),
    /// `task_complete` with an error.
    Failed,
    /// `turn_aborted`.
    Cancelled,
}

impl Rollout {
    fn new(session: Arc<str>) -> Self {
        Self {
            session,
            items: Vec::new(),
            places: HashMap::new(),
            turns: HashMap::new(),
            calls: HashMap::new(),
            subagents: HashMap::new(),
            current_turn: Key::Null,
            latest_turn: Key::Null,
            quota: None,
            records: 0,
            keys: Keys::default(),
        }
    }

    /// `codexTurn`: the turn `key` names, made when the rollout first names
    /// it; none for a key JavaScript takes for false.
    fn turn(&mut self, key: &Key) -> Option<&mut Turn> {
        key.truthy()
            .then(|| self.turns.entry(key.clone()).or_default())
    }

    /// A record of the turn `key` names, if any: the turn has started, and
    /// is the latest.
    fn start(&mut self, key: &Key) {
        if let Some(open) = self.turn(key) {
            open.started = true;
        } else {
            return;
        }
        self.latest_turn = key.clone();
    }

    /// The turn a record is in: the one it names, else the current one.
    fn named_or_current(&mut self, named: Option<&Value>) -> Key {
        match named {
            None | Some(Value::Null) => self.current_turn.clone(),
            named => self.keys.of(named),
        }
    }

    /// A `response_item`: a message, a tool call or a tool's output.
    fn response_item(
        &mut self,
        payload: Option<&Value>,
        at: Value,
        seq: &Value,
    ) -> Result<(), Stop> {
        let metadata = field(payload, "internal_chat_message_metadata_passthrough");
        let turn = self.named_or_current(field(metadata, "turn_id"));
        self.start(&turn);
        let role = match field(payload, "role").and_then(Value::as_str) {
            Some("user") => Some(Role::User),
            Some("assistant") => Some(Role::Assistant),
            _ => None,
        };
        match (field(payload, "type").and_then(Value::as_str), role) {
            (Some("message"), Some(role)) => {
                let text = content_text(field(payload, "content"));
                if js::trim(&text).is_empty() {
                    return Ok(());
                }
                let complete = role == Role::User;
                let place = self.add_item(field(payload, "id"), role, text, complete, at, seq)?;
                if role == Role::Assistant {
                    self.answered_in(&turn, place);
                }
            }
            (Some("function_call" | "custom_tool_call"), _) => {
                let call = self
                    .keys
                    .of(nullish_or(field(payload, "call_id"), field(payload, "id")));
                if call.truthy() && turn.truthy() {
                    if let Some(open) = self.turn(&turn) {
                        open.tools.insert(call.clone());
                    }
                    self.calls.insert(call, turn);
                }
            }
            (Some("function_call_output" | "custom_tool_call_output"), _) => {
                let call = self.keys.of(field(payload, "call_id"));
                let owner = match self.calls.get(&call) {
                    Some(owner) if !owner.is_nullish() => owner.clone(),
                    _ => turn,
                };
                if let Some(open) = self.turn(&owner) {
                    open.tools.remove(&call);
                }
                let output = visible_text(field(payload, "output"));
                self.add_item(field(payload, "id"), Role::Tool, output, true, at, seq)?;
            }
            _ => {}
        }
        Ok(())
    }

    /// An `event_msg`: the rate limits, a turn's start or end, or an item
    /// Codex started or completed.
    fn event(&mut self, payload: Option<&Value>, at: Value, seq: &Value) -> Result<(), Stop> {
        // Read once: an object here is one key, wherever the event uses it.
        let named = self.keys.of(field(payload, "turn_id"));
        let turn = if named.is_nullish() {
            self.current_turn.clone()
        } else {
            named.clone()
        };
        let kind = field(payload, "type").and_then(Value::as_str);
        match kind {
            Some("token_count") => {
                if let Some(limits) =
                    field(payload, "rate_limits").filter(|limits| js::truthy(Some(limits)))
                {
                    self.quota = Some(Arc::new(codex_quota(limits).map_err(Stop::Failed)?));
                }
            }
            Some("task_started") => {
                self.current_turn = named.clone();
                self.latest_turn = named.clone();
                self.named_turn(&named, "task_started", seq)?.started = true;
            }
            Some("task_complete") => {
                self.latest_turn = named.clone();
                let end = if js::truthy(field(payload, "error")) {
                    End::Failed
                } else {
                    End::Complete(field(payload, "last_agent_message").cloned())
                };
                self.named_turn(&named, "task_complete", seq)?.end = Some(end);
            }
            Some("turn_aborted") => {
                self.latest_turn = named.clone();
                self.named_turn(&named, "turn_aborted", seq)?.end = Some(End::Cancelled);
            }
            Some(event @ ("item_started" | "item_completed")) => {
                let completed = event == "item_completed";
                self.native_item(field(payload, "item"), completed, &turn, at, seq)?;
            }
            _ => {}
        }
        Ok(())
    }

    /// The turn a turn's own event names: one that names none fails the
    /// look, as setting a field of Node's `null` turn threw.
    fn named_turn(&mut self, named: &Key, event: &str, seq: &Value) -> Result<&mut Turn, Stop> {
        let at = js::text(Some(seq)).into_owned();
        self.turn(named).ok_or_else(|| {
            Stop::Failed(format!(
                "a codex {event} event names no turn at record {at}"
            ))
        })
    }

    /// An item Codex started or completed itself: a message, a command, a
    /// subagent's start or end.
    fn native_item(
        &mut self,
        native: Option<&Value>,
        completed: bool,
        turn: &Key,
        at: Value,
        seq: &Value,
    ) -> Result<(), Stop> {
        self.start(turn);
        match field(native, "type").and_then(Value::as_str) {
            Some("UserMessage") if completed => {
                let text = content_text(field(native, "content"));
                if !js::trim(&text).is_empty() {
                    self.add_item(field(native, "id"), Role::User, text, true, at, seq)?;
                }
            }
            Some("AgentMessage") if completed => {
                let text = content_text(field(native, "content"));
                if js::trim(&text).is_empty() {
                    return Ok(());
                }
                let phase = field(native, "phase").and_then(Value::as_str);
                let final_answer = phase == Some("final_answer");
                let place = self.add_item(
                    field(native, "id"),
                    Role::Assistant,
                    text.clone(),
                    final_answer,
                    at,
                    seq,
                )?;
                let kept = &mut self.items[place];
                if final_answer {
                    let text: Arc<str> = Arc::from(text);
                    kept.item.text = Arc::clone(&text);
                    kept.final_text = Some(text);
                }
                // Codex's progress notes ("I'll read the diff…"), marked by Codex itself.
                if phase == Some("commentary") {
                    kept.item.commentary = true;
                }
                self.answered_in(turn, place);
            }
            Some("CommandExecution") => {
                let command = self.keys.of(field(native, "id"));
                if !completed {
                    if command.truthy() {
                        if let Some(open) = self.turn(turn) {
                            open.tools.insert(command);
                        }
                    }
                    return Ok(());
                }
                if let Some(open) = self.turn(turn) {
                    open.tools.remove(&command);
                }
                let output = match nullish_or(
                    field(native, "aggregated_output"),
                    field(native, "formatted_output"),
                ) {
                    Some(output) if !output.is_null() => js::text(Some(output)).into_owned(),
                    _ => format!(
                        "{}{}",
                        or_empty(field(native, "stdout")),
                        or_empty(field(native, "stderr"))
                    ),
                };
                self.add_item(field(native, "id"), Role::Tool, output, true, at, seq)?;
            }
            Some("SubAgentActivity") => {
                let agent = self.keys.of(nullish_or(
                    field(native, "agent_thread_id"),
                    field(native, "id"),
                ));
                if !agent.truthy() {
                    return Ok(());
                }
                match field(native, "kind").and_then(Value::as_str) {
                    Some("started") => {
                        if let Some(open) = self.turn(turn) {
                            open.subagents.insert(agent.clone());
                        }
                        self.subagents.insert(agent, turn.clone());
                    }
                    Some("completed") => {
                        let owner = match self.subagents.get(&agent) {
                            Some(owner) if !owner.is_nullish() => owner.clone(),
                            _ => turn.clone(),
                        };
                        if let Some(open) = self.turn(&owner) {
                            open.subagents.remove(&agent);
                        }
                        self.subagents.remove(&agent);
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// `addItem`: the item `id` names, added, or brought up to date. Its
    /// text is the latest the rollout gave it until Codex marked a final
    /// answer; it is complete once anything said so; its time is the
    /// latest record's. Its role is the one it was first given.
    fn add_item(
        &mut self,
        id: Option<&Value>,
        role: Role,
        text: String,
        complete: bool,
        at: Value,
        seq: &Value,
    ) -> Result<usize, Stop> {
        let id = native_id(id, "codex item", seq).map_err(Stop::Failed)?;
        if let Some(&place) = self.places.get(&id) {
            let kept = &mut self.items[place];
            if !text.is_empty() && *kept.item.text != *text && kept.final_text.is_none() {
                kept.item.text = Arc::from(text);
            }
            kept.item.complete |= complete;
            kept.item.at = at;
            return Ok(place);
        }
        let place = self.items.len();
        self.places.insert(Arc::clone(&id), place);
        self.items.push(Kept {
            item: Item {
                id,
                role,
                text: Arc::from(text),
                complete,
                at,
                commentary: false,
            },
            final_text: None,
        });
        Ok(place)
    }

    /// The item at `place` is among the answers of the turn `turn` names.
    fn answered_in(&mut self, turn: &Key, place: usize) {
        let id = Arc::clone(&self.items[place].item.id);
        if let Some(open) = self.turn(turn) {
            if !open.answers.contains(&id) {
                open.answers.push(id);
            }
        }
    }

    /// Whether a complete answer of `turn` is the final answer its
    /// `task_complete` names (`proven`).
    fn proven(&self, turn: &Turn, named: Option<&Value>) -> bool {
        turn.answers.iter().any(|id| {
            self.places.get(id).is_some_and(|&place| {
                let kept = &self.items[place];
                kept.item.complete
                    && match (&kept.final_text, named) {
                        (None, None) => true,
                        (Some(text), Some(Value::String(named))) => **text == **named,
                        _ => false,
                    }
            })
        })
    }
}

impl Parser for Rollout {
    fn visit(&mut self, record: Value, index: usize) -> Result<(), Stop> {
        self.records += 1;
        if record.is_null() {
            return Err(Stop::Failed(null_record(index)));
        }
        // `jsonlSeq`: the rollout's own ordinal, when it is a whole number.
        let seq = match record.get("ordinal") {
            Some(Value::Number(ordinal)) if is_whole(ordinal) => Value::Number(ordinal.clone()),
            _ => Value::from(index),
        };
        let at = match record.get("timestamp") {
            None | Some(Value::Null) => seq.clone(),
            Some(at) => at.clone(),
        };
        // `record.payload ?? {}`: none and null hold no field.
        let payload = record.get("payload");
        match record.get("type").and_then(Value::as_str) {
            Some("response_item") => self.response_item(payload, at, &seq),
            Some("event_msg") => self.event(payload, at, &seq),
            _ => Ok(()),
        }
    }
}

impl Answer for Rollout {
    fn result(&self) -> Result<Record, String> {
        if self.records == 0 {
            return Err(format!("empty codex rollout for {}", self.session));
        }
        let latest = if self.latest_turn.truthy() {
            self.turns.get(&self.latest_turn)
        } else {
            None
        };
        let in_flight = latest.is_some_and(|turn| {
            (turn.started && turn.end.is_none())
                || !turn.tools.is_empty()
                || !turn.subagents.is_empty()
        });
        let end = latest.and_then(|turn| Some((turn, turn.end.as_ref()?)));
        let mut record = Record::new();
        record.items = self.items.iter().map(|kept| kept.item.clone()).collect();
        record.in_flight = in_flight;
        record.failed = matches!(end, Some((_, End::Failed)));
        record.quota = self.quota.clone();
        record.settlement = match end {
            _ if in_flight => Settlement::InFlight,
            // A task_complete whose final answer cannot be matched proves nothing.
            Some((turn, End::Complete(named))) if !self.proven(turn, named.as_ref()) => {
                Settlement::Unknown
            }
            Some(_) => Settlement::Settled,
            None => Settlement::Unknown,
        };
        Ok(record)
    }
}

/// A field of a value that may be none (`value?.name`): none of anything
/// that is no object.
fn field<'a>(value: Option<&'a Value>, name: &str) -> Option<&'a Value> {
    value.and_then(|value| value.get(name))
}

/// `first ?? second`.
fn nullish_or<'a>(first: Option<&'a Value>, second: Option<&'a Value>) -> Option<&'a Value> {
    match first {
        None | Some(Value::Null) => second,
        first => first,
    }
}

/// `${value ?? ''}`.
fn or_empty(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => String::new(),
        value => js::text(value).into_owned(),
    }
}

/// `Number.isInteger`: a number with no fraction.
fn is_whole(number: &Number) -> bool {
    number.is_i64()
        || number.is_u64()
        || number.as_f64().is_some_and(|double| double.fract() == 0.0)
}

/// `contentText`: a message's text, from text, or from a list of parts
/// whose own text (`text`, else `Text`) is joined a line each.
fn content_text(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(|part| nullish_or(part.get("text"), part.get("Text"))?.as_str())
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests;

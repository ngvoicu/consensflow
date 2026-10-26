//! What Devin's main chain says: the messages from the chain's head up to its
//! first, oldest first, and whether Devin's question tool waits for an answer.
//!
//! Devin's async compactor, in the middle of a turn, begins a chain of its own:
//! a system message that sums the conversation up (`devin-rs/summary`), below it
//! copies of the steps it keeps, each a new node with the id Devin gave its
//! message, and the turn goes on below the copies. The user's message the steps
//! answer is in none of it: it stays in the store, outside the main chain (T-652,
//! the live board's T-602 of 2026-10-26: 190 messages on the chain, none a
//! user's). It is read back where it was: before the first copy of a step that
//! answers it, found by the step's original, the oldest row with the step's id
//! off the chain. So the window's record still holds the message a task was given
//! by, which its result, a stop's look and its turn's end are found from.
//!
//! A message's fields are read as Node read them: what V8 threw on (a
//! message that is null, tool calls that are no list) fails the look here
//! too, in words of this module's own.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use cf_base::js;
use serde_json::Value;

use super::store::{Row, Store};
use crate::shared::record::key::{Key, Keys};
use crate::shared::record::reading::{Item, Role};
use crate::shared::record::sqlite::{text_of, Cell};

/// The messages on the main chain, and whether its question tool waits.
#[derive(Default)]
pub(super) struct Chain {
    pub(super) entries: Vec<Entry>,
    pub(super) asking: bool,
}

/// A message of the chain as its item, with what its completeness rests on.
pub(super) struct Entry {
    pub(super) item: Item,
    /// The request it answers: the last user message's client id, else that
    /// message's own; null before any, as after a compaction that left no user's
    /// message on the chain.
    pub(super) request: Key,
    /// The model's request an assistant's message is of (`metadata.request_id`): null
    /// for any other, or where it names none.
    pub(super) native: Key,
    /// Devin stored it as its turn's end (`finish_reason: "stop"`).
    pub(super) stopped: bool,
}

impl Chain {
    /// The chain ending at the store's head. Devin moves its main chain's
    /// head once a step is over: a call its question tool still waits on
    /// hangs below the head, and so may its answer (seen on Windows,
    /// 2026-10-03), newest child after newest child.
    pub(super) fn of(store: &Store) -> Result<Self, String> {
        let mut keys = Keys::default();
        let mut asking = Key::Null;
        let mut entries = Vec::new();
        let mut ids = HashSet::new();
        let mut request = Key::Null;
        let mut chain = ancestors(store)?;
        chain.reverse();
        for row in with_cut_users(store, chain)? {
            let message = row.message()?;
            follow(&mut asking, message, row, &mut keys)?;
            let id = match message.get("message_id") {
                Some(Value::String(id)) if ids.insert(id.as_str()) => id,
                _ => return Err("invalid Devin message identity".to_owned()),
            };
            let role = match message.get("role").and_then(Value::as_str) {
                Some("user") => Role::User,
                Some("assistant") => Role::Assistant,
                Some("tool") => Role::Tool,
                Some("system" | "custom") => Role::Custom,
                _ => return Err("unknown Devin message role".to_owned()),
            };
            let metadata = |name: &str| {
                message
                    .get("metadata")
                    .and_then(|metadata| metadata.get(name))
            };
            if role == Role::User {
                let client = metadata("extensions")
                    .and_then(|extensions| extensions.get("chisel/client-message-id"))
                    .filter(|client| !client.is_null());
                request = keys.of(client.or(message.get("message_id")));
            }
            let text = match message.get("content") {
                Some(Value::String(text)) => text.clone(),
                Some(Value::Array(parts)) => parts_text(parts)?,
                _ => String::new(),
            };
            let at = metadata("created_at")
                .filter(|at| !at.is_null())
                .cloned()
                .or_else(|| row.created_at.as_ref().map(Cell::json));
            entries.push(Entry {
                item: Item {
                    id: Arc::from(id.as_str()),
                    role,
                    text: Arc::from(text),
                    complete: role != Role::Assistant,
                    at,
                    commentary: false,
                    compaction: false,
                },
                request: request.clone(),
                native: if role == Role::Assistant {
                    keys.of(metadata("request_id"))
                } else {
                    Key::Null
                },
                stopped: metadata("finish_reason").and_then(Value::as_str) == Some("stop"),
            });
        }
        let mut met = HashSet::new();
        let mut below = store.newest_child(store.head());
        while let Some(row) = below {
            if !met.insert(&row.node) {
                return Err(format!(
                    "Devin's rows below the main chain's head lead back to row {}",
                    text_of(row.id.as_ref())
                ));
            }
            follow(&mut asking, row.message()?, row, &mut keys)?;
            below = store.newest_child(&row.node);
        }
        Ok(Self {
            entries,
            asking: asking != Key::Null,
        })
    }
}

/// The rows of the main chain, oldest first, with the user's messages a compaction cut
/// out of it put back, each once, before the first copy of a step that answers it.
fn with_cut_users<'a>(store: &'a Store, chain: Vec<&'a Row>) -> Result<Vec<&'a Row>, String> {
    let mut originals: Option<HashMap<&str, &Row>> = None;
    let on_chain: HashSet<&Key> = chain.iter().map(|row| &row.node).collect();
    let mut ids: HashSet<&str> = HashSet::new();
    let mut rows = Vec::with_capacity(chain.len());
    // After a compaction's summary, until a user's message on the chain: the user's
    // message put back last.
    let mut cut: Option<Option<&Key>> = None;
    for row in &chain {
        let message = row.message()?;
        match message.get("role").and_then(Value::as_str) {
            Some("user") => cut = None,
            Some("system") if summary(message) => cut = Some(None),
            Some("assistant" | "tool") => {
                if let Some(last) = &mut cut {
                    let originals = originals.get_or_insert_with(|| off_chain(store, &on_chain));
                    let user = row
                        .message_id()
                        .and_then(|id| originals.get(id))
                        .and_then(|original| user_of(store, original));
                    if let Some(user) = user.filter(|user| *last != Some(&user.node)) {
                        *last = Some(&user.node);
                        rows.push(user);
                    }
                }
            }
            _ => {}
        }
        rows.push(row);
    }
    if rows.len() > chain.len() {
        // A message put back that the chain holds as well is not put back: its id would
        // be the chain's twice.
        ids.extend(chain.iter().filter_map(|row| row.message_id()));
        rows.retain(|row| {
            on_chain.contains(&row.node) || !ids.contains(row.message_id().unwrap_or(""))
        });
    }
    Ok(rows)
}

/// Whether `message` is the summary Devin's compactor begins a chain with.
fn summary(message: &Value) -> bool {
    message
        .get("metadata")
        .and_then(|metadata| metadata.get("extensions"))
        .and_then(|extensions| extensions.get("devin-rs/summary"))
        .is_some_and(|summary| !summary.is_null())
}

/// The rows off the main chain by the ids of their messages, the oldest of each id: the
/// original of a message a compaction copied.
fn off_chain<'a>(store: &'a Store, on_chain: &HashSet<&Key>) -> HashMap<&'a str, &'a Row> {
    let mut originals = HashMap::new();
    for row in store.rows() {
        if let (false, Some(id)) = (on_chain.contains(&row.node), row.message_id()) {
            originals.entry(id).or_insert(row);
        }
    }
    originals
}

/// The user's message `row` answers: the nearest user's above it, if any. A row that
/// cannot be read, or rows that lead back to themselves, answer none.
fn user_of<'a>(store: &'a Store, row: &'a Row) -> Option<&'a Row> {
    let mut visited = HashSet::new();
    let mut at = row;
    loop {
        if at.message().ok()?.get("role").and_then(Value::as_str) == Some("user") {
            return Some(at);
        }
        if !visited.insert(&at.node) {
            return None;
        }
        at = store.row(&at.parent)?;
    }
}

/// The rows from the head up to the chain's first, the head's first.
fn ancestors(store: &Store) -> Result<Vec<&Row>, String> {
    let mut chain = Vec::new();
    let mut visited = HashSet::new();
    let mut node = store.head();
    while *node != Key::Null {
        if !visited.insert(node) {
            return Err("cyclic Devin main chain".to_owned());
        }
        let row = store
            .row(node)
            .ok_or_else(|| "missing Devin main chain ancestor".to_owned())?;
        chain.push(row);
        node = &row.parent;
    }
    Ok(chain)
}

/// `asking` after `message`: its question tool's call, until a tool message
/// answers it.
fn follow(asking: &mut Key, message: &Value, row: &Row, keys: &mut Keys) -> Result<(), String> {
    let call = question_call(message, row)?;
    match (message.get("role").and_then(Value::as_str), call) {
        (Some("assistant"), Some(call)) => *asking = keys.of(call.get("id")),
        (Some("tool"), _) if keys.of(message.get("tool_call_id")) == *asking => {
            *asking = Key::Null;
        }
        _ => {}
    }
    Ok(())
}

/// The call of Devin's question tool among a message's tool calls
/// (`message.tool_calls?.find((c) => c.name === 'ask_user_question')`), or
/// why V8 threw: a message that is null, tool calls that are no list, or a
/// call that is null before the one found.
fn question_call<'a>(message: &'a Value, row: &Row) -> Result<Option<&'a Value>, String> {
    let at = || text_of(row.id.as_ref());
    if message.is_null() {
        return Err(format!(
            "Devin's message at row {} is null, where an object was read",
            at()
        ));
    }
    match message.get("tool_calls") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Array(calls)) => {
            for call in calls {
                if call.is_null() {
                    return Err(format!(
                        "Devin's message at row {} holds a tool call that is null",
                        at()
                    ));
                }
                if call.get("name").and_then(Value::as_str) == Some("ask_user_question") {
                    return Ok(Some(call));
                }
            }
            Ok(None)
        }
        Some(_) => Err(format!(
            "Devin's message at row {} holds tool calls that are no list",
            at()
        )),
    }
}

/// The text of a message's parts, those of type `text`, joined
/// (`.filter((part) => part.type === 'text').map((part) => part.text).join('')`):
/// or why V8 threw, on a part that is null or a text with a `toString` of its
/// own.
fn parts_text(parts: &[Value]) -> Result<String, String> {
    if parts.iter().any(Value::is_null) {
        return Err("a Devin message's content holds a part that is null".to_owned());
    }
    let mut text = String::new();
    for part in parts {
        if part.get("type").and_then(Value::as_str) != Some("text") {
            continue;
        }
        match part.get("text") {
            None | Some(Value::Null) => {}
            said => text.push_str(&js::string(said)?),
        }
    }
    Ok(text)
}

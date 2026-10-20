//! A record, as far as replay reads it (the projection).
//!
//! A look's records wait until all of them are in, and a transcript of 300 MB
//! holds a hundred thousand. Most of what they say nobody reads: a message's
//! usage and thinking, a tool's input and what it returned besides its text,
//! snapshots, renderings. So a record waits as what replay reads of it,
//! which is little, and the rest is dropped as soon as the record is visited.
//!
//! What replay fails on, it still fails on when the record is replayed: a
//! text that cannot be made, an id that is none, are held as the failure, and
//! raised where replay asked for them. A failure belongs to the replay, which
//! comes after every line is read, and not to the reading of the line.

use std::borrow::Cow;
use std::sync::Arc;

use cf_base::js;
use serde_json::{Map, Value};

use super::patterns::OUTPUT;
use super::text::{claude_text, claude_tool_text, is_interrupt};
use super::{field, kind};

/// What Claude Code says, in a message of its own, as it closes the turn it
/// found cut when it resumed the conversation.
const CUT_TURN_CLOSED: &str = "No response requested.";

/// A record, as replay reads it.
#[derive(Debug, PartialEq)]
pub(super) enum Projected {
    /// A record replay passes over: of a type it does not read, or of one it
    /// reads that says nothing of the turn.
    Other,
    /// What a `UserPromptSubmit` hook added to the prompt.
    Hook(Box<Hook>),
    /// An operation on Claude Code's queue of messages, with the content it
    /// names where it names one.
    Enqueue(Result<String, String>),
    Dequeue,
    PopAll(Result<String, String>),
    Remove(Result<String, String>),
    Assistant(Box<Assistant>),
    User(Box<User>),
    /// The output of a `/clear`, a child of the record with this uuid.
    Clear(Arc<str>),
    /// A boundary record that ends the turn of the answer before it.
    Boundary(Boundary),
}

/// A record that ends the turn of the answer before it.
#[derive(Debug, PartialEq)]
pub(super) struct Boundary {
    /// The record's uuid, when that is text.
    pub(super) uuid: Option<Arc<str>>,
    /// Whether the record says background agents or workflows are still running:
    /// none for a record that says nothing of them (a stop hook's summary), as a
    /// `turn_duration` says by its counts.
    pub(super) agents: Option<bool>,
}

/// An attachment that holds a hook's context.
#[derive(Debug, PartialEq)]
pub(super) struct Hook {
    pub(super) at: Option<Value>,
    pub(super) text: String,
    pub(super) uuid: Option<Arc<str>>,
}

/// An assistant's record.
#[derive(Debug, PartialEq)]
pub(super) struct Assistant {
    pub(super) at: Option<Value>,
    pub(super) message_id: Option<Arc<str>>,
    pub(super) uuid: Option<Arc<str>>,
    /// The text of the message's content.
    pub(super) text: String,
    /// The tool calls and results of the content, in order.
    pub(super) blocks: Vec<Block>,
    /// The message stopped as `end_turn` or `stop_sequence`.
    pub(super) ended: bool,
    /// Claude Code's own close of the turn it found cut as it resumed the
    /// conversation: no model's words.
    pub(super) closes_a_cut_turn: bool,
    /// The API refused the request (`isApiErrorMessage`).
    pub(super) refused: bool,
    /// A refusal for quota: a status of 429, or the error `rate_limit`.
    pub(super) rate_limited: bool,
}

/// A block of an assistant's content that opens or closes a call.
#[derive(Debug, PartialEq)]
pub(super) enum Block {
    /// A `tool_use` or `server_tool_use`, by the id it says.
    Use(Option<Value>),
    /// A `tool_result` or `advisor_tool_result`.
    Result(ToolResult),
}

/// The result of a tool in a block: the call it answers, as the block says
/// it, and its text.
#[derive(Debug, PartialEq)]
pub(super) struct ToolResult {
    pub(super) call: Option<Value>,
    pub(super) text: Result<String, String>,
}

/// A user's record.
#[derive(Debug, PartialEq)]
pub(super) struct User {
    pub(super) at: Option<Value>,
    pub(super) uuid: Option<Arc<str>>,
    /// The results of tools in the content (not an advisor's).
    pub(super) results: Vec<ToolResult>,
    /// The text of the message's content.
    pub(super) text: String,
    /// Claude Code's own record of an interrupt.
    pub(super) interrupt: bool,
    /// The prompt came from the queue (`promptSource`).
    pub(super) queued: bool,
    /// The summary a compaction writes (`isCompactSummary`): Claude Code's own.
    pub(super) summary: bool,
    /// The uuid of the record this one is a child of, where it is what a
    /// command typed at Claude's prompt printed (`OUTPUT`), of the main
    /// conversation: Claude Code's own words, which end the command's turn
    /// where they follow it.
    pub(super) output_of: Option<Arc<str>>,
}

/// The record `record` of the session `session`, as replay reads it.
pub(super) fn project(record: &Value, session: &str) -> Projected {
    match kind(record) {
        Some("attachment") => hook(record, session),
        Some("queue-operation") => queue(record),
        Some("assistant") => assistant(record),
        Some("user") => user(record, session),
        Some("system") => system(record, session),
        _ => Projected::Other,
    }
}

/// `record.timestamp ?? seq`, but for the `seq`: none for none and null.
fn timestamp(record: &Value) -> Option<Value> {
    match record.get("timestamp") {
        None | Some(Value::Null) => None,
        Some(at) => Some(at.clone()),
    }
}

/// An id that is text and not empty (`nativeId` takes no other).
fn text_id(value: Option<&Value>) -> Option<Arc<str>> {
    match value {
        Some(Value::String(id)) if !id.is_empty() => Some(Arc::from(id.as_str())),
        _ => None,
    }
}

/// A field as `Keys` and `nativeId` read it: a scalar as it is, a list or an
/// object as an empty one, since any is a key no other is and an id none.
fn said(value: Option<&Value>) -> Option<Value> {
    match value {
        None => None,
        Some(Value::Array(_)) => Some(Value::Array(Vec::new())),
        Some(Value::Object(_)) => Some(Value::Object(Map::new())),
        Some(scalar) => Some(scalar.clone()),
    }
}

/// Whether the record is of the session's main conversation, not of another
/// session or of a sidechain.
fn of_main_conversation(record: &Value, session: &str) -> bool {
    record.get("sessionId").and_then(Value::as_str) == Some(session)
        && record.get("isSidechain") == Some(&Value::Bool(false))
}

/// The blocks of a message's content: none for content that is no list.
fn blocks(content: Option<&Value>) -> &[Value] {
    match content {
        Some(Value::Array(blocks)) => blocks,
        _ => &[],
    }
}

fn tool_result(block: &Value) -> ToolResult {
    ToolResult {
        call: said(block.get("tool_use_id")),
        text: claude_tool_text(block.get("content")),
    }
}

/// An attachment: what a `UserPromptSubmit` hook added to the prompt, a
/// context of the conversation that is its own, not the assistant's.
fn hook(record: &Value, session: &str) -> Projected {
    let attachment = record.get("attachment");
    let says =
        |name: &str, wanted: &str| field(attachment, name).and_then(Value::as_str) == Some(wanted);
    if !(of_main_conversation(record, session)
        && says("type", "hook_additional_context")
        && says("hookEvent", "UserPromptSubmit"))
    {
        return Projected::Other;
    }
    let Some(Value::Array(parts)) = field(attachment, "content") else {
        return Projected::Other;
    };
    let text = parts
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>()
        .join("\n");
    if text.is_empty() {
        return Projected::Other;
    }
    Projected::Hook(Box::new(Hook {
        at: timestamp(record),
        text,
        uuid: text_id(record.get("uuid")),
    }))
}

/// An operation on the queue. `String(record.content ?? '')` is what each
/// that names a content reads, or the failure V8 threw.
fn queue(record: &Value) -> Projected {
    let content = || match record.get("content") {
        None | Some(Value::Null) => Ok(String::new()),
        content => js::string(content).map(Cow::into_owned),
    };
    match record.get("operation").and_then(Value::as_str) {
        Some("enqueue") => Projected::Enqueue(content()),
        Some("dequeue") => Projected::Dequeue,
        Some("popAll") => Projected::PopAll(content()),
        Some("remove") => Projected::Remove(content()),
        _ => Projected::Other,
    }
}

fn assistant(record: &Value) -> Projected {
    // `record.message ?? {}`: none and null hold no field.
    let message = record.get("message").filter(|message| !message.is_null());
    let in_message = |name: &str| field(message, name);
    let content = in_message("content");
    let blocks = blocks(content)
        .iter()
        .filter_map(|block| match kind(block) {
            Some("tool_use" | "server_tool_use") => Some(Block::Use(said(block.get("id")))),
            Some("advisor_tool_result" | "tool_result") => Some(Block::Result(tool_result(block))),
            _ => None,
        })
        .collect();
    let status = record.get("apiErrorStatus").and_then(Value::as_f64);
    let text = claude_text(content);
    let refused = record.get("isApiErrorMessage") == Some(&Value::Bool(true));
    // Claude Code 2.1.296 to 2.1.302 write it with the next message (T-442's
    // record; `quit-before-a-word-resumed.jsonl`). A refusal for quota may say
    // the same words, and is the API's.
    let closes_a_cut_turn = !refused
        && in_message("model").and_then(Value::as_str) == Some("<synthetic>")
        && text == CUT_TURN_CLOSED;
    Projected::Assistant(Box::new(Assistant {
        at: timestamp(record),
        message_id: text_id(in_message("id")),
        uuid: text_id(record.get("uuid")),
        text,
        blocks,
        ended: matches!(
            in_message("stop_reason").and_then(Value::as_str),
            Some("end_turn" | "stop_sequence")
        ),
        closes_a_cut_turn,
        refused,
        rate_limited: status == Some(429.0)
            || record.get("error").and_then(Value::as_str) == Some("rate_limit"),
    }))
}

fn user(record: &Value, session: &str) -> Projected {
    let content = field(record.get("message"), "content");
    let text = claude_text(content);
    let output_of = record
        .get("parentUuid")
        .and_then(Value::as_str)
        .filter(|_| of_main_conversation(record, session) && OUTPUT.is_match(&text))
        .map(Arc::from);
    Projected::User(Box::new(User {
        at: timestamp(record),
        uuid: text_id(record.get("uuid")),
        results: blocks(content)
            .iter()
            .filter(|block| kind(block) == Some("tool_result"))
            .map(tool_result)
            .collect(),
        text,
        interrupt: is_interrupt(record),
        queued: record.get("promptSource").and_then(Value::as_str) == Some("queued"),
        summary: record.get("isCompactSummary") == Some(&Value::Bool(true)),
        output_of,
    }))
}

/// A system record: a `/clear`, or the boundary record that says a turn that
/// answered is over.
fn system(record: &Value, session: &str) -> Projected {
    match record.get("subtype").and_then(Value::as_str) {
        Some("local_command") => clear(record, session).map_or(Projected::Other, Projected::Clear),
        // 2.1.263/265/266's root query finalizer emits a `turn_duration` only
        // after query completion, after stop hooks, and when not aborted;
        // candidate/tool/queue/hook guards establish readiness. The exact
        // installed call sites and native fixture are documented beside
        // tests/engine/fixtures/completion/claude-code/v263-tool-loop.jsonl.
        // Later versions (2.1.296) write its counts of the background agents and
        // workflows still running when there are any (and none when there are
        // none): the turn is over all the same, and the counts are its word.
        Some("turn_duration") => {
            root_duration(record).map_or(Projected::Other, |agents| boundary(record, Some(agents)))
        }
        Some("stop_hook_summary")
            if record.get("preventedContinuation") == Some(&Value::Bool(false)) =>
        {
            boundary(record, None)
        }
        _ => Projected::Other,
    }
}

fn boundary(record: &Value, agents: Option<bool>) -> Projected {
    Projected::Boundary(Boundary {
        uuid: text_id(record.get("uuid")),
        agents,
    })
}

/// The uuid of the parent of a `local_command` record that has the shape of
/// the output of a `/clear`: of the main conversation, not a meta record, at
/// the level of information, saying nothing. That the parent is the user's
/// `/clear`, with nothing open, is for replay to tell.
fn clear(record: &Value, session: &str) -> Option<Arc<str>> {
    let shaped = of_main_conversation(record, session)
        && record.get("isMeta") == Some(&Value::Bool(false))
        && record.get("level").and_then(Value::as_str) == Some("info")
        && record.get("content").and_then(Value::as_str)
            == Some("<local-command-stdout></local-command-stdout>");
    if !shaped {
        return None;
    }
    record
        .get("parentUuid")
        .and_then(Value::as_str)
        .map(Arc::from)
}

/// `Number.isSafeInteger`, `>= 0`: a count as Claude Code writes one.
fn is_count(count: f64) -> bool {
    count >= 0.0 && count.fract() == 0.0 && count <= 9_007_199_254_740_991.0
}

/// Whether a `turn_duration` is the root conversation's, which is the end of its
/// turn: in the main conversation, with a duration and a message count that are
/// numbers, and counts of the background agents and workflows pending that are
/// counts where it has them. Says whether it names any pending.
///
/// Departs from Node's reader on purpose (the owner's decision of 2026-10-11):
/// Node asked the counts to be none or 0, and read a record that named any as no
/// end of the turn, which then stayed in flight until the last agent finished.
/// A count that is no count (a string, a fraction, a number below 0) is still no
/// end of the turn here.
fn root_duration(record: &Value) -> Option<bool> {
    // `Number.isFinite`, `>= 0`: a number alone, a JSON one always finite here.
    let duration = record.get("durationMs").and_then(Value::as_f64);
    let count = record.get("messageCount").and_then(Value::as_f64);
    let root = record.get("isSidechain") == Some(&Value::Bool(false))
        && duration.is_some_and(|duration| duration >= 0.0)
        && count.is_some_and(is_count);
    if !root {
        return None;
    }
    let mut running = false;
    for name in ["pendingBackgroundAgentCount", "pendingWorkflowCount"] {
        match record.get(name) {
            None => {}
            Some(Value::Number(pending)) => {
                let pending = pending.as_f64().filter(|pending| is_count(*pending))?;
                running |= pending > 0.0;
            }
            Some(_) => return None,
        }
    }
    Some(running)
}

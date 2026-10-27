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

use cf_base::{js, time};
use serde_json::{Map, Value};

use super::jobs::Watch;
use super::patterns::OUTPUT;
use super::text::{claude_text, claude_tool_text, is_interrupt};
use super::{field, kind};
use crate::shared::quota::{account_refusal, spent_extra_usage, Refusal};
use crate::shared::record::reading::Source;

/// What Claude Code says, in a message of its own, as it closes the turn it
/// found cut when it resumed the conversation.
const CUT_TURN_CLOSED: &str = "No response requested.";

/// A record, as replay reads it.
#[derive(Debug, PartialEq)]
pub(super) enum Projected {
    /// A record replay passes over: of a type it does not read, or of one it
    /// reads that says nothing of the turn.
    Other,
    /// The harness's own words in the conversation: what a `UserPromptSubmit` hook added to
    /// the prompt, and the companion of a turn (`turnCompanion`).
    Hook(Box<Hook>),
    /// Claude Code's own notification of a job's event or end, brought into a turn
    /// under way (an attachment, `queued_command` of
    /// `"commandMode":"task-notification"`): its text, and the time its record says.
    Notified(String, Option<Arc<str>>),
    /// An operation on Claude Code's queue of messages, with the content it
    /// names where it names one.
    Enqueue(Result<String, String>),
    Dequeue,
    PopAll(Result<String, String>),
    Remove(Result<String, String>),
    Assistant(Box<Assistant>),
    User(Box<User>),
    /// What a command typed at Claude's prompt printed, written as the system's
    /// record (a `/clear`'s, a `/compact` that found nothing to compact), a
    /// child of the record with this uuid.
    Output(Arc<str>),
    /// A boundary record that ends the turn of the answer before it.
    Boundary(Boundary),
}

/// A record that ends the turn of the answer before it.
#[derive(Debug, PartialEq)]
pub(super) struct Boundary {
    /// The record's uuid, when that is text.
    pub(super) uuid: Option<Arc<str>>,
    /// How many background agents and workflows the record says are still
    /// running: none for a record that says nothing of them (a stop hook's
    /// summary), as a `turn_duration` says by its counts.
    pub(super) agents: Option<usize>,
}

/// The harness's own words, an item of its own and no user's: an attachment that holds a
/// hook's context, or the companion of a turn.
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
    /// A refusal for quota: a status of 429, the error `rate_limit`, or words
    /// that say the account's extra usage is spent.
    pub(super) rate_limited: bool,
    /// What the account refused for, where it is its billing or its login: no limit that
    /// resets (MUS-23).
    pub(super) account: Option<Refusal>,
}

/// A block of an assistant's content that opens or closes a call.
#[derive(Debug, PartialEq)]
pub(super) enum Block {
    /// A `tool_use` or `server_tool_use`, by the id it says, and what call of the main
    /// conversation it is, where the reader reads its result.
    Use(Option<Value>, Call),
    /// A `tool_result` or `advisor_tool_result`.
    Result(ToolResult),
}

/// A call of the main conversation whose result the reader reads: a TaskStop's names the
/// job it stopped, and a Monitor's the monitor it armed, which its input says.
#[derive(Debug, PartialEq)]
pub(super) enum Call {
    TaskStop,
    Monitor(Watch),
    Other,
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
    /// The turn Claude Code begins by itself for a notification of a job's event or
    /// end: from the system (`promptSource`), for a notification (`turnOrigin`, and
    /// `origin.kind`; 2.1.241 has only the second, 2.1.277 on both).
    pub(super) notification: bool,
    /// Claude Code wrote it to the transcript alone (`queueTranscriptOnly`), and runs no
    /// turn for it: 2.1.292's notification of the jobs a resumed session's last process
    /// left, after which it waits at its prompt.
    pub(super) transcript_only: bool,
    /// The job a tool result of the record says it started (see [`job`]), where the record
    /// is of the main conversation (a subagent's jobs are its own, and its end wakes it):
    /// the call the record's first result answers, as the block says it, and the job.
    pub(super) job: Option<(Option<Value>, Arc<str>)>,
    /// What a TaskStop result of the record says it stopped, of the main conversation:
    /// the call it answers, as the block says it, and the job's id.
    pub(super) stopped: Option<(Option<Value>, Arc<str>)>,
}

/// The record `record` of the session `session`, as replay reads it.
pub(super) fn project(record: &Value, session: &str) -> Projected {
    match kind(record) {
        Some("attachment") => notified(record, session).unwrap_or_else(|| hook(record, session)),
        Some("queue-operation") => queue(record),
        Some("assistant") => assistant(record, session),
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

/// The attachment that brings Claude Code's own notification of a job into a turn under
/// way (2.1.277: the queued notification is taken back, `"reason": "absorbed_mid_turn"`,
/// and the turn reads it as this), in the main conversation: its text. None for
/// any other attachment, a human's message brought in so among them
/// (`"commandMode":"prompt"`).
fn notified(record: &Value, session: &str) -> Option<Projected> {
    let attachment = record.get("attachment");
    let says =
        |name: &str, wanted: &str| field(attachment, name).and_then(Value::as_str) == Some(wanted);
    let ours = of_main_conversation(record, session)
        && says("type", "queued_command")
        && says("commandMode", "task-notification");
    let text = field(attachment, "prompt").and_then(Value::as_str)?;
    let at = record
        .get("timestamp")
        .and_then(Value::as_str)
        .map(Arc::from);
    ours.then(|| Projected::Notified(text.to_owned(), at))
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

fn assistant(record: &Value, session: &str) -> Projected {
    // `record.message ?? {}`: none and null hold no field.
    let message = record.get("message").filter(|message| !message.is_null());
    let in_message = |name: &str| field(message, name);
    let content = in_message("content");
    let main = of_main_conversation(record, session);
    let made = record
        .get("timestamp")
        .and_then(Value::as_str)
        .and_then(time::parse);
    let blocks = blocks(content)
        .iter()
        .filter_map(|block| match kind(block) {
            Some("tool_use" | "server_tool_use") => {
                Some(Block::Use(said(block.get("id")), call(block, main, made)))
            }
            Some("advisor_tool_result" | "tool_result") => Some(Block::Result(tool_result(block))),
            _ => None,
        })
        .collect();
    let status = record.get("apiErrorStatus").and_then(Value::as_f64);
    let text = claude_text(content);
    let refused = record.get("isApiErrorMessage") == Some(&Value::Bool(true));
    // Anthropic's spent extra usage is a 400 (`invalid_request`), its words the refusal.
    let spent = spent_extra_usage(&text);
    // Claude Code writes it with the next message of a turn it was cut in
    // (T-442's record, 2.1.296; `quit-before-a-word-resumed.jsonl`), and still
    // does in 2.1.305, the newest seen (2026-10-21). A refusal for quota may
    // say the same words, and is the API's.
    let closes_a_cut_turn = !refused
        && in_message("model").and_then(Value::as_str) == Some("<synthetic>")
        && text == CUT_TURN_CLOSED;
    let rate_limited = status == Some(429.0)
        || record.get("error").and_then(Value::as_str) == Some("rate_limit")
        || spent;
    Projected::Assistant(Box::new(Assistant {
        at: timestamp(record),
        message_id: text_id(in_message("id")),
        uuid: text_id(record.get("uuid")),
        blocks,
        ended: matches!(
            in_message("stop_reason").and_then(Value::as_str),
            Some("end_turn" | "stop_sequence")
        ),
        closes_a_cut_turn,
        refused,
        rate_limited,
        account: if refused && !rate_limited {
            account_refusal(&text, status)
        } else {
            None
        },
        text,
    }))
}

fn user(record: &Value, session: &str) -> Projected {
    let content = field(record.get("message"), "content");
    let text = claude_text(content);
    let holds_results = blocks(content)
        .iter()
        .any(|block| kind(block) == Some("tool_result"));
    if is_companion(record) && !holds_results && !js::trim(&text).is_empty() {
        return Projected::Hook(Box::new(Hook {
            at: timestamp(record),
            text,
            uuid: text_id(record.get("uuid")),
        }));
    }
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
        notification: is_notification(record),
        transcript_only: record.get("queueTranscriptOnly") == Some(&Value::Bool(true)),
        job: of_main_conversation(record, session)
            .then(|| job(record))
            .flatten()
            .map(|job| (first_call(content), job)),
        stopped: of_main_conversation(record, session)
            .then(|| stopped(record, content))
            .flatten(),
    }))
}

/// Whether the user's record is the companion of a turn: what Claude Code writes beside
/// the message it was given, or a tool's call, and that is its own and no word of the
/// user's (`turnCompanion`; 2.1.308 writes `[Image: source: <path>]`, a line for each
/// picture it made an attachment of, after the message that named the paths). Claude Code
/// takes it for no prompt of the user's itself. `isMeta` is on other records that are the
/// user's turn all the same (the caveat of a command), and is no part of the test.
fn is_companion(record: &Value) -> bool {
    record.get("turnCompanion") == Some(&Value::Bool(true))
}

/// Whether a user's record is the turn Claude Code begins by itself for a notification
/// of a job, by what the record says of where it came from: no words of it make it one,
/// as a message that quotes a notification is none.
fn is_notification(record: &Value) -> bool {
    let says = |value: Option<&Value>, wanted: &str| value.and_then(Value::as_str) == Some(wanted);
    says(record.get("promptSource"), "system")
        && (says(record.get("turnOrigin"), "task_notification")
            || says(field(record.get("origin"), "kind"), "task-notification"))
}

/// The job a tool's result says it started, by its id: a command run in the background
/// (`backgroundTaskId`), or a monitor, whose start says its task and its time (2.1.305:
/// `taskId`, and `timeoutMs`, a number: 0 for one that is `persistent`).
fn job(record: &Value) -> Option<Arc<str>> {
    let result = record.get("toolUseResult");
    if let Some(id) = text_id(field(result, "backgroundTaskId")) {
        return Some(id);
    }
    field(result, "timeoutMs").and_then(Value::as_i64)?;
    text_id(field(result, "taskId"))
}

/// The call the first tool result of a user's `content` answers, as its block says it.
fn first_call(content: Option<&Value>) -> Option<Value> {
    blocks(content)
        .iter()
        .find(|block| kind(block) == Some("tool_result"))
        .and_then(|block| said(block.get("tool_use_id")))
}

/// What call of the main conversation (`main`) a `tool_use` block makes, by its tool's
/// name: a Monitor's input says what it is called (`description`) and what it watches
/// ([`source`]), and its record when it was made (`made`, milliseconds since the epoch).
fn call(block: &Value, main: bool, made: Option<i64>) -> Call {
    if !main || kind(block) != Some("tool_use") {
        return Call::Other;
    }
    let input = block.get("input");
    match block.get("name").and_then(Value::as_str) {
        Some("TaskStop") => Call::TaskStop,
        Some("Monitor") => Call::Monitor(Watch {
            called: made,
            description: field(input, "description")
                .and_then(Value::as_str)
                .map_or_else(|| Arc::from(""), Arc::from),
            source: source(input),
        }),
        _ => Call::Other,
    }
}

/// What a Monitor's `input` watches, as Claude Code chooses it (2.1.307, 2.1.308: it takes
/// exactly one source that is truthy, and runs a socket where `ws` is one, whatever
/// `command` says, `"command":""` among it: Astraeus on 472b1d96). A command only where
/// `command` is a text that is not empty and nothing else is a source; a socket where `ws`
/// is one; and where neither, a monitor with no process of the window to look for, which no
/// Claude Code this reader knows starts (`Source::Server`).
fn source(input: Option<&Value>) -> Source {
    let truthy = |value: Option<&Value>| match value {
        None | Some(Value::Null | Value::Bool(false)) => false,
        Some(Value::String(text)) => !text.is_empty(),
        Some(Value::Number(number)) => number.as_f64().is_some_and(|number| number != 0.0),
        Some(_) => true,
    };
    if truthy(field(input, "ws")) {
        Source::Socket
    } else if field(input, "command")
        .and_then(Value::as_str)
        .is_some_and(|command| !command.is_empty())
    {
        Source::Command
    } else {
        Source::Server
    }
}

/// What a TaskStop call's result says it stopped: the call it answers and the job, where
/// the record holds that one result alone, whose result is no error (2.1.305:
/// `toolUseResult.task_id`, and its message says it stopped it).
fn stopped(record: &Value, content: Option<&Value>) -> Option<(Option<Value>, Arc<str>)> {
    let id = text_id(field(record.get("toolUseResult"), "task_id"))?;
    let results: Vec<&Value> = blocks(content)
        .iter()
        .filter(|block| kind(block) == Some("tool_result"))
        .collect();
    let [result] = results.as_slice() else {
        return None;
    };
    if result.get("is_error") == Some(&Value::Bool(true)) {
        return None;
    }
    Some((said(result.get("tool_use_id")), id))
}

/// A system record: what a command typed at the prompt printed, or the
/// boundary record that says a turn that answered is over.
fn system(record: &Value, session: &str) -> Projected {
    match record.get("subtype").and_then(Value::as_str) {
        Some("local_command") => {
            output(record, session).map_or(Projected::Other, Projected::Output)
        }
        // 2.1.263/265/266's root query finalizer emits a `turn_duration` only
        // after query completion, after stop hooks, and when not aborted;
        // candidate/tool/queue/hook guards establish readiness. The exact
        // installed call sites and native fixture are documented beside
        // tests/engine/fixtures/completion/claude-code/v263-tool-loop.jsonl.
        // 2.1.305, the newest seen, still writes it from the interactive
        // session's paths, and holds it back while swarm tasks or detached tool
        // calls run.
        // Later versions (2.1.296) write its counts of the background agents and
        // workflows still running when there are any (and none when there are
        // none): the turn is over all the same, and the counts are its word.
        Some("turn_duration") => {
            root_duration(record).map_or(Projected::Other, |agents| boundary(record, Some(agents)))
        }
        // The Stop hook's summary ends a turn too: Claude Code 2.1.305, the
        // newest seen, holds `turn_duration` back while swarm tasks or
        // detached tool calls run (see `claude::install::settings`).
        Some("stop_hook_summary")
            if record.get("preventedContinuation") == Some(&Value::Bool(false)) =>
        {
            boundary(record, None)
        }
        _ => Projected::Other,
    }
}

fn boundary(record: &Value, agents: Option<usize>) -> Projected {
    Projected::Boundary(Boundary {
        uuid: text_id(record.get("uuid")),
        agents,
    })
}

/// The uuid of the parent of a `local_command` record that has the shape of
/// what a command printed: of the main conversation, not a meta record, at the
/// level of information, its content the command's output (`OUTPUT`: a
/// `/clear`'s says nothing, a `/compact` that found nothing to compact says so).
/// That the parent is the user's command, with nothing open, is for replay to
/// tell. A command Claude Code writes as the system's record too (`/mcp`,
/// `/plugin`) began no turn, and its output is the child of no user's record.
///
/// Departs from Node's reader on purpose (the live run of 2026-10-20, Claude
/// Code 2.1.304: a `/compact` typed into a conversation just cleared): Node read
/// the empty output of a `/clear` alone.
fn output(record: &Value, session: &str) -> Option<Arc<str>> {
    let shaped = of_main_conversation(record, session)
        && record.get("isMeta") == Some(&Value::Bool(false))
        && record.get("level").and_then(Value::as_str) == Some("info")
        && record
            .get("content")
            .and_then(Value::as_str)
            .is_some_and(|content| OUTPUT.is_match(content));
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
/// counts where it has them. Says how many it names pending, of both.
///
/// Departs from Node's reader on purpose (the owner's decision of 2026-10-11):
/// Node asked the counts to be none or 0, and read a record that named any as no
/// end of the turn, which then stayed in flight until the last agent finished.
/// A count that is no count (a string, a fraction, a number below 0) is still no
/// end of the turn here.
fn root_duration(record: &Value) -> Option<usize> {
    // `Number.isFinite`, `>= 0`: a number alone, a JSON one always finite here.
    let duration = record.get("durationMs").and_then(Value::as_f64);
    let count = record.get("messageCount").and_then(Value::as_f64);
    let root = record.get("isSidechain") == Some(&Value::Bool(false))
        && duration.is_some_and(|duration| duration >= 0.0)
        && count.is_some_and(is_count);
    if !root {
        return None;
    }
    let mut running = 0_usize;
    for name in ["pendingBackgroundAgentCount", "pendingWorkflowCount"] {
        match record.get(name) {
            None => {}
            Some(Value::Number(pending)) => {
                let pending = pending.as_f64().filter(|pending| is_count(*pending))?;
                // A whole number from 0 to 2^53 - 1, which `as` saturates for a
                // machine whose `usize` is smaller.
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let pending = pending as usize;
                running = running.saturating_add(pending);
            }
            Some(_) => return None,
        }
    }
    Some(running)
}

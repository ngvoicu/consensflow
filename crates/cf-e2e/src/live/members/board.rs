//! What a check reads off the board and the windows, and how it waits for it: the
//! wait that ends with what it found, the conversation a window was opened on, the
//! number of the task `cf task add` printed, a member's window as the board has it,
//! and the human's Pause of a task.

use std::time::Duration;

use serde_json::Value;

use super::super::{text, until, Log};
use crate::Result;

/// What `look` first finds within `within`, looking again every `step`: the wait
/// of a check, which says what it waits for and for how long, and ends with what
/// it found. None when the time ran out is a check that did not hold, and is
/// not an error; a look that fails is.
pub fn find<T>(
    log: &Log,
    what: &str,
    within: Duration,
    step: Duration,
    mut look: impl FnMut() -> Result<Option<T>>,
) -> Result<Option<T>> {
    log.say(format!("waiting up to {} s for {what}", within.as_secs()));
    match until(within, step, || match look() {
        Ok(None) => None,
        Ok(Some(found)) => Some(Ok(found)),
        Err(failed) => Some(Err(failed)),
    }) {
        Some(Ok(found)) => Ok(Some(found)),
        Some(Err(failed)) => Err(failed),
        None => Ok(None),
    }
}

/// The conversation a window was opened on, from its `pane.open`: the word after
/// `--session-id` (Claude Code and Pi are given the id of a new conversation) or
/// after `--resume` (a window on one that is kept).
pub fn session_of(open: &Value) -> Option<String> {
    let argv = open["argv"].as_array()?;
    let flag = argv
        .iter()
        .position(|word| word == "--session-id" || word == "--resume")?;
    argv.get(flag + 1)?.as_str().map(str::to_owned)
}

/// The number of the task `cf task add --json` printed.
pub(super) fn task_number(printed: &str) -> Option<i64> {
    let said: Value = serde_json::from_str(printed).ok()?;
    said["task"]["number"].as_i64().or(said["number"].as_i64())
}

/// What a task that is over is in: its work is done, or it never will be.
const OVER: [&str; 4] = ["done", "accepted", "failed", "cancelled"];

/// Whether the task of `thread` (as the board reads it with its whole thread,
/// [`super::Members::task`]) is over.
pub fn is_over(thread: &Value) -> bool {
    thread["state"]
        .as_str()
        .is_some_and(|state| OVER.contains(&state))
}

/// The messages of `thread` of the kind `kind`, oldest first.
fn messages_of<'t>(thread: &'t Value, kind: &'t str) -> impl Iterator<Item = &'t Value> {
    thread["messages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(move |message| message["kind"] == kind)
}

/// Whether the brief of `thread` (its first message of the kind `task`) has reached
/// the member's window: the daemon has seen it in the window's record (`delivered`),
/// or the member has read it. A window the lane reads as working may still be
/// waiting for it: it is `delivering` for as long as the window has not shown it.
pub fn brief_delivered(thread: &Value) -> bool {
    messages_of(thread, "task")
        .next()
        .is_some_and(|brief| matches!(brief["state"].as_str(), Some("delivered" | "read")))
}

/// The first question the member asked in `thread`.
pub fn first_question(thread: &Value) -> Option<Value> {
    messages_of(thread, "question").next().cloned()
}

/// The last result the member gave in `thread`.
pub fn last_result(thread: &Value) -> Option<&Value> {
    messages_of(thread, "result").last()
}

/// What the chief was given of a task's result: the body of the first result in the
/// chief's `inbox` that has been delivered to it. None until it has, which a task
/// that is done may be a moment from (the daemon delivers it to the chief's window
/// as it takes the task's end).
pub fn delivered_result(inbox: &[Value]) -> Option<String> {
    inbox
        .iter()
        .find(|message| message["kind"] == "result" && message["state"] == "delivered")
        .map(|message| text(&message["body"]))
}

/// The notes of a chief's `inbox` that say T-`task`'s window left work: what the daemon
/// tells whoever gave a task when the window that worked on it closed with background
/// work still running, which the close ends (a turn's end ends nothing since turn-end's
/// smallest rule of 2026-10-21), oldest first. None where it left nothing.
pub fn notes_of_work(inbox: &[Value], task: i64) -> Vec<String> {
    let opening = format!("T-{task}'s window closed with background work");
    inbox
        .iter()
        .filter(|message| message["kind"] == "note")
        .filter_map(|message| message["body"].as_str())
        .filter(|body| body.starts_with(&opening))
        .map(str::to_owned)
        .collect()
}

/// The first answer given in `thread`, to whichever question.
pub fn first_answer(thread: &Value) -> Option<Value> {
    messages_of(thread, "answer").next().cloned()
}

/// The answer in `thread` to `question`.
pub fn answer_to(thread: &Value, question: &Value) -> Option<Value> {
    messages_of(thread, "answer")
        .find(|message| message["replyTo"] == question["id"])
        .cloned()
}

/// A member's window: its lane's participant, the pane the daemon opened it in,
/// and what the board says it is doing.
#[derive(Debug, Clone)]
pub struct MemberWindow {
    /// The participant's handle: `bench-claude-1`, the name of its session.
    pub handle: String,
    /// The pane, as the pane host takes it in a request: `id` and `generation`.
    pub pane: Value,
    /// The `pane.open` the daemon made for it, the newest of its pane's.
    pub open: Option<Value>,
    /// What the board says it is doing.
    pub activity: Value,
    /// Its whole lane.
    pub lane: Value,
}

impl MemberWindow {
    /// The pane's id.
    pub fn id(&self) -> &str {
        self.pane["id"].as_str().unwrap_or_default()
    }

    /// The conversation it opened on.
    pub fn session(&self) -> Option<String> {
        self.open.as_ref().and_then(session_of)
    }
}

/// The human's Pause of a task, and when it was made.
#[derive(Debug, Clone)]
pub struct Paused {
    /// When it was made, in milliseconds since the epoch.
    pub made_ms: i64,
    /// What the daemon answered.
    pub answer: Value,
}

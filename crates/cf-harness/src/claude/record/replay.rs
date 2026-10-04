//! A record replayed (`replay`, `hosts/lib/completion/claude-code.js`): what
//! it says of the turn, by its type.

use std::borrow::Cow;
use std::sync::Arc;

use cf_base::{js, time};
use serde_json::Value;

use super::patterns::CLEAR;
use super::text::{claude_text, claude_tool_text, is_interrupt};
use super::{field, kind, Candidate, Terminal, Transcript};
use crate::shared::quota::exhausted_quota;
use crate::shared::record::jsonl::Stop;
use crate::shared::record::reading::{native_id, Role};

impl Transcript {
    /// The record at `seq` among the transcript's, with its `place` in the
    /// tree when it has one.
    pub(super) fn replay(
        &mut self,
        record: &Value,
        place: Option<usize>,
        seq: usize,
    ) -> Result<(), Stop> {
        let number = Value::from(seq);
        // `record.timestamp ?? seq`.
        let at = match record.get("timestamp") {
            None | Some(Value::Null) => &number,
            Some(at) => at,
        };
        match kind(record) {
            Some("attachment") => self.hook_context(record, at, seq),
            Some("queue-operation") => self.queue_operation(record),
            Some("assistant") => self.assistant(record, at, seq),
            Some("user") => self.user(record, place, at, seq),
            Some("system") => {
                self.system(record);
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// An attachment: what a `UserPromptSubmit` hook added to the prompt, a
    /// context of the conversation that is its own, not the assistant's.
    fn hook_context(&mut self, record: &Value, at: &Value, seq: usize) -> Result<(), Stop> {
        let attachment = record.get("attachment");
        let of = |name: &str| field(attachment, name);
        let says = |name: &str, wanted: &str| of(name).and_then(Value::as_str) == Some(wanted);
        if !(self.is_of_main_conversation(record)
            && says("type", "hook_additional_context")
            && says("hookEvent", "UserPromptSubmit"))
        {
            return Ok(());
        }
        let Some(Value::Array(parts)) = of("content") else {
            return Ok(());
        };
        let text = parts
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join("\n");
        if !text.is_empty() {
            let id = id_of(record.get("uuid"), "claude hook context", seq)?;
            self.items.push(id, Role::Custom, &text, at, seq);
        }
        Ok(())
    }

    /// Whether the record is of this session's main conversation, not of
    /// another session or of a sidechain.
    fn is_of_main_conversation(&self, record: &Value) -> bool {
        record.get("sessionId").and_then(Value::as_str) == Some(&*self.session)
            && record.get("isSidechain") == Some(&Value::Bool(false))
    }

    /// An operation on Claude Code's queue of messages.
    fn queue_operation(&mut self, record: &Value) -> Result<(), Stop> {
        match record.get("operation").and_then(Value::as_str) {
            Some("enqueue") => self.queues.enqueue(queued_content(record)?),
            Some("dequeue") => self.queues.dequeue(),
            Some("popAll") => self.queues.pop_all(queued_content(record)?),
            Some("remove") => self.queues.remove(&queued_content(record)?),
            _ => {}
        }
        Ok(())
    }

    /// An assistant's record: a fragment of a message, the tool calls it
    /// makes and the results it holds; its refusal when the API refused; and
    /// its answer, if it ended the turn.
    fn assistant(&mut self, record: &Value, at: &Value, seq: usize) -> Result<(), Stop> {
        // `record.message ?? {}`: none and null hold no field.
        let message = record.get("message").filter(|message| !message.is_null());
        let in_message = |name: &str| field(message, name);
        self.queues.answered();
        let named = in_message("id").and_then(Value::as_str);
        if self
            .active_assistant
            .as_deref()
            .is_some_and(|active| Some(active) != named)
        {
            self.open_tools.clear();
            let ended = self.candidate.as_ref().map(|candidate| &*candidate.item_id);
            if ended != named {
                if let Some(candidate) = self.candidate.take() {
                    self.hooks.remove(&candidate.item_id);
                }
            }
        }
        self.turn_open = true;
        self.terminal = None;
        let refused = record.get("isApiErrorMessage") == Some(&Value::Bool(true));
        if !refused {
            self.failed = false;
        }
        // The latest assistant record has the last word on quota.
        self.quota = None;
        let message_id = id_of(in_message("id"), "claude message", seq)?;
        self.active_assistant = Some(Arc::clone(&message_id));
        let item = self.items.assistant(Arc::clone(&message_id), at, seq);
        let text = claude_text(in_message("content"));
        // The record's uuid is asked for even of a record that says nothing.
        let uuid = id_of(record.get("uuid"), "claude record", seq)?;
        self.items.fragment(item, Arc::clone(&uuid), &text);

        for block in blocks(in_message("content")) {
            if matches!(kind(block), Some("tool_use" | "server_tool_use")) {
                let call = self.keys.of(block.get("id"));
                if call.truthy() {
                    self.open_tools.insert(call);
                }
            }
            if matches!(kind(block), Some("advisor_tool_result" | "tool_result")) {
                self.tool_result(block, at, seq)?;
            }
        }

        if refused {
            self.hooks.clear();
            self.candidate = None;
            self.turn_open = false;
            self.failed = true;
            let status = record.get("apiErrorStatus").and_then(Value::as_f64);
            if status == Some(429.0)
                || record.get("error").and_then(Value::as_str) == Some("rate_limit")
            {
                let at_ms = date_parse(record.get("timestamp")).map_err(Stop::Failed)?;
                let quota = exhausted_quota(&text, at_ms, &self.local).map_err(Stop::Failed)?;
                self.quota = Some(Arc::new(quota));
            }
            self.terminal = Some(Terminal::Native);
            return Ok(());
        }

        if matches!(
            in_message("stop_reason").and_then(Value::as_str),
            Some("end_turn" | "stop_sequence")
        ) {
            self.hooks.insert(Arc::clone(&message_id));
            self.candidate = Some(Candidate {
                item_id: message_id,
                uuid,
            });
        }
        Ok(())
    }

    /// A tool's result in a block: the call it answers is closed, and the
    /// result is an item. A block that names no call is passed over.
    fn tool_result(&mut self, block: &Value, at: &Value, seq: usize) -> Result<(), Stop> {
        let call = self.keys.of(block.get("tool_use_id"));
        if !call.truthy() {
            return Ok(());
        }
        self.open_tools.remove(&call);
        let text = claude_tool_text(block.get("content")).map_err(Stop::Failed)?;
        let id = id_of(block.get("tool_use_id"), "claude tool result", seq)?;
        self.items.tool(id, &text, at, seq);
        Ok(())
    }

    /// A user's record: the results of tools it holds, the interrupt that
    /// ends a turn, or a real prompt, which begins one, unless it is the
    /// late ancestor of an answer that already ended its turn.
    fn user(
        &mut self,
        record: &Value,
        place: Option<usize>,
        at: &Value,
        seq: usize,
    ) -> Result<(), Stop> {
        let content = field(record.get("message"), "content");
        for block in blocks(content) {
            if kind(block) == Some("tool_result") {
                self.tool_result(block, at, seq)?;
            }
        }
        let text = claude_text(content);
        if is_interrupt(record) {
            let id = id_of(record.get("uuid"), "claude user", seq)?;
            self.items.push(id, Role::User, &text, at, seq);
            self.turn_open = false;
            self.hooks.clear();
            self.terminal = Some(Terminal::Native);
            self.candidate = None;
            return Ok(());
        }
        if js::trim(&text).is_empty() {
            return Ok(());
        }
        let id = id_of(record.get("uuid"), "claude user", seq)?;
        self.items.push(id, Role::User, &text, at, seq);
        if self.late_ancestor(place) {
            return Ok(());
        }
        let queued = record.get("promptSource").and_then(Value::as_str) == Some("queued");
        self.queues.consumed(&text, queued);
        self.open_tools.clear();
        self.hooks.clear();
        self.active_assistant = None;
        self.failed = false;
        self.turn_open = true;
        self.candidate = None;
        self.terminal = None;
        Ok(())
    }

    /// `lateAncestor`: whether the user record at `place` is one of the
    /// records that came before the answer that already ended the turn.
    fn late_ancestor(&mut self, user: Option<usize>) -> bool {
        let (Some(Terminal::Derived { item_id, uuid }), Some(candidate)) =
            (&self.terminal, &self.candidate)
        else {
            return false;
        };
        if item_id != &candidate.item_id {
            return false;
        }
        self.ancestry
            .is_late(user, uuid.as_deref(), &candidate.uuid)
    }

    /// A system record: a `/clear`, or the boundary record that says a turn
    /// that answered is over.
    fn system(&mut self, record: &Value) {
        let subtype = record.get("subtype").and_then(Value::as_str);
        if subtype == Some("local_command") && self.is_clear(record) {
            self.turn_open = false;
            self.candidate = None;
            self.terminal = Some(Terminal::Native);
            return;
        }
        // 2.1.263/265/266's root query finalizer emits a `turn_duration` only
        // after query completion, after stop hooks, and when not aborted. The
        // transcript omits optional background counts; candidate/tool/queue/
        // hook guards establish readiness. The exact installed call sites and
        // native fixture are documented beside
        // tests/engine/fixtures/completion/claude-code/v263-tool-loop.jsonl.
        let duration = subtype == Some("turn_duration") && is_root_duration(record);
        if !(duration || subtype == Some("stop_hook_summary")) {
            return;
        }
        let Some(candidate) = &self.candidate else {
            return;
        };
        if !duration && record.get("preventedContinuation") != Some(&Value::Bool(false)) {
            return;
        }
        let item_id = Arc::clone(&candidate.item_id);
        self.hooks.remove(&item_id);
        self.items.complete(&item_id);
        self.turn_open = false;
        let uuid = record
            .get("uuid")
            .and_then(Value::as_str)
            .filter(|uuid| !uuid.is_empty())
            .map(Arc::from);
        self.terminal = Some(Terminal::Derived { item_id, uuid });
    }

    /// Whether a `local_command` record is the output of a `/clear` that is
    /// the user's turn last pushed, with nothing open: a native end of the
    /// turn.
    fn is_clear(&self, record: &Value) -> bool {
        let Some(command) = self.items.last() else {
            return false;
        };
        self.is_of_main_conversation(record)
            && record.get("isMeta") == Some(&Value::Bool(false))
            && record.get("level").and_then(Value::as_str) == Some("info")
            && record.get("content").and_then(Value::as_str)
                == Some("<local-command-stdout></local-command-stdout>")
            && command.role == Role::User
            && record.get("parentUuid").and_then(Value::as_str) == Some(&*command.id)
            && CLEAR.is_match(&command.text)
            && self.open_tools.is_empty()
            && self.hooks.is_empty()
    }
}

/// Whether a `turn_duration` is the root conversation's: in the main
/// conversation, with a duration and a message count that are numbers, and
/// no background agent or workflow pending.
fn is_root_duration(record: &Value) -> bool {
    // `Number.isFinite`, `>= 0`: a number alone, a JSON one always finite here.
    let duration = record.get("durationMs").and_then(Value::as_f64);
    let count = record.get("messageCount").and_then(Value::as_f64);
    // `Number.isSafeInteger`, `>= 0`.
    let counted = count.is_some_and(|count| {
        count >= 0.0 && count.fract() == 0.0 && count <= 9_007_199_254_740_991.0
    });
    let nothing_pending = ["pendingBackgroundAgentCount", "pendingWorkflowCount"]
        .into_iter()
        .all(|name| match record.get(name) {
            None => true,
            Some(Value::Number(pending)) => pending.as_f64() == Some(0.0),
            Some(_) => false,
        });
    record.get("isSidechain") == Some(&Value::Bool(false))
        && duration.is_some_and(|duration| duration >= 0.0)
        && counted
        && nothing_pending
}

/// The blocks of a message's content: none for content that is no list.
fn blocks(content: Option<&Value>) -> &[Value] {
    match content {
        Some(Value::Array(blocks)) => blocks,
        _ => &[],
    }
}

/// `nativeId` of a record's field, as the look's failure.
fn id_of(value: Option<&Value>, what: &str, seq: usize) -> Result<Arc<str>, Stop> {
    native_id(value, what, &Value::from(seq)).map_err(Stop::Failed)
}

/// `String(record.content ?? '')`, or the failure V8 threw: the content a
/// queue operation names.
fn queued_content(record: &Value) -> Result<String, Stop> {
    match record.get("content") {
        None | Some(Value::Null) => Ok(String::new()),
        content => js::string(content)
            .map(Cow::into_owned)
            .map_err(Stop::Failed),
    }
}

/// `Date.parse(value)`: the time a record says, in milliseconds, NaN where it
/// names none. A value that is no text is read as the text `String` makes of
/// it, which V8 fails on an object with a `toString` of its own.
///
/// Kept from Node on purpose, as `time::parse` keeps it: only the date-time
/// format is read. A `timestamp` in another form (a bare number, `Sep 19
/// 2026`) names no time here, where V8's legacy parser may read a date: Claude
/// Code writes `toISOString`'s, and no record of its has been seen to say
/// otherwise.
fn date_parse(value: Option<&Value>) -> Result<f64, String> {
    let text = js::string(value)?;
    // A time `parse` reads is within what a double holds exactly.
    #[allow(clippy::cast_precision_loss)]
    let ms = time::parse(&text).map_or(f64::NAN, |ms| ms as f64);
    Ok(ms)
}

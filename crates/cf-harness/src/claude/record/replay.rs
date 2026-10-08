//! A record replayed: what it says of the turn, by its type. A record is
//! replayed as the projection made of it (`project`): what is read of it, and
//! no more.

use std::sync::Arc;

use cf_base::{js, time};
use serde_json::Value;

use super::patterns::CLEAR;
use super::project::{Assistant, Block, Hook, Projected, ToolResult, User};
use super::{Candidate, Terminal, Transcript};
use crate::shared::quota::exhausted_quota;
use crate::shared::record::reading::{native_id, Role};

impl Transcript {
    /// The record at `seq` among the transcript's, with its `place` in the
    /// tree when it has one.
    pub(super) fn replay(
        &mut self,
        record: &Projected,
        place: Option<usize>,
        seq: usize,
    ) -> Result<(), String> {
        match record {
            Projected::Other => {}
            Projected::Hook(hook) => self.hook_context(hook, seq)?,
            Projected::Enqueue(content) => self.queues.enqueue(content.clone()?),
            Projected::Dequeue => self.queues.dequeue(),
            Projected::PopAll(content) => self.queues.pop_all(content.clone()?),
            Projected::Remove(content) => {
                self.queues.remove(content.as_ref().map_err(Clone::clone)?);
            }
            Projected::Assistant(assistant) => self.assistant(assistant, seq)?,
            Projected::User(user) => self.user(user, place, seq)?,
            Projected::Clear(parent) => self.clear(parent),
            Projected::Boundary(uuid) => self.boundary(uuid.as_ref()),
        }
        Ok(())
    }

    /// An attachment that holds what a `UserPromptSubmit` hook added to the
    /// prompt: an item of its own, not the assistant's.
    fn hook_context(&mut self, hook: &Hook, seq: usize) -> Result<(), String> {
        let number = Value::from(seq);
        // `record.timestamp ?? seq`.
        let at = hook.at.as_ref().unwrap_or(&number);
        let id = id_of(hook.uuid.as_ref(), "claude hook context", seq)?;
        self.items.push(id, Role::Custom, &hook.text, at, seq);
        Ok(())
    }

    /// An assistant's record: a fragment of a message, the tool calls it
    /// makes and the results it holds; its refusal when the API refused; and
    /// its answer, if it ended the turn.
    fn assistant(&mut self, record: &Assistant, seq: usize) -> Result<(), String> {
        let number = Value::from(seq);
        let at = record.at.as_ref().unwrap_or(&number);
        self.queues.answered();
        let named = record.message_id.as_deref();
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
        if !record.refused {
            self.failed = false;
        }
        // The latest assistant record has the last word on quota.
        self.quota = None;
        let message_id = id_of(record.message_id.as_ref(), "claude message", seq)?;
        self.active_assistant = Some(Arc::clone(&message_id));
        let item = self.items.assistant(Arc::clone(&message_id), at, seq);
        // The record's uuid is asked for even of a record that says nothing.
        let uuid = id_of(record.uuid.as_ref(), "claude record", seq)?;
        self.items.fragment(item, Arc::clone(&uuid), &record.text);

        for block in &record.blocks {
            match block {
                Block::Use(id) => {
                    let call = self.keys.of(id.as_ref());
                    if call.truthy() {
                        self.open_tools.insert(call);
                    }
                }
                Block::Result(result) => self.tool_result(result, at, seq)?,
            }
        }

        if record.refused {
            self.hooks.clear();
            self.candidate = None;
            self.turn_open = false;
            self.failed = true;
            if record.rate_limited {
                let at_ms = date_parse(record.at.as_ref())?;
                let quota = exhausted_quota(&record.text, at_ms, &self.local)?;
                self.quota = Some(Arc::new(quota));
            }
            self.terminal = Some(Terminal::Native);
            return Ok(());
        }

        if record.ended {
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
    fn tool_result(&mut self, result: &ToolResult, at: &Value, seq: usize) -> Result<(), String> {
        let call = self.keys.of(result.call.as_ref());
        if !call.truthy() {
            return Ok(());
        }
        self.open_tools.remove(&call);
        let text = result.text.as_ref().map_err(Clone::clone)?;
        let id = native_id(
            result.call.as_ref(),
            "claude tool result",
            &Value::from(seq),
        )?;
        self.items.tool(id, text, at, seq);
        Ok(())
    }

    /// A user's record: the results of tools it holds, the interrupt that
    /// ends a turn, or a real prompt, which begins one, unless it is the
    /// late ancestor of an answer that already ended its turn.
    fn user(&mut self, record: &User, place: Option<usize>, seq: usize) -> Result<(), String> {
        let number = Value::from(seq);
        let at = record.at.as_ref().unwrap_or(&number);
        for result in &record.results {
            self.tool_result(result, at, seq)?;
        }
        if record.interrupt {
            let id = id_of(record.uuid.as_ref(), "claude user", seq)?;
            self.items.push(id, Role::User, &record.text, at, seq);
            self.turn_open = false;
            self.hooks.clear();
            self.terminal = Some(Terminal::Native);
            self.candidate = None;
            return Ok(());
        }
        if js::trim(&record.text).is_empty() {
            return Ok(());
        }
        let id = id_of(record.uuid.as_ref(), "claude user", seq)?;
        self.items.push(id, Role::User, &record.text, at, seq);
        if self.late_ancestor(place) {
            return Ok(());
        }
        self.queues.consumed(&record.text, record.queued);
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

    /// The output of a `/clear` that is the user's turn last pushed, with
    /// nothing open: a native end of the turn. `parent` is the uuid of the
    /// record the output is a child of.
    fn clear(&mut self, parent: &str) {
        let Some(command) = self.items.last() else {
            return;
        };
        let of_the_command = command.role == Role::User
            && parent == &*command.id
            && CLEAR.is_match(&command.text)
            && self.open_tools.is_empty()
            && self.hooks.is_empty();
        if of_the_command {
            self.turn_open = false;
            self.candidate = None;
            self.terminal = Some(Terminal::Native);
        }
    }

    /// The boundary record that says a turn that answered is over, by its
    /// uuid when that is text.
    fn boundary(&mut self, uuid: Option<&Arc<str>>) {
        let Some(candidate) = &self.candidate else {
            return;
        };
        let item_id = Arc::clone(&candidate.item_id);
        self.hooks.remove(&item_id);
        self.items.complete(&item_id);
        self.turn_open = false;
        self.terminal = Some(Terminal::Derived {
            item_id,
            uuid: uuid.cloned(),
        });
    }
}

/// `nativeId` of an id the projection held as text: the failure of a record
/// that gave none.
fn id_of(id: Option<&Arc<str>>, what: &str, seq: usize) -> Result<Arc<str>, String> {
    match id {
        Some(id) => Ok(Arc::clone(id)),
        None => native_id(None, what, &Value::from(seq)),
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

//! A record replayed: what it says of the turn, by its type. A record is
//! replayed as the projection made of it (`project`): what is read of it, and
//! no more.

use std::sync::Arc;

use cf_base::{js, time};
use serde_json::Value;

use super::patterns::QUIET_COMMAND;
use super::project::{
    Assistant, Block, Boundary, Call, Compacting, Hook, Projected, ToolResult, User,
};
use super::{Candidate, Terminal, Transcript, TypedCompact};
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
            // Claude Code's notification brought into a turn under way: the jobs it says are
            // over have ended. Queued, it ended none: a message a human typed is queued alike.
            Projected::Notified(text, at) => self.jobs.ended_by(text, at.as_deref()),
            Projected::Enqueue(content) => self.queues.enqueue(content.clone()?),
            Projected::Dequeue => self.queues.dequeue(),
            Projected::PopAll(content) => self.queues.pop_all(content.clone()?),
            Projected::Remove(content) => {
                self.queues.remove(content.as_ref().map_err(Clone::clone)?);
            }
            Projected::Assistant(assistant) => self.assistant(assistant, seq)?,
            Projected::User(user) => self.user(user, place, seq)?,
            Projected::Output(parent) => self.output(parent),
            Projected::Boundary(boundary) => self.boundary(boundary),
        }
        Ok(())
    }

    /// The harness's own words: an attachment that holds what a `UserPromptSubmit`
    /// hook added to the prompt, or the user's record that is the companion of a turn
    /// (`turnCompanion`: the sources of the images Claude Code attached to the
    /// message before it). An item of its own, neither the assistant's nor a message
    /// of the user's, so the turn is still the message it follows and begins no turn
    /// of its own.
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
    /// its answer, if it ended the turn. Claude Code's own close of a turn it
    /// found cut is no one's answer.
    fn assistant(&mut self, record: &Assistant, seq: usize) -> Result<(), String> {
        let number = Value::from(seq);
        let at = record.at.as_ref().unwrap_or(&number);
        self.queues.answered();
        self.typed_compact = None;
        self.put_back = false;
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
            self.failure = None;
        }
        // The latest assistant record has the last word on quota.
        self.quota = None;
        let message_id = id_of(record.message_id.as_ref(), "claude message", seq)?;
        self.active_assistant = Some(Arc::clone(&message_id));
        let item = self.items.assistant(Arc::clone(&message_id), at, seq);
        if record.closes_a_cut_turn {
            self.items.synthetic(item);
        }
        // The record's uuid is asked for even of a record that says nothing.
        let uuid = id_of(record.uuid.as_ref(), "claude record", seq)?;
        self.items.fragment(item, Arc::clone(&uuid), &record.text);

        for block in &record.blocks {
            match block {
                Block::Use(id, made) => {
                    let call = self.keys.of(id.as_ref());
                    if call.truthy() {
                        match made {
                            Call::TaskStop => {
                                self.task_stops.insert(call.clone());
                            }
                            Call::Monitor(watch) => {
                                self.monitors.insert(call.clone(), watch.clone());
                            }
                            Call::Other => {}
                        }
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
            self.failure = Some(record.text.clone());
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
    /// late ancestor of an answer that already ended its turn. The summary a
    /// compaction writes is an item as any, and begins no turn where none is
    /// open: Claude Code compacts a conversation while it is idle, and runs no
    /// turn after (the owner's report of 2026-10-15, a Resume that told a chief
    /// at rest its turn was cut). One in a turn, as Node read it.
    ///
    /// What a command typed at Claude's prompt printed (`/compact`, `/model`)
    /// is an item as any, and a native end of the command's turn where it
    /// follows the command, which runs no model, with nothing open
    /// (`follows_its_command`): the command, the words Claude
    /// Code writes before it (`<local-command-caveat>`, and the bare `/compact`
    /// of 2.1.303) and the summary of a compaction read as a prompt each, and
    /// no model runs after (the long run of 2026-10-19, a chief at rest after
    /// a `/compact` read at work for 80 minutes). Departs from Node's reader,
    /// which read the output as a prompt too.
    ///
    /// A compaction cancelled with Escape (Claude Code 2.1.304 and 2.1.310, the
    /// live suite `commands-at-rest`) writes the bare `/compact` as it was typed,
    /// and at the Escape the words before a command, a sibling of it, and
    /// nothing more: a native end of the turn the bare command began
    /// (`cancels`), and Claude Code puts the `/compact` back in its input box
    /// (`Lines::put_back`). A compaction at work writes nothing after its bare
    /// command but its boundary and summary, and those words after them, their
    /// child.
    ///
    /// A result that says its call started a job in the background (a command, a
    /// monitor) starts it, and a TaskStop call's result that says it stopped one ends
    /// it; a notification Claude Code takes as a turn of its own ends the jobs its head
    /// says are over (`jobs`): its own, which the record says it is, and no message
    /// that quotes one. One Claude Code wrote to the transcript alone
    /// (`queueTranscriptOnly`) begins no turn and answers nothing: it is an item of the
    /// harness's, so that the answer the turn ended with is still its last words. The
    /// summary a compaction writes is marked so (`Item::compaction`): no message to the
    /// agent, and no end of an answer.
    fn user(&mut self, record: &User, place: Option<usize>, seq: usize) -> Result<(), String> {
        let number = Value::from(seq);
        let at = record.at.as_ref().unwrap_or(&number);
        let typed = self.typed_compact.take();
        self.put_back = false;
        for result in &record.results {
            self.tool_result(result, at, seq)?;
        }
        if let Some((call, job)) = &record.job {
            let watch = self.monitors.remove(&self.keys.of(call.as_ref()));
            self.jobs.started(Arc::clone(job), watch);
        }
        if let Some((call, job)) = &record.stopped {
            if self.task_stops.remove(&self.keys.of(call.as_ref())) {
                self.jobs.stopped(job);
            }
        }
        if record.notification {
            self.jobs
                .ended_by(&record.text, record.at.as_ref().and_then(Value::as_str));
        }
        if record.interrupt {
            let id = id_of(record.uuid.as_ref(), "claude user", seq)?;
            self.items.push(id, Role::User, &record.text, at, seq);
            // A call the interrupt cut is over: a server tool's (the advisor's)
            // gets no result to close it.
            self.open_tools.clear();
            self.turn_open = false;
            self.hooks.clear();
            self.terminal = Some(Terminal::Native);
            self.candidate = None;
            return Ok(());
        }
        if js::trim(&record.text).is_empty() {
            return Ok(());
        }
        let ends = record
            .output_of
            .as_deref()
            .is_some_and(|parent| self.follows_its_command(parent));
        let cancels = typed.is_some_and(|typed| Self::cancels(&typed, record));
        let id = id_of(record.uuid.as_ref(), "claude user", seq)?;
        if record.summary {
            self.items.summary(id, &record.text, at, seq);
        } else if record.transcript_only {
            self.items.push(id, Role::Custom, &record.text, at, seq);
        } else {
            self.items.push(id, Role::User, &record.text, at, seq);
        }
        if ends || cancels {
            self.ended_natively();
            self.put_back = cancels;
            return Ok(());
        }
        if (record.summary && !self.turn_open) || self.late_ancestor(place) {
            return Ok(());
        }
        if let Some((Compacting::Typed, parent)) = &record.compacting {
            self.typed_compact = Some(TypedCompact {
                parent: Arc::clone(parent),
            });
        }
        self.queues.consumed(&record.text, record.queued);
        // Written to the transcript alone, it runs no turn: the turn is as it was (2.1.292
        // writes the jobs a resumed session's last process left so, and waits at its prompt).
        if record.transcript_only {
            return Ok(());
        }
        self.open_tools.clear();
        self.hooks.clear();
        self.active_assistant = None;
        self.failure = None;
        self.turn_open = true;
        self.candidate = None;
        self.terminal = None;
        Ok(())
    }

    /// `lateAncestor`: whether the user record at `place` is one of the
    /// records that came before the answer that already ended the turn.
    fn late_ancestor(&mut self, user: Option<usize>) -> bool {
        let (Some(Terminal::Derived { item_id, uuid, .. }), Some(candidate)) =
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

    /// Whether the user's record `record` is the cancel of the compaction the
    /// bare `/compact` `typed` began: the words before a command, a sibling of
    /// that bare command (the same parent). The bare command is forgotten at
    /// the next record of the user's or the assistant's (`typed_compact`), so
    /// nothing came between them but Claude Code's own bookkeeping.
    fn cancels(typed: &TypedCompact, record: &User) -> bool {
        matches!(&record.compacting, Some((Compacting::Caveat, parent)) if *parent == typed.parent)
    }

    /// What a command printed, written as the system's record, that follows
    /// the command with nothing open: a native end of the command's turn, as
    /// the user's record of it is (`user`). A `/clear`'s says nothing; a
    /// `/compact` that found nothing to compact (Claude Code 2.1.304, the live
    /// run of 2026-10-20) or a `/reload-skills` says what it did. `parent` is
    /// the uuid of the record the output is a child of.
    fn output(&mut self, parent: &str) {
        if self.follows_its_command(parent) {
            self.ended_natively();
        }
    }

    /// Whether an output, a child of the record `parent`, follows the command
    /// it is the output of, and the command runs no model after it: the user's
    /// turn last pushed is that record, one of the commands Claude Code does by
    /// itself (`QUIET_COMMAND`), and nothing is open. What another command
    /// printed is a prompt's words, as Node read it: the model may be asked
    /// about it next (2.1.303's `/plan <description>`).
    fn follows_its_command(&self, parent: &str) -> bool {
        self.items.last().is_some_and(|last| {
            last.role == Role::User
                && parent == &*last.id
                && QUIET_COMMAND.is_match(&last.text)
                && self.open_tools.is_empty()
                && self.hooks.is_empty()
        })
    }

    /// The turn ended as its record says, with no answer to settle on.
    fn ended_natively(&mut self) {
        self.turn_open = false;
        self.candidate = None;
        self.terminal = Some(Terminal::Native);
    }

    /// The boundary record that says a turn that answered is over, by its
    /// uuid when that is text, and how many agents it says are still running
    /// when it says. A boundary that says nothing of them (a stop hook's summary)
    /// leaves what an earlier one of the same turn said.
    fn boundary(&mut self, boundary: &Boundary) {
        let Some(candidate) = &self.candidate else {
            return;
        };
        let item_id = Arc::clone(&candidate.item_id);
        self.hooks.remove(&item_id);
        self.items.complete(&item_id);
        self.turn_open = false;
        // A record of the turn that follows is the turn's own: the terminal is
        // none until its boundary, so what an earlier one said is this turn's.
        let said = match self.terminal {
            Some(Terminal::Derived { agents, .. }) => agents,
            _ => 0,
        };
        self.terminal = Some(Terminal::Derived {
            item_id,
            uuid: boundary.uuid.clone(),
            agents: boundary.agents.unwrap_or(said),
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

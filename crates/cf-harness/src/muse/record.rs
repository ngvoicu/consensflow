//! Muse Code's record of a session: its log, `session.jsonl` in the session's folder
//! (`muse::paths`), an event log of JSON lines. A look's answer is the log's alone.
//!
//! The first line is a frame (`retained_frame`, the log's own bookkeeping), and every
//! other line an envelope: `{schema_version, id, stream, sequence, recorded_at (µs),
//! record_type, durability, causation_id, payload_type, payload}`. What it says of the
//! conversation, as Muse 1.4.13 writes it on its own fake model (`--provider echo`;
//! the fixtures of `tests/fixtures/muse`, T-703):
//!
//! - `runtime.user_intent.accepted`: a message of the user's ([`Role::User`]), by its
//!   `intent_id`, its words in `refill_blocks`; a message pasted while a turn runs is
//!   one too, at once (`wake_policy: do_not_wake`), and the run that later takes it up
//!   has its id and no prompt of its own;
//! - a run's events (`payload_type: runtime.session`, `payload.kind: run`, by
//!   `run_id`): `started` begins a turn; `assistant_message_committed` is the agent's
//!   message ([`Role::Assistant`], by its `message_id`); `context_block_updated` is
//!   what Muse or a hook put in the model's context ([`Role::Custom`], by the record's
//!   id), and `hook_run_terminal` a hook's run, an item only where it says something;
//!   `terminal` ends the turn (`completed`, `cancelled`, `failed`, with a `reason`),
//!   and `run_retracted` after a `cancelled` one (`no_output_interrupt_restore`) says
//!   the turn was stopped before a word of it and its message is back in the box;
//! - `session.end` closes a process's segment of the log, and none is written where
//!   the process was killed. It decides nothing here: a quit at work ends the turn
//!   first (`terminal`, then `session.end`), and a resume after a kill writes a
//!   `session.end` for the dead process (`crash_inferred`) and ends its turn itself
//!   (`terminal cancelled`, `resume_reconcile:orphaned_by_process_loss`).
//!
//! The latest turn decides. It is in flight while it has started and not ended (a
//! window at work, or one whose process was cut: no window open, that is a cut turn),
//! or holds a tool call or a question open; settled once it ended. A turn that failed
//! is settled and failed, its reason the failure. A turn cancelled and taken back is
//! settled and [`Record::took_back`]: its message stays among the items, as Claude's
//! does, where Claude's reader cannot tell it and the window's look infers it from the
//! interrupt it pressed (`claude::stopped`). The turn a resume ended for a dead process
//! is settled, and no one's words, as the close Claude Code writes of a cut turn is
//! no one's answer.
//!
//! What a turn with tools, questions, approvals, compaction, background work or a
//! refusal for quota writes was never seen (the Mac's Muse login runs no model turn):
//! its reading is the `provisional` module's, from the shapes Muse's exported schema and
//! its own session reader name, until the live fixtures of MUS-02 replace them.
//!
//! A log written by a Muse older than [`version::MINIMUM`] (its
//! `runtime.session.metadata`, `build.semver`) is not read: its shapes are not known.

mod provisional;

use std::collections::HashMap;
use std::sync::Arc;

use cf_base::env::Env;
use cf_base::time;
use serde_json::Value;

use self::provisional::Provisional;
use super::paths::transcript;
use super::version::{self, Version};
use crate::shared::record::cache::Look;
use crate::shared::record::followed::{Answer, Followed, Parser, TranscriptReader};
use crate::shared::record::jsonl::Stop;
use crate::shared::record::reading::{native_id, null_record, Item, Record, Role, Settlement};

/// The reader of the session `session`, in the Muse data folder `env` names.
pub fn reader(session: &str, env: &Env) -> Box<dyn Look + Send> {
    let locating = session.to_owned();
    let env = env.clone();
    let reading: Arc<str> = Arc::from(session);
    Box::new(TranscriptReader::new(
        Followed::new(
            Box::new(move || transcript(&locating, &env)),
            Box::new(move || Log::new(Arc::clone(&reading))),
        ),
        format!("muse session log for {session}"),
    ))
}

/// What a log said so far.
struct Log {
    session: Arc<str>,
    /// Every item, in the order the log first named it.
    items: Vec<Kept>,
    /// Where each item is among them, by its native id.
    places: HashMap<Arc<str>, usize>,
    runs: HashMap<Arc<str>, Run>,
    /// The run the last `started` began.
    latest: Option<Arc<str>>,
    /// The envelopes read, the frame aside.
    records: usize,
    /// The place among the records of the one being read.
    reading: usize,
    /// What only the schema says of: tools, questions, background work, quota.
    provisional: Provisional,
}

/// An item as the log left it, and the place among its records of the record that
/// first named it.
struct Kept {
    item: Item,
    began: usize,
}

/// A turn, as the log told it.
#[derive(Default)]
struct Run {
    end: Option<End>,
    /// Muse put its message back in the box (`run_retracted`).
    retracted: bool,
}

/// How a turn ended (`terminal`).
enum End {
    Completed,
    Cancelled,
    Failed { reason: Option<String> },
}

impl Log {
    fn new(session: Arc<str>) -> Self {
        Self {
            session,
            items: Vec::new(),
            places: HashMap::new(),
            runs: HashMap::new(),
            latest: None,
            records: 0,
            reading: 0,
            provisional: Provisional::default(),
        }
    }

    /// An envelope: what its `payload_type` says.
    fn envelope(&mut self, record: &Value, at: &Value, seq: &Value) -> Result<(), Stop> {
        let payload = record.get("payload");
        match record.get("payload_type").and_then(Value::as_str) {
            Some("runtime.session.metadata") => built(payload),
            Some("runtime.user_intent.accepted") => self.intent(payload, at, seq),
            Some("runtime.session") => match field(payload, "kind").and_then(Value::as_str) {
                Some("run") => self.run_event(record, payload, at, seq),
                Some("task") => {
                    let kind = field(field(payload, "event"), "kind").and_then(Value::as_str);
                    self.provisional.task_event(field(payload, "task_id"), kind);
                    Ok(())
                }
                _ => Ok(()),
            },
            _ => Ok(()),
        }
    }

    /// A message handed to the session: the user's when it is a chat of the main
    /// surface, else the harness's own.
    fn intent(&mut self, payload: Option<&Value>, at: &Value, seq: &Value) -> Result<(), Stop> {
        let kind = field(field(payload, "semantic_kind"), "kind").and_then(Value::as_str);
        let surface = field(payload, "surface").and_then(Value::as_str);
        // Provisional (schema only, MUS-02): a message of another kind or surface
        // (a slash command's, a side chat's) was never seen; it is Muse's own.
        let role = if kind == Some("chat") && surface == Some("main") {
            Role::User
        } else {
            Role::Custom
        };
        let text = blocks_text(field(payload, "refill_blocks"));
        if text.trim().is_empty() {
            return Ok(());
        }
        let id =
            native_id(field(payload, "intent_id"), "muse intent", seq).map_err(Stop::Failed)?;
        self.add(id, role, text, at);
        Ok(())
    }

    /// An event of a run.
    fn run_event(
        &mut self,
        record: &Value,
        payload: Option<&Value>,
        at: &Value,
        seq: &Value,
    ) -> Result<(), Stop> {
        let run: Arc<str> =
            native_id(field(payload, "run_id"), "muse run", seq).map_err(Stop::Failed)?;
        let event = field(payload, "event");
        let text = || {
            field(event, "text")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned()
        };
        match field(event, "kind").and_then(Value::as_str) {
            Some("started") => {
                self.runs.entry(Arc::clone(&run)).or_default();
                self.latest = Some(run);
            }
            Some("assistant_message_committed") => {
                let text = text();
                if !text.trim().is_empty() {
                    let id = native_id(field(event, "message_id"), "muse message", seq)
                        .map_err(Stop::Failed)?;
                    self.add(id, Role::Assistant, text, at);
                }
            }
            Some("context_block_updated") => {
                let text = text();
                if !text.trim().is_empty() {
                    let id =
                        native_id(record.get("id"), "muse record", seq).map_err(Stop::Failed)?;
                    self.add(id, Role::Custom, text, at);
                }
            }
            Some("hook_run_terminal") => {
                let said = ["stdout", "stderr", "error"]
                    .iter()
                    .filter_map(|name| field(event, name).and_then(Value::as_str))
                    .filter(|said| !said.trim().is_empty())
                    .collect::<Vec<_>>()
                    .join("\n");
                if !said.is_empty() {
                    let id =
                        native_id(record.get("id"), "muse record", seq).map_err(Stop::Failed)?;
                    self.add(id, Role::Custom, said, at);
                }
            }
            Some("terminal") => {
                let reason = field(event, "reason").and_then(Value::as_str);
                let end = match field(event, "terminal").and_then(Value::as_str) {
                    Some("completed") => End::Completed,
                    // Muse's own end of a turn whose process was gone, at a resume
                    // (`resume_reconcile:orphaned_by_process_loss`), is one too.
                    Some("cancelled") => End::Cancelled,
                    // An end Muse may add later ("wire-open") ends the turn as a failure
                    // with no words: it is over, and not known to have answered.
                    _ => End::Failed {
                        reason: reason
                            .filter(|reason| !reason.trim().is_empty())
                            .map(str::to_owned),
                    },
                };
                if let End::Failed { reason } = &end {
                    self.provisional.failed(&run, reason.as_deref(), at);
                }
                self.runs.entry(run).or_default().end = Some(end);
            }
            Some("run_retracted") => {
                self.runs.entry(run).or_default().retracted = true;
            }
            Some(kind) => {
                let said = self.provisional.event(kind, &run, record, event, at, seq)?;
                for (id, role, text, compaction) in said {
                    let place = self.add(id, role, text, at);
                    self.items[place].item.compaction |= compaction;
                }
            }
            None => {}
        }
        Ok(())
    }

    /// The item `id` names, added, or brought up to date: its text the latest the log
    /// gave it, its time the latest record's, its role the one it was first given.
    fn add(&mut self, id: Arc<str>, role: Role, text: String, at: &Value) -> usize {
        if let Some(&place) = self.places.get(&id) {
            let kept = &mut self.items[place].item;
            kept.text = Arc::from(text);
            kept.at = Some(at.clone());
            return place;
        }
        let place = self.items.len();
        self.places.insert(Arc::clone(&id), place);
        self.items.push(Kept {
            item: Item {
                id,
                role,
                text: Arc::from(text),
                complete: true,
                at: Some(at.clone()),
                commentary: false,
                compaction: false,
            },
            began: self.reading,
        });
        place
    }
}

/// A log written by a Muse older than the minimum fails the look; one that names no
/// version is read.
fn built(payload: Option<&Value>) -> Result<(), Stop> {
    let semver = field(field(field(payload, "record"), "build"), "semver").and_then(Value::as_str);
    match semver.and_then(Version::of) {
        Some(found) if !found.supported() => Err(Stop::Failed(format!(
            "written by Muse Code {}, older than {}, whose record ConsensFlow does not read",
            found.semver(),
            version::minimum()
        ))),
        _ => Ok(()),
    }
}

/// `value.name`, where `value` is an object that has it.
pub(super) fn field<'a>(value: Option<&'a Value>, name: &str) -> Option<&'a Value> {
    value.and_then(|value| value.get(name))
}

/// The words of a list of blocks (`refill_blocks`): each text block's text, on a line
/// of its own.
fn blocks_text(blocks: Option<&Value>) -> String {
    blocks
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|block| field(Some(block), "kind").and_then(Value::as_str) == Some("text"))
        .filter_map(|block| block.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n")
}

/// When a record was written, as the readings say a time: `recorded_at`'s
/// microseconds as the instant's ISO text, else the record's place.
fn recorded_at(record: &Value, seq: &Value) -> Value {
    record
        .get("recorded_at")
        .and_then(Value::as_i64)
        .and_then(|micros| time::time_clip((micros / 1000) as f64))
        .map_or_else(|| seq.clone(), |ms| Value::from(time::iso(ms)))
}

impl Parser for Log {
    fn visit(&mut self, record: Value, index: usize) -> Result<(), Stop> {
        if record.is_null() {
            return Err(Stop::Failed(null_record(index)));
        }
        // The frame that begins a log is its bookkeeping, and no record of the session.
        if record.get("retained_frame").is_some() {
            return Ok(());
        }
        self.records += 1;
        self.reading = index;
        let seq = match record.get("sequence") {
            Some(Value::Number(sequence)) if sequence.is_u64() => Value::Number(sequence.clone()),
            _ => Value::from(index),
        };
        let at = recorded_at(&record, &seq);
        self.envelope(&record, &at, &seq)
    }
}

impl Answer for Log {
    fn result(&self) -> Result<Record, String> {
        if self.records == 0 {
            return Err(format!("empty muse session log for {}", self.session));
        }
        let latest = self
            .latest
            .as_ref()
            .and_then(|run| Some((run, self.runs.get(run)?)));
        let open = latest.is_some_and(|(run, _)| self.provisional.holds_open(run));
        let in_flight = latest.is_some_and(|(_, turn)| turn.end.is_none()) || open;
        let end = latest.and_then(|(_, turn)| turn.end.as_ref());
        let mut record = Record::new();
        record.items = self.items.iter().map(|kept| kept.item.clone()).collect();
        record.lines.places = Some(self.items.iter().map(|kept| kept.began).collect());
        record.in_flight = in_flight;
        record.asking = latest.is_some_and(|(run, _)| self.provisional.asking(run));
        if let Some(End::Failed { reason }) = end {
            record.failed = true;
            record.failure.clone_from(reason);
        }
        record.took_back =
            matches!(end, Some(End::Cancelled)) && latest.is_some_and(|(_, turn)| turn.retracted);
        record.quota = latest.and_then(|(run, _)| self.provisional.quota(run));
        record.pending = self.provisional.pending();
        record.settlement = match end {
            _ if in_flight => Settlement::InFlight,
            Some(_) => Settlement::Settled,
            None => Settlement::Unknown,
        };
        Ok(record)
    }
}

#[cfg(test)]
mod tests;

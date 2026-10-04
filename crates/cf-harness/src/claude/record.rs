//! Claude Code's record of a session (`claudeReader` and `claudeParser`,
//! `hosts/lib/completion/claude-code.js`): its transcript, a JSONL file in
//! its `projects` folder. A look's answer is the transcript's alone.
//!
//! The transcript says what the session did in records of several types: the
//! user's turns, the assistant's messages (each written as one record or
//! several, its fragments), the operations on Claude Code's queue of
//! messages, the attachments a hook added, and the system's own records of a
//! turn's end.
//!
//! A turn is settled once the transcript says it ended, with no tool call or
//! queued message open: natively (an API error, an interrupt, a `/clear`), or
//! by the boundary record a turn that answered writes after its stop hooks
//! ran (a `turn_duration`, or a `stop_hook_summary` that did not prevent
//! continuation). A turn that has begun and not ended is in flight.
//!
//! A look's records wait until all of them are in, and are then replayed in
//! order, so that a decision may rest on a record that comes after the one it
//! is about (`ancestry`). A record read later that such a decision looked up
//! has the whole transcript read again.
//!
//! The transcript's ids key the reader's sets as they keyed Node's (see
//! [`Key`]), so a record that says something odd is read as Node read it.

mod ancestry;
mod items;
mod patterns;
mod queues;
mod replay;
#[cfg(test)]
mod tests;
mod text;

use std::collections::HashSet;
use std::sync::Arc;

use cf_base::env::Env;
use jiff::tz::TimeZone;
use serde_json::Value;

use super::paths::transcript;
use crate::shared::quota::Quota;
use crate::shared::record::cache::Look;
use crate::shared::record::followed::{Answer, Followed, Parser, TranscriptReader};
use crate::shared::record::jsonl::Stop;
use crate::shared::record::key::{Key, Keys};
use crate::shared::record::reading::{null_record, Record, Settlement};
use ancestry::Ancestry;
use items::Items;
use queues::Queues;

/// The reader of the session `session`, in the Claude Code folders `env`
/// names. A reset a refusal names at a time of day, and in no zone, is read
/// in `local`.
pub fn reader(session: &str, env: &Env, local: &TimeZone) -> Box<dyn Look + Send> {
    let locating = session.to_owned();
    let env = env.clone();
    let reading: Arc<str> = Arc::from(session);
    let local = local.clone();
    Box::new(TranscriptReader::new(
        Followed::new(
            Box::new(move || transcript(&locating, &env)),
            Box::new(move || Transcript::new(Arc::clone(&reading), local.clone())),
        ),
        format!("claude session {session}"),
    ))
}

/// What a transcript said so far.
struct Transcript {
    session: Arc<str>,
    /// The zone a reset that names none is read in.
    local: TimeZone,
    items: Items,
    /// The tool calls made and not yet answered in the turn, by the id of the
    /// block that made each.
    open_tools: HashSet<Key>,
    queues: Queues,
    /// The assistant messages that ended their turn, and whose stop hooks
    /// the transcript has not yet said ran, by their item's id.
    hooks: HashSet<Arc<str>>,
    turn_open: bool,
    /// The assistant message that ended the latest turn, unless a later
    /// record says it did not.
    candidate: Option<Candidate>,
    /// How the latest turn ended, if it did.
    terminal: Option<Terminal>,
    /// The native id of the message the latest assistant record belongs to.
    active_assistant: Option<Arc<str>>,
    failed: bool,
    quota: Option<Arc<Quota>>,
    /// The records visited.
    count: usize,
    ancestry: Ancestry,
    /// The records of the look that are not yet replayed.
    pending: Vec<Pending>,
    keys: Keys,
}

/// The assistant message that ended its turn: its item, and its record's uuid.
struct Candidate {
    item_id: Arc<str>,
    uuid: Arc<str>,
}

/// How the latest turn ended.
enum Terminal {
    /// The record says so: an API error, an interrupt, a `/clear`.
    Native,
    /// A boundary record came after the candidate. It names the candidate's
    /// item and its own uuid, when that is text.
    Derived {
        item_id: Arc<str>,
        uuid: Option<Arc<str>>,
    },
}

/// A record visited and not yet replayed, with its place in the tree and
/// among the records.
struct Pending {
    record: Value,
    place: Option<usize>,
    seq: usize,
}

impl Transcript {
    fn new(session: Arc<str>, local: TimeZone) -> Self {
        Self {
            session,
            local,
            items: Items::default(),
            open_tools: HashSet::new(),
            queues: Queues::default(),
            hooks: HashSet::new(),
            turn_open: false,
            candidate: None,
            terminal: None,
            active_assistant: None,
            failed: false,
            quota: None,
            count: 0,
            ancestry: Ancestry::default(),
            pending: Vec::new(),
            keys: Keys::default(),
        }
    }

    /// `candidateCanSettle`: the turn ended at a boundary record, with
    /// nothing left open.
    fn candidate_can_settle(&self) -> bool {
        matches!(self.terminal, Some(Terminal::Derived { .. }))
            && self.open_tools.is_empty()
            && self.queues.turns() == 0
            && self.hooks.is_empty()
    }

    /// Whether the turn is settled, in flight, or unknown, in that order of
    /// what the transcript proves: settled when it ended, natively or at a
    /// boundary record, with nothing open.
    fn settlement(&self) -> Settlement {
        let open = !self.open_tools.is_empty() || self.queues.turns() > 0;
        let ended_natively = matches!(self.terminal, Some(Terminal::Native)) && !open;
        if ended_natively || self.candidate_can_settle() {
            Settlement::Settled
        } else if self.turn_open || open || !self.hooks.is_empty() || self.terminal.is_some() {
            Settlement::InFlight
        } else {
            Settlement::Unknown
        }
    }
}

impl Parser for Transcript {
    /// A record waits for the rest of its look. Its uuid, when a decision
    /// already looked it up, has the transcript read again.
    fn visit(&mut self, record: Value, index: usize) -> Result<(), Stop> {
        self.count += 1;
        if record.is_null() {
            return Err(Stop::Failed(null_record(index)));
        }
        let place = self.ancestry.place(&record, &self.session)?;
        self.pending.push(Pending {
            record,
            place,
            seq: index,
        });
        Ok(())
    }

    /// The records of the look are all in: they are replayed in order.
    fn flush(&mut self) -> Result<(), String> {
        let records = std::mem::take(&mut self.pending);
        for Pending { record, place, seq } in records {
            self.replay(&record, place, seq)?;
        }
        Ok(())
    }
}

impl Answer for Transcript {
    fn result(&self) -> Result<Record, String> {
        if self.count == 0 {
            return Err(format!("empty claude session {}", self.session));
        }
        let settlement = self.settlement();
        let mut record = Record::new();
        record.items = self.items.sorted()?;
        record.in_flight = settlement == Settlement::InFlight;
        record.failed = self.failed;
        record.quota = self.quota.clone();
        record.settlement = settlement;
        Ok(record)
    }
}

/// A field of a value that may be none (`value?.name`): none of anything
/// that is no object.
fn field<'a>(value: Option<&'a Value>, name: &str) -> Option<&'a Value> {
    value.and_then(|value| value.get(name))
}

/// A record's or a block's `type`, when it is text.
fn kind(value: &Value) -> Option<&str> {
    value.get("type").and_then(Value::as_str)
}

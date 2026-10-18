//! Claude Code's record of a session: its transcript, a JSONL file in its
//! `projects` folder. A look's answer is the transcript's alone.
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
//! A queued message holds the turn until a prompt takes it, and the queue is
//! the process's own, in its memory: what a process had queued and not begun
//! went with it, and no record says so. ConsensFlow knows where a process
//! ended, as it closes a window before it starts the program again on the
//! window's conversation: a look is told how long the transcript was then
//! (`Options::relaunched_at`), and the queue is empty from that byte on, what
//! the new process queued after it kept (`queues`). A reader keeps the latest
//! it was told, as a look that tells none (the engine's own, of a window it
//! does not hold) knows no better. Claude Code's own close of the turn the old
//! process was cut in (`No response requested.`) is no answer: the item is the
//! harness's commentary.
//!
//! A turn that ended may have left background agents or workflows running, which
//! its `turn_duration` says by its counts (`pendingBackgroundAgentCount`,
//! `pendingWorkflowCount`, left out when none). It is settled all the same, and
//! the reading says the agents are pending (`agents_pending`): the window takes
//! input between turns, and the end of an agent wakes a turn of its own, whose
//! record says what is left. (Departs from Node's reader, which took such a
//! record for no end of the turn, on the owner's decision of 2026-10-11.)
//!
//! A look's records wait until all of them are in, and are then replayed in
//! order, so that a decision may rest on a record that comes after the one it
//! is about (`ancestry`). A record read later that such a decision looked up
//! has the whole transcript read again. A record waits as what replay reads
//! of it (`project`), since most of what a transcript holds is read by
//! nobody.
//!
//! The transcript's ids key the reader's sets as they keyed Node's (see
//! [`Key`]), so a record that says something odd is read as Node read it.

mod ancestry;
mod items;
mod keep;
mod patterns;
mod project;
mod queues;
mod replay;
#[cfg(test)]
mod tests;
mod text;

use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use cf_base::env::Env;
use cf_base::json::from_slice_lossy_keeping;
use jiff::tz::TimeZone;
use serde_json::Value;

use super::paths::transcript;
use crate::shared::quota::Quota;
use crate::shared::record::cache::{Look, Options};
use crate::shared::record::followed::{Answer, Followed, Parser, TranscriptReader};
use crate::shared::record::jsonl::Stop;
use crate::shared::record::key::{Key, Keys};
use crate::shared::record::reading::{null_record, Reading, Record, Settlement};
use ancestry::Ancestry;
use items::Items;
use project::{project, Projected};
use queues::Queues;

/// The reader of the session `session`, in the Claude Code folders `env`
/// names. A reset a refusal names at a time of day, and in no zone, is read
/// in `local`.
pub fn reader(session: &str, env: &Env, local: &TimeZone) -> Box<dyn Look + Send> {
    let locating = session.to_owned();
    let env = env.clone();
    let reading: Arc<str> = Arc::from(session);
    let local = local.clone();
    let relaunched = Arc::new(AtomicU64::new(0));
    let told = Arc::clone(&relaunched);
    Box::new(Relaunched {
        reader: TranscriptReader::new(
            Followed::new(
                Box::new(move || transcript(&locating, &env)),
                Box::new(move || {
                    let at = told.load(Ordering::Relaxed);
                    Transcript::new(Arc::clone(&reading), local.clone(), at)
                }),
            ),
            format!("claude session {session}"),
        ),
        at: relaunched,
    })
}

/// A transcript's reader, and the byte the latest process ConsensFlow started
/// on its conversation began at (0 for none told), which a state made afresh
/// begins with.
struct Relaunched {
    reader: TranscriptReader<Transcript>,
    at: Arc<AtomicU64>,
}

impl Look for Relaunched {
    /// A look told of a later start than the reader knows has its state told
    /// too, or read again from the transcript's start where it read past it.
    fn look(&mut self, options: &Options, now_ms: i64) -> Arc<Reading> {
        if let Some(at) = options
            .relaunched_at
            .filter(|at| *at > self.at.load(Ordering::Relaxed))
        {
            self.at.store(at, Ordering::Relaxed);
            self.reader.retell(|state| state.relaunched(at));
        }
        self.reader.look(options, now_ms)
    }
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
    /// Where the latest process ConsensFlow started on the conversation began,
    /// as this state was told.
    relaunch: Option<Relaunch>,
    /// The byte the line of the latest record visited begins at.
    last_at: Option<u64>,
}

/// A process ConsensFlow started on the conversation: the byte the transcript
/// ended at then, and whether replay has reached it, the queue emptied there.
struct Relaunch {
    at: u64,
    reached: bool,
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
    /// item and its own uuid, when that is text, and says whether the turn left
    /// background agents or workflows running.
    Derived {
        item_id: Arc<str>,
        uuid: Option<Arc<str>>,
        agents: bool,
    },
}

/// A record visited and not yet replayed, as replay reads it, with its place
/// in the tree, among the records and in the file.
struct Pending {
    record: Projected,
    place: Option<usize>,
    seq: usize,
    at: u64,
}

impl Transcript {
    /// What the transcript of `session` says before any record, where the
    /// latest process ConsensFlow started on it began at the byte
    /// `relaunched_at` (none where 0).
    fn new(session: Arc<str>, local: TimeZone, relaunched_at: u64) -> Self {
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
            relaunch: (relaunched_at > 0).then_some(Relaunch {
                at: relaunched_at,
                reached: false,
            }),
            last_at: None,
        }
    }

    /// Told that the latest process ConsensFlow started on the conversation
    /// began at the byte `at`: false where a record from there on was read
    /// already, which only a read from the start can tell apart.
    fn relaunched(&mut self, at: u64) -> bool {
        if self.last_at.is_some_and(|last| last >= at) {
            return false;
        }
        self.relaunch = Some(Relaunch { at, reached: false });
        true
    }

    /// Replay is at the byte `at`: the process that wrote what came before
    /// the latest relaunch is gone, and so is what it had queued, and the
    /// background agents its last turn left running. Its last answer is no
    /// candidate of what follows: no record past the relaunch ends that turn
    /// again, or says agents of it run. What a message of another's would have
    /// cleaned of it goes as it would (`replay`'s `assistant`): the calls left
    /// open, and the stop hooks its answer waited on, which never run.
    fn reach(&mut self, at: u64) {
        let Some(relaunch) = self.relaunch.as_mut() else {
            return;
        };
        if !relaunch.reached && relaunch.at <= at {
            relaunch.reached = true;
            self.queues.restart();
            self.open_tools.clear();
            if let Some(candidate) = self.candidate.take() {
                self.hooks.remove(&candidate.item_id);
            }
            if let Some(Terminal::Derived { agents, .. }) = self.terminal.as_mut() {
                *agents = false;
            }
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
    /// A line is built into the members that are read of it, and no more
    /// (`keep`).
    fn parse(line: &[u8]) -> serde_json::Result<Value> {
        from_slice_lossy_keeping(line, &keep::RECORD)
    }

    /// A record whose place in the file is not told, as one at its start.
    fn visit(&mut self, record: Value, index: usize) -> Result<(), Stop> {
        self.visit_at(record, index, 0)
    }

    /// A record waits for the rest of its look, as what replay reads of it.
    /// Its uuid, when a decision already looked it up, has the transcript
    /// read again.
    fn visit_at(&mut self, record: Value, index: usize, at: u64) -> Result<(), Stop> {
        self.count += 1;
        if record.is_null() {
            return Err(Stop::Failed(null_record(index)));
        }
        let place = self.ancestry.place(&record, &self.session)?;
        self.pending.push(Pending {
            record: project(&record, &self.session),
            place,
            seq: index,
            at,
        });
        self.last_at = Some(at);
        Ok(())
    }

    /// The records of the look are all in: they are replayed in order, and
    /// the transcript has been read to its end, which a relaunch the reader
    /// was told is never after: the file was that long then.
    fn flush(&mut self) -> Result<(), String> {
        let records = std::mem::take(&mut self.pending);
        for Pending {
            record,
            place,
            seq,
            at,
        } in records
        {
            self.reach(at);
            self.replay(&record, place, seq)?;
        }
        self.reach(u64::MAX);
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
        // Said of a turn that is over: one with a call open or a message queued
        // is in flight, whatever its duration said.
        record.agents_pending = settlement == Settlement::Settled
            && matches!(self.terminal, Some(Terminal::Derived { agents: true, .. }));
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

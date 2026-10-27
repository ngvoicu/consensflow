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
//! queued message open: natively (an API error, an interrupt, the output of a
//! command typed at Claude's prompt that runs no model, a `/clear`'s or a
//! `/compact`'s, a compaction cancelled with Escape), or
//! by the boundary record a turn that answered writes after its stop hooks
//! ran (a `turn_duration`, or a `stop_hook_summary` that did not prevent
//! continuation). A turn that has begun and not ended is in flight.
//!
//! A queued message holds the turn until a prompt takes it, and the queue is
//! the process's own, in its memory: what a process had queued and not begun
//! went with it, and no record says so. ConsensFlow knows where a process
//! ended, as it closes a window before it starts the program again on the
//! window's conversation: a look is told how long the transcript was then
//! (`Options::relaunched`), and the queue is empty from that byte on, what
//! the new process queued after it kept (`queues`). A reader keeps the latest
//! it was told, as a look that tells none (the engine's own, of a window it
//! does not hold) knows no better. The length holds in the transcript it was
//! taken of, which still has the bytes it was taken after
//! ([`Boundary::held_by`]), checked whenever the transcript is read from its
//! start: one written anew without them has other bytes there, and what the
//! running process wrote in it would be cut, so its boundary is void, and the
//! process's next start tells a boundary of its own, shorter or not (both
//! reviews of round 2 of the relaunch boundary, 2026-10-18). A void boundary
//! clears nothing: what the old process left open holds the turn, and Finish
//! takes the answer. Claude Code's own close of the turn the old
//! process was cut in (`No response requested.`) is no answer: the item is the
//! harness's commentary.
//!
//! A turn that ended may have left background agents, workflows or jobs running,
//! and Claude Code wakes its agent for each as it ends. The agents and workflows
//! are on the `turn_duration` that ends the turn, by its counts
//! (`pendingBackgroundAgentCount`, `pendingWorkflowCount`, left out when none);
//! the jobs, which a Bash call run in the background or a Monitor call started,
//! are not counted there, and the reader counts them itself (`jobs`). The turn is
//! settled all the same, and the reading says how many are pending (`pending`): the
//! window takes input between turns, and the end of one wakes a turn of its own,
//! whose record says what is left. (Departs from Node's reader, which took a
//! `turn_duration` that names agents for no end of the turn, on the owner's
//! decision of 2026-10-11.) The relaunch that empties the queue (above) retires
//! the old process's jobs too: their notifications went with it, and the reading
//! counts them as retired, not as ended (`jobs`).
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
mod jobs;
mod keep;
mod patterns;
mod project;
mod queues;
mod replay;
#[cfg(test)]
mod tests;
mod text;

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};

use cf_base::env::Env;
use cf_base::json::from_slice_lossy_keeping;
use jiff::tz::TimeZone;
use serde_json::Value;

use super::paths::transcript;
use crate::shared::quota::Quota;
use crate::shared::record::boundary::Boundary;
use crate::shared::record::cache::{Look, Options};
use crate::shared::record::followed::{Answer, Followed, Parser, TranscriptReader};
use crate::shared::record::jsonl::Stop;
use crate::shared::record::key::{Key, Keys};
use crate::shared::record::reading::{self, null_record, Reading, Record, Settlement};
use ancestry::Ancestry;
use items::Items;
use jobs::Jobs;
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
    let relaunched = Arc::new(Mutex::new(None));
    let told = Arc::clone(&relaunched);
    Box::new(Relaunched {
        reader: TranscriptReader::new(
            Followed::new(
                Box::new(move || transcript(&locating, &env)),
                Box::new(move |file: &Path| {
                    let at = checked(lock(&told).as_ref(), file);
                    Transcript::new(Arc::clone(&reading), local.clone(), at)
                }),
            ),
            format!("claude session {session}"),
        ),
        told: relaunched,
    })
}

/// A transcript's reader, and where the latest process ConsensFlow started on
/// its conversation began (none told), which a state made afresh begins with
/// where the transcript it reads holds it.
struct Relaunched {
    reader: TranscriptReader<Transcript>,
    told: Arc<Mutex<Option<Boundary>>>,
}

impl Look for Relaunched {
    /// A look told of a later start than the reader knows, or of any where the
    /// one it knows no longer holds, has its state told too, or read again from
    /// the transcript's start where it read past it. An earlier start told late
    /// changes nothing: in a transcript that holds the later one, it is before it.
    fn look(&mut self, options: &Options, now_ms: i64) -> Arc<Reading> {
        if let Some(boundary) = options.relaunched.as_ref() {
            let mut told = lock(&self.told);
            let later = match told.as_ref() {
                None => true,
                Some(known) => boundary.at > known.at || !self.reader.holds(known),
            };
            if later && told.as_ref() != Some(boundary) {
                *told = Some(boundary.clone());
                drop(told);
                // One the transcript does not hold is read from its start, where the
                // state made afresh is told none.
                self.reader.retell(|state, file| {
                    checked(Some(boundary), file).is_some_and(|at| state.relaunched(at))
                });
            }
        }
        self.reader.look(options, now_ms)
    }
}

/// The byte the boundary `told` is at, where `file` holds it: none where it was
/// taken of a transcript `file` is no longer, or none was told.
fn checked(told: Option<&Boundary>, file: &Path) -> Option<u64> {
    told.filter(|told| told.at > 0 && told.held_by(file))
        .map(|told| told.at)
}

/// What `told` holds, a panic of another holder's notwithstanding.
fn lock(told: &Mutex<Option<Boundary>>) -> std::sync::MutexGuard<'_, Option<Boundary>> {
    told.lock().unwrap_or_else(PoisonError::into_inner)
}

impl TranscriptReader<Transcript> {
    /// Whether the transcript the reader reads holds `boundary`, so far as it
    /// knows: one it has not found yet is not told otherwise.
    fn holds(&self, boundary: &Boundary) -> bool {
        self.file().is_none_or(|file| boundary.held_by(file))
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
    /// The TaskStop calls made in the main conversation, by the id of the block that
    /// made each: the result of one says the job it stopped.
    task_stops: HashSet<Key>,
    /// The Monitor calls made in the main conversation, by the id of the block that made
    /// each, and what each watches: the result of one says the monitor it armed.
    monitors: HashMap<Key, jobs::Watch>,
    queues: Queues,
    /// The jobs started in the background that Claude Code has said no end of.
    jobs: Jobs,
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
    /// The latest turn failed: the API refused it, in these words.
    failure: Option<String>,
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
    /// The bare `/compact` that began the turn, while nothing has moved it since:
    /// what a cancel of the compaction ends.
    typed_compact: Option<TypedCompact>,
    /// The turn ended at such a cancel, and Claude Code put the `/compact` back in
    /// its input box (`Lines::put_back`), until a record moves the turn again.
    put_back: bool,
}

/// A `/compact` as it was typed: the record it is a child of.
struct TypedCompact {
    parent: Arc<str>,
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
    /// The record says so: an API error, an interrupt, the output of a command
    /// that runs no model.
    Native,
    /// A boundary record came after the candidate. It names the candidate's
    /// item and its own uuid, when that is text, and says how many background
    /// agents and workflows the turn left running.
    Derived {
        item_id: Arc<str>,
        uuid: Option<Arc<str>>,
        agents: usize,
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
    /// `relaunched_at`, if one did where it holds.
    fn new(session: Arc<str>, local: TimeZone, relaunched_at: Option<u64>) -> Self {
        Self {
            session,
            local,
            items: Items::default(),
            open_tools: HashSet::new(),
            task_stops: HashSet::new(),
            monitors: HashMap::new(),
            queues: Queues::default(),
            jobs: Jobs::default(),
            hooks: HashSet::new(),
            turn_open: false,
            candidate: None,
            terminal: None,
            active_assistant: None,
            failure: None,
            quota: None,
            count: 0,
            ancestry: Ancestry::default(),
            pending: Vec::new(),
            keys: Keys::default(),
            relaunch: relaunched_at.map(|at| Relaunch { at, reached: false }),
            last_at: None,
            typed_compact: None,
            put_back: false,
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
    /// background agents its last turn left running; the jobs it started are
    /// retired, as no notification of theirs will come (`jobs`). Its last answer is no
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
            self.jobs.retire();
            self.open_tools.clear();
            // The process that cancelled a compaction, and its input box, are gone.
            self.typed_compact = None;
            self.put_back = false;
            if let Some(candidate) = self.candidate.take() {
                self.hooks.remove(&candidate.item_id);
            }
            if let Some(Terminal::Derived { agents, .. }) = self.terminal.as_mut() {
                *agents = 0;
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
        let (items, places) = self.items.sorted()?;
        record.items = items;
        record.lines.places = Some(places);
        record.in_flight = settlement == Settlement::InFlight;
        record.failed = self.failure.is_some();
        record.failure = self.failure.clone().filter(|said| !said.trim().is_empty());
        record.quota = self.quota.clone();
        record.settlement = settlement;
        // The jobs are those Claude Code has said no end of, whenever the record is
        // read: Finish takes a turn still under way, and says what runs (Astraeus on
        // 0eec91d2: a reading in flight said none). The agents are the word of the
        // boundary that ends a turn, and said of a turn that is over alone: one with a
        // call open or a message queued is in flight, whatever its duration said.
        let agents = match (settlement, &self.terminal) {
            (Settlement::Settled, Some(Terminal::Derived { agents, .. })) => *agents,
            _ => 0,
        };
        record.pending = reading::Pending {
            agents,
            jobs: self.jobs.running(),
            retired: self.jobs.retired(),
            noticed: self.jobs.noticed_count(),
        };
        record.monitors = self.jobs.monitors();
        record.lines.put_back = self.put_back && settlement == Settlement::Settled;
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

//! Pi's record of a session: its JSONL session file, and the word ConsensFlow's extension
//! writes beside it (the `evidence` module): the brain's `architecture/pi-turn-state.md`.
//!
//! **Whether a turn runs is Pi's own word.** The extension follows Pi's run state and writes
//! it as one word (`working`, `busy`, `rest`) with a count of the runs that ended and the last
//! of them: each its leaf and Pi's own flag of an abort. Where that word is there the session
//! file never decides whether a turn runs: `working` is at work; `busy` is neither at rest nor
//! at work (a compaction, a prompt in hand: nothing is interrupted, taken as a result or sent);
//! `rest` is at rest once the file as read holds the leaf it names (a session with no file yet is
//! at rest with nothing to read). A run that ended is never missed: each look reports the oldest
//! end it has not reported, read up to that run's own leaf, whatever the word says now, one look
//! for each, in order.
//!
//! **The file gives the words, and how a run ended:** the last assistant step at or before the
//! run's leaf since the last user message. `stop` is its answer; `length` and `deferred` failed
//! with their cause; `aborted` was interrupted; `error` was interrupted where Pi's flag says it
//! aborted, or where Pi's own omission names that very step (a `context_edit` of it with no
//! replacement, and no step after it: a retry cancelled in its backoff), and failed otherwise; a
//! `toolUse` step, a tool's result or no step ended with no answer, interrupted. A call in a step
//! that ended `aborted` or `error` never ran: never open, never named. A step Pi omitted is left
//! out of the answer's words. An entry that asks the model nothing changes nothing.
//!
//! **Without the word** (the extension did not load, a window an older ConsensFlow opened, the
//! moment before its first word) the file decides by time alone. A run is open while the last
//! message is the user's or a tool's result, or a call is not answered; else the window is at
//! rest [`QUIET_MS`] after the last entry a request can follow, by that entry's own time, and
//! until then neither at rest nor at work (`settles_at`): a stop presses no key for it.
//!
//! Pi's reader is no `TranscriptReader`: its answer also rests on the word and on the clock,
//! which change while the transcript does not, so every look works its answer out anew, and
//! returns a new reading. A refusal's quota stays the same object until the transcript
//! changes it.
//!
//! Kept from Node on purpose: the look's options are `piSettlement` alone (Node also read the
//! alias `options.pi`, which no caller passes). The session's records key its set of open tool
//! calls as they keyed Node's (see `Key`).

pub(crate) mod evidence;

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use cf_base::env::Env;
use cf_base::file::stat;
use cf_base::js;
use jiff::tz::TimeZone;
use serde_json::Value;

use self::evidence::{State, Word};
use super::paths::transcript;
use crate::shared::quota::{
    account_refusal, busy_quota, busy_refusal, exhausted_quota, refused_for_quota,
    unavailable_quota, Quota,
};
use crate::shared::record::cache::{Look, Options};
use crate::shared::record::calls::{describe, LastCall};
use crate::shared::record::drawn::{self, Action, Detail, Kind, Status};
use crate::shared::record::followed::{Followed, Parser};
use crate::shared::record::jsonl::Stop;
use crate::shared::record::key::{Key, Keys};
use crate::shared::record::reading::{
    native_id, null_record, Item, Reading, Record, Role, Settlement,
};

/// How long after the last entry a request can follow, in milliseconds, a window with no word
/// of the extension's is at rest.
const QUIET_MS: i64 = 120_000;

/// The reader of the session `session`, in the Pi folders `env` names. A
/// reset a refusal names at a time of day, and in no zone, is read in
/// `local`.
pub fn reader(session: &str, env: &Env, local: &TimeZone) -> Box<dyn Look + Send> {
    let locating = session.to_owned();
    let searched = env.clone();
    let reading: Arc<str> = Arc::from(session);
    let local = local.clone();
    Box::new(Reader {
        session: Arc::clone(&reading),
        env: env.clone(),
        transcript: Followed::new(
            Box::new(move || transcript(&locating, &searched)),
            Box::new(move |_: &Path| Session::new(Arc::clone(&reading), local.clone())),
        ),
        taken: None,
    })
}

/// A session's reader: its transcript followed, the environment that says where the
/// extension's word is, and the ends of runs it has reported.
struct Reader {
    session: Arc<str>,
    env: Env,
    transcript: Followed<Session>,
    /// The launch whose word the reader read, and how many of its runs' ends it has reported. A
    /// reader made new (the daemon restarted, a reader forgotten unread) starts from the word's
    /// count.
    taken: Option<(String, u64)>,
}

impl Look for Reader {
    fn look(&mut self, options: &Options, now_ms: i64) -> Arc<Reading> {
        Arc::new(
            self.read(options, now_ms)
                .unwrap_or_else(|reason| Reading::unreadable(&reason)),
        )
    }
}

/// What a look says of the turn.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Turn {
    /// At work: by the extension's word (`said`), or by a file that is behind the word.
    AtWork { said: bool },
    /// The word says `busy`: neither at rest nor at work.
    Busy,
    /// A run ended, read up to the leaf `upto` (the file's end where none), with Pi's flag.
    Ended {
        upto: Option<Arc<str>>,
        flag: Option<bool>,
    },
    /// No word: the file leaves a run open.
    Open,
    /// No word: the file ended a run, and is at rest at `settles_at` if nothing moves the clock.
    Quiet { settles_at: i64 },
}

impl Reader {
    /// What the session says now, or why it cannot be read.
    fn read(&mut self, options: &Options, now_ms: i64) -> Result<Reading, String> {
        let word = evidence::word(&self.session, &self.env, options)?;
        let read = self.transcript.read(now_ms).map_err(|stop| stop.reason())?;
        let Some(word) = word else {
            return match read {
                Some(read) => {
                    let mut record = read.state.unworded(now_ms, &read.file)?;
                    record.lines.lost = read.lost;
                    Ok(Reading::Known(record))
                }
                None => Ok(Reading::missing(&format!("pi session {}", self.session))),
            };
        };
        let turn = turn(
            &mut self.taken,
            &word,
            read.as_ref().map(|read| &*read.state),
        );
        Ok(Reading::Known(match read {
            Some(read) => {
                if read.state.records == 0 {
                    return Err(format!("empty pi session {}", self.session));
                }
                let mut record = read.state.reading(&turn);
                record.lines.lost = read.lost;
                record
            }
            // Pi writes a session's file at its first conversation: nothing to read yet.
            None => Session::nothing(&turn),
        }))
    }
}

/// The turn the word says, the file `session` as read (none where there is no file yet): the
/// oldest end not yet reported (`taken` counts those reported) first, one a look.
fn turn(taken_so_far: &mut Option<(String, u64)>, word: &Word, session: Option<&Session>) -> Turn {
    let holds = |leaf: Option<&String>| match (leaf, session) {
        (Some(leaf), Some(session)) => session.marks.contains_key(leaf.as_str()),
        _ => true,
    };
    let taken = match &*taken_so_far {
        Some((launch, taken)) if *launch == word.launch => (*taken).min(word.ended),
        _ => word.ended,
    };
    if word.ended > taken {
        // The oldest end kept that is not reported; more than the word keeps between two
        // looks are merged into the oldest kept.
        if let Some(end) = word.ends.iter().find(|end| end.n > taken) {
            if !holds(end.leaf.as_ref()) {
                *taken_so_far = Some((word.launch.clone(), taken));
                return Turn::AtWork { said: false };
            }
            *taken_so_far = Some((word.launch.clone(), end.n));
            return Turn::Ended {
                upto: end.leaf.as_deref().map(Arc::from),
                flag: end.aborted,
            };
        }
    }
    *taken_so_far = Some((word.launch.clone(), word.ended));
    match word.state {
        State::Working => Turn::AtWork { said: true },
        State::Busy => Turn::Busy,
        State::Rest if !holds(word.leaf.as_ref()) => Turn::AtWork { said: false },
        State::Rest => Turn::Ended {
            upto: word.leaf.as_deref().map(Arc::from),
            flag: word
                .ends
                .last()
                .filter(|end| end.n == word.ended)
                .and_then(|end| end.aborted),
        },
    }
}

/// The last assistant step since the last user message, and what it says of the run's end.
#[derive(Clone)]
struct Step {
    id: Arc<str>,
    /// Its `stopReason`.
    stop: Option<Arc<str>>,
    /// Why it failed, where Pi said (`errorMessage`).
    failure: Option<String>,
    /// ConsensFlow's plain description of a stop that returned no whole reply.
    failure_kind: Option<&'static str>,
    quota: Option<Arc<Quota>>,
    /// Pi's own omission names it (a `context_edit` of it with no replacement).
    omitted: bool,
}

/// The last message of the conversation's own, which says, without the word, whether Pi has a
/// request to make.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Tail {
    Nothing,
    User,
    Assistant,
    ToolResult,
}

/// What a user message took out of view: the refused turn's, as it stood before it.
#[derive(Clone)]
struct Taken {
    step: Option<Step>,
    tail: Tail,
}

/// The session as it stood at one entry: what a look up to that leaf reads.
#[derive(Clone)]
struct Mark {
    items: usize,
    beside: usize,
    step: Option<Step>,
    last_call: Option<Arc<LastCall>>,
}

/// What a session said so far.
#[derive(Clone)]
struct Session {
    session: Arc<str>,
    /// The zone a reset that names none is read in.
    local: TimeZone,
    items: Vec<Item>,
    /// The place among the records of the record each item is of.
    places: Vec<usize>,
    /// The place among the records of the one being read.
    reading: usize,
    /// The tool calls made and not yet answered in the turn.
    open_tools: HashSet<Key>,
    /// The last assistant step since the last user message.
    step: Option<Step>,
    tail: Tail,
    /// When the last entry a request can follow was written: none where there is none yet, an
    /// unknown time where that entry says none (the file's time stands for it).
    clock: Option<Option<i64>>,
    /// The last tool call the session shows, as a line (`calls::describe`).
    last_call: Option<Arc<LastCall>>,
    /// Every entry's id and its parent's (`parentId`, null for the first), for the path from
    /// the last entry back: the branch Pi is on.
    parents: HashMap<Arc<str>, Option<Arc<str>>>,
    /// The id of the last entry of any kind: the branch's leaf.
    leaf: Option<Arc<str>>,
    /// A record names no `parentId` at all: a linear session, where file order is the path.
    linear: bool,
    /// What the latest user message took out of view, with its id: given back where that
    /// message is not on the path to the leaf (the retry belongs to another branch).
    taken: Option<(Arc<str>, Taken)>,
    /// The session at each entry, by its id.
    marks: HashMap<Arc<str>, Mark>,
    records: usize,
    keys: Keys,
    /// The items only the history shows (a step's thinking, a compaction), each with the
    /// place among the items it stands before.
    beside: Vec<(usize, Item)>,
    /// What the history draws of each call a step made, by its id, and the time of the
    /// step: what its result's item says of it.
    calls: HashMap<Key, (Detail, Option<i64>)>,
}

impl Session {
    fn new(session: Arc<str>, local: TimeZone) -> Self {
        Self {
            session,
            local,
            items: Vec::new(),
            places: Vec::new(),
            reading: 0,
            open_tools: HashSet::new(),
            step: None,
            tail: Tail::Nothing,
            clock: None,
            last_call: None,
            parents: HashMap::new(),
            leaf: None,
            linear: false,
            taken: None,
            marks: HashMap::new(),
            records: 0,
            keys: Keys::default(),
            beside: Vec::new(),
            calls: HashMap::new(),
        }
    }

    /// The session as it stands where the last user message took a refusal out of view but
    /// is not on the path from the leaf: Pi was put back on the refused branch, and the
    /// retry belongs to another. None where there is nothing to give back.
    fn given_back(&self) -> Option<Self> {
        let (user, taken) = self.taken.as_ref()?;
        if self.linear || self.on_path(user) {
            return None;
        }
        let mut back = self.clone();
        back.taken = None;
        back.step.clone_from(&taken.step);
        back.tail = taken.tail;
        Some(back)
    }

    /// Whether the entry `id` is on the path from the leaf to the first entry.
    fn on_path(&self, id: &str) -> bool {
        let mut at = self.leaf.clone();
        let mut steps = 0;
        while let Some(entry) = at {
            if &*entry == id {
                return true;
            }
            steps += 1;
            if steps > self.parents.len() {
                return false;
            }
            at = self.parents.get(&entry).cloned().flatten();
        }
        false
    }

    /// An item only the history shows, where it stands: after the items so far.
    fn beside(&mut self, item: Item) {
        self.beside.push((self.items.len(), item));
    }

    /// A `compaction`: Pi compacted the conversation, from `tokensBefore` tokens, into its
    /// `summary`. No item of the conversation's words: the history's alone.
    fn compaction(&mut self, record: &Value, at: Value, seq: &Value) -> Result<(), Stop> {
        let id = native_id(record.get("id"), "pi compaction", seq).map_err(Stop::Failed)?;
        let summary = record
            .get("summary")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let detail = Detail {
            tokens: record.get("tokensBefore").and_then(Value::as_u64),
            ..Detail::of(Kind::Compaction)
        };
        self.beside(drawn::beside_item(
            id,
            Role::Custom,
            summary,
            Some(at),
            detail,
        ));
        Ok(())
    }

    fn push(&mut self, id: Arc<str>, role: Role, text: String, complete: bool, at: Value) {
        self.places.push(self.reading);
        self.items.push(Item {
            id,
            role,
            text: Arc::from(text),
            complete,
            at: Some(at),
            commentary: false,
            synthetic: false,
            compaction: false,
            detail: None,
        });
    }

    /// A `custom_message`: what an extension put in the conversation itself.
    fn custom(&mut self, record: &Value, at: Value, seq: &Value) -> Result<(), Stop> {
        let text = match record.get("content") {
            Some(Value::String(text)) => text.clone(),
            content => pi_text(content),
        };
        if !text.is_empty() {
            let id = native_id(record.get("id"), "pi custom message", seq).map_err(Stop::Failed)?;
            self.push(id, Role::Custom, text, true, at);
        }
        Ok(())
    }

    /// A `message`: the user's, a tool's result, or the assistant's.
    fn message(
        &mut self,
        record: &Value,
        message: Option<&Value>,
        at: Value,
        seq: &Value,
    ) -> Result<(), Stop> {
        let id = native_id(record.get("id"), "pi message", seq).map_err(Stop::Failed)?;
        let field = |name: &str| message.and_then(|message| message.get(name));
        match field("role").and_then(Value::as_str) {
            Some("user") => {
                let text = pi_text(field("content"));
                if js::trim(&text).is_empty() {
                    return Ok(());
                }
                self.open_tools.clear();
                self.taken = Some((
                    Arc::clone(&id),
                    Taken {
                        step: self.step.take(),
                        tail: self.tail,
                    },
                ));
                self.push(id, Role::User, text, true, at);
                self.tail = Tail::User;
            }
            Some("toolResult") => {
                let call = self.keys.of(field("toolCallId"));
                if call.truthy() {
                    self.open_tools.remove(&call);
                }
                let detail = self.result_detail(&call, &field, &at);
                self.push(id, Role::Tool, pi_text(field("content")), true, at);
                if let Some(result) = self.items.last_mut() {
                    result.detail = Some(Arc::new(detail));
                }
                self.tail = Tail::ToolResult;
            }
            Some("assistant") => self.assistant(id, &field, at)?,
            _ => {}
        }
        Ok(())
    }

    /// An assistant's step, with the fields of its message: its tool calls open, unless it
    /// ended `aborted` or `error` (Pi's loop returns there without running them), and how it
    /// ended.
    fn assistant<'a>(
        &mut self,
        id: Arc<str>,
        field: &dyn Fn(&str) -> Option<&'a Value>,
        at: Value,
    ) -> Result<(), Stop> {
        let made = millis(&at);
        let stop = field("stopReason").and_then(Value::as_str);
        let abandoned = matches!(stop, Some("aborted" | "error"));
        if let Some(Value::Array(blocks)) = field("content") {
            for (place, block) in blocks.iter().enumerate() {
                match block.get("type").and_then(Value::as_str) {
                    Some("toolCall") => {
                        let call = self.keys.of(block.get("id"));
                        if call.truthy() {
                            self.calls.insert(call.clone(), (called(block), made));
                        }
                        // A call of a step that ended in an abort or an error never ran.
                        if abandoned {
                            continue;
                        }
                        let name = block.get("name").and_then(Value::as_str);
                        if let Some(line) = describe(name, block.get("arguments")) {
                            self.last_call = Some(line);
                        }
                        if call.truthy() {
                            self.open_tools.insert(call);
                        }
                    }
                    // The step's thinking, which stands before its words.
                    Some("thinking") => {
                        let thought = block
                            .get("thinking")
                            .and_then(Value::as_str)
                            .unwrap_or_default();
                        if !js::trim(thought).is_empty() {
                            let id = Arc::from(format!("{id}:thinking:{place}"));
                            let detail = Detail::of(Kind::Thinking);
                            self.beside(drawn::beside_item(
                                id,
                                Role::Custom,
                                thought,
                                Some(at.clone()),
                                detail,
                            ));
                        }
                    }
                    _ => {}
                }
            }
        }
        self.push(
            Arc::clone(&id),
            Role::Assistant,
            pi_text(field("content")),
            stop == Some("stop"),
            at,
        );
        // A run that failed or was aborted before the model's first token: the step
        // is Pi's own (`handleRunFailure`, Pi 1.4.0), or a provider's abort of a reply
        // that had not begun, and no turn got through.
        if abandoned && !the_model_wrote(field("content")) {
            if let Some(step) = self.items.last_mut() {
                step.synthetic = true;
            }
        }
        self.taken = None;
        self.tail = Tail::Assistant;
        let mut step = Step {
            id,
            stop: stop.map(Arc::from),
            failure: None,
            failure_kind: None,
            quota: None,
            omitted: false,
        };
        match stop {
            Some("error") => {
                // `String(message.errorMessage ?? 'provider error')`.
                let failure = match field("errorMessage") {
                    None | Some(Value::Null) => "provider error".to_owned(),
                    Some(message) => js::string(Some(message))
                        .map_err(Stop::Failed)?
                        .into_owned(),
                };
                let said = !matches!(field("errorMessage"), None | Some(Value::Null));
                step.failure = said.then(|| failure.clone());
                // An account refused for its billing or its login has no reset to wait for
                // (MUS-23); a limit that resets is the quota.
                let refusal = account_refusal(&failure, None);
                // A provider's refusal for a moment (an overloaded model, a 5xx) is held and
                // tried again, not the turn's failure (T-1112).
                let busy = busy_refusal(&failure, None);
                if refusal.is_some() || refused_for_quota(&failure) || busy {
                    // A JSON `null` time is no time, not the epoch `Number(null)` makes of it.
                    let at_ms = js::to_number(field("timestamp").filter(|at| !at.is_null()))
                        .map_err(Stop::Failed)?;
                    let quota = match refusal {
                        Some(refusal) => unavailable_quota(at_ms, refusal, &failure),
                        None if refused_for_quota(&failure) => {
                            exhausted_quota(&failure, at_ms, &self.local)
                        }
                        None => busy_quota(at_ms, &failure),
                    }
                    .map_err(Stop::Failed)?;
                    step.quota = Some(Arc::new(quota));
                }
            }
            Some("length") => step.failure_kind = Some("the member's reply was cut at the model's output limit; nothing was taken as its result"),
            Some("deferred") => step.failure_kind = Some("the provider deferred the member's reply; no result was returned"),
            _ => {}
        }
        self.step = Some(step);
        Ok(())
    }

    /// A `context_edit`: Pi's own omission of a step from the model's context (no replacement)
    /// leaves that step out of the answer's words, and names it as the one a cancelled retry
    /// was about to try again. Any other edit says nothing of a run.
    fn context_edit(&mut self, record: &Value) {
        if !matches!(record.get("replacement"), None | Some(Value::Null)) {
            return;
        }
        let Some(target) = record.get("targetId").and_then(Value::as_str) else {
            return;
        };
        if let Some(item) = self
            .items
            .iter_mut()
            .rev()
            .find(|item| &*item.id == target && item.role == Role::Assistant)
        {
            item.synthetic = true;
        }
        if let Some(step) = self.step.as_mut().filter(|step| &*step.id == target) {
            step.omitted = true;
        }
    }

    /// What the history draws of a tool's result: the call's name and header, how it
    /// ended (`isError`), how long it took (`durationMs`, else the time from its step to
    /// its result) and the change an edit made (`details.diff`).
    fn result_detail<'a>(
        &self,
        call: &Key,
        field: &dyn Fn(&str) -> Option<&'a Value>,
        at: &Value,
    ) -> Detail {
        let (mut detail, made) = self
            .calls
            .get(call)
            .cloned()
            .unwrap_or_else(|| (Detail::of(Kind::Tool), None));
        if detail.name.is_none() {
            detail.name = field("toolName").and_then(Value::as_str).map(Arc::from);
        }
        detail.status = Some(if field("isError") == Some(&Value::Bool(true)) {
            Status::Failed
        } else {
            Status::Ok
        });
        detail.took_ms = field("durationMs")
            .and_then(Value::as_u64)
            .or_else(|| drawn::took(made, millis(at)));
        detail.diff = field("details")
            .and_then(|details| details.get("diff"))
            .and_then(Value::as_str)
            .filter(|diff| !diff.is_empty())
            .map(Arc::from);
        detail
    }

    /// The reading of the turn `turn`, read up to the leaf it names (the file's end where none, or
    /// where the file has no such entry).
    fn reading(&self, turn: &Turn) -> Record {
        let upto = match turn {
            Turn::Ended {
                upto: Some(leaf), ..
            } => self.marks.get(leaf),
            _ => None,
        };
        let (items, beside, step, last_call) = match upto {
            Some(mark) => (mark.items, mark.beside, mark.step.as_ref(), &mark.last_call),
            None => (
                self.items.len(),
                self.beside.len(),
                self.step.as_ref(),
                &self.last_call,
            ),
        };
        let mut record = Record::new();
        record.items = self.items[..items].to_vec();
        record.lines.places = Some(self.places[..items].to_vec());
        record.beside = self.beside[..beside].to_vec();
        record.last_call.clone_from(last_call);
        record.quota = step.and_then(|step| step.quota.clone());
        of_turn(&mut record, turn, step);
        record
    }

    /// The reading of a session with no file yet.
    fn nothing(turn: &Turn) -> Record {
        let mut record = Record::new();
        of_turn(&mut record, turn, None);
        record.aborted = false;
        record
    }

    /// The reading with no word of the extension's, at `now_ms`: the file decides by time
    /// alone (`file`'s own time stands for an entry that says none).
    fn unworded(&self, now_ms: i64, file: &Path) -> Result<Record, String> {
        if self.records == 0 {
            return Err(format!("empty pi session {}", self.session));
        }
        if let Some(given_back) = self.given_back() {
            return given_back.unworded(now_ms, file);
        }
        let open =
            !self.open_tools.is_empty() || matches!(self.tail, Tail::User | Tail::ToolResult);
        let ended = Turn::Ended {
            upto: None,
            flag: None,
        };
        let turn = if open {
            Turn::Open
        } else {
            let since = match self.clock {
                None => None,
                Some(Some(at)) => Some(at),
                // A clock reading is within the 53 bits of a double for millions of years.
                #[allow(clippy::cast_possible_truncation)]
                Some(None) => Some(
                    stat(file)
                        .map_err(|error| format!("{}: {error}", file.display()))?
                        .mtime_ms as i64,
                ),
            };
            match since {
                Some(since) if now_ms < since + QUIET_MS => Turn::Quiet {
                    settles_at: since + QUIET_MS,
                },
                _ => ended,
            }
        };
        Ok(self.reading(&turn))
    }
}

/// The record's word of the turn, and how the run ended where it ended (`step`: the last
/// assistant step at or before the run's leaf, since the last user message).
fn of_turn(record: &mut Record, turn: &Turn, step: Option<&Step>) {
    match turn {
        Turn::AtWork { said } => {
            record.in_flight = true;
            record.settlement = Settlement::InFlight;
            record.lines.said_at_work = *said;
        }
        Turn::Busy => {
            record.settlement = Settlement::Unknown;
            record.lines.busy = true;
        }
        Turn::Open => {
            record.in_flight = true;
            record.settlement = Settlement::InFlight;
        }
        Turn::Quiet { settles_at } => {
            record.in_flight = true;
            record.settlement = Settlement::InFlight;
            record.lines.settles_at = Some(*settles_at);
        }
        Turn::Ended { flag, .. } => {
            record.settlement = Settlement::Settled;
            match step.map(|step| (step, step.stop.as_deref())) {
                Some((_, Some("stop"))) => {}
                Some((step, Some("length" | "deferred"))) => {
                    record.failed = true;
                    record.failure_kind = step.failure_kind.map(str::to_owned);
                }
                Some((step, Some("error"))) if *flag != Some(true) && !step.omitted => {
                    record.failed = true;
                    record.failure = step.failure.clone().filter(|said| !said.trim().is_empty());
                }
                // An abort, a cancelled retry, a step that called a tool, a tool's result
                // or no step: the run ended with no answer, interrupted.
                _ => record.aborted = !record.items.is_empty(),
            }
        }
    }
}

impl Parser for Session {
    fn visit(&mut self, record: Value, index: usize) -> Result<(), Stop> {
        self.records += 1;
        self.reading = index;
        if record.is_null() {
            return Err(Stop::Failed(null_record(index)));
        }
        let seq = Value::from(index);
        let mut entry = None;
        if let Some(id) = record
            .get("id")
            .and_then(Value::as_str)
            .filter(|_| record.get("type").and_then(Value::as_str) != Some("session"))
        {
            let id: Arc<str> = Arc::from(id);
            match record.get("parentId") {
                None => self.linear = true,
                Some(parent) => {
                    let parent = parent.as_str().map(Arc::from);
                    self.parents.insert(Arc::clone(&id), parent);
                }
            }
            self.leaf = Some(Arc::clone(&id));
            entry = Some(id);
        }
        // `record.message ?? {}`: none and null hold no field.
        let message = record.get("message").filter(|message| !message.is_null());
        // The entry's own time, for the quiet without the word: its own `timestamp` first.
        let own = [
            record.get("timestamp"),
            message.and_then(|message| message.get("timestamp")),
        ]
        .into_iter()
        .flatten()
        .find(|at| !at.is_null());
        let at = own.unwrap_or(&seq).clone();
        let asks = match record.get("type").and_then(Value::as_str) {
            Some("message") => matches!(
                message
                    .and_then(|message| message.get("role"))
                    .and_then(Value::as_str),
                Some("user" | "assistant" | "toolResult" | "system" | "bashExecution")
            ),
            Some("custom_message" | "compaction" | "branch_summary" | "context_edit") => true,
            _ => false,
        };
        if asks {
            self.clock = Some(own.and_then(millis));
        }
        match record.get("type").and_then(Value::as_str) {
            Some("custom_message") => self.custom(&record, at, &seq)?,
            Some("message") => self.message(&record, message, at, &seq)?,
            Some("compaction") => self.compaction(&record, at, &seq)?,
            Some("context_edit") => self.context_edit(&record),
            _ => {}
        }
        if let Some(entry) = entry {
            self.marks.insert(
                entry,
                Mark {
                    items: self.items.len(),
                    beside: self.beside.len(),
                    step: self.step.clone(),
                    last_call: self.last_call.clone(),
                },
            );
        }
        Ok(())
    }
}

/// A record's time in milliseconds since the epoch: a timestamp's text, or a number of them.
fn millis(at: &Value) -> Option<i64> {
    match at {
        Value::String(text) => cf_base::time::parse(text),
        Value::Number(number) => number.as_i64(),
        _ => None,
    }
}

/// What the history draws of a `toolCall` block while it runs: its tool's name and header,
/// from its `arguments` by the tool it is (Pi's own: `read`, `bash`, `edit`, `write`).
fn called(block: &Value) -> Detail {
    let name = block
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| !name.is_empty())
        .unwrap_or("tool");
    let arguments = block.get("arguments");
    let says = |member: &str| {
        arguments
            .and_then(|arguments| arguments.get(member))
            .and_then(Value::as_str)
    };
    let number = |member: &str| {
        arguments
            .and_then(|arguments| arguments.get(member))
            .and_then(Value::as_u64)
    };
    let action = match name {
        "bash" => Action::Shell(says("command").unwrap_or_default()),
        "read" => Action::Read {
            path: says("path").unwrap_or_default(),
            from: number("offset"),
            lines: number("limit"),
        },
        "edit" => Action::Edit(says("path").unwrap_or_default()),
        "write" => Action::Write(says("path").unwrap_or_default()),
        _ => Action::Other {
            name,
            about: ["command", "path", "pattern", "query", "url"]
                .into_iter()
                .find_map(says),
        },
    };
    drawn::tool(name, drawn::header(action), Some(Status::Running))
}

/// `piText`: the text blocks of a message's content, a line each; none for
/// content that is no list.
fn pi_text(content: Option<&Value>) -> String {
    let Some(Value::Array(blocks)) = content else {
        return String::new();
    };
    blocks
        .iter()
        .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
        .filter_map(|block| block.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Whether an assistant's `content` holds anything of the model's: words, its
/// thinking, a tool call.
fn the_model_wrote(content: Option<&Value>) -> bool {
    let Some(Value::Array(blocks)) = content else {
        return false;
    };
    blocks.iter().any(|block| {
        let said = |name: &str| {
            block
                .get(name)
                .and_then(Value::as_str)
                .is_some_and(|text| !js::trim(text).is_empty())
        };
        match block.get("type").and_then(Value::as_str) {
            Some("toolCall") => true,
            Some("text") => said("text"),
            Some("thinking") => said("thinking"),
            _ => false,
        }
    })
}

#[cfg(test)]
mod tests;

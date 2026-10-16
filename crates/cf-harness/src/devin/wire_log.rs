//! Devin's wire log of one launch, as its adapter follows it: the conversation
//! the window shows, the permission mode that conversation is in, and Devin's
//! word on its quota, a refusal after the latest prompt: in its message text
//! ("Reached overall message rate limit … reset in 35 minutes", "Usage limit
//! reached", "Quota exhausted"), or as the prompt's own error. The log only
//! grows, so each look reads what was appended since the last; one that was
//! replaced is read again from its start ([`WireLog::read`]).
//!
//! The TUI's own words to its agent are in it too, a request with its method, and
//! two things are read from them. A prompt the TUI puts to its agent begins a turn
//! (`session/prompt`, with the conversation in its params) before the agent has
//! said a word of it: it is asked until the agent ends a turn (`cause`) or answers
//! its request. And the message last pasted is the agent's to run once a request
//! of the TUI holds its text, which the log is told to expect ([`WireLog::expect`]).
//!
//! Kept from Node on purpose:
//! - where V8 threw (a line that is JSON `null`, or a configuration that is
//!   no list), the look fails in a sentence of this module's own, its lines
//!   before that one read and the log's offset moved past it, as in Node;
//! - a line nested past what serde_json reads, or holding a number past a
//!   double's range, is passed over as a line that is no JSON is, where Node
//!   read it (`cf_base::json::from_slice_lossy`).

use std::borrow::Cow;
use std::cell::RefCell;
use std::fs::File;
use std::hash::{DefaultHasher, Hasher};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};

use cf_base::file::{identity, mtime_ms, settled, FileError, Identity};
use cf_base::js;
use cf_base::json::from_slice_lossy;
use jiff::tz::TimeZone;
use regex::Regex;
use serde_json::Value;

use super::wire::{mode_in, shown_in};
use crate::shared::pattern::compile;
use crate::shared::quota::{exhausted_quota, Quota};

/// What Devin's own messages say when it refuses for its quota: `/rate
/// limit|usage limit|quota exhausted|quota has been exhausted/i`.
static REFUSAL: LazyLock<Regex> = LazyLock::new(|| {
    compile("(?i-u:rate limit|usage limit|quota exhausted|quota has been exhausted)")
});

/// What a line says that cannot be read for the conversation the window shows.
const UNREADABLE: &str =
    "Devin's wire log holds a line that cannot be read for the conversation it shows";

/// How long after a prompt of the TUI a window is read as in the turn it begins,
/// when its agent has said nothing of that turn: what an agent takes to its first
/// word, and well past it. A turn that says nothing for longer is read as the
/// record reads it.
pub(super) const PROMPT_GAP_MS: i64 = 30_000;

/// What the log says so far.
pub(super) struct Said {
    /// The conversation the window shows, once it said.
    pub(super) shown: Option<String>,
    /// The permission mode the shown conversation is in, once the log said it
    /// (`normal`, `bypass`…).
    pub(super) mode: Option<String>,
    /// Devin's word on its quota, shared with every look that finds it.
    pub(super) quota: Option<Arc<Quota>>,
    /// The TUI put its agent a message that holds the text the log was told to
    /// expect: the agent's own queue has it, whatever its record shows yet.
    pub(super) sent: bool,
    /// When a look first read the last prompt of the shown conversation that
    /// nothing has answered or ended since: its turn is begun, though its agent has
    /// said nothing of it. None where no prompt waits so.
    pub(super) asked_at: Option<i64>,
    /// How many prompts of the shown conversation the log has read: one read since a
    /// look is a turn begun since it, whatever has come of it.
    pub(super) prompts: u64,
}

/// A prompt the TUI put to its agent: the id of its request, and when a look first
/// read it.
struct Asked {
    id: Value,
    at_ms: i64,
}

/// How much of a log a look reads at a time when it reads again what it has read.
const CHUNK: u64 = 64 * 1_024;

/// What the lookers have read of a log.
#[derive(Default)]
struct State {
    /// How much of the log has been read.
    offset: u64,
    /// Which file the log was ([`cf_base::file::identity`]), when it was last written
    /// to, and a hash of every byte read of it, as the look that read up to `offset`
    /// found them, and when that look began: what tells the log read from another
    /// that replaced it.
    identity: Option<Identity>,
    written: Option<f64>,
    looked_ms: i64,
    read: DefaultHasher,
    /// What follows its last newline: a line still being written.
    carry: String,
    shown: Option<String>,
    mode: Option<String>,
    quota: Option<Arc<Quota>>,
    /// The text of the message last pasted, which the TUI's words are read for.
    expected: Option<String>,
    sent: bool,
    /// The prompts of the shown conversation that no turn's end or answer closed.
    asked: Vec<Asked>,
    /// How many prompts of the shown conversation have been read.
    prompts: u64,
}

/// One launch's wire log, and what a look at it has read.
pub(super) struct WireLog {
    file: PathBuf,
    /// The machine's zone, which a reset named by a time of day alone is in.
    zone: TimeZone,
    state: RefCell<State>,
}

impl WireLog {
    pub(super) fn new(file: &str, zone: TimeZone) -> Self {
        Self {
            file: PathBuf::from(file),
            zone,
            state: RefCell::new(State::default()),
        }
    }

    pub(super) fn path(&self) -> &Path {
        &self.file
    }

    /// The message about to be pasted begins with `text` (its first line, which
    /// holds its id): a request of the TUI that holds it, read from now on, says
    /// the agent's own queue has the message.
    pub(super) fn expect(&self, text: &str) {
        let mut state = self.state.borrow_mut();
        state.expected = Some(text.to_owned()).filter(|text| !text.is_empty());
        state.sent = false;
    }

    /// What the log says now, read from where the last look stopped: a log
    /// that cannot be opened says what it said before. One that was replaced is
    /// read from its start, the mode read of the one before it forgotten: a log
    /// that shrank, that is another file, or that was written to and does not hold
    /// what was read of it, whether it grew or not. `at_ms` is when it is, read
    /// before the log is.
    pub(super) fn read(&self, at_ms: i64) -> Result<Said, String> {
        let mut state = self.state.borrow_mut();
        let Ok(mut file) = File::open(&self.file) else {
            return Ok(state.said());
        };
        let metadata = file
            .metadata()
            .map_err(|error| FileError::call(error, "fstat", None).to_string())?;
        let (size, written) = (metadata.len(), mtime_ms(&metadata).ok());
        let identity = identity(&file).ok();
        if state.offset > 0 && state.replaced(&mut file, size, identity, written)? {
            state.offset = 0;
            state.read = DefaultHasher::default();
            state.carry.clear();
            state.asked.clear();
            state.mode = None;
        }
        state.identity = identity;
        state.written = written;
        state.looked_ms = at_ms;
        let from = state.offset;
        if size > from {
            let appended = read_from(&mut file, from, size)?;
            state.offset = size;
            state.read.write(&appended);
            let text = format!("{}{}", state.carry, String::from_utf8_lossy(&appended));
            let mut lines: Vec<&str> = text.split('\n').collect();
            state.carry = lines.pop().unwrap_or_default().to_owned();
            for line in lines {
                state.follow(line, at_ms, &self.zone)?;
            }
        }
        Ok(state.said())
    }
}

/// The hash of the first `length` bytes of `file`, as [`State`]'s `read` hashes what a
/// look reads, read a [`CHUNK`] at a time.
fn hash_of(file: &mut File, length: u64) -> Result<u64, String> {
    let mut hash = DefaultHasher::default();
    let mut from = 0;
    while from < length {
        let to = (from + CHUNK).min(length);
        hash.write(&read_from(file, from, to)?);
        from = to;
    }
    Ok(hash.finish())
}

/// `size - from` bytes of `file`, from `from`: what a read of them found,
/// and zeros where the log was cut short between its size and its reading.
fn read_from(file: &mut File, from: u64, size: u64) -> Result<Vec<u8>, String> {
    let said = |error: std::io::Error| FileError::call(error, "read", None).to_string();
    let length =
        usize::try_from(size - from).map_err(|_| "The log is too long to read".to_owned())?;
    let mut buffer = vec![0; length];
    file.seek(SeekFrom::Start(from)).map_err(said)?;
    let mut filled = 0;
    while filled < length {
        match file.read(&mut buffer[filled..]) {
            Ok(0) => break,
            Ok(read) => filled += read,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => return Err(said(error)),
        }
    }
    Ok(buffer)
}

impl State {
    /// Whether the log `file` (`size` bytes long, the file `identity`, last written to
    /// at `written`) is another than the one read so far: it is shorter than what was
    /// read, it is another file, or what it holds up to where the last look stopped is
    /// not what was read, whatever its length did. A configuration rewritten at its size
    /// from `bypass` to `normal` was read as the one before it, and so was one the log
    /// grew past with the same first kilobyte (Astra and Sol on the B1 fix, twice). Its
    /// bytes are read again unless its time is the one the last look found and was
    /// [`settled`] then: on Windows a log rewritten within the clock's tick keeps its
    /// time (main's Windows CI, run 38942619741).
    fn replaced(
        &self,
        file: &mut File,
        size: u64,
        identity: Option<Identity>,
        written: Option<f64>,
    ) -> Result<bool, String> {
        if size < self.offset || identity != self.identity {
            return Ok(true);
        }
        if let Some(written) = written.filter(|written| Some(*written) == self.written) {
            if settled(written, self.looked_ms) {
                return Ok(false);
            }
        }
        Ok(hash_of(file, self.offset)? != self.read.finish())
    }

    fn said(&self) -> Said {
        Said {
            shown: self.shown.clone(),
            mode: self.mode.clone(),
            quota: self.quota.clone(),
            sent: self.sent,
            asked_at: self.asked.last().map(|asked| asked.at_ms),
            prompts: self.prompts,
        }
    }

    /// What one line of the log says: a line that is no JSON says nothing.
    fn follow(&mut self, line: &str, at_ms: i64, zone: &TimeZone) -> Result<(), String> {
        let Ok(event) = from_slice_lossy(line.as_bytes()) else {
            return Ok(());
        };
        if let Some(shown) = shown_in(&event).map_err(|_| UNREADABLE.to_owned())? {
            // Another conversation is in the mode its own configuration says.
            if self.shown.as_deref() != Some(shown) {
                self.mode = None;
            }
            self.shown = Some(shown.to_owned());
        }
        if let Some(mode) = mode_in(&event, self.shown.as_deref()) {
            self.mode = Some(mode.to_owned());
        }
        match event.get("method").and_then(Value::as_str) {
            Some(method) => {
                self.put_to_the_agent(method, &event, at_ms);
                if method == "session/prompt" {
                    self.quota = None;
                }
            }
            None => self.ended(&event),
        }
        let text = event
            .get("update")
            .filter(|update| {
                update.get("sessionUpdate").and_then(Value::as_str) == Some("agent_message_chunk")
            })
            .and_then(|update| update.get("content")?.get("text")?.as_str());
        if let Some(text) = text.filter(|text| REFUSAL.is_match(text)) {
            self.quota = Some(exhausted(text, at_ms, zone)?);
        }
        // Or a refusal of the prompt itself, as Devin 3000.11 writes one: a
        // JSON-RPC error, "Quota exhausted." (-32011, resource_exhausted).
        if let Some(refused) = event.get("error").filter(|error| refuses(error)) {
            self.quota = Some(exhausted(&words(refused.get("message"))?, at_ms, zone)?);
        }
        // Or the turn's own end for it, as Devin wrote it on a Pro plan
        // (2026-10-03): cause quota_exhausted, its words in errorMessage
        // ("Your daily usage quota has been exhausted.").
        if event.get("cause").and_then(Value::as_str) == Some("quota_exhausted") {
            self.quota = Some(exhausted(&words(event.get("errorMessage"))?, at_ms, zone)?);
        }
        Ok(())
    }

    /// A request of the TUI to its agent, read at `at_ms`: one that holds the text
    /// expected is the message the agent's queue took, and a prompt of the shown
    /// conversation is a turn begun.
    fn put_to_the_agent(&mut self, method: &str, request: &Value, at_ms: i64) {
        if self
            .expected
            .as_deref()
            .is_some_and(|text| mentions(request, text))
        {
            self.sent = true;
        }
        let session = request
            .get("params")
            .and_then(|params| params.get("sessionId"))
            .and_then(Value::as_str);
        if method == "session/prompt" && session.is_some() && session == self.shown.as_deref() {
            self.asked.push(Asked {
                id: request.get("id").cloned().unwrap_or(Value::Null),
                at_ms,
            });
            self.prompts += 1;
        }
    }

    /// A record that is no request: a turn's end names its conversation and
    /// closes every prompt asked of it, and an answer to a request names its id.
    fn ended(&mut self, record: &Value) {
        if record.get("cause").is_some() {
            if record.get("sessionId").and_then(Value::as_str) == self.shown.as_deref() {
                self.asked.clear();
            }
        } else if record.get("result").is_some() || record.get("error").is_some() {
            if let Some(id) = record.get("id") {
                self.asked.retain(|asked| asked.id != *id);
            }
        }
    }
}

/// Whether any text in `value` holds `text`.
fn mentions(value: &Value, text: &str) -> bool {
    match value {
        Value::String(words) => words.contains(text),
        Value::Array(items) => items.iter().any(|item| mentions(item, text)),
        Value::Object(fields) => fields.values().any(|field| mentions(field, text)),
        _ => false,
    }
}

/// Whether a JSON-RPC error refuses the prompt for the quota: by its code,
/// by its kind, or by its words.
fn refuses(error: &Value) -> bool {
    error.get("code").and_then(Value::as_f64) == Some(-32011.0)
        || error
            .get("data")
            .and_then(|data| data.get("cognition.ai/errorKind"))
            .and_then(Value::as_str)
            == Some("resource_exhausted")
        || error
            .get("message")
            .and_then(Value::as_str)
            .is_some_and(|message| REFUSAL.is_match(message))
}

/// `value ?? ''` as the text `String` makes of it: nothing is no words.
fn words(value: Option<&Value>) -> Result<Cow<'_, str>, String> {
    match value {
        None | Some(Value::Null) => Ok(Cow::Borrowed("")),
        some => js::string(some),
    }
}

/// A refusal at `at_ms` (`exhaustedQuota(text, Date.now())`), with the reset
/// its words name, a time of day alone in `zone`.
fn exhausted(text: &str, at_ms: i64, zone: &TimeZone) -> Result<Arc<Quota>, String> {
    exhausted_quota(text, at_ms as f64, zone).map(Arc::new)
}

#[cfg(test)]
mod tests;

//! Devin's wire log of one launch, as its adapter follows it: the conversation
//! the window shows, and Devin's word on its quota, a refusal after the latest
//! prompt: in its message text ("Reached overall message rate limit … reset in
//! 35 minutes", "Usage limit reached", "Quota exhausted"), or as the prompt's
//! own error. The log only grows, so each look reads what was appended since
//! the last.
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
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};

use cf_base::file::FileError;
use cf_base::js;
use cf_base::json::from_slice_lossy;
use jiff::tz::TimeZone;
use regex::Regex;
use serde_json::Value;

use super::wire::shown_in;
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

/// What the log says so far.
pub(super) struct Said {
    /// The conversation the window shows, once it said.
    pub(super) shown: Option<String>,
    /// Devin's word on its quota, shared with every look that finds it.
    pub(super) quota: Option<Arc<Quota>>,
}

/// What the lookers have read of a log.
#[derive(Default)]
struct State {
    /// How much of the log has been read.
    offset: u64,
    /// What follows its last newline: a line still being written.
    carry: String,
    shown: Option<String>,
    quota: Option<Arc<Quota>>,
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

    /// What the log says now, read from where the last look stopped: a log
    /// that cannot be opened says what it said before, and one that shrank
    /// was replaced, and is read from its start. `at_ms` is when it is.
    pub(super) fn read(&self, at_ms: i64) -> Result<Said, String> {
        let mut state = self.state.borrow_mut();
        let Ok(mut file) = File::open(&self.file) else {
            return Ok(state.said());
        };
        let size = file
            .metadata()
            .map_err(|error| FileError::call(error, "fstat", None).to_string())?
            .len();
        let mut from = state.offset;
        if size < from {
            from = 0;
            state.carry.clear();
        }
        if size > from {
            let appended = read_from(&mut file, from, size)?;
            state.offset = size;
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
    fn said(&self) -> Said {
        Said {
            shown: self.shown.clone(),
            quota: self.quota.clone(),
        }
    }

    /// What one line of the log says: a line that is no JSON says nothing.
    fn follow(&mut self, line: &str, at_ms: i64, zone: &TimeZone) -> Result<(), String> {
        let Ok(event) = from_slice_lossy(line.as_bytes()) else {
            return Ok(());
        };
        if let Some(shown) = shown_in(&event).map_err(|_| UNREADABLE.to_owned())? {
            self.shown = Some(shown.to_owned());
        }
        if event.get("method").and_then(Value::as_str) == Some("session/prompt") {
            self.quota = None;
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

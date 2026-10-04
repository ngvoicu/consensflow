//! Pi's record of a session (`piReader` and `piParser`,
//! `hosts/lib/completion/pi.js`): its JSONL session file, and what
//! ConsensFlow's extension writes beside it (the `evidence` module).
//!
//! Pi writes no session file until an assistant message is complete, so a
//! first request that hangs leaves nothing to read: the extension's working
//! marker, when it is there, says that turn is in flight, not unknown.
//!
//! The session's last assistant step decides the turn. It is settled when
//! that step stopped or was aborted (an Escape: a pause, a tell, the human),
//! no tool call is open, and either the extension's evidence names it, or the
//! file has been quiet for 120 seconds (`QUIET_MS`): Pi's provider backoff is
//! capped at 60 seconds, so two of them without an append say no retry is
//! coming.
//!
//! Pi's reader is no `TranscriptReader`: its answer also rests on the
//! evidence and on the file's quiet, which change while the transcript does
//! not, so every look works its answer out anew, and returns a new reading. A
//! refusal's quota stays the same object until the transcript changes it.
//!
//! Kept from Node on purpose: the look's options are `piSettlement` alone
//! (Node also read the alias `options.pi`, which no caller passes). The
//! session's records key its set of open tool calls as they keyed Node's (see
//! `Key`).

mod evidence;

use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

use cf_base::env::Env;
use cf_base::file::stat;
use cf_base::js;
use jiff::tz::TimeZone;
use serde_json::Value;

use super::paths::transcript;
use crate::shared::quota::{exhausted_quota, refused_for_quota, Quota};
use crate::shared::record::cache::{Look, Options};
use crate::shared::record::followed::{Followed, Parser};
use crate::shared::record::jsonl::Stop;
use crate::shared::record::key::{Key, Keys};
use crate::shared::record::reading::{
    native_id, null_record, Item, Reading, Record, Role, Settlement,
};

/// How long a session file must be quiet, in milliseconds, for a turn that
/// ended to be settled with no evidence of the extension's.
const QUIET_MS: f64 = 120_000.0;

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
            Box::new(move || Session::new(Arc::clone(&reading), local.clone())),
        ),
    })
}

/// A session's reader: its transcript followed, and the environment that says
/// where the extension's evidence is.
struct Reader {
    session: Arc<str>,
    env: Env,
    transcript: Followed<Session>,
}

impl Look for Reader {
    fn look(&mut self, options: &Options, now_ms: i64) -> Arc<Reading> {
        Arc::new(
            self.read(options, now_ms)
                .unwrap_or_else(|reason| Reading::unreadable(&reason)),
        )
    }
}

impl Reader {
    /// What the session says now, or why it cannot be read.
    fn read(&mut self, options: &Options, now_ms: i64) -> Result<Reading, String> {
        match self.transcript.read().map_err(|stop| stop.reason())? {
            Some(read) => read.state.result(&self.env, options, now_ms, &read.file),
            None => {
                if !evidence::working(&self.session, &self.env, options)? {
                    return Ok(Reading::unreadable(&format!(
                        "no pi session {}",
                        self.session
                    )));
                }
                let mut working = Record::new();
                working.in_flight = true;
                working.settlement = Settlement::InFlight;
                Ok(Reading::Known(working))
            }
        }
    }
}

/// What a session said so far.
struct Session {
    session: Arc<str>,
    /// The zone a reset that names none is read in.
    local: TimeZone,
    items: Vec<Item>,
    /// The tool calls made and not yet answered in the turn.
    open_tools: HashSet<Key>,
    turn_open: bool,
    /// How the last assistant step ended, if it ended.
    terminal: Option<Terminal>,
    failed: bool,
    quota: Option<Arc<Quota>>,
    records: usize,
    keys: Keys,
}

/// The end of the last assistant step: how, and which item it was.
struct Terminal {
    how: End,
    id: Arc<str>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum End {
    /// `stopReason: "stop"`.
    Stopped,
    /// `stopReason: "aborted"`: the turn is over, not failed.
    Aborted,
    /// `stopReason: "error"`.
    Failed,
}

impl Session {
    fn new(session: Arc<str>, local: TimeZone) -> Self {
        Self {
            session,
            local,
            items: Vec::new(),
            open_tools: HashSet::new(),
            turn_open: false,
            terminal: None,
            failed: false,
            quota: None,
            records: 0,
            keys: Keys::default(),
        }
    }

    fn push(&mut self, id: Arc<str>, role: Role, text: String, complete: bool, at: Value) {
        self.items.push(Item {
            id,
            role,
            text: Arc::from(text),
            complete,
            at,
            commentary: false,
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
                self.failed = false;
                self.push(id, Role::User, text, true, at);
                self.turn_open = true;
                self.terminal = None;
            }
            Some("toolResult") => {
                let call = self.keys.of(field("toolCallId"));
                if call.truthy() {
                    self.open_tools.remove(&call);
                }
                self.push(id, Role::Tool, pi_text(field("content")), true, at);
            }
            Some("assistant") => self.assistant(id, &field, at)?,
            _ => {}
        }
        Ok(())
    }

    /// An assistant's step, with the fields of its message: its tool calls
    /// open, and how it ended says what the turn is.
    fn assistant<'a>(
        &mut self,
        id: Arc<str>,
        field: &dyn Fn(&str) -> Option<&'a Value>,
        at: Value,
    ) -> Result<(), Stop> {
        if let Some(Value::Array(blocks)) = field("content") {
            for block in blocks {
                if block.get("type").and_then(Value::as_str) == Some("toolCall") {
                    let call = self.keys.of(block.get("id"));
                    if call.truthy() {
                        self.open_tools.insert(call);
                    }
                }
            }
        }
        let stop = field("stopReason").and_then(Value::as_str);
        self.push(
            Arc::clone(&id),
            Role::Assistant,
            pi_text(field("content")),
            stop == Some("stop"),
            at,
        );
        self.quota = None;
        self.turn_open = true;
        match stop {
            Some("stop") => {
                self.failed = false;
                self.terminal = Some(Terminal {
                    how: End::Stopped,
                    id,
                });
            }
            Some("aborted") => {
                self.failed = false;
                self.terminal = Some(Terminal {
                    how: End::Aborted,
                    id,
                });
            }
            Some("error") => {
                self.failed = true;
                // `String(message.errorMessage ?? 'provider error')`.
                let failure = match field("errorMessage") {
                    None | Some(Value::Null) => "provider error".to_owned(),
                    Some(message) => js::string(Some(message))
                        .map_err(Stop::Failed)?
                        .into_owned(),
                };
                if refused_for_quota(&failure) {
                    let at_ms = js::to_number(field("timestamp")).map_err(Stop::Failed)?;
                    let quota =
                        exhausted_quota(&failure, at_ms, &self.local).map_err(Stop::Failed)?;
                    self.quota = Some(Arc::new(quota));
                }
                self.terminal = Some(Terminal {
                    how: End::Failed,
                    id,
                });
            }
            _ => self.terminal = None,
        }
        Ok(())
    }

    /// The answer, with the extension's evidence and the session file's quiet
    /// as they are now. Fails for a session of no records, for evidence that
    /// cannot be read, and for a file that cannot be looked at.
    fn result(
        &self,
        env: &Env,
        options: &Options,
        now_ms: i64,
        file: &Path,
    ) -> Result<Reading, String> {
        if self.records == 0 {
            return Err(format!("empty pi session {}", self.session));
        }
        let frontier = evidence::frontier(&self.session, env, options)?;
        let has_native_boundary = self
            .terminal
            .as_ref()
            .is_some_and(|terminal| frontier.as_deref() == Some(&*terminal.id));
        let written = stat(file)
            .map_err(|error| format!("{}: {error}", file.display()))?
            .mtime_ms;
        // A clock reading is within the 53 bits of a double for millions of years.
        #[allow(clippy::cast_precision_loss)]
        let quiet = now_ms as f64 - written >= QUIET_MS;
        let open = !self.open_tools.is_empty();
        let can_settle = self
            .terminal
            .as_ref()
            .is_some_and(|terminal| terminal.how != End::Failed)
            && quiet
            && !open;
        let native_settled = has_native_boundary && !open;
        let in_flight = open
            || match self.terminal {
                Some(_) => !(quiet || native_settled),
                None => self.turn_open,
            };
        let mut record = Record::new();
        record.items = self.items.clone();
        record.in_flight = in_flight;
        record.failed = self.failed;
        record.quota = self.quota.clone();
        record.settlement = if native_settled || can_settle {
            Settlement::Settled
        } else if in_flight {
            Settlement::InFlight
        } else {
            Settlement::Unknown
        };
        Ok(Reading::Known(record))
    }
}

impl Parser for Session {
    fn visit(&mut self, record: Value, index: usize) -> Result<(), Stop> {
        self.records += 1;
        if record.is_null() {
            return Err(Stop::Failed(null_record(index)));
        }
        let seq = Value::from(index);
        // `record.message ?? {}`: none and null hold no field.
        let message = record.get("message").filter(|message| !message.is_null());
        let at = [
            record.get("timestamp"),
            message.and_then(|message| message.get("timestamp")),
        ]
        .into_iter()
        .flatten()
        .find(|at| !at.is_null())
        .unwrap_or(&seq)
        .clone();
        match record.get("type").and_then(Value::as_str) {
            Some("custom_message") => self.custom(&record, at, &seq),
            Some("message") => self.message(&record, message, at, &seq),
            _ => Ok(()),
        }
    }
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

#[cfg(test)]
mod tests;

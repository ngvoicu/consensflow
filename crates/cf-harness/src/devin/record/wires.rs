//! Each launch's wire log: what the stock TUI and its agent said to each other,
//! in `integrations/devin/<launch>/wire.jsonl` under ConsensFlow's home, read
//! for one session.
//!
//! A turn Devin is still on shows only on the wire: thoughts, messages and
//! tool calls after the last end; its store holds the finished steps. A
//! tool's last word is not new work: a shell an Escape left running ends
//! after its turn did, and its window would read as working for good. Nor is
//! the history a window reopened on the conversation replays, which carries
//! its timestamps and ends in no turn end (poker-lab's T-4 read as working
//! for good after its window was opened again, 2026-10-03).

use std::collections::{HashMap, HashSet};
use std::path::Path;

use cf_base::env::Env;
use cf_base::file::is_missing;
use cf_base::{js, path};
use serde_json::Value;

use crate::shared::record::find::entries;
use crate::shared::record::home;
use crate::shared::record::jsonl::{read_on, Looked, Seen, Stop};
use crate::shared::record::key::Key;

/// The wire updates that mean Devin is on a turn; settings and mode updates are not work.
const WORK: [&str; 4] = [
    "agent_thought_chunk",
    "agent_message_chunk",
    "tool_call",
    "tool_call_update",
];

/// What the launches' wire logs said of a session.
#[derive(Default)]
pub(super) struct Wires {
    /// The launches' folders, as Node's `readdir` listed them.
    launches: Vec<String>,
    /// Each launch's log, as read so far.
    logs: HashMap<String, Log>,
    /// How each request's turn ended, by the request.
    outcomes: HashMap<String, Outcome>,
}

/// One launch's log: where the last look stopped, and what it said so far.
#[derive(Default)]
struct Log {
    seen: Option<Seen>,
    said: Said,
}

/// What a log said of the session.
#[derive(Default)]
struct Said {
    /// The reply being streamed.
    active: Option<Active>,
    /// Devin is on a turn.
    busy: bool,
    /// The log said something of the session.
    mine: bool,
}

/// A reply being streamed: its stream, its text so far, and the request it
/// answers once the log names it.
struct Active {
    id: String,
    text: String,
    request: Option<String>,
}

/// How a request's turn ended, with the reply it streamed.
#[derive(PartialEq)]
pub(super) struct Outcome {
    pub(super) text: String,
    pub(super) cause: Cause,
}

/// What ended a turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Cause {
    Complete,
    Cancelled,
    Error,
    /// Devin ended the turn for its quota: over, as a cancelled one is.
    QuotaExhausted,
}

impl Wires {
    /// A look at every launch's log, each read on from where the last look
    /// left it: whether any said something new. A log that is not the one
    /// read so far, or one read before and gone, takes what every log said
    /// with it, and all are read again.
    pub(super) fn read(&mut self, session: &str, env: &Env) -> Result<bool, String> {
        let consensflow = match env.os("CONSENSFLOW_HOME") {
            Some(consensflow) => consensflow.to_string_lossy().into_owned(),
            None => path::join(&[&home(env)?, ".consensflow"]),
        };
        let root = path::join(&[&consensflow, "integrations", "devin"]);
        self.launches = launches(&root)?;
        let only = js::stringify(&Value::from(session));
        loop {
            let mut changed = false;
            let mut whole = true;
            let mut present = HashSet::new();
            for launch in &self.launches {
                let file = path::join(&[&root, launch, "wire.jsonl"]);
                let kept = self.logs.remove(launch);
                let was_kept = kept.is_some();
                let mut log = kept.unwrap_or_default();
                let outcomes = &mut self.outcomes;
                let looked = read_on(
                    Path::new(&file),
                    log.seen.as_ref(),
                    &mut |event, _| log.said.visit(&event, session, outcomes),
                    Some(only.as_bytes()),
                );
                match looked {
                    Ok(Looked::NotTheFile) => {
                        whole = false;
                        break;
                    }
                    Ok(looked) => {
                        if let Looked::Read(seen) = looked {
                            log.seen = Some(seen);
                            changed = true;
                        }
                        self.logs.insert(launch.clone(), log);
                        present.insert(launch.as_str());
                    }
                    Err(Stop::Io(error)) if is_missing(&error) => {
                        if was_kept {
                            self.logs.insert(launch.clone(), log);
                        }
                    }
                    Err(stop) => return Err(stop.reason()),
                }
            }
            if whole
                && self
                    .logs
                    .keys()
                    .all(|launch| present.contains(launch.as_str()))
            {
                return Ok(changed);
            }
            self.logs.clear();
            self.outcomes.clear();
        }
    }

    /// Whether Devin is at work on the session, as the log written last of
    /// those that said something of it says: another session's, written
    /// later, says nothing of this one. Of two written at once, the one
    /// listed later says.
    pub(super) fn working(&self) -> bool {
        let mut working = false;
        let mut latest = -1.0;
        for launch in &self.launches {
            let Some(log) = self.logs.get(launch) else {
                continue;
            };
            if let (true, Some(seen)) = (log.said.mine, &log.seen) {
                if seen.mtime_ms() >= latest {
                    latest = seen.mtime_ms();
                    working = log.said.busy;
                }
            }
        }
        working
    }

    /// How the turn of `request` ended, if a log said so.
    pub(super) fn outcome(&self, request: &Key) -> Option<&Outcome> {
        match request {
            Key::Text(request) => self.outcomes.get(&**request),
            _ => None,
        }
    }
}

impl Said {
    /// A record of the log (`visitWire`): what it says of the session's
    /// turn. A line that names the session is never JSON's `null`.
    fn visit(
        &mut self,
        event: &Value,
        session: &str,
        outcomes: &mut HashMap<String, Outcome>,
    ) -> Result<(), Stop> {
        if event.get("sessionId").and_then(Value::as_str) != Some(session) {
            return Ok(());
        }
        self.mine = true;
        let update = |name: &str| event.get("update").and_then(|update| update.get(name));
        let meta = |name: &str| update("_meta").and_then(|meta| meta.get(name));
        let kind = update("sessionUpdate").and_then(Value::as_str);
        let tool_ended = kind == Some("tool_call_update")
            && matches!(
                update("status").and_then(Value::as_str),
                Some("completed" | "failed" | "cancelled")
            );
        let replayed = meta("cognition.ai/timestamp").is_some();
        if kind.is_some_and(|kind| WORK.contains(&kind)) && !tool_ended && !replayed {
            self.busy = true;
        }
        if kind == Some("agent_message_chunk") {
            // History replay has timestamps but no streaming UUID.
            let Some(Value::String(id)) = meta("cognition.ai/streamingMessageId") else {
                return Ok(());
            };
            let content = update("content");
            if content
                .and_then(|content| content.get("type"))
                .and_then(Value::as_str)
                != Some("text")
            {
                return Ok(());
            }
            let active = match &mut self.active {
                Some(active) if active.id == *id => active,
                active => active.insert(Active {
                    id: id.clone(),
                    text: String::new(),
                    request: None,
                }),
            };
            let piece = js::string(content.and_then(|content| content.get("text")))
                .map_err(Stop::Failed)?;
            active.text.push_str(&piece);
        }
        if let (Some(active), Some(Value::String(request))) =
            (&mut self.active, event.get("turnClientMessageId"))
        {
            active.request = Some(request.clone());
        }
        let cause = match event.get("cause").and_then(Value::as_str) {
            Some("complete") => Cause::Complete,
            Some("cancelled") => Cause::Cancelled,
            Some("error") => Cause::Error,
            Some("quota_exhausted") => Cause::QuotaExhausted,
            _ => return Ok(()),
        };
        if let Some(active) = &self.active {
            if let Some(request) = active
                .request
                .as_ref()
                .filter(|request| !request.is_empty())
            {
                let outcome = Outcome {
                    text: active.text.clone(),
                    cause,
                };
                if outcomes
                    .get(request)
                    .is_some_and(|previous| *previous != outcome)
                {
                    return Err(Stop::Failed(
                        "conflicting Devin completion evidence".to_owned(),
                    ));
                }
                outcomes.insert(request.clone(), outcome);
            }
        }
        self.active = None;
        self.busy = false;
        Ok(())
    }
}

/// The launches' folders under `root`, as Node's `readdir` lists them: a
/// link to a folder is none, as its entry says. None when there is no `root`.
fn launches(root: &str) -> Result<Vec<String>, String> {
    match entries(Path::new(root)) {
        Ok(entries) => Ok(entries
            .into_iter()
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect()),
        Err(error) if is_missing(&error) => Ok(Vec::new()),
        Err(error) => Err(format!("{root}: {error}")),
    }
}

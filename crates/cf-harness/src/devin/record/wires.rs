//! Each launch's wire log: what the stock TUI and its agent said to each other,
//! in `integrations/devin/<launch>/wire.jsonl` under ConsensFlow's home, read
//! for one session.
//!
//! A turn Devin is still on shows only on the wire: thoughts, messages and
//! tool calls after the last end; its store holds the finished steps. What a
//! tool says of a call it began is not new work, whatever its status: a shell
//! an Escape left running ends after its turn did, and a command a turn left
//! running may go on reporting while it runs (a ping says a line a second), and
//! the window would read as working for good, its task never done. Nor is
//! the history a window reopened on the conversation replays, which carries
//! its timestamps and ends in no turn end (poker-lab's T-4 read as working
//! for good after its window was opened again, 2026-10-03). Devin 3000.11
//! stamps the work of a turn at work too: what tells it from a replay is the
//! stream its words are said in, and Devin's own word that the turn is at work
//! (`session_info_update`, `working` until `blocked`), which a replay never says.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use cf_base::env::Env;
use cf_base::file::is_missing;
use cf_base::{js, path};
use serde_json::Value;

use crate::devin::adapter::BYPASS;
use crate::shared::record::find::entries;
use crate::shared::record::home;
use crate::shared::record::jsonl::{read_on, Looked, Seen, Stop};
use crate::shared::record::key::Key;

/// The wire updates that mean Devin is on a turn: its thoughts, its words and the calls
/// it begins. Settings and mode updates are not work, and nor is an update of a call
/// that began (`tool_call_update`): a turn that is on shows its calls and words first,
/// and a call's later words (a shell's output, its end) come after its turn's end as
/// well.
const WORK: [&str; 3] = ["agent_thought_chunk", "agent_message_chunk", "tool_call"];

/// The words a stream of Devin's agent is said in: only those a turn at work streams carry
/// a stream's id, which the history a reopened window replays has not.
const STREAMED: [&str; 2] = ["agent_thought_chunk", "agent_message_chunk"];

/// What the launches' wire logs said of a session.
#[derive(Default)]
pub(super) struct Wires {
    /// The launches' folders, as Node's `readdir` listed them.
    launches: Vec<String>,
    /// Each launch's log, as read so far.
    logs: HashMap<String, Log>,
    /// How each request's turn ended, by the request.
    outcomes: HashMap<String, Outcome>,
    /// How each turn ended, by the model's request it ended in (Devin's own request id),
    /// where the wire names one ([`Active::native`]).
    natives: HashMap<String, Outcome>,
}

/// One launch's log: where the last look stopped, and what it said so far.
#[derive(Default)]
struct Log {
    seen: Option<Seen>,
    said: Said,
    /// What ConsensFlow gave the launch as its first message behind `/bypass`
    /// (`adapter::first_message`), read from its prompt file once the log said
    /// something of the session, which the launch writes before its window opens:
    /// none where it gave the launch none so.
    bypassed: Option<Option<String>>,
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

/// A reply being streamed: its stream, its text so far, the request it
/// answers once the log names it, and the model's request the turn ends in, once the
/// log names that (`turnRequestId`, beside the request as the turn ends: what the
/// stored reply holds as its `metadata.request_id`).
struct Active {
    id: String,
    text: String,
    request: Option<String>,
    native: Option<String>,
}

/// How a request's turn ended, with the reply it streamed.
#[derive(Clone, PartialEq)]
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
    /// with it, and all are read again. `now_ms` is when the look began.
    pub(super) fn read(&mut self, session: &str, env: &Env, now_ms: i64) -> Result<bool, String> {
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
                let ended = &mut Ended {
                    outcomes: &mut self.outcomes,
                    natives: &mut self.natives,
                };
                let looked = read_on(
                    Path::new(&file),
                    log.seen.as_ref(),
                    now_ms,
                    &mut |event, _| log.said.visit(&event, session, ended),
                    Some(only.as_bytes()),
                );
                match looked {
                    Ok(Looked::NotTheFile) => {
                        whole = false;
                        break;
                    }
                    Ok(looked) => {
                        match looked {
                            Looked::Read(seen) => {
                                log.seen = Some(seen);
                                changed = true;
                            }
                            Looked::Held(seen) => log.seen = Some(seen),
                            Looked::Unchanged | Looked::NotTheFile => {}
                        }
                        if log.said.mine && log.bypassed.is_none() {
                            log.bypassed = Some(bypassed(&root, launch)?);
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
            self.natives.clear();
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

    /// What ConsensFlow gave the session's launches as their first messages behind
    /// `/bypass`, each as it was behind the command.
    pub(super) fn bypassed(&self) -> impl Iterator<Item = &str> {
        self.logs
            .values()
            .filter_map(|log| log.bypassed.as_ref()?.as_deref())
    }

    /// How the turn of `request` ended, if a log said so.
    pub(super) fn outcome(&self, request: &Key) -> Option<&Outcome> {
        match request {
            Key::Text(request) => self.outcomes.get(&**request),
            _ => None,
        }
    }

    /// How the turn that ended in the model's request `native` ended, if a log said so,
    /// naming that request both as the turn ended and in the end's statistics.
    pub(super) fn native_outcome(&self, native: &Key) -> Option<&Outcome> {
        match native {
            Key::Text(native) => self.natives.get(&**native),
            _ => None,
        }
    }
}

/// Where a log's ends of turns go: by the request, and by the model's request.
struct Ended<'a> {
    outcomes: &'a mut HashMap<String, Outcome>,
    natives: &'a mut HashMap<String, Outcome>,
}

/// `outcome` as how the turn of `key` ended in `outcomes`: a log that says another of
/// the same turn fails the look.
fn record(
    outcomes: &mut HashMap<String, Outcome>,
    key: &str,
    outcome: &Outcome,
) -> Result<(), Stop> {
    if outcomes
        .get(key)
        .is_some_and(|previous| previous != outcome)
    {
        return Err(Stop::Failed(
            "conflicting Devin completion evidence".to_owned(),
        ));
    }
    outcomes.insert(key.to_owned(), outcome.clone());
    Ok(())
}

impl Said {
    /// A record of the log (`visitWire`): what it says of the session's
    /// turn. A line that names the session is never JSON's `null`.
    fn visit(&mut self, event: &Value, session: &str, ended: &mut Ended<'_>) -> Result<(), Stop> {
        if event.get("sessionId").and_then(Value::as_str) != Some(session) {
            return Ok(());
        }
        self.mine = true;
        let update = |name: &str| event.get("update").and_then(|update| update.get(name));
        let meta = |name: &str| update("_meta").and_then(|meta| meta.get(name));
        let kind = update("sessionUpdate").and_then(Value::as_str);
        // Devin 3000.11 stamps every record of its work with its time, as it stamps the
        // history a reopened window replays: work stamped so is the turn's where its stream
        // is named (a replay names none), and Devin says itself when a turn is at work
        // (`statusEnum` `working`) and when it waits for its user (`blocked`, before the
        // turn's end), which a replay never says at work (two probes of 2026-10-26, T-660:
        // stamped work alone read a Devin turn at rest through a minute's command).
        let replayed = meta("cognition.ai/timestamp").is_some()
            && !(kind.is_some_and(|kind| STREAMED.contains(&kind))
                && meta("cognition.ai/streamingMessageId").is_some());
        if kind.is_some_and(|kind| WORK.contains(&kind)) && !replayed {
            self.busy = true;
        }
        if kind == Some("session_info_update") {
            match meta("cognition.ai/statusEnum").and_then(Value::as_str) {
                Some("working") => self.busy = true,
                Some("blocked") => self.busy = false,
                _ => {}
            }
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
                    native: None,
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
            active.native = match event.get("turnRequestId") {
                Some(Value::String(native)) => Some(native.clone()),
                _ => None,
            };
        }
        let cause = match event.get("cause").and_then(Value::as_str) {
            Some("complete") => Cause::Complete,
            Some("cancelled") => Cause::Cancelled,
            Some("error") => Cause::Error,
            Some("quota_exhausted") => Cause::QuotaExhausted,
            _ => return Ok(()),
        };
        if let Some(active) = &self.active {
            let outcome = Outcome {
                text: active.text.clone(),
                cause,
            };
            if let Some(request) = active
                .request
                .as_ref()
                .filter(|request| !request.is_empty())
            {
                record(ended.outcomes, request, &outcome)?;
                // The model's request, where the end names the same one in its
                // statistics: a compaction can leave a stored reply no other way back to
                // the turn it ended (T-652).
                let stated = event
                    .get("stats")
                    .and_then(|stats| stats.get("requestId"))
                    .and_then(Value::as_str);
                if let Some(native) = active
                    .native
                    .as_deref()
                    .filter(|native| !native.is_empty() && stated == Some(native))
                {
                    record(ended.natives, native, &outcome)?;
                }
            }
        }
        self.active = None;
        self.busy = false;
        Ok(())
    }
}

/// What the launch `launch` under `root` was given as its first message behind
/// `/bypass`, from its prompt file (`install::prepare_prompt`): none where it has none,
/// or one that is not behind the command.
fn bypassed(root: &str, launch: &str) -> Result<Option<String>, String> {
    let file = path::join(&[root, launch, "prompt.txt"]);
    match std::fs::read_to_string(&file) {
        Ok(prompt) => Ok(prompt
            .strip_prefix(BYPASS)
            .and_then(|behind| behind.strip_prefix(' '))
            .map(str::to_owned)),
        Err(error) if is_missing(&error) => Ok(None),
        Err(error) => Err(format!("{file}: {error}")),
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

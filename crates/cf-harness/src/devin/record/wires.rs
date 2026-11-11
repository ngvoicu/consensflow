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
use crate::devin::kept::{self, behind_bypass, KeptFiles};
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
    /// What was kept of launches whose folders went (`kept`), as read so far.
    kept_files: KeptFiles,
    /// The prompts ConsensFlow gave the session's launches whose folders went, behind
    /// `/bypass`, as they were kept.
    kept: Vec<String>,
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
    /// A turn began (work was said, or Devin's word that it works) and its end was not
    /// said: `blocked` is no end, it comes where Devin waits for its user and before the
    /// turn's end (T-660), so a turn that waits is still one that is open. Only the turn's
    /// own end (`cause`) closes it.
    open: bool,
    /// The texts of the messages Devin said back as the open turn took them in, oldest first:
    /// live ([`said_back_live`]), or an older Devin's with a turn open around it
    /// ([`Said::unclaimed`]); the turn's own, and any typed into it while it worked, which the
    /// same turn takes (its end names each, T-1423). None once it ended.
    said_back: Vec<String>,
    /// The texts of the messages an older Devin said back ([`said_back_unmarked`]) since the
    /// last turn's end, waiting for a turn to open: Devin 3000.11 says its message back just
    /// after `working`, and some older logs before the turn's first streamed word, while a
    /// replay's are never followed by a turn's work and end at a bare `turnClientMessageId`
    /// (the kit's 34 logs, T-1423). They join [`Said::said_back`] when the turn opens, and
    /// are dropped at the next turn end or `turnClientMessageId`.
    unclaimed: Vec<String>,
    /// The call whose permission dialog the turn waits on ([`asks_permission`]), until the
    /// dialog is answered (that call's next update) or the turn ends.
    asking: Option<String>,
    /// The log said something of the session.
    mine: bool,
    /// The user's messages of the prompts the log has listed as the session's steps
    /// ([`Said::listed`]), from its first `steps` line on: none before it.
    steps: Option<HashSet<String>>,
}

/// The text of a message a line says back with no word of whether it is live: a
/// `user_message_chunk` with no event type and no event id, as Devin 3000.11 says back every
/// message, live or replayed (127 of 127 in the kit's 34 logs). Whether it was taken in live
/// is told by the turn around it ([`Said::unclaimed`]).
fn said_back_unmarked(event: &Value) -> Option<&str> {
    let update = event.get("update")?;
    if update.get("sessionUpdate").and_then(Value::as_str) != Some("user_message_chunk") {
        return None;
    }
    let meta = update.get("_meta");
    let marked = |name: &str| meta.is_some_and(|meta| meta.get(name).is_some());
    if marked("cognition.ai/eventType") || marked("cognition.ai/eventId") {
        return None;
    }
    Some(
        update
            .get("content")
            .and_then(|content| content.get("text"))
            .and_then(Value::as_str)
            .unwrap_or(""),
    )
}

/// The text of the message a line says back live as a turn takes it in, as Devin 3000.12.43
/// writes one (T-1417, 17 of 17 messages in three live runs; T-1423, 15 of 15 in two more):
/// a `user_message_chunk` whose `eventType` is `user_message`, with the message's client id
/// and no event id of its own. The same message replayed by a reopened window has an event id,
/// and Devin 3000.11 says its messages back, live or replayed, with neither an event type nor
/// the message's own text field (127 of 127 in the kit's 34 logs of 2026-10-21 and
/// 2026-11-05): no line of an older Devin, and no replay, opens a turn by this rule (an older
/// Devin's is the turn's by the turn around it, [`said_back_unmarked`]). The text is the
/// message's own (`cognition.ai/message`), else the line's.
pub(in crate::devin) fn said_back_live(event: &Value) -> Option<&str> {
    let update = event.get("update")?;
    if update.get("sessionUpdate").and_then(Value::as_str) != Some("user_message_chunk") {
        return None;
    }
    let meta = update.get("_meta")?;
    let live = meta.get("cognition.ai/eventType").and_then(Value::as_str) == Some("user_message")
        && meta
            .get("cognition.ai/clientMessageId")
            .and_then(Value::as_str)
            .is_some_and(|id| !id.is_empty())
        && meta.get("cognition.ai/eventId").is_none();
    if !live {
        return None;
    }
    meta.get("cognition.ai/message")
        .and_then(Value::as_str)
        .or_else(|| update.get("content")?.get("text")?.as_str())
        .or(Some(""))
}

/// The call a line asks the user to allow: Devin's permission dialog, a line of its own with
/// the call and the answers it offers at its top (`toolCall`, `options`), which Devin 3000.11
/// wrote after `blocked` and Devin 3000.12.43 writes with no word before it (T-1417).
fn asks_permission(event: &Value) -> Option<&str> {
    event.get("options")?;
    event
        .get("toolCall")?
        .get("toolCallId")
        .and_then(Value::as_str)
}

/// What one launch's log says of each session's turns, by the rules the record reads every
/// log by ([`Said::visit`]): whether one is open. A window's own launch writes the log, and
/// no other process does: the turns it holds are the window's own (`adapter`'s `observe`).
#[derive(Default)]
pub(in crate::devin) struct Turns {
    /// What the log says so far, by the session each line names.
    said: HashMap<String, Said>,
}

impl Turns {
    /// One line of the log. A line the record's look would fail on still says what it
    /// says of a turn: the record's look tells of the failure.
    pub(in crate::devin) fn follow(&mut self, event: &Value) {
        let Some(session) = event.get("sessionId").and_then(Value::as_str) else {
            return;
        };
        let said = self.said.entry(session.to_owned()).or_default();
        // How the turns ended is the record's to keep: here, each end is an end.
        let _ = said.visit(
            event,
            session,
            &mut Ended {
                outcomes: &mut HashMap::new(),
                natives: &mut HashMap::new(),
            },
        );
    }

    /// The log is read again from its start: what it says now is read anew.
    pub(in crate::devin) fn read_anew(&mut self) {
        self.said.clear();
    }

    /// Whether a turn is open on `session` in the log: begun, and its end not said.
    pub(in crate::devin) fn open(&self, session: &str) -> bool {
        self.said.get(session).is_some_and(|said| said.open)
    }

    /// The deliveries, by the header that begins each, that the log said back as the turn
    /// open on `session` took them in ([`Said::said_back`]: live, or an older Devin's with
    /// the turn open around it): none where no turn is open, none of a replay, and none for
    /// a message with no header of ConsensFlow's.
    pub(in crate::devin) fn said_back(&self, session: &str) -> Vec<i64> {
        self.said
            .get(session)
            .filter(|said| said.open)
            .map(|said| {
                said.said_back
                    .iter()
                    .filter_map(|text| crate::contract::delivery_of(text))
                    .collect()
            })
            .unwrap_or_default()
    }
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
    /// Why the turn failed, where the turn's end said (`errorMessage`, as a
    /// quota's end says it).
    pub(super) error: Option<String>,
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
        // What was kept of launches whose folders went is the launches' own word: what
        // came or went of it changes what the session reads.
        self.kept_files
            .read(&path::join(&[&consensflow, "integrations", kept::FOLDER]));
        let kept = self.kept_files.for_session(session);
        let kept_changed = kept != self.kept;
        self.kept = kept;
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
                return Ok(changed || kept_changed);
            }
            self.logs.clear();
            self.outcomes.clear();
            self.natives.clear();
        }
    }

    /// The torn lines of the session's own that the logs passed over, each by
    /// the place among its log's records it stood at, the logs in launch order.
    pub(super) fn lost(&self) -> Vec<usize> {
        self.launches
            .iter()
            .filter_map(|launch| self.logs.get(launch)?.seen.as_ref())
            .flat_map(|seen| seen.passed_over().iter().copied())
            .collect()
    }

    /// Whether Devin is at work on the session, as the log written last of
    /// those that said something of it says: another session's, written
    /// later, says nothing of this one. Of two written at once, the one
    /// listed later says.
    pub(super) fn working(&self) -> bool {
        self.said_last().is_some_and(|said| said.busy)
    }

    /// Whether a turn is open on the session, as the same log says: one that began and whose
    /// end it did not say. Devin says `blocked` where it waits for its user and before the
    /// turn's end, so a turn that is not at work may be open still; the end alone closes it.
    pub(super) fn turn_open(&self) -> bool {
        self.said_last().is_some_and(|said| said.open)
    }

    /// What the log written last of those that said something of the session says (see
    /// [`Wires::working`]).
    fn said_last(&self) -> Option<&Said> {
        let mut said = None;
        let mut latest = -1.0;
        for launch in &self.launches {
            let Some(log) = self.logs.get(launch) else {
                continue;
            };
            if let (true, Some(seen)) = (log.said.mine, &log.seen) {
                if seen.mtime_ms() >= latest {
                    latest = seen.mtime_ms();
                    said = Some(&log.said);
                }
            }
        }
        said
    }

    /// What ConsensFlow gave the session's launches as their first messages behind
    /// `/bypass`, each as it was behind the command: the launches' folders say it, and
    /// what was kept of those whose folders went.
    pub(super) fn bypassed(&self) -> impl Iterator<Item = &str> {
        self.logs
            .values()
            .filter_map(|log| log.bypassed.as_ref()?.as_deref())
            .chain(self.kept.iter().map(String::as_str))
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
/// the same turn fails the look, and every look after it while the log says both (the
/// client id's rule, which the model request's ends keep too). A retried turn whose two
/// ends disagree reads unknown for good, never settled early (Calliope's P3-3 on T-655).
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
    /// The summaries of the prompts a `steps` line lists that no `steps` line of the log listed
    /// before, by the user's message each began with, the command they were given behind taken
    /// off: Devin keeps a prompt's first line as its summary, the header of ours, and 3000.11 keeps
    /// `/bypass` ahead of it (the kit's `devin-resume-after-note-bypass`). The log's first `steps`
    /// line lists what it found (a reopened window's replay, a new one's none) and is no prompt's.
    /// Every prompt a log listed new was a message taken in, and its turn ended in a `cause`
    /// (T-1432: 38 in Devin 3000.12.43's 21 kept logs and the 3000.11 kit's 34), but in the
    /// launches killed in their turn. None for a line that is no `steps` line.
    fn listed(&mut self, event: &Value) -> Option<Vec<String>> {
        let listed = event.get("steps")?.as_array()?;
        let prompts = listed.iter().filter_map(|step| {
            (step.get("kind").and_then(Value::as_str) == Some("prompt"))
                .then(|| step.get("userMessageId").and_then(Value::as_str))
                .flatten()
                .map(|message| (message, step.get("summary").and_then(Value::as_str)))
        });
        let Some(known) = self.steps.as_mut() else {
            self.steps = Some(prompts.map(|(message, _)| message.to_owned()).collect());
            return None;
        };
        let new = prompts
            .filter(|(message, _)| known.insert((*message).to_owned()))
            .map(|(_, summary)| {
                let summary = summary.unwrap_or("");
                summary
                    .strip_prefix(BYPASS)
                    .filter(|rest| rest.starts_with(char::is_whitespace))
                    .map_or(summary, str::trim_start)
                    .to_owned()
            })
            .collect();
        Some(new)
    }

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
        // What the TUI answers itself, at once, to a command of its own (`/login`, `/mcp`,
        // the workspace's folders) is said as Devin's words, flagged instant, with no turn
        // and no end: work it would be for good (Calliope's P3-1 on T-683, T-688).
        let instant = meta("cognition.ai/instant") == Some(&Value::Bool(true));
        if kind.is_some_and(|kind| WORK.contains(&kind)) && !replayed && !instant {
            self.busy = true;
        }
        if kind == Some("session_info_update") {
            match meta("cognition.ai/statusEnum").and_then(Value::as_str) {
                Some("working") => self.busy = true,
                Some("blocked") => self.busy = false,
                _ => {}
            }
        }
        // Devin 3000.12.43 says no word of its own on a turn (T-1417): the message it says back
        // live is where the turn takes it in, as `working` was, and its first streamed word
        // comes 1 to 6.6 s later, or after a model's whole first call. A message typed into a
        // turn at work is said back into that turn, and the turn's one end names it too.
        if let Some(text) = said_back_live(event) {
            self.busy = true;
            self.said_back.push(text.to_owned());
        }
        // A prompt the log lists among the steps for the first time is a message the turn took
        // in, where Devin says none back: `/bypass <message>` (T-1432) is said back by no line.
        if let Some(prompts) = self.listed(event) {
            self.busy |= !prompts.is_empty();
            self.said_back.extend(prompts);
        }
        // An older Devin's message said back is the turn's where a turn is open around it: at
        // its line (3000.11 says `working` first) or by the turn's first work before the next
        // end (T-1423). A replay's never is.
        if let Some(text) = said_back_unmarked(event) {
            self.unclaimed.push(text.to_owned());
        }
        // Its permission dialog waits for the user, as `blocked` said before it on 3000.11;
        // the answer goes on with the call (its `tool_call_update`, T-1417 run 3) or ends the
        // turn (an Escape: the call failed, and `cancelled`, T-1423). The turn is still open.
        if let Some(call) = asks_permission(event) {
            self.busy = false;
            self.asking = Some(call.to_owned());
        } else if kind == Some("tool_call_update")
            && self.open
            && self.asking.is_some()
            && update("toolCallId").and_then(Value::as_str) == self.asking.as_deref()
        {
            self.busy = true;
            self.asking = None;
        }
        // A turn is open from its first work, the message said back, or Devin's word that it
        // works: `blocked` and a dialog end nothing, the turn's own end does (below).
        self.open |= self.busy;
        if self.open {
            self.said_back.append(&mut self.unclaimed);
        }
        if event.get("turnClientMessageId").is_some() {
            self.unclaimed.clear();
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
            Some("complete") => Some(Cause::Complete),
            Some("cancelled") => Some(Cause::Cancelled),
            Some("error") => Some(Cause::Error),
            Some("quota_exhausted") => Some(Cause::QuotaExhausted),
            // A cause of another name ends the turn all the same, and says nothing of how: a
            // turn its said-back message opened is never held open by a word this reader
            // does not know (T-1423).
            Some(_) => None,
            None => return Ok(()),
        };
        if let (Some(active), Some(cause)) = (&self.active, cause) {
            let error = event
                .get("errorMessage")
                .and_then(Value::as_str)
                .filter(|said| cause == Cause::Error && !said.trim().is_empty())
                .map(str::to_owned);
            let outcome = Outcome {
                text: active.text.clone(),
                cause,
                error,
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
        // Every cancel recorded ends so, at once: the app's Stop over thoughts, over a command
        // in the foreground or in the background, and an Escape over a permission dialog
        // (`cancelled`, T-1423, Devin 3000.12.43); and 3000.11's (four of four in the kit).
        self.active = None;
        self.busy = false;
        self.open = false;
        self.said_back.clear();
        self.unclaimed.clear();
        self.asking = None;
        Ok(())
    }
}

/// What the launch `launch` under `root` was given as its first message behind
/// `/bypass`, from its prompt file (`kept::behind_bypass`).
fn bypassed(root: &str, launch: &str) -> Result<Option<String>, String> {
    let file = path::join(&[root, launch, "prompt.txt"]);
    behind_bypass(Path::new(&file)).map_err(|error| format!("{file}: {error}"))
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

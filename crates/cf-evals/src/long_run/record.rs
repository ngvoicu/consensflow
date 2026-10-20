//! A harness's own record of a conversation, read as it is on disk and by no
//! code of the product's: what the watch holds the daemon's word to. Claude Code
//! is read whole (`<config>/projects/<folder>/<session>.jsonl`), for it is where
//! the field's shapes were met: an idle compaction (A5), a job it runs in the
//! background and the turn it starts by itself when the job ends (A2). For every
//! other harness the watch has the daemon's copy of the record, item by item
//! (the ledger's `transcript`), which is where it reads them.
//!
//! What a record says is brought down to [`Entry`]s: a prompt, an answer, a
//! tool's call and its result, the end of a turn, a compaction (Claude Code's
//! own summary of the conversation, which is no turn: `compact_boundary`, and
//! the user's record `isCompactSummary`), a job started in the background (the
//! tool result's `toolUseResult.backgroundTaskId`, or the words `running in
//! background with ID: …` a long command is moved there with) and the
//! notification of its end (`<task-notification>` naming its id, in what Claude
//! Code queued or began a turn with itself, never in a message a person or
//! ConsensFlow typed), and the operations on Claude Code's queue of messages (a
//! message waiting there is a turn not over).
//!
//! The record is replayed as the product's reader replays it
//! (`crates/cf-harness/src/claude/record/`): a message queued and not yet taken keeps
//! the turn in flight, a removal or a `popAll` naming it as the product names it (its
//! envelope's attributes set aside where its words differ); and a user's record Claude
//! Code wrote after the answer it began (its late ancestor: the answer's parent chain,
//! through attachments alone, leads to it, and the turn had ended at the answer's own
//! boundary) begins no turn. So every record's place in the conversation's tree is
//! kept, by its uuid.
//!
//! An entry's words are evidence: masked whole as they are read (the run's names
//! shown), then cut. What the queue's operations and the prompts say is kept whole
//! besides, unmasked, for the replay to match them by, and never said.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::OnceLock;

use cf_e2e::pattern;
use regex::Regex;
use serde_json::Value;

use super::mask::{cut, Names};
use crate::clock;

/// How much of a line's words an entry keeps: what evidence says of it. A record is
/// read for hours and held at every look, and what is read of a line's words (the
/// jobs it starts or ends) is read before they are cut.
const KEPT: usize = 600;

/// `/^<([A-Za-z][\w-]*)(?:\s[^>]*)?>/`: the opening tag of the envelope Claude Code
/// wraps a cross-session message in (the product's `ENVELOPE`). Group 1 is its name.
const ENVELOPE: &str = r"^<([A-Za-z][A-Za-z0-9_-]*)(?:\s[^>]*)?>";

/// What a user's record of what a command typed at Claude's prompt printed opens
/// with (the product's `OUTPUT`).
const OUTPUTS: [&str; 2] = ["<local-command-stdout>", "<local-command-stderr>"];

/// What the user's record of a command typed at Claude's prompt opens with: its name
/// (the product's `COMMAND`, `<command-name>/compact</command-name>`).
const COMMAND: &str = "<command-name>/";

/// What Claude Code writes before a command typed at its prompt, as a meta record.
const CAVEAT: &str = "<local-command-caveat>";

/// What a line of a record is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    /// The words a turn began with: a person's, ConsensFlow's, or Claude Code's
    /// own when a job's end wakes it.
    Prompt,
    /// What the agent said.
    Answer,
    ToolCall,
    ToolResult,
    /// The agent's message stopped as an answer (`end_turn`, `stop_sequence`): the
    /// turn's end once its stop hooks have run, not before.
    Answered,
    /// The boundary a turn that answered writes after its stop hooks ran: a root
    /// `turn_duration`, or a `stop_hook_summary` that did not prevent continuation.
    Boundary,
    /// The provider refused the message: the turn is over, natively.
    Refused,
    /// The turn was stopped by the person.
    Interrupted,
    /// Claude Code compacted the conversation: no turn.
    Compaction,
    /// What a command typed at Claude's prompt printed (`<local-command-stdout>`,
    /// `<local-command-stderr>`): the end, natively, of the turn its command began,
    /// where it follows the command.
    Output,
    /// How many background agents and workflows a root `turn_duration` says its turn
    /// left running (`pendingBackgroundAgentCount`, `pendingWorkflowCount`; none when
    /// it says none): Claude Code wakes the agent as each ends, and says the count again
    /// at the end of every turn after, so what it says last is how many run.
    Agents(u64),
    /// A job was started in the background, with its id.
    JobStarted(String),
    /// The job with this id ended, and Claude Code said so.
    JobEnded(String),
    /// A message put on Claude Code's queue (its words the entry's).
    Enqueued,
    /// The oldest queued message taken for a turn.
    Dequeued,
    /// A queued message taken back by the user, to be sent anew.
    PoppedAll,
    /// A queued message forgotten.
    Removed,
}

/// A line of a record, brought down to what the watch reads of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// When the record says it was written, in milliseconds.
    pub at: Option<i64>,
    pub kind: Kind,
    /// Its words, as one line of evidence: masked, then cut.
    pub text: String,
    /// What a prompt or an operation on the queue says, whole and as it was: what the
    /// replay matches them by. Empty for every other entry.
    content: String,
    /// The line's uuid, and its parent's, where it says them.
    pub uuid: Option<String>,
    pub parent: Option<String>,
    /// Whether a prompt came from the queue (`promptSource: queued`).
    pub queued: bool,
}

/// A record's place in the conversation's tree, for the late ancestor.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Place {
    parent: Option<String>,
    /// Of the main conversation, not a sidechain.
    main: bool,
    attachment: bool,
    /// Two records said this uuid: no decision tells them apart.
    claimed_twice: bool,
}

/// The messages Claude Code's queue operations say are waiting, taken, or taken
/// back, by their words, as the product's reader keeps them (`queues.rs`).
#[derive(Debug, Default)]
struct Queues {
    queued: VecDeque<String>,
    dequeued: VecDeque<String>,
    popped: VecDeque<String>,
}

impl Queues {
    fn turns(&self) -> usize {
        self.queued.len() + self.dequeued.len() + self.popped.len()
    }

    fn dequeue(&mut self) {
        self.dequeued
            .push_back(self.queued.pop_front().unwrap_or_default());
    }

    /// A `popAll`: the message it names is taken back, out of the queue as the queue
    /// held it when it held it, else as the operation said it.
    fn pop_all(&mut self, content: &str) {
        let queued = position(&self.queued, content).and_then(|at| self.queued.remove(at));
        self.popped
            .push_back(queued.unwrap_or_else(|| content.to_owned()));
    }

    /// A `remove`: the message it names, forgotten, queued or taken.
    fn remove(&mut self, content: &str) {
        for queue in [&mut self.queued, &mut self.dequeued] {
            if let Some(at) = position(queue, content) {
                queue.remove(at);
            }
        }
    }

    /// The user's real prompt `text` begins a turn: out of the queues.
    fn consumed(&mut self, text: &str, from_queue: bool) {
        if let Some(at) = self.popped.iter().position(|entry| entry == text) {
            self.popped.remove(at);
        }
        if from_queue || !self.dequeued.is_empty() {
            self.dequeued.pop_front();
        } else if let Some(at) = self.queued.iter().position(|entry| entry == text) {
            self.queued.remove(at);
        }
    }
}

/// The place of the first message `removed` names (`sameQueuedContent`).
fn position(entries: &VecDeque<String>, removed: &str) -> Option<usize> {
    entries
        .iter()
        .position(|entry| same_queued_content(entry, removed))
}

/// `sameQueuedContent`: whether a queue removal names the same queued message, as the
/// product reads it (`crates/cf-harness/src/claude/record/text.rs`). Claude Code wraps
/// a cross-session message in an envelope tag whose attributes differ between the
/// enqueue and the remove record, so the envelope's attributes are set aside after an
/// exact match fails.
fn same_queued_content(queued: &str, removed: &str) -> bool {
    queued == removed || envelope(queued) == envelope(removed)
}

/// `content` with the attributes of its opening tag left out, when it opens on an
/// envelope.
fn envelope(content: &str) -> std::borrow::Cow<'_, str> {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    pattern::once(&PATTERN, ENVELOPE).replace(content, "<${1}>")
}

/// What a replay of the record by a moment found.
struct Replayed<'r> {
    open: bool,
    queued: usize,
    /// The entry that last moved the turn.
    last: Option<&'r Entry>,
    /// The background agents and workflows the turn's own boundary counted.
    agents: u64,
}

/// Where a turn stood at a moment, as the record says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stood {
    /// A turn was at work: its prompt or a tool was the last thing said.
    InFlight,
    /// The last turn had ended, or had been stopped.
    AtRest,
}

/// A harness's record of one conversation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Record {
    pub entries: Vec<Entry>,
    /// Every record's place in the conversation's tree, by its uuid.
    places: BTreeMap<String, Place>,
}

impl Record {
    /// The record Claude Code's lines `text` make, a JSON object to a line, with no
    /// run's names to show.
    pub fn claude(text: &str) -> Self {
        let mut record = Self::default();
        record.add_claude(text, &Names::default());
        record
    }

    /// The record with what Claude Code's lines `text` add to it: a record is read
    /// as it grows, the new lines alone, each entry's words masked (the run's `names`
    /// shown) before they are cut.
    pub fn add_claude(&mut self, text: &str, names: &Names) {
        for line in text
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        {
            self.place(&line);
            self.entries.extend(claude_entries(&line, names));
        }
    }

    /// The line's place in the tree, where it has a uuid.
    fn place(&mut self, line: &Value) {
        let Some(uuid) = line["uuid"].as_str().filter(|uuid| !uuid.is_empty()) else {
            return;
        };
        let place = Place {
            parent: line["parentUuid"].as_str().map(str::to_owned),
            main: line["isSidechain"] == false,
            attachment: line["type"] == "attachment",
            claimed_twice: false,
        };
        self.places
            .entry(uuid.to_owned())
            .and_modify(|claimed| claimed.claimed_twice = true)
            .or_insert(place);
    }

    /// The place of `uuid`, where one record alone claims it.
    fn lookup(&self, uuid: Option<&str>) -> Option<(&str, &Place)> {
        let (uuid, place) = self.places.get_key_value(uuid?)?;
        (!place.claimed_twice).then_some((uuid.as_str(), place))
    }

    /// `lateAncestor`: whether the user's record `user` is an ancestor of the answer
    /// `candidate`, whose turn ended at a boundary that is its child (`boundary_parent`):
    /// the answer itself and attachments the only records between, all of the main
    /// conversation.
    fn is_late(&self, user: Option<&str>, boundary_parent: Option<&str>, candidate: &str) -> bool {
        if user.is_none() || boundary_parent != Some(candidate) {
            return false;
        }
        let mut at = self.lookup(Some(candidate));
        let mut seen = BTreeSet::new();
        while let Some((uuid, place)) = at {
            if !place.main || !seen.insert(uuid) {
                return false;
            }
            if Some(uuid) == user {
                return true;
            }
            if uuid != candidate && !place.attachment {
                return false;
            }
            at = self.lookup(place.parent.as_deref());
        }
        false
    }

    /// The record replayed to `at`, as the product's reader replays Claude Code's: a
    /// prompt or the agent's work opens a turn; an answered message is the turn's
    /// candidate end, which a boundary written after it ends (a long stop hook leaves
    /// the turn at work until then); an interrupt or a refusal ends it natively, and so
    /// does what a command typed at the prompt printed, where it follows the command
    /// (the records a `/compact` writes before it, its bare command among them, read as
    /// prompts each). A compaction moves nothing: the summary of an idle conversation
    /// begins no turn, and one in a turn leaves it at work. A message queued and not
    /// taken is a turn not over; a late ancestor begins none. The agents a turn's
    /// boundary counts are that turn's, and the next turn's work counts none until its
    /// own boundary does.
    fn replay(&self, at: i64) -> Replayed<'_> {
        let mut queues = Queues::default();
        let (mut open, mut agents, mut last) = (false, 0, None);
        // The answer that may end the turn, and the boundary that ended it (its parent).
        let mut candidate: Option<Option<&str>> = None;
        let mut ended: Option<(Option<&str>, Option<&str>)> = None;
        for entry in self
            .entries
            .iter()
            .filter(|entry| entry.at.is_none_or(|said| said <= at))
        {
            match &entry.kind {
                Kind::Output if last.is_some_and(|said| follows_its_command(said, entry)) => {
                    (open, candidate, ended, agents) = (false, None, None, 0);
                }
                Kind::Prompt | Kind::Output => {
                    let late = ended.is_some_and(|(parent, answer)| {
                        answer.is_some_and(|answer| {
                            self.is_late(entry.uuid.as_deref(), parent, answer)
                        })
                    });
                    if late {
                        continue;
                    }
                    queues.consumed(&entry.content, entry.queued);
                    (open, candidate, ended, agents) = (true, None, None, 0);
                }
                Kind::ToolCall | Kind::ToolResult => {
                    queues.popped.clear();
                    (open, candidate, ended, agents) = (true, None, None, 0);
                }
                Kind::Answer => {
                    queues.popped.clear();
                    (open, ended, agents) = (true, None, 0);
                }
                Kind::Answered => {
                    queues.popped.clear();
                    (open, ended, agents) = (true, None, 0);
                    candidate = Some(entry.uuid.as_deref());
                }
                Kind::Boundary => {
                    let Some(answer) = candidate else {
                        continue;
                    };
                    open = false;
                    ended = Some((entry.parent.as_deref(), answer));
                }
                Kind::Interrupted | Kind::Refused => {
                    queues.popped.clear();
                    (open, candidate, ended, agents) = (false, None, None, 0);
                }
                Kind::Agents(count) => {
                    if ended.is_some() {
                        agents = *count;
                    }
                    continue;
                }
                Kind::Enqueued => queues.queued.push_back(entry.content.clone()),
                Kind::Dequeued => queues.dequeue(),
                Kind::PoppedAll => queues.pop_all(&entry.content),
                Kind::Removed => queues.remove(&entry.content),
                Kind::Compaction | Kind::JobStarted(_) | Kind::JobEnded(_) => continue,
            }
            last = Some(entry);
        }
        Replayed {
            open,
            queued: queues.turns(),
            last,
            agents,
        }
    }

    /// Where the turn stood at `at`, and the entry that last moved it ([`Record`]'s
    /// replay): in flight while a turn is open or a message waits in the queue. None
    /// where nothing was said by then.
    pub fn stood_at(&self, at: i64) -> Option<(Stood, &Entry)> {
        let replayed = self.replay(at);
        let stood = if replayed.open || replayed.queued > 0 {
            Stood::InFlight
        } else {
            Stood::AtRest
        };
        replayed.last.map(|entry| (stood, entry))
    }

    /// The jobs started by `at` that no notification had said were over by then:
    /// each by its id, with the entry that started it.
    pub fn jobs_pending_at(&self, at: i64) -> Vec<&Entry> {
        let by_then = || {
            self.entries
                .iter()
                .filter(move |entry| entry.at.is_none_or(|said| said <= at))
        };
        let ended: BTreeSet<&str> = by_then()
            .filter_map(|entry| match &entry.kind {
                Kind::JobEnded(id) => Some(id.as_str()),
                _ => None,
            })
            .collect();
        let mut seen = BTreeSet::new();
        by_then()
            .filter(|entry| match &entry.kind {
                Kind::JobStarted(id) => !ended.contains(id.as_str()) && seen.insert(id.as_str()),
                _ => false,
            })
            .collect()
    }

    /// Whether the last thing the record says of its turns is a compaction: no turn
    /// since Claude Code compacted the conversation (a job's start or end, or a count
    /// of agents, aside, and the records of the command that ended there: what it
    /// printed, the command, and the words before it, which a `/compact` typed at the
    /// prompt writes after its summary).
    pub fn ends_in_compaction(&self) -> bool {
        let mut said = self
            .entries
            .iter()
            .rev()
            .filter(|entry| {
                !matches!(
                    entry.kind,
                    Kind::JobStarted(_) | Kind::JobEnded(_) | Kind::Agents(_)
                )
            })
            .peekable();
        let prompt = |entry: &Entry, opening: &str| {
            entry.kind == Kind::Prompt && entry.content.starts_with(opening)
        };
        if said.next_if(|entry| entry.kind == Kind::Output).is_some() {
            said.next_if(|entry| prompt(entry, COMMAND));
            said.next_if(|entry| prompt(entry, CAVEAT));
        }
        said.next()
            .is_some_and(|entry| entry.kind == Kind::Compaction)
    }

    /// How many background agents and workflows the last turn by `at` left running, as
    /// its own boundary counted them: none where it counted none or said nothing of
    /// them, and none once the next turn began. A count of an earlier turn is never
    /// carried over.
    pub fn agents_pending_at(&self, at: i64) -> u64 {
        self.replay(at).agents
    }

    /// When it last said anything: a turn's work, a compaction, a job's end.
    pub fn last_said(&self) -> Option<i64> {
        self.entries.iter().filter_map(|entry| entry.at).max()
    }

    /// Its last `count` entries said by `at`, as lines of evidence.
    pub fn lines_by(&self, at: i64, count: usize) -> Vec<String> {
        let said: Vec<&Entry> = self
            .entries
            .iter()
            .filter(|entry| entry.at.is_none_or(|when| when <= at))
            .collect();
        said[said.len().saturating_sub(count)..]
            .iter()
            .map(|entry| entry.line())
            .collect()
    }
}

impl Entry {
    /// The entry as a line of evidence: when, what, and the start of its words (masked
    /// as they were read).
    pub fn line(&self) -> String {
        let when = self
            .at
            .and_then(|at| jiff::Timestamp::from_millisecond(at).ok())
            .map_or_else(|| "?".to_owned(), clock::iso);
        format!(
            "{when} {:?}: {}",
            self.kind,
            crate::js::first(&crate::js::squeeze(&self.text), 160)
        )
    }
}

/// The ids of the jobs a text says are running in the background: Claude Code's
/// and Devin's own words for a command moved there.
pub fn jobs_started_in(text: &str) -> Vec<String> {
    static STARTED: OnceLock<Regex> = OnceLock::new();
    pattern::once(
        &STARTED,
        r"running in (?:the )?background with ID: ?([A-Za-z0-9_-]+)",
    )
    .captures_iter(text)
    .map(|found| found[1].to_owned())
    .collect()
}

/// The ids of the jobs the `<task-notification>`s of a text say have ended.
pub fn jobs_ended_in(text: &str) -> Vec<String> {
    static ENDED: OnceLock<Regex> = OnceLock::new();
    pattern::once(
        &ENDED,
        r"(?s)<task-notification>.*?<task-id>\s*([^<\s]+)\s*</task-id>",
    )
    .captures_iter(text)
    .map(|found| found[1].to_owned())
    .collect()
}

/// The words of a message's content: a string, or the text of its parts.
fn words_of(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts
            .iter()
            .map(|part| match part["type"].as_str() {
                Some("text") => part["text"].as_str().unwrap_or_default().to_owned(),
                Some("tool_result") => words_of(&part["content"]),
                _ => String::new(),
            })
            .filter(|words| !words.is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// Whether a user's record is one Claude Code wrote itself: what a job's end
/// queued, which is no word of a person's or of ConsensFlow's.
fn claudes_own(line: &Value) -> bool {
    line["turnOrigin"] == "task_notification" || line["promptSource"] == "system"
}

/// `Number.isSafeInteger`, `>= 0`: a count as Claude Code writes one.
fn is_count(value: &Value) -> bool {
    value.as_f64().is_some_and(|count| {
        count >= 0.0 && count.fract() == 0.0 && count <= 9_007_199_254_740_991.0
    })
}

/// Whether `output` follows the command it is what was printed of, as the product
/// reads it: the entry that last moved the turn is the user's command, and `output`
/// is its child.
fn follows_its_command(last: &Entry, output: &Entry) -> bool {
    last.kind == Kind::Prompt
        && last.content.starts_with(COMMAND)
        && last.uuid.is_some()
        && last.uuid == output.parent
}

/// Whether a system record is the boundary that ends a turn that answered, as the
/// product reads one: a `turn_duration` of the root conversation (in it, with a
/// duration and a message count, and its counts of what is pending counts where it
/// has them), or a `stop_hook_summary` that did not prevent continuation.
fn is_boundary(line: &Value) -> bool {
    match line["subtype"].as_str() {
        Some("turn_duration") => {
            line["isSidechain"] == false
                && line["durationMs"].as_f64().is_some_and(|ms| ms >= 0.0)
                && is_count(&line["messageCount"])
                && ["pendingBackgroundAgentCount", "pendingWorkflowCount"]
                    .iter()
                    .all(|name| line.get(*name).is_none_or(is_count))
        }
        Some("stop_hook_summary") => line["preventedContinuation"] == false,
        _ => false,
    }
}

/// What one line of Claude Code's record says, its words masked with the run's
/// `names` shown. A line of a sidechain (a subagent's) is none of the main
/// conversation's turns.
fn claude_entries(line: &Value, names: &Names) -> Vec<Entry> {
    let at = line["timestamp"].as_str().and_then(clock::millis);
    let (uuid, parent) = (
        line["uuid"].as_str().map(str::to_owned),
        line["parentUuid"].as_str().map(str::to_owned),
    );
    let queued = line["promptSource"] == "queued";
    let entry = |kind: Kind, text: &str| Entry {
        at,
        content: if matches!(
            kind,
            Kind::Prompt | Kind::Enqueued | Kind::PoppedAll | Kind::Removed
        ) {
            text.to_owned()
        } else {
            String::new()
        },
        kind,
        text: cut(text, KEPT, names),
        uuid: uuid.clone(),
        parent: parent.clone(),
        queued,
    };
    if line["isSidechain"] == true {
        return Vec::new();
    }
    let content = &line["message"]["content"];
    match line["type"].as_str() {
        Some("system") if is_boundary(line) => {
            let subtype = line["subtype"].as_str().unwrap_or_default();
            let mut said = vec![entry(Kind::Boundary, subtype)];
            if subtype == "turn_duration" {
                let count = ["pendingBackgroundAgentCount", "pendingWorkflowCount"]
                    .iter()
                    .filter_map(|name| line[*name].as_u64())
                    .sum();
                said.push(entry(Kind::Agents(count), subtype));
            }
            said
        }
        Some("system") if line["subtype"] == "compact_boundary" => {
            vec![entry(Kind::Compaction, "compact_boundary")]
        }
        Some("system") => Vec::new(),
        Some("queue-operation") => {
            let text = line["content"].as_str().unwrap_or_default();
            let mut said: Vec<Entry> = jobs_ended_in(text)
                .into_iter()
                .map(|id| entry(Kind::JobEnded(id), text))
                .collect();
            let operation = match line["operation"].as_str() {
                Some("enqueue") => Some(Kind::Enqueued),
                Some("dequeue") => Some(Kind::Dequeued),
                Some("popAll") => Some(Kind::PoppedAll),
                Some("remove") => Some(Kind::Removed),
                _ => None,
            };
            said.extend(operation.map(|kind| entry(kind, text)));
            said
        }
        Some("assistant") => {
            let mut said: Vec<Entry> = content
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|part| match part["type"].as_str() {
                    Some("text") => Some(entry(
                        Kind::Answer,
                        part["text"].as_str().unwrap_or_default(),
                    )),
                    Some("tool_use") => Some(entry(
                        Kind::ToolCall,
                        part["name"].as_str().unwrap_or_default(),
                    )),
                    _ => None,
                })
                .collect();
            if line["isApiErrorMessage"] == true {
                said.push(entry(Kind::Refused, ""));
            } else if matches!(
                line["message"]["stop_reason"].as_str(),
                Some("end_turn" | "stop_sequence")
            ) {
                said.push(entry(Kind::Answered, ""));
            }
            said
        }
        Some("user") => {
            let text = words_of(content);
            if line["isCompactSummary"] == true {
                return vec![entry(Kind::Compaction, &text)];
            }
            let tool = content
                .as_array()
                .is_some_and(|parts| parts.iter().any(|part| part["type"] == "tool_result"));
            if tool {
                let mut said = vec![entry(Kind::ToolResult, &text)];
                let named = line["toolUseResult"]["backgroundTaskId"]
                    .as_str()
                    .map(|id| vec![id.to_owned()])
                    .unwrap_or_else(|| jobs_started_in(&text));
                said.extend(
                    named
                        .into_iter()
                        .map(|id| entry(Kind::JobStarted(id), &text)),
                );
                return said;
            }
            if text
                .trim_start()
                .starts_with("[Request interrupted by user")
            {
                return vec![entry(Kind::Interrupted, &text)];
            }
            if OUTPUTS.iter().any(|tag| text.starts_with(tag)) {
                return vec![entry(Kind::Output, &text)];
            }
            let mut said: Vec<Entry> = if claudes_own(line) {
                jobs_ended_in(&text)
                    .into_iter()
                    .map(|id| entry(Kind::JobEnded(id), &text))
                    .collect()
            } else {
                Vec::new()
            };
            // A user's record with no words (an image alone, a meta record) begins no
            // turn, as the product reads it.
            if !text.trim().is_empty() {
                said.push(entry(Kind::Prompt, &text));
            }
            said
        }
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests;

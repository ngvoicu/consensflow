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

/// What a record of what a command typed at Claude's prompt printed opens with,
/// the user's or the system's (the product's `OUTPUT`).
const OUTPUTS: [&str; 2] = ["<local-command-stdout>", "<local-command-stderr>"];

/// The commands typed at Claude's prompt that run no model after what they printed (the
/// product's `QUIET_COMMAND`): the user's record of one opens with its name,
/// `<command-name>/compact</command-name>`. Another (2.1.303's `/plan <description>`)
/// may go on to ask the model.
const QUIET: [&str; 5] = ["clear", "compact", "effort", "model", "reload-skills"];

/// What Claude Code writes before a command typed at its prompt, as a meta record.
const CAVEAT: &str = "<local-command-caveat>";

/// `/^\/compact(?:\s[\s\S]*)?$/`: what Claude Code writes as the user's turn the moment
/// `/compact` is typed, before it compacts (the product's `BARE_COMPACT`): the command as
/// typed, with its instructions where it was given some.
const BARE_COMPACT: &str = r"^/compact(?:\s[\s\S]*)?$";

/// What opens Claude Code's notification of a job (the product's `NOTIFICATION`).
const NOTIFICATION: &str = "<task-notification>";

/// The statuses Claude Code writes of a job that is over (the product's `OVER`).
const OVER: [&str; 4] = ["completed", "failed", "killed", "stopped"];

/// The marker of a shell 2.1.301's summary of the shells a previous process left found
/// still running, before its id: it leaves it running (the product's `LIVE`).
const LIVE: &str = "__orphan_summary_live__:";

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
    /// `<local-command-stderr>`), as the user's record: the end, natively, of the
    /// turn its command began, where it follows the command; a prompt's words where
    /// it does not, as the product reads them.
    Output,
    /// The same, as the system's record (`local_command`): the end of its command's
    /// turn where it follows the command, and nothing where it does not, as the
    /// product reads it. A command the system writes whole (`/mcp`, `/plugin`) has
    /// its own record the system's too, which is nothing, and began no turn.
    Printed,
    /// An assistant's record with neither words nor a tool's call (its thinking alone):
    /// the agent is at work all the same, as the product reads it.
    Thought,
    /// What Claude Code wrote into the record that begins no turn and ends none, as the
    /// product reads it: a human's message it took into a turn under way (an attachment), and
    /// the companion of a turn (the sources of the images it attached).
    Noted,
    /// A user's record Claude Code wrote to the transcript alone (`queueTranscriptOnly`:
    /// 2.1.292's notification of the jobs a resumed session's last process left, after which
    /// it waits at its prompt): it takes its message out of the queue, and begins no turn.
    Transcribed,
    /// How many background agents and workflows a root `turn_duration` says its turn
    /// left running (`pendingBackgroundAgentCount`, `pendingWorkflowCount`; none when
    /// it says none): Claude Code wakes the agent as each ends, and says the count again
    /// at the end of every turn after, so what it says last is how many run.
    Agents(u64),
    /// A job was started in the background, with its id.
    JobStarted(String),
    /// The job with this id ended, and Claude Code said so: a notification of its own
    /// whose head says the job is over, delivered.
    JobEnded(String),
    /// The job with this id was stopped by a `TaskStop` call of the main conversation whose
    /// result says so: Claude Code writes no notification of a job its agent stopped.
    JobStopped(String),
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
    /// Whether the record is a meta record (`isMeta`): the words Claude Code writes before a
    /// command are one, and a bare `/compact` typed is not.
    meta: bool,
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
    /// How many messages wait in any state, but a notification queued that begins no turn
    /// (the product's `Queues::turns`).
    fn turns(&self) -> usize {
        let queued = self
            .queued
            .iter()
            .filter(|content| !begins_no_turn(content));
        queued.count() + self.dequeued.len() + self.popped.len()
    }

    /// A `dequeue`: the oldest queued message is taken, or an empty one when the record never
    /// queued it. A notification that begins no turn is gone once taken.
    fn dequeue(&mut self) {
        let taken = self.queued.pop_front().unwrap_or_default();
        if !begins_no_turn(&taken) {
            self.dequeued.push_back(taken);
        }
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
    /// The operation on Claude Code's queue that last came: it moves no turn, but a message
    /// that waits is a turn not over.
    queue_last: Option<&'r Entry>,
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
    /// The uuids of the user's records Claude Code marks as its summary of a compaction
    /// (`isCompactSummary: true`): the product copies each as an item of that id.
    summaries: BTreeSet<String>,
    /// When a process that wrote the record was proved gone, in milliseconds ([`Record::gone_at`]).
    gone: BTreeSet<i64>,
    /// The `TaskStop` calls the main conversation made and no result has answered, by the id of
    /// the block that made each: the result of one says the job it stopped (the product's
    /// `task_stops`).
    task_stops: BTreeSet<String>,
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
            if line["type"] == "user" && line["isCompactSummary"] == true {
                if let Some(uuid) = line["uuid"].as_str() {
                    self.summaries.insert(uuid.to_owned());
                }
            }
            self.entries
                .extend(claude_entries(&line, names, &mut self.task_stops));
        }
    }

    /// Whether the record `uuid` is one Claude Code marks as its summary of a compaction,
    /// by the record's own flag: one line alone carries the uuid, and it is flagged. A uuid
    /// another line carries too, flagged or not, is no summary's (the product's copy keeps
    /// the later line's words under it).
    pub fn is_summary(&self, uuid: &str) -> bool {
        self.summaries.contains(uuid) && self.lookup(Some(uuid)).is_some()
    }

    /// The process that wrote the record was proved gone at `at`, in milliseconds: the pane
    /// host said its window exited and named nothing of it that it could not end. What it had
    /// queued in its memory and not begun went with it, and so did the background agents its
    /// last turn left running, though no line says so: what the record says after `at` is a
    /// later process's. The product's reader empties them where ConsensFlow proved the same
    /// before it opened the window again (T-442, `crates/cf-harness/src/claude/record.rs`).
    pub fn gone_at(&mut self, at: i64) {
        self.gone.insert(at);
    }

    /// Whether a line of the record carries `uuid`.
    pub fn carries(&self, uuid: &str) -> bool {
        self.places.contains_key(uuid)
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
    /// own boundary does. Where the process that wrote it was proved gone ([`Record::gone_at`]),
    /// its queue and its agents are emptied there, and its last answer ends no turn again,
    /// as the product's reader empties them at a relaunch.
    fn replay(&self, at: i64) -> Replayed<'_> {
        let mut queues = Queues::default();
        let (mut open, mut agents, mut last) = (false, 0, None);
        let mut queue_last = None;
        // The answer that may end the turn, and the boundary that ended it (its parent).
        let mut candidate: Option<Option<&str>> = None;
        let mut ended: Option<(Option<&str>, Option<&str>)> = None;
        // The bare `/compact` that began the turn, by the record it is a child of, while
        // nothing has moved it since: what a cancel of the compaction ends.
        let mut typed: Option<&str> = None;
        let mut gone = self.gone.range(..=at).peekable();
        for entry in self
            .entries
            .iter()
            .filter(|entry| entry.at.is_none_or(|said| said <= at))
        {
            let relaunched = entry.at.is_some_and(|said| {
                let mut crossed = false;
                while gone.next_if(|gone| **gone < said).is_some() {
                    crossed = true;
                }
                crossed
            });
            if relaunched {
                (queues, candidate, agents, typed) = (Queues::default(), None, 0, None);
            }
            // The user's records and the assistant's forget the bare `/compact` a cancel would
            // end (`typed_compact`): only the words before a command that come next are its
            // cancel. Claude Code's own bookkeeping, a boundary, an operation on its queue,
            // forgets nothing.
            let compacting = if forgets_the_command(&entry.kind) {
                typed.take()
            } else {
                None
            };
            match &entry.kind {
                Kind::Output | Kind::Printed
                    if last.is_some_and(|said| follows_its_command(said, entry)) =>
                {
                    (open, candidate, ended, agents) = (false, None, None, 0);
                }
                Kind::Printed => continue,
                // A compaction cancelled with Escape (Claude Code 2.1.304 and 2.1.310) writes the
                // bare `/compact` as it was typed and, at the Escape, the words before a command
                // as a sibling of it, and nothing more: the end of the turn the bare command
                // began.
                Kind::Prompt
                    if entry.meta
                        && entry.content.starts_with(CAVEAT)
                        && compacting.is_some()
                        && compacting == entry.parent.as_deref() =>
                {
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
                    if entry.kind == Kind::Prompt && !entry.meta && is_bare_compact(&entry.content)
                    {
                        typed = entry.parent.as_deref();
                    }
                }
                Kind::ToolCall => {
                    queues.popped.clear();
                    (open, candidate, ended, agents) = (true, None, None, 0);
                }
                // A tool's result closes its call and says nothing of the turn, as the product
                // reads it: the call began or went on with the assistant's record.
                Kind::ToolResult => continue,
                Kind::Answer | Kind::Thought => {
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
                Kind::Enqueued | Kind::Dequeued | Kind::PoppedAll | Kind::Removed => {
                    match &entry.kind {
                        Kind::Enqueued => queues.queued.push_back(entry.content.clone()),
                        Kind::Dequeued => queues.dequeue(),
                        Kind::PoppedAll => queues.pop_all(&entry.content),
                        _ => queues.remove(&entry.content),
                    }
                    queue_last = Some(entry);
                    continue;
                }
                Kind::Transcribed => {
                    queues.consumed(&entry.content, entry.queued);
                    continue;
                }
                Kind::Compaction
                | Kind::Noted
                | Kind::JobStarted(_)
                | Kind::JobEnded(_)
                | Kind::JobStopped(_) => continue,
            }
            last = Some(entry);
        }
        if gone.next().is_some() {
            (queues, agents) = (Queues::default(), 0);
        }
        Replayed {
            open,
            queued: queues.turns(),
            last,
            queue_last,
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
        let moved = replayed
            .last
            .or_else(|| replayed.queue_last.filter(|_| replayed.queued > 0));
        moved.map(|entry| (stood, entry))
    }

    /// The jobs started by `at` that Claude Code had not said were over by then, as the
    /// product's reader counts the jobs that run (`jobs`): each by its id, with the entry
    /// that started it. A job is over when a notification of Claude Code's own says it is,
    /// by its head's status, or a `TaskStop` call's result says it stopped it; one the
    /// process that started it was proved gone from ([`Record::gone_at`]) is retired, no
    /// longer waited on.
    pub fn jobs_pending_at(&self, at: i64) -> Vec<&Entry> {
        let by_then = || {
            self.entries
                .iter()
                .filter(move |entry| entry.at.is_none_or(|said| said <= at))
        };
        let ended: BTreeSet<&str> = by_then()
            .filter_map(|entry| match &entry.kind {
                Kind::JobEnded(id) | Kind::JobStopped(id) => Some(id.as_str()),
                _ => None,
            })
            .collect();
        let retired = |entry: &Entry| {
            entry
                .at
                .is_some_and(|said| self.gone.range(said..=at).next().is_some())
        };
        let mut seen = BTreeSet::new();
        by_then()
            .filter(|entry| match &entry.kind {
                Kind::JobStarted(id) => {
                    !ended.contains(id.as_str()) && !retired(entry) && seen.insert(id.as_str())
                }
                _ => false,
            })
            .collect()
    }

    /// Whether the last thing the record says of its turns is a compaction: no turn
    /// since Claude Code compacted the conversation (a job's start or end, or a count
    /// of agents, aside, and the records of each command after it that runs no model:
    /// what it printed, the command, and the words before it, which a `/compact` typed
    /// at the prompt writes after its summary, and what a command the system writes
    /// whole printed, which began no turn). A command that may go on to the model is
    /// no such thing: a turn may be at work after it.
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
        while let Some(output) =
            said.next_if(|entry| matches!(entry.kind, Kind::Output | Kind::Printed))
        {
            let its_command = |entry: &&Entry| {
                quiet(entry) && entry.uuid.is_some() && entry.uuid == output.parent
            };
            if said.next_if(its_command).is_some() {
                said.next_if(|entry| {
                    entry.kind == Kind::Prompt && entry.content.starts_with(CAVEAT)
                });
            } else if output.kind == Kind::Output {
                return false;
            }
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

/// The ids of the jobs a notification says are over, as the product reads one (`jobs`): the
/// text opens with one, and its head says by its `<status>` that the job is over
/// (`completed`, `failed`, `killed`, `stopped`); a notification with no status, as a
/// monitor's event has, ends none. Only the head is read, field after field in the order
/// Claude Code writes them, so what a command printed in the body or after it ends nothing,
/// and the jobs a summary marks as still running are left running.
pub fn jobs_ended_in(text: &str) -> Vec<String> {
    let head = head(text);
    if !head.status.is_some_and(|status| OVER.contains(&status)) {
        return Vec::new();
    }
    let live: BTreeSet<&str> = head
        .ids
        .iter()
        .filter_map(|id| id.strip_prefix(LIVE))
        .collect();
    head.ids
        .iter()
        .filter(|id| !id.starts_with(LIVE) && !live.contains(*id))
        .map(|id| (*id).to_owned())
        .collect()
}

/// What the head of a notification says: the jobs it names, in order, and how it went.
#[derive(Default)]
struct Head<'a> {
    ids: Vec<&'a str>,
    status: Option<&'a str>,
}

/// The fields of the head of a notification, in the order Claude Code writes them (the
/// product's `FIELDS`): the first names a job, and the fifth says how it went.
const FIELDS: [&str; 7] = [
    "task-id",
    "tool-use-id",
    "task-type",
    "output-file",
    "status",
    "summary",
    "event",
];
const TASK_ID: usize = 0;
const STATUS: usize = 4;

/// The head of the notification `text`: nothing where the text does not open with one.
fn head(text: &str) -> Head<'_> {
    let mut head = Head::default();
    let Some(rest) = text.strip_prefix(NOTIFICATION) else {
        return head;
    };
    let mut lines = rest.split('\n');
    // The opening tag is a line of its own.
    if lines.next() != Some("") {
        return head;
    }
    let mut at = None;
    for line in lines {
        let Some((place, value)) = field(line) else {
            break;
        };
        // Each field once, in its order; only a job's id again.
        if at.is_some_and(|at| place < at || (place == at && place != TASK_ID)) {
            break;
        }
        at = Some(place);
        match place {
            TASK_ID if !value.is_empty() => head.ids.push(value),
            STATUS => head.status = Some(value),
            _ => {}
        }
    }
    head
}

/// The place in [`FIELDS`] of the field `line` is (`<tag>value</tag>`, the whole line), and
/// its value, trimmed: none where the line is no field of a head.
fn field(line: &str) -> Option<(usize, &str)> {
    let (tag, rest) = line.strip_prefix('<')?.split_once('>')?;
    let place = FIELDS.iter().position(|field| *field == tag)?;
    let value = rest
        .strip_suffix('>')?
        .strip_suffix(tag)?
        .strip_suffix("</")?;
    Some((place, value.trim()))
}

/// Whether `text` is the notification of a job Claude Code itself ended (`killed`), which it
/// takes from its queue and begins no turn for (the product's `begins_no_turn`).
fn begins_no_turn(text: &str) -> bool {
    head(text).status == Some("killed")
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

/// Whether a user's record is the turn Claude Code begins by itself for a notification of a
/// job, by what the record says of where it came from (the product's `is_notification`): from
/// the system (`promptSource`), for a notification (`turnOrigin`, and `origin.kind`; 2.1.241
/// has only the second, 2.1.277 on both). No words of it make it one: a message that quotes a
/// notification is none.
fn claudes_own(line: &Value) -> bool {
    line["promptSource"] == "system"
        && (line["turnOrigin"] == "task_notification"
            || line["origin"]["kind"] == "task-notification")
}

/// An id that is text and not empty (the product's `text_id`).
fn text_id(value: &Value) -> Option<String> {
    value
        .as_str()
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
}

/// The job a tool result says it started, by its id (the product's `job`): a command run in
/// the background (`backgroundTaskId`), or a monitor, whose start says its task and its time
/// (2.1.305: `taskId`, and `timeoutMs`, a number: 0 for one that is `persistent`).
fn job_started_by(line: &Value) -> Option<String> {
    let result = &line["toolUseResult"];
    if let Some(id) = text_id(&result["backgroundTaskId"]) {
        return Some(id);
    }
    result["timeoutMs"].as_i64()?;
    text_id(&result["taskId"])
}

/// The job a `TaskStop` call's result says it stopped (the product's `stopped`): where the
/// record holds that one result alone, whose result is no error, and which answers a
/// `TaskStop` call of the main conversation made before (`stops`: the call is answered).
fn job_stopped_by(line: &Value, stops: &mut BTreeSet<String>) -> Option<String> {
    let id = text_id(&line["toolUseResult"]["task_id"])?;
    let results: Vec<&Value> = line["message"]["content"]
        .as_array()?
        .iter()
        .filter(|part| part["type"] == "tool_result")
        .collect();
    let [result] = results.as_slice() else {
        return None;
    };
    if result["is_error"] == true {
        return None;
    }
    stops.remove(result["tool_use_id"].as_str()?).then_some(id)
}

/// Whether a user's record is Claude Code's own record of an interrupt (the product's
/// `is_interrupt`): a list of one text block, which says one of its two markers and nothing
/// else. What the user types is text, never such a list.
fn is_interrupt(line: &Value) -> bool {
    let message = &line["message"];
    message["role"] == "user"
        && matches!(
            message["content"].as_array().map(Vec::as_slice),
            Some([only]) if only["type"] == "text"
                && matches!(
                    only["text"].as_str(),
                    Some("[Request interrupted by user]" | "[Request interrupted by user for tool use]")
                )
        )
}

/// `Number.isSafeInteger`, `>= 0`: a count as Claude Code writes one.
fn is_count(value: &Value) -> bool {
    value.as_f64().is_some_and(|count| {
        count >= 0.0 && count.fract() == 0.0 && count <= 9_007_199_254_740_991.0
    })
}

/// Whether the record of this kind is a user's or the assistant's: the next of either forgets
/// the bare `/compact` that a cancel of the compaction would end.
fn forgets_the_command(kind: &Kind) -> bool {
    matches!(
        kind,
        Kind::Prompt
            | Kind::Output
            | Kind::Transcribed
            | Kind::Interrupted
            | Kind::ToolResult
            | Kind::Compaction
            | Kind::Answer
            | Kind::Thought
            | Kind::ToolCall
            | Kind::Answered
            | Kind::Refused
    )
}

/// Whether `text` is a `/compact` as it was typed ([`BARE_COMPACT`]).
fn is_bare_compact(text: &str) -> bool {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    pattern::once(&PATTERN, BARE_COMPACT).is_match(text)
}

/// Whether `output` follows the command it is what was printed of, and the command
/// runs no model after it, as the product reads it: the entry that last moved the
/// turn is the user's command, one of the quiet ones, and `output` is its child.
fn follows_its_command(last: &Entry, output: &Entry) -> bool {
    quiet(last) && last.uuid.is_some() && last.uuid == output.parent
}

/// Whether `entry` is the user's record of a command that runs no model ([`QUIET`]).
fn quiet(entry: &Entry) -> bool {
    entry.kind == Kind::Prompt
        && QUIET.iter().any(|name| {
            entry
                .content
                .strip_prefix("<command-name>/")
                .and_then(|rest| rest.strip_prefix(name))
                .is_some_and(|rest| rest.starts_with("</command-name>"))
        })
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
fn claude_entries(line: &Value, names: &Names, stops: &mut BTreeSet<String>) -> Vec<Entry> {
    let at = line["timestamp"].as_str().and_then(clock::millis);
    let (uuid, parent) = (
        line["uuid"].as_str().map(str::to_owned),
        line["parentUuid"].as_str().map(str::to_owned),
    );
    let queued = line["promptSource"] == "queued";
    let meta = line["isMeta"] == true;
    let entry = |kind: Kind, text: &str| Entry {
        at,
        content: if matches!(
            kind,
            Kind::Prompt | Kind::Transcribed | Kind::Enqueued | Kind::PoppedAll | Kind::Removed
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
        meta,
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
        // What a command printed, written as the system's record (a `/clear`'s, a
        // `/compact` that found nothing to compact), as the product reads one.
        Some("system")
            if line["subtype"] == "local_command"
                && line["isMeta"] == false
                && line["level"] == "info"
                && line["content"]
                    .as_str()
                    .is_some_and(|text| OUTPUTS.iter().any(|tag| text.starts_with(tag))) =>
        {
            vec![entry(
                Kind::Printed,
                line["content"].as_str().unwrap_or_default(),
            )]
        }
        Some("system") => Vec::new(),
        // Only Claude Code's own notification ends a job, never words that look like one: a
        // message typed while a turn is open is queued too, and the queue's record says nothing
        // of where its content came from. A notification ends its jobs once Claude Code
        // delivers it (the user's record that begins its turn, or the attachment that brings it
        // into a turn under way), and until then the job is pending.
        Some("queue-operation") => {
            let text = line["content"].as_str().unwrap_or_default();
            let operation = match line["operation"].as_str() {
                Some("enqueue") => Some(Kind::Enqueued),
                Some("dequeue") => Some(Kind::Dequeued),
                Some("popAll") => Some(Kind::PoppedAll),
                Some("remove") => Some(Kind::Removed),
                _ => None,
            };
            operation
                .map(|kind| entry(kind, text))
                .into_iter()
                .collect()
        }
        // What an attachment brings into a turn under way: Claude Code's own notification of a
        // job, which ends the jobs its head says are over, and a human's message, which begins
        // no turn (the product's `Notified` and `Absorbed`).
        Some("attachment") => {
            let attachment = &line["attachment"];
            if attachment["type"] != "queued_command" {
                return Vec::new();
            }
            match attachment["commandMode"].as_str() {
                Some("task-notification") => {
                    let text = attachment["prompt"].as_str().unwrap_or_default();
                    jobs_ended_in(text)
                        .into_iter()
                        .map(|id| entry(Kind::JobEnded(id), text))
                        .collect()
                }
                Some("prompt") if attachment["origin"]["kind"] == "human" => {
                    let text = words_of(&attachment["prompt"]);
                    if uuid.is_some() && !text.trim().is_empty() {
                        vec![entry(Kind::Noted, &text)]
                    } else {
                        Vec::new()
                    }
                }
                _ => Vec::new(),
            }
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
                    Some("tool_use") => {
                        if part["name"] == "TaskStop" {
                            stops.extend(text_id(&part["id"]));
                        }
                        Some(entry(
                            Kind::ToolCall,
                            part["name"].as_str().unwrap_or_default(),
                        ))
                    }
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
            } else if said.is_empty() {
                // Its thinking alone: the agent is at work, as the product reads any record of it.
                said.push(entry(Kind::Thought, ""));
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
                // The job a result says it started (a command run in the background, a
                // monitor), by the structured fields of the record, and the job its result
                // says a `TaskStop` call stopped.
                let mut said = vec![entry(Kind::ToolResult, &text)];
                said.extend(job_started_by(line).map(|id| entry(Kind::JobStarted(id), &text)));
                said.extend(
                    job_stopped_by(line, stops).map(|id| entry(Kind::JobStopped(id), &text)),
                );
                return said;
            }
            if is_interrupt(line) {
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
            // turn, as the product reads it; nor does one Claude Code wrote to the
            // transcript alone (`queueTranscriptOnly`: 2.1.292's notification of the jobs a
            // resumed session's last process left, after which it waits at its prompt), nor
            // the companion of a turn (`turnCompanion`: the sources of the images it attached
            // to the message before it).
            if !text.trim().is_empty() {
                let kind = if line["queueTranscriptOnly"] == true {
                    Kind::Transcribed
                } else if line["turnCompanion"] == true {
                    Kind::Noted
                } else {
                    Kind::Prompt
                };
                said.push(entry(kind, &text));
            }
            said
        }
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests;

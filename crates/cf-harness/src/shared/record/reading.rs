//! What a look at a harness's own record of a conversation reads: the record's
//! items in its order, and what they say of the turn, written as Node wrote a
//! reading.

use std::sync::Arc;

use cf_base::js;
use serde::ser::{SerializeMap, Serializer};
use serde::Serialize;
use serde_json::Value;

use crate::shared::quota::Quota;

/// A look's reading: what the record says, or that it could not say, with
/// the reason (`{unknown: true, reason}`).
#[derive(Debug, Clone, PartialEq)]
pub enum Reading {
    /// The look could not read the record (it failed, or never answered): what
    /// the record holds is not known.
    Unknown(String),
    /// The record is not there: a conversation its harness has written nothing of
    /// yet. It was read, and holds nothing. Said as an unknown one is, its reason
    /// as written.
    Missing(String),
    Known(Record),
}

impl Reading {
    /// `unreadable`: a failure to read, said after `unreadable: `.
    pub(crate) fn unreadable(reason: &str) -> Self {
        Reading::Unknown(format!("unreadable: {reason}"))
    }

    /// The reading of a record that is not there, said `unreadable: no <what>`.
    pub(crate) fn missing(what: &str) -> Self {
        Reading::Missing(format!("unreadable: no {what}"))
    }

    /// Whether the look could not read the record at all, so what it holds is not
    /// known. A record that is not there ([`Reading::Missing`]) was read.
    pub fn read_failed(&self) -> bool {
        matches!(self, Reading::Unknown(_))
    }

    /// Why the record was not read, where it was not.
    pub fn reason(&self) -> Option<&str> {
        match self {
            Reading::Unknown(reason) | Reading::Missing(reason) => Some(reason),
            Reading::Known(_) => None,
        }
    }
}

impl Serialize for Reading {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Reading::Unknown(reason) | Reading::Missing(reason) => {
                let mut map = serializer.serialize_map(Some(2))?;
                map.serialize_entry("unknown", &true)?;
                map.serialize_entry("reason", reason)?;
                map.end()
            }
            Reading::Known(record) => record.serialize(serializer),
        }
    }
}

/// What a record says (`resultBase`): its items, and what they say of the
/// turn. The quota is shared, as Node shared the object: the scheduler
/// tells an old refusal from a new one by the object it holds.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    pub items: Vec<Item>,
    pub in_flight: bool,
    /// Its own question dialog is open: the window waits for the human's answer.
    pub asking: bool,
    pub failed: bool,
    /// Why the turn failed, in the harness's own words where its record has
    /// them (an API's error, a provider's message): the reason the engine's
    /// note gives. None where the record says no more than that it failed. No
    /// reading Node wrote has it.
    #[serde(skip)]
    pub failure: Option<String>,
    pub quota: Option<Arc<Quota>>,
    pub settlement: Settlement,
    /// What the settled turn left running that its harness wakes its agent for as
    /// each ends: said only for what there is, and no reading Node wrote has it.
    #[serde(flatten)]
    pub pending: Pending,
    /// The monitors among the jobs pending, as the harness's record says them (Claude
    /// Code's): said only where there are any, and no reading Node wrote has them.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub monitors: Vec<Monitor>,
    /// Where the record's lines stand: those torn and passed over, the record each item
    /// began in, and what the last of them leave in the harness's input box. Never said:
    /// no reading Node wrote has it.
    #[serde(skip)]
    pub lines: Box<Lines>,
}

/// Where a record's lines stand, as its reader read them (T-223, T-290, T-483). Behind a box in
/// [`Record`], which a reader's `Err` holds too ([`crate::records::Open`]): a reading stays
/// as small as one without them (the merge with turn-end, T-494).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Lines {
    /// The lines of the record that were torn and passed over, each by the
    /// place among the records it stood at, in order: records lost, which the
    /// items miss.
    pub lost: Vec<usize>,
    /// The place among the records of the record each item began in, one an
    /// item, which [`Lines::lost`] is counted in: none where the harness's
    /// items are not its record's lines (Devin's, from its store).
    pub places: Option<Vec<usize>>,
    /// The turn is over, and its harness put the command it began with back in its
    /// input box, where what is pasted next goes in after it: Claude Code's compaction
    /// cancelled with Escape (its reader, T-483).
    pub put_back: bool,
}

/// A monitor a turn armed that its harness has said no end of: its task's id, what its
/// agent called it, what it watches, the time of the record of the call that armed it
/// (milliseconds since the epoch, where the record says one), and the notice of its end
/// that came with no word of how it ended, where one did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Monitor {
    pub id: Arc<str>,
    pub description: Arc<str>,
    pub source: Source,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub called: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notice: Option<Notice>,
}

/// What a monitor watches, as the call that armed it says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Source {
    /// A command, which a process of the window runs: its call names one, not empty,
    /// and no other source.
    Command,
    /// A socket (`ws`), which no process of the window runs.
    Socket,
    /// Neither: no process of the window to look for. Claude Code 2.1.308 refuses such a
    /// call (its schema takes exactly one of `command` and `ws`), which starts no job, and
    /// an MCP server's own monitor is armed by no Monitor call: it is the safe reading of a
    /// call no version this reader knows writes (Calliope on 136a7158).
    Server,
}

/// A notice of a monitor's end as the record reports it, not verified: the line in
/// `<event>` with no status, which is Claude Code's own or a line of the watched
/// command's that says the same, and the time its record says it came, where it says
/// one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Notice {
    pub end: End,
    pub at: Option<Arc<str>>,
}

/// How a monitor's notice says it ended, which may be a command's line that says so.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum End {
    /// It says its time ran out (`[Monitor expired after …]`, `[Monitor timed out …]`).
    Expired,
    /// It says it was stopped (`[Monitor stopped]`, `[Monitor stopped — too much output
    /// …]`).
    Stopped,
    /// It says its socket closed (`[WebSocket closed: …]`).
    Closed,
}

/// What a turn left running that its harness wakes its agent for as each ends (Claude
/// Code does). The window takes input between turns, and the end of one wakes a turn of
/// its own: a task whose turn ended so is not done. The jobs are known of a turn still
/// under way too; the agents only of a turn that is over.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Pending {
    /// The background agents and workflows, as the harness counts them on the record
    /// that ends the turn, and again on the record of each turn after it: what it says
    /// last is how many run. Said as `agentsPending: true` where there are any, as it
    /// was before the count was kept.
    #[serde(
        rename = "agentsPending",
        serialize_with = "any",
        skip_serializing_if = "is_none"
    )]
    pub agents: usize,
    /// The jobs a tool call started in the background (a command, a monitor) and the
    /// harness has said no end of, as the reader counts them from the records that
    /// start and end each (`claude::record::jobs`).
    #[serde(rename = "jobsPending", skip_serializing_if = "is_none")]
    pub jobs: usize,
    /// The jobs of a process ConsensFlow saw gone before it opened the conversation
    /// again, of which no end was said: no longer waited on, and not known to have
    /// ended (a job its shell detached can outlive its window).
    #[serde(rename = "jobsRetired", skip_serializing_if = "is_none")]
    pub retired: usize,
    /// The monitors among the jobs that said they ended ([`Notice`]): pending all the same,
    /// as a line of a watched command's could say it. Where the notice is Claude Code's
    /// the monitor is over in Claude Code and wakes its agent no more (T-181); where it is
    /// a command's line, the monitor watches on. What runs in the window tells which of a
    /// command's ([`Record::monitors`] says which watch one).
    #[serde(rename = "monitorsNoticed", skip_serializing_if = "is_none")]
    pub noticed: usize,
}

impl Pending {
    /// How many it waits on, of agents and jobs: what keeps a task working.
    pub fn total(self) -> usize {
        self.agents.saturating_add(self.jobs)
    }
}

/// Whether a count is none, for a reading that leaves a count of none unsaid.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_none(count: &usize) -> bool {
    *count == 0
}

/// A count said as whether there are any.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn any<S: Serializer>(count: &usize, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_bool(*count > 0)
}

impl Record {
    /// `resultBase()`: no items, nothing in flight, settlement unknown.
    pub(crate) fn new() -> Self {
        Self {
            items: Vec::new(),
            in_flight: false,
            asking: false,
            failed: false,
            failure: None,
            quota: None,
            settlement: Settlement::Unknown,
            pending: Pending::default(),
            monitors: Vec::new(),
            lines: Box::default(),
        }
    }
}

/// Whether the turn is over: `{state: 'unknown' | 'in-flight' | 'settled'}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Settlement {
    Unknown,
    InFlight,
    Settled,
}

impl Serialize for Settlement {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let state = match self {
            Settlement::Unknown => "unknown",
            Settlement::InFlight => "in-flight",
            Settlement::Settled => "settled",
        };
        let mut map = serializer.serialize_map(Some(1))?;
        map.serialize_entry("state", state)?;
        map.end()
    }
}

/// Who an item is from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
    Tool,
    /// What the harness put in the conversation itself: an extension's
    /// message, a hook's context.
    Custom,
}

/// An item as a reading returns it (`emit`): its native id, who it is from,
/// its text, whether it is complete, and when, as the record had it (a
/// timestamp's text, or a number). Its id and text are shared, so a later
/// reading that keeps the item copies neither.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub id: Arc<str>,
    pub role: Role,
    pub text: Arc<str>,
    pub complete: bool,
    /// None where JavaScript held `undefined`: a column the store has not.
    /// A reading leaves it out, as `JSON.stringify` does.
    pub at: Option<Value>,
    /// A progress note, as Codex marks one: said only when true.
    pub commentary: bool,
    /// The summary a compaction writes in the user's place (Claude Code's
    /// `isCompactSummary`): the harness's own, and no message of the user's. Never
    /// said: a reading is the one Node wrote, which read it as the user's.
    pub compaction: bool,
}

impl Serialize for Item {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("id", &*self.id)?;
        map.serialize_entry("role", &self.role)?;
        map.serialize_entry("text", &*self.text)?;
        map.serialize_entry("complete", &self.complete)?;
        if let Some(at) = &self.at {
            map.serialize_entry("at", at)?;
        }
        if self.commentary {
            map.serialize_entry("commentary", &true)?;
        }
        map.end()
    }
}

/// `nativeId`: an item's id as the record gave it, or the failure of a record
/// that gave none: `missing native <kind> id at record <seq>`.
pub(crate) fn native_id(
    value: Option<&Value>,
    kind: &str,
    seq: &Value,
) -> Result<Arc<str>, String> {
    match value {
        Some(Value::String(id)) if !id.is_empty() => Ok(Arc::from(id.as_str())),
        _ => Err(format!(
            "missing native {kind} id at record {}",
            js::text(Some(seq))
        )),
    }
}

/// Why a record that is JSON's `null` fails its look: Node read a field of
/// it, and V8 threw.
pub(crate) fn null_record(index: usize) -> String {
    format!("record {index} is null, where an object was read")
}

/// `visibleText`: what a value shows as text. Text is itself, nothing is
/// nothing, a list is its parts on lines of their own (see [`visible_part`]),
/// and anything else is its JSON.
pub(crate) fn visible_text(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(text)) => text.clone(),
        None | Some(Value::Null) => String::new(),
        Some(Value::Array(parts)) => parts
            .iter()
            .map(visible_part)
            .collect::<Vec<_>>()
            .join("\n"),
        Some(other) => js::stringify(other),
    }
}

/// A part of a list as [`visible_text`] shows it: text is itself, an object
/// its own `text` when it has one, anything else its JSON.
pub(crate) fn visible_part(part: &Value) -> String {
    match part {
        Value::String(text) => text.clone(),
        Value::Object(fields) => match fields.get("text") {
            Some(Value::String(text)) => text.clone(),
            _ => js::stringify(part),
        },
        other => js::stringify(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_reading_is_written_as_node_wrote_one() {
        let mut record = Record::new();
        record.items.push(Item {
            id: Arc::from("msg_1"),
            role: Role::Assistant,
            text: Arc::from("Done."),
            complete: true,
            at: Some(json!("2026-09-06T17:27:06.000Z")),
            commentary: false,
            compaction: false,
        });
        record.items.push(Item {
            id: Arc::from("msg_2"),
            role: Role::Assistant,
            text: Arc::from("Reading the diff"),
            complete: false,
            at: None,
            commentary: true,
            compaction: false,
        });
        // A compaction's summary is the user's item it was to Node.
        record.items.push(Item {
            id: Arc::from("u_1"),
            role: Role::User,
            text: Arc::from("Summary"),
            complete: true,
            at: None,
            commentary: false,
            compaction: true,
        });
        record.settlement = Settlement::InFlight;
        assert_eq!(
            serde_json::to_string(&Reading::Known(record)).unwrap(),
            r#"{"items":[{"id":"msg_1","role":"assistant","text":"Done.","complete":true,"at":"2026-09-06T17:27:06.000Z"},{"id":"msg_2","role":"assistant","text":"Reading the diff","complete":false,"commentary":true},{"id":"u_1","role":"user","text":"Summary","complete":true}],"inFlight":false,"asking":false,"failed":false,"quota":null,"settlement":{"state":"in-flight"}}"#
        );
        // What a settled turn left running is said only where there is some: agents as
        // they always were, the jobs, the retired jobs and the monitors noticed by their
        // counts, and the monitors.
        let mut pending = Record::new();
        pending.pending = Pending {
            agents: 2,
            jobs: 1,
            retired: 3,
            noticed: 1,
        };
        pending.monitors = vec![Monitor {
            id: Arc::from("b615qv2gd"),
            description: Arc::from("ticks"),
            source: Source::Command,
            called: None,
            notice: Some(Notice {
                end: End::Expired,
                at: Some(Arc::from("2026-10-21T23:03:10.329Z")),
            }),
        }];
        let written = serde_json::to_string(&Reading::Known(pending)).unwrap();
        assert!(
            written.ends_with(r#""agentsPending":true,"jobsPending":1,"jobsRetired":3,"monitorsNoticed":1,"monitors":[{"id":"b615qv2gd","description":"ticks","source":"command","notice":{"end":"expired","at":"2026-10-21T23:03:10.329Z"}}]}"#),
            "{written}"
        );
        assert_eq!(
            serde_json::to_string(&Reading::unreadable("no codex rollout for s")).unwrap(),
            r#"{"unknown":true,"reason":"unreadable: no codex rollout for s"}"#
        );
    }

    #[test]
    fn a_record_that_is_not_there_was_read_and_one_that_could_not_be_read_failed() {
        let missing = Reading::missing("claude transcript for s");
        assert_eq!(
            serde_json::to_string(&missing).unwrap(),
            r#"{"unknown":true,"reason":"unreadable: no claude transcript for s"}"#,
            "said as it was before it had a name of its own"
        );
        assert!(!missing.read_failed());
        assert!(Reading::unreadable("the records thread ended").read_failed());
        // A failure whose words begin as a missing record's do is a failure still:
        // SQLite's own, in OpenCode's store.
        assert!(Reading::unreadable("no such column: id").read_failed());
        assert!(Reading::Unknown("missing session id".to_owned()).read_failed());
        assert!(!Reading::Known(Record::new()).read_failed());
    }

    #[test]
    fn an_item_with_no_native_id_fails_its_record() {
        assert_eq!(
            native_id(Some(&json!("a")), "codex item", &json!(3))
                .unwrap()
                .as_ref(),
            "a"
        );
        for missing in [None, Some(json!("")), Some(json!(5)), Some(json!(null))] {
            assert_eq!(
                native_id(missing.as_ref(), "codex item", &json!(3)).unwrap_err(),
                "missing native codex item id at record 3"
            );
        }
    }

    #[test]
    fn visible_text_is_what_node_showed() {
        assert_eq!(visible_text(Some(&json!("x"))), "x");
        assert_eq!(visible_text(None), "");
        assert_eq!(visible_text(Some(&json!(null))), "");
        assert_eq!(
            visible_text(Some(
                &json!(["a", { "text": "b" }, { "type": "image" }, 2.0, null, ["c"]])
            )),
            "a\nb\n{\"type\":\"image\"}\n2\nnull\n[\"c\"]"
        );
        assert_eq!(visible_text(Some(&json!({ "a": 1.5 }))), r#"{"a":1.5}"#);
        assert_eq!(visible_text(Some(&json!(true))), "true");
    }
}

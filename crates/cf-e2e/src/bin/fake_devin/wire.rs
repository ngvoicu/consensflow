//! Devin's wire log (`CHISEL_PURE_ACP_WIRE_LOG`): a JSON record to a line of what
//! its TUI and its agent say to each other, as much of it as the daemon reads. The
//! conversation a window shows is named by the record that configures its mode;
//! a turn is at work from the first thought or word of its agent until a record
//! that says how it ended (`cause`), and what that was is told with the request it
//! answers (`turnClientMessageId`, the name the store holds the user's message
//! by). Each record is one write, so a reader never meets one cut in two.
//!
//! A log [`Wire::stamped`] is written as Devin 3000.11 writes one (T-660, two probes
//! of 2026-10-26): every record of the turn's work is stamped with its time, the
//! agent's words are said in a stream, the turn begins with Devin's word that it works
//! (`session_info_update`, `statusEnum` `working`) and the user's message said back, in
//! place of the TUI's request, and ends with its word that it waits (`blocked`). A window
//! opened again on its conversation replays its last turn: stamped, in no stream, and
//! with no end of the turn.
//!
//! A log of [`Shape::Unsaid`] is written as Devin 3000.12.43 writes one (T-1417, live
//! 2026-11-11): stamped as 3000.11's, its words in streams, but with no word of Devin's on
//! the turn, neither `working` nor `blocked`: from the message said back (which names the
//! TUI's client message) to its agent's first streamed thought, nothing on the wire says
//! the turn is at work.

use std::fs::File;
use std::io::{self, Write};
use std::path::Path;

use serde_json::{json, Value};

/// A wire log, and the conversation it is of.
pub struct Wire {
    file: File,
    session: String,
    shape: Shape,
    /// The prompt steps its `steps` lines list so far.
    steps: Vec<Value>,
}

/// Which Devin's log a window writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    /// The stand-in's own: the TUI's request for each prompt, which no real Devin writes
    /// (T-102), and nothing stamped.
    Requests,
    /// Devin 3000.11's (T-660): its work stamped, its turns begun and ended by its word on
    /// them.
    Stamped,
    /// Devin 3000.12.43's (T-1417): stamped as 3000.11's, with no word of Devin's on a turn.
    Unsaid,
}

impl Shape {
    /// The shape `FAKE_DEVIN_STAMPS` names: none where it is unset or empty, 3000.12.43's
    /// where it names 3000.12, 3000.11's for any other word.
    pub fn named(word: Option<&str>) -> Self {
        match word {
            None | Some("") => Self::Requests,
            Some(word) if word.starts_with("3000.12") => Self::Unsaid,
            Some(_) => Self::Stamped,
        }
    }

    /// What the Devin of this shape says to `--version`.
    pub fn version(self) -> &'static str {
        match self {
            Self::Requests | Self::Stamped => "devin 3000.11.92 (stand-in)",
            Self::Unsaid => "devin 3000.12.43 (stand-in)",
        }
    }
}

/// The client message a replayed turn is named by.
const REPLAYED: &str = "replayed-client";

/// The updates of a turn's work that Devin 3000.11 stamps with their time.
const STAMPED: [&str; 4] = [
    "user_message_chunk",
    "agent_thought_chunk",
    "agent_message_chunk",
    "tool_call",
];

impl Wire {
    /// The log in `path`, made where it is not there, of the conversation `session`.
    pub fn open(path: &Path, session: &str) -> io::Result<Self> {
        Ok(Self {
            file: cf_e2e::door::append(path).map_err(std::io::Error::from)?,
            session: session.to_owned(),
            shape: Shape::Requests,
            steps: Vec::new(),
        })
    }

    /// What the window writes from now on is of the conversation `session` (`/new`): the
    /// window's key reader said it shows it.
    pub fn switch(&mut self, session: &str) {
        session.clone_into(&mut self.session);
    }

    /// The log written as Devin 3000.11 writes one, where `stamped`.
    #[cfg(test)]
    #[must_use]
    pub fn stamped(self, stamped: bool) -> Self {
        self.shape(if stamped {
            Shape::Stamped
        } else {
            Shape::Requests
        })
    }

    /// The log written in `shape`.
    #[must_use]
    pub fn shape(mut self, shape: Shape) -> Self {
        self.shape = shape;
        self
    }

    /// Which Devin's log this is.
    pub fn shape_of(&self) -> Shape {
        self.shape
    }

    /// Devin's word on the turn, `working` or `blocked`, stamped as Devin stamps it: none
    /// in a log of a Devin that says none.
    fn status(&mut self, state: &str) -> io::Result<()> {
        if self.shape != Shape::Stamped {
            return Ok(());
        }
        let at = jiff::Timestamp::now().as_millisecond();
        self.record(json!({
            "update": {
                "sessionUpdate": "session_info_update",
                "_meta": {
                    "cognition.ai/timestamp": at,
                    "cognition.ai/eventType": "status_update",
                    "cognition.ai/statusEnum": state,
                    "cognition.ai/userActionRequired": null,
                },
            },
        }))
    }

    /// The last turn of the conversation, replayed into a window opened again on it as
    /// Devin 3000.11 replays one: the user's message and the answer, stamped and in no
    /// stream, the turn named on a line of its own with no cause, and Devin's word that it
    /// waits (15 of 15 replays in the kit's logs, T-1423; 3000.12.43: no word).
    ///
    /// Devin 3000.12.43 (T-1417, run 3) replays the message with its client message and an
    /// event id of its own, the turn's work stamped in no stream, and a bare
    /// `turnClientMessageId` for the turn, with no cause and no word of its own.
    pub fn replayed(&mut self, user: Option<&str>, answer: Option<&str>) -> io::Result<()> {
        if self.shape == Shape::Requests {
            return Ok(());
        }
        for (kind, text) in [
            ("user_message_chunk", user),
            ("agent_message_chunk", answer),
        ] {
            if let Some(text) = text {
                let mut said = json!({
                    "update": { "sessionUpdate": kind, "content": { "type": "text", "text": text } },
                });
                if kind == "user_message_chunk" {
                    // 3000.11 replays the message as it said it live (127 of 127 in the kit's
                    // logs): its client message and its part, no event type, no event id.
                    said["update"]["_meta"] = json!({
                        "cognition.ai/clientMessageId": REPLAYED,
                        "cognition.ai/messageSubIndex": 0,
                    });
                }
                if self.shape == Shape::Unsaid && kind == "user_message_chunk" {
                    said["update"]["_meta"] = json!({
                        "cognition.ai/clientMessageId": REPLAYED,
                        "cognition.ai/eventId": "1",
                        "cognition.ai/eventType": "user_message",
                        "cognition.ai/message": text,
                        "cognition.ai/messageSubIndex": 0,
                    });
                }
                self.record(said)?;
            }
        }
        if user.is_some() {
            self.raw(&json!({ "sessionId": self.session, "turnClientMessageId": REPLAYED }))?;
        }
        self.status("blocked")
    }

    /// One record of the conversation, whole in one write: a record of the turn's work
    /// stamped with its time where the log is [`Wire::stamped`].
    fn record(&mut self, mut record: Value) -> io::Result<()> {
        // Devin 3000.12.43 names a turn on a line of its own at its end (`ended`); no line of
        // its work carries the turn's client message at the top: none of the 5 recorded logs
        // of T-1417 and T-1423 has one (the review T-1429).
        if self.shape == Shape::Unsaid && record.get("update").is_some() {
            if let Some(line) = record.as_object_mut() {
                line.remove("turnClientMessageId");
            }
        }
        record["sessionId"] = json!(self.session);
        let kind = record["update"]["sessionUpdate"]
            .as_str()
            .unwrap_or_default();
        if self.shape != Shape::Requests && STAMPED.contains(&kind) {
            record["update"]["_meta"]["cognition.ai/timestamp"] =
                json!(jiff::Timestamp::now().to_string());
        }
        self.raw(&record)
    }

    /// One record as it is, whole in one write: a request of the TUI names its
    /// conversation in its params, and no record of its own.
    fn raw(&mut self, record: &Value) -> io::Result<()> {
        let mut line = record.to_string();
        line.push('\n');
        self.file.write_all(line.as_bytes())
    }

    /// The window shows this conversation: the TUI configures it, its permission mode
    /// `mode` (`normal`, `bypass`) among the options.
    pub fn announce(&mut self, mode: &str) -> io::Result<()> {
        self.record(json!({
            "update": {
                "sessionUpdate": "config_option_update",
                "configOptions": [
                    { "id": "model", "currentValue": "fake" },
                    { "id": "mode", "currentValue": mode },
                ],
            },
        }))
    }

    /// The conversation's steps, the prompt `text` among them, as Devin 3000.12.43 lists them
    /// (T-1432: every prompt, a line of them all, its summary the prompt's first line with no
    /// `/bypass`). The stand-in lists them only where Devin says no message back: a window
    /// opened again silent, and the message it is given behind `/bypass`.
    pub fn listed(&mut self, text: &str) -> io::Result<()> {
        let number = self.steps.len() + 1;
        self.steps.push(json!({
            "stepId": format!("step-{number}"),
            "stepNumber": number,
            "kind": "prompt",
            "userMessageId": format!("message-{number}"),
            "revertTargetNodeId": number * 10,
            "forkTargetNodeId": number * 10 + 2,
            "summary": text.lines().next().unwrap_or_default(),
        }));
        let steps = json!({ "sessionId": self.session, "steps": self.steps });
        self.raw(&steps)
    }

    /// The conversation's mode changed to `mode`: its configuration, and the change on
    /// its own, as Devin 3000.11.92 says a `/bypass`.
    pub fn mode_changed(&mut self, mode: &str) -> io::Result<()> {
        self.announce(mode)?;
        self.record(json!({
            "update": { "sessionUpdate": "current_mode_update", "currentModeId": mode },
        }))
    }

    /// The call `call` of the turn `client` failed with `words`: refused for its
    /// permission where `rejected` (Devin's mark), as Devin's agent says a call its user
    /// skipped or a dialog left with Escape.
    pub fn call_failed(
        &mut self,
        call: &str,
        words: &str,
        rejected: bool,
        client: &str,
    ) -> io::Result<()> {
        self.record(json!({
            "turnClientMessageId": client,
            "update": {
                "sessionUpdate": "tool_call_update",
                "toolCallId": call,
                "status": "failed",
                "content": [{ "type": "content", "content": { "type": "text", "text": words } }],
                "_meta": {
                    "cognition.ai/inferenceToolName": "exec",
                    "cognition.ai/rejected": rejected,
                },
            },
        }))
    }

    /// The dialog that asks the user to allow the call `call` of `command`, as Devin
    /// 3000.12.43 writes it (T-1417, run 3): a line of its own, with the dialog's options and
    /// no word that the turn waits. Devin 3000.11.234 said `blocked` before it (the kit's
    /// resumed members of 2026-11-05); the stand-in's 3000.11 shape writes neither.
    pub fn asking(&mut self, call: &str, command: &str) -> io::Result<()> {
        if self.shape != Shape::Unsaid {
            return Ok(());
        }
        self.raw(&json!({
            "sessionId": self.session,
            "toolCall": {
                "toolCallId": call,
                "_meta": { "cognition.ai/editableCommand": command },
            },
            "options": [
                { "optionId": "allow_once", "name": "Allow", "kind": "allow_once" },
                { "optionId": "switch_bypass", "name": "Yes, switch to bypass mode", "kind": "allow_always" },
                { "optionId": "reject_once", "name": "Reject", "kind": "reject_once" },
            ],
        }))
    }

    /// The TUI puts the user's message to its agent: the request `id`, as the log
    /// of a real one has it (the conversation in its params, the words in a block
    /// of text). Its turn answers `client`.
    pub fn prompt(&mut self, id: u64, text: &str, client: &str) -> io::Result<()> {
        if self.shape != Shape::Requests {
            // Devin 3000.11 logs no request of its TUI's: it says it works, and says the
            // message back. Devin 3000.12.43 only says it back, naming the client message.
            self.status("working")?;
            // 3000.11's names the client message and the part (the kit's 34 logs); 3000.12.43's
            // the message's own text and its event type too, and no event id (T-1417, T-1423).
            let mut said = json!({
                "update": {
                    "sessionUpdate": "user_message_chunk",
                    "content": { "type": "text", "text": text },
                    "_meta": {
                        "cognition.ai/clientMessageId": client,
                        "cognition.ai/messageSubIndex": 0,
                    },
                },
            });
            if self.shape == Shape::Unsaid {
                said["update"]["_meta"]["cognition.ai/eventType"] = json!("user_message");
                said["update"]["_meta"]["cognition.ai/message"] = json!(text);
            }
            return self.record(said);
        }
        let session = self.session.clone();
        self.raw(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "session/prompt",
            "params": { "sessionId": session, "prompt": [{ "type": "text", "text": text }] },
        }))
    }

    /// The agent thinks: its turn is at work.
    pub fn thinking(&mut self, client: &str) -> io::Result<()> {
        let mut record = json!({
            "turnClientMessageId": client,
            "update": {
                "sessionUpdate": "agent_thought_chunk",
                "content": { "type": "text", "text": "thinking" },
            },
        });
        if self.shape != Shape::Requests {
            record["update"]["_meta"] =
                json!({ "cognition.ai/streamingMessageId": format!("{client}-thought") });
        }
        self.record(record)
    }

    /// The agent says `text`, in the message `stream`.
    pub fn said(&mut self, text: &str, stream: &str, client: &str) -> io::Result<()> {
        self.record(json!({
            "turnClientMessageId": client,
            "update": {
                "sessionUpdate": "agent_message_chunk",
                "content": { "type": "text", "text": text },
                "_meta": { "cognition.ai/streamingMessageId": stream },
            },
        }))
    }

    /// The TUI asks its agent to cancel the turn: a request of its own on the stand-in's
    /// log and 3000.11's; none on 3000.12.43's, which says only the turn's end (T-1423).
    pub fn cancel(&mut self) -> io::Result<()> {
        if self.shape == Shape::Unsaid {
            return Ok(());
        }
        self.record(json!({ "method": "session/cancel" }))
    }

    /// The turn that answers `client` ended: `complete`, or `cancelled`. Devin 3000.12.43
    /// names the turn on a line of its own, then says the cause on the next, with no word
    /// before them (T-1423, the app's Stop over its thoughts: a bare `turnClientMessageId`
    /// and `cancelled`).
    pub fn ended(&mut self, cause: &str, client: &str) -> io::Result<()> {
        if self.shape == Shape::Unsaid {
            self.raw(&json!({ "sessionId": self.session, "turnClientMessageId": client }))?;
            return self.raw(&json!({ "sessionId": self.session, "cause": cause, "stats": {} }));
        }
        self.status("blocked")?;
        self.record(json!({ "turnClientMessageId": client, "cause": cause }))
    }

    /// The turn that answers `client` ended for its account's quota, as Devin wrote one on a Pro
    /// plan (poker-lab, 2026-10-03): cause `quota_exhausted`, its words in `errorMessage`.
    pub fn ended_for_quota(&mut self, client: &str, words: &str) -> io::Result<()> {
        self.status("blocked")?;
        self.record(json!({
            "turnClientMessageId": client,
            "cause": "quota_exhausted",
            "errorMessage": words,
        }))
    }

    /// The turn that answers `client` ended in the model's request `request`, as Devin
    /// 3000.11 says it (`wire-b09accf0.jsonl` of T-652, lines 46275-46276): a record that
    /// names the turn's message and its request (`turnRequestId`), then the cause, with
    /// the request again in its statistics.
    pub fn ended_in(&mut self, cause: &str, client: &str, request: &str) -> io::Result<()> {
        self.status("blocked")?;
        self.record(json!({ "turnClientMessageId": client, "turnRequestId": request }))?;
        self.record(json!({ "cause": cause, "stats": { "requestId": request } }))
    }

    /// The agent begins a call of its shell tool that runs `command`: its turn is at work.
    pub fn called(&mut self, command: &str, client: &str) -> io::Result<()> {
        self.record(json!({
            "turnClientMessageId": client,
            "update": {
                "sessionUpdate": "tool_call",
                "rawInput": { "command": command },
            },
        }))
    }

    /// The call goes on to say what its command printed, once its turn has ended as
    /// well: what a command that reports (a ping says a line a second) leaves on the wire.
    pub fn reported(&mut self, line: &str) -> io::Result<()> {
        self.record(json!({
            "update": { "sessionUpdate": "tool_call_update", "content": line },
        }))
    }

    /// Another handle on the same log, for a thread of its own to write what a command
    /// reports: a record is one write, so what two write is never cut in two.
    pub fn another(&self) -> io::Result<Self> {
        Ok(Self {
            file: self.file.try_clone()?,
            session: self.session.clone(),
            shape: self.shape,
            steps: Vec::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn records(path: &Path) -> Vec<Value> {
        std::fs::read_to_string(path)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    #[test]
    fn a_turn_is_said_as_the_daemon_reads_one_from_the_prompt_to_the_cause_that_ends_it() {
        let folder = cf_e2e::door::made("cf-test-").unwrap();
        let path = folder.path().join("wire.jsonl");
        let mut wire = Wire::open(&path, "calm-river").unwrap();
        wire.announce("bypass").unwrap();
        wire.prompt(2, "[ConsensFlow m-1 · note]\nhello", "client-1")
            .unwrap();
        wire.thinking("client-1").unwrap();
        wire.said("DONE", "stream-1", "client-1").unwrap();
        wire.ended("complete", "client-1").unwrap();
        let records = records(&path);
        assert_eq!(records.len(), 5);
        for record in records.iter().filter(|record| record["method"].is_null()) {
            assert_eq!(record["sessionId"], "calm-river");
        }
        // The record that names the conversation the window shows.
        assert_eq!(
            records[0]["update"]["sessionUpdate"],
            "config_option_update"
        );
        assert!(records[0]["update"]["configOptions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|option| option["id"] == "mode" && option["currentValue"] == "bypass"));
        // The TUI's request: its id, the conversation in its params, and the words.
        assert_eq!(records[1]["method"], "session/prompt");
        assert_eq!(records[1]["id"], 2);
        assert_eq!(records[1]["params"]["sessionId"], "calm-river");
        assert_eq!(
            records[1]["params"]["prompt"][0]["text"],
            "[ConsensFlow m-1 · note]\nhello"
        );
        assert!(records[1].get("sessionId").is_none());
        // Work: a thought, then words in a message of their own.
        assert_eq!(records[2]["update"]["sessionUpdate"], "agent_thought_chunk");
        assert_eq!(records[3]["update"]["sessionUpdate"], "agent_message_chunk");
        assert_eq!(records[3]["update"]["content"]["text"], "DONE");
        assert_eq!(
            records[3]["update"]["_meta"]["cognition.ai/streamingMessageId"],
            "stream-1"
        );
        // The end, with the request it answers.
        assert_eq!(records[4]["cause"], "complete");
        assert_eq!(records[4]["turnClientMessageId"], "client-1");
    }

    #[test]
    fn a_turn_ended_in_a_model_request_names_it_as_devin_3000_11_does() {
        let folder = cf_e2e::door::made("cf-test-").unwrap();
        let path = folder.path().join("wire.jsonl");
        let mut wire = Wire::open(&path, "calm-river").unwrap();
        wire.ended_in("complete", "client-1", "request-9").unwrap();
        let records = records(&path);
        assert_eq!(records.len(), 2);
        assert_eq!(records[0]["turnClientMessageId"], "client-1");
        assert_eq!(records[0]["turnRequestId"], "request-9");
        assert_eq!(records[1]["cause"], "complete");
        assert_eq!(records[1]["stats"]["requestId"], "request-9");
        assert!(records[1].get("turnClientMessageId").is_none());
    }

    #[test]
    fn a_stamped_turn_is_said_as_devin_3000_11_says_it_and_a_replay_with_no_stream_and_no_end() {
        let folder = cf_e2e::door::made("cf-test-").unwrap();
        let path = folder.path().join("wire.jsonl");
        let mut wire = Wire::open(&path, "calm-river").unwrap().stamped(true);
        wire.prompt(2, "hello", "client-1").unwrap();
        wire.thinking("client-1").unwrap();
        wire.said("DONE", "stream-1", "client-1").unwrap();
        wire.ended("complete", "client-1").unwrap();
        wire.replayed(Some("hello"), Some("DONE")).unwrap();
        let records = records(&path);
        let kind = |at: usize| records[at]["update"]["sessionUpdate"].clone();
        let meta = |at: usize, name: &str| records[at]["update"]["_meta"][name].clone();
        // No request of the TUI's: Devin's word that it works, and the message said back.
        assert!(records.iter().all(|record| record.get("method").is_none()));
        assert_eq!(kind(0), "session_info_update");
        assert_eq!(meta(0, "cognition.ai/statusEnum"), "working");
        assert!(meta(0, "cognition.ai/timestamp").is_i64());
        assert_eq!(kind(1), "user_message_chunk");
        // The work stamped, its words in streams.
        for at in [1, 2, 3] {
            assert!(
                meta(at, "cognition.ai/timestamp").is_string(),
                "{}",
                records[at]
            );
        }
        assert_eq!(
            meta(2, "cognition.ai/streamingMessageId"),
            "client-1-thought"
        );
        assert_eq!(meta(3, "cognition.ai/streamingMessageId"), "stream-1");
        // Its word that it waits, then the end.
        assert_eq!(meta(4, "cognition.ai/statusEnum"), "blocked");
        assert_eq!(records[5]["cause"], "complete");
        // The replay: stamped, in no stream, the turn named with no cause (the kit's 15 of 15),
        // and Devin's word that it waits.
        assert_eq!(kind(6), "user_message_chunk");
        assert!(meta(6, "cognition.ai/eventId").is_null());
        assert!(meta(6, "cognition.ai/eventType").is_null());
        assert_eq!(kind(7), "agent_message_chunk");
        assert!(meta(7, "cognition.ai/timestamp").is_string());
        assert!(meta(7, "cognition.ai/streamingMessageId").is_null());
        assert_eq!(records[8]["turnClientMessageId"], REPLAYED);
        assert!(records[8].get("cause").is_none());
        assert_eq!(meta(9, "cognition.ai/statusEnum"), "blocked");
        assert_eq!(records.len(), 10);
        // A log not stamped replays nothing.
        let plain = folder.path().join("plain.jsonl");
        Wire::open(&plain, "calm-river")
            .unwrap()
            .replayed(Some("hello"), Some("DONE"))
            .unwrap();
        assert!(super::tests::records(&plain).is_empty());
    }

    #[test]
    fn a_turn_of_devin_3000_12_is_stamped_and_streamed_with_no_word_of_devins_on_it() {
        // Devin 3000.12.43's lines of a turn (T-1417, run 2, 2026-11-11): the message said
        // back with its client message, stamped; streamed thought and words; the end. No
        // `session_info_update` with a `statusEnum`, before the turn or at its end.
        let folder = cf_e2e::door::made("cf-test-").unwrap();
        let path = folder.path().join("wire.jsonl");
        let mut wire = Wire::open(&path, "calm-river")
            .unwrap()
            .shape(Shape::named(Some("3000.12")));
        wire.prompt(2, "hello", "client-1").unwrap();
        wire.thinking("client-1").unwrap();
        wire.called("pwd", "client-1").unwrap();
        wire.said("DONE", "stream-1", "client-1").unwrap();
        wire.ended("complete", "client-1").unwrap();
        wire.replayed(Some("hello"), Some("DONE")).unwrap();
        // A dialog's line, which the 3000.11 shape does not write.
        wire.asking("exec:0#call-1", "sleep 60").unwrap();
        let records = records(&path);
        let kind = |at: usize| records[at]["update"]["sessionUpdate"].clone();
        let meta = |at: usize, name: &str| records[at]["update"]["_meta"][name].clone();
        assert!(records
            .iter()
            .all(|record| record["update"]["_meta"]["cognition.ai/statusEnum"].is_null()));
        assert!(records.iter().all(|record| record.get("method").is_none()));
        assert_eq!(kind(0), "user_message_chunk");
        assert_eq!(meta(0, "cognition.ai/clientMessageId"), "client-1");
        assert_eq!(meta(0, "cognition.ai/eventType"), "user_message");
        for record in &records[..4] {
            assert!(
                record["update"]["_meta"]["cognition.ai/timestamp"].is_string(),
                "{record}"
            );
        }
        assert_eq!(
            meta(1, "cognition.ai/streamingMessageId"),
            "client-1-thought"
        );
        // No line of the work names its turn at the top: none of the recorded logs has one.
        assert!(records[..4]
            .iter()
            .all(|record| record.get("turnClientMessageId").is_none()));
        // A call is stamped and in no stream, as a replay is.
        assert_eq!(kind(2), "tool_call");
        assert!(meta(2, "cognition.ai/streamingMessageId").is_null());
        assert_eq!(meta(3, "cognition.ai/streamingMessageId"), "stream-1");
        // The end: the turn named on a line of its own, then its cause.
        assert_eq!(records[4]["turnClientMessageId"], "client-1");
        assert!(records[4].get("cause").is_none());
        assert_eq!(records[5]["cause"], "complete");
        // The replay: stamped, in no stream, the message with an event id of its own, and
        // the turn named with no cause.
        assert_eq!(kind(6), "user_message_chunk");
        assert_eq!(meta(6, "cognition.ai/eventId"), "1");
        assert_eq!(kind(7), "agent_message_chunk");
        assert!(meta(7, "cognition.ai/streamingMessageId").is_null());
        assert!(records[8]["turnClientMessageId"].is_string());
        assert!(records[8].get("cause").is_none());
        assert_eq!(records[9]["toolCall"]["toolCallId"], "exec:0#call-1");
        assert_eq!(records[9]["options"][0]["kind"], "allow_once");
        assert_eq!(records.len(), 10);
        assert_eq!(Shape::Unsaid.version(), "devin 3000.12.43 (stand-in)");
        assert_eq!(Shape::named(Some("1")), Shape::Stamped);
        assert_eq!(Shape::named(None), Shape::Requests);
    }

    #[test]
    fn a_cancel_is_a_request_of_the_tui_and_the_turn_it_ends_says_so() {
        let folder = cf_e2e::door::made("cf-test-").unwrap();
        let path = folder.path().join("wire.jsonl");
        let mut wire = Wire::open(&path, "calm-river").unwrap();
        wire.cancel().unwrap();
        wire.ended("cancelled", "client-1").unwrap();
        let records = records(&path);
        assert_eq!(records[0]["method"], "session/cancel");
        assert_eq!(records[1]["cause"], "cancelled");
    }

    #[test]
    fn a_cancel_of_devin_3000_12_is_the_turn_named_and_its_cause_with_no_request_of_the_tui() {
        let folder = cf_e2e::door::made("cf-test-").unwrap();
        let path = folder.path().join("wire.jsonl");
        let mut wire = Wire::open(&path, "calm-river")
            .unwrap()
            .shape(Shape::Unsaid);
        wire.prompt(2, "[ConsensFlow m-1 · note]\nhello", "client-1")
            .unwrap();
        wire.cancel().unwrap();
        wire.ended("cancelled", "client-1").unwrap();
        let records = records(&path);
        assert_eq!(records.len(), 3, "{records:#?}");
        let meta = &records[0]["update"]["_meta"];
        assert_eq!(meta["cognition.ai/eventType"], "user_message");
        assert_eq!(meta["cognition.ai/clientMessageId"], "client-1");
        assert_eq!(
            meta["cognition.ai/message"],
            "[ConsensFlow m-1 · note]\nhello"
        );
        assert!(meta.get("cognition.ai/eventId").is_none());
        assert_eq!(
            records[1],
            json!({ "sessionId": "calm-river", "turnClientMessageId": "client-1" })
        );
        assert_eq!(records[2]["cause"], "cancelled");
        assert_eq!(records[2]["sessionId"], "calm-river");
    }

    #[test]
    fn a_command_the_turn_left_goes_on_reporting_in_records_of_a_tool_after_the_turn_ended() {
        let folder = cf_e2e::door::made("cf-test-").unwrap();
        let path = folder.path().join("wire.jsonl");
        let mut wire = Wire::open(&path, "calm-river").unwrap();
        wire.called("sleep 600", "client-1").unwrap();
        wire.ended("complete", "client-1").unwrap();
        // A thread of its own writes beside the turn's, to the same log.
        let mut apart = wire.another().unwrap();
        apart.reported("tick").unwrap();
        wire.reported("tock").unwrap();
        let records = records(&path);
        assert_eq!(records.len(), 4);
        assert_eq!(records[0]["update"]["sessionUpdate"], "tool_call");
        assert_eq!(records[0]["update"]["rawInput"]["command"], "sleep 600");
        assert_eq!(records[1]["cause"], "complete");
        for (record, line) in records[2..].iter().zip(["tick", "tock"]) {
            assert_eq!(record["sessionId"], "calm-river");
            assert_eq!(record["update"]["sessionUpdate"], "tool_call_update");
            assert_eq!(record["update"]["content"], line);
        }
    }

    #[test]
    fn a_change_of_mode_and_a_refused_call_are_said_as_devin_says_them() {
        let folder = cf_e2e::door::made("cf-test-").unwrap();
        let path = folder.path().join("wire.jsonl");
        let mut wire = Wire::open(&path, "calm-river").unwrap();
        wire.mode_changed("bypass").unwrap();
        wire.call_failed(
            "call-1",
            "Tool execution was rejected: User skipped this tool call",
            true,
            "client-1",
        )
        .unwrap();
        let records = records(&path);
        assert_eq!(records.len(), 3);
        assert_eq!(
            records[0]["update"]["sessionUpdate"],
            "config_option_update"
        );
        assert_eq!(records[1]["update"]["sessionUpdate"], "current_mode_update");
        assert_eq!(records[1]["update"]["currentModeId"], "bypass");
        assert_eq!(records[1]["sessionId"], "calm-river");
        let failed = &records[2]["update"];
        assert_eq!(failed["status"], "failed");
        assert_eq!(failed["toolCallId"], "call-1");
        assert_eq!(
            failed["content"][0]["content"]["text"],
            "Tool execution was rejected: User skipped this tool call"
        );
        assert_eq!(failed["_meta"]["cognition.ai/rejected"], true);
    }

    #[test]
    fn a_log_that_is_there_is_gone_on_with_and_not_written_over() {
        let folder = cf_e2e::door::made("cf-test-").unwrap();
        let path = folder.path().join("wire.jsonl");
        Wire::open(&path, "calm-river")
            .unwrap()
            .announce("bypass")
            .unwrap();
        Wire::open(&path, "brisk-ember")
            .unwrap()
            .announce("normal")
            .unwrap();
        let records = records(&path);
        assert_eq!(records.len(), 2);
        assert_eq!(records[1]["sessionId"], "brisk-ember");
    }
}

//! Devin's wire log (`CHISEL_PURE_ACP_WIRE_LOG`): a JSON record to a line of what
//! its TUI and its agent say to each other, as much of it as the daemon reads. The
//! conversation a window shows is named by the record that configures its mode;
//! a turn is at work from the first thought or word of its agent until a record
//! that says how it ended (`cause`), and what that was is told with the request it
//! answers (`turnClientMessageId`, the name the store holds the user's message
//! by). Each record is one write, so a reader never meets one cut in two.

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::Path;

use serde_json::{json, Value};

/// A wire log, and the conversation it is of.
pub struct Wire {
    file: File,
    session: String,
}

impl Wire {
    /// The log in `path`, made where it is not there, of the conversation `session`.
    pub fn open(path: &Path, session: &str) -> io::Result<Self> {
        Ok(Self {
            file: OpenOptions::new().create(true).append(true).open(path)?,
            session: session.to_owned(),
        })
    }

    /// What the window writes from now on is of the conversation `session` (`/new`): the
    /// window's key reader said it shows it.
    pub fn switch(&mut self, session: &str) {
        session.clone_into(&mut self.session);
    }

    /// One record of the conversation, whole in one write.
    fn record(&mut self, mut record: Value) -> io::Result<()> {
        record["sessionId"] = json!(self.session);
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

    /// The TUI puts the user's message to its agent: the request `id`, as the log
    /// of a real one has it (the conversation in its params, the words in a block
    /// of text).
    pub fn prompt(&mut self, id: u64, text: &str) -> io::Result<()> {
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
        self.record(json!({
            "turnClientMessageId": client,
            "update": {
                "sessionUpdate": "agent_thought_chunk",
                "content": { "type": "text", "text": "thinking" },
            },
        }))
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

    /// The TUI asks its agent to cancel the turn.
    pub fn cancel(&mut self) -> io::Result<()> {
        self.record(json!({ "method": "session/cancel" }))
    }

    /// The turn that answers `client` ended: `complete`, or `cancelled`.
    pub fn ended(&mut self, cause: &str, client: &str) -> io::Result<()> {
        self.record(json!({ "turnClientMessageId": client, "cause": cause }))
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
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("wire.jsonl");
        let mut wire = Wire::open(&path, "calm-river").unwrap();
        wire.announce("bypass").unwrap();
        wire.prompt(2, "[ConsensFlow m-1 · note]\nhello").unwrap();
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
    fn a_cancel_is_a_request_of_the_tui_and_the_turn_it_ends_says_so() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("wire.jsonl");
        let mut wire = Wire::open(&path, "calm-river").unwrap();
        wire.cancel().unwrap();
        wire.ended("cancelled", "client-1").unwrap();
        let records = records(&path);
        assert_eq!(records[0]["method"], "session/cancel");
        assert_eq!(records[1]["cause"], "cancelled");
    }

    #[test]
    fn a_command_the_turn_left_goes_on_reporting_in_records_of_a_tool_after_the_turn_ended() {
        let folder = tempfile::tempdir().unwrap();
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
        let folder = tempfile::tempdir().unwrap();
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
        let folder = tempfile::tempdir().unwrap();
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

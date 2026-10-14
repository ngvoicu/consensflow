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

    /// One record of the conversation, whole in one write.
    fn record(&mut self, mut record: Value) -> io::Result<()> {
        record["sessionId"] = json!(self.session);
        let mut line = record.to_string();
        line.push('\n');
        self.file.write_all(line.as_bytes())
    }

    /// The window shows this conversation: the TUI configures its mode.
    pub fn announce(&mut self) -> io::Result<()> {
        self.record(json!({
            "update": {
                "sessionUpdate": "config_option_update",
                "configOptions": [{ "id": "model" }, { "id": "mode" }],
            },
        }))
    }

    /// The TUI puts the user's message to its agent.
    pub fn prompt(&mut self, client: &str) -> io::Result<()> {
        self.record(json!({
            "method": "session/prompt",
            "params": { "turnClientMessageId": client },
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
        wire.announce().unwrap();
        wire.prompt("client-1").unwrap();
        wire.thinking("client-1").unwrap();
        wire.said("DONE", "stream-1", "client-1").unwrap();
        wire.ended("complete", "client-1").unwrap();
        let records = records(&path);
        assert_eq!(records.len(), 5);
        for record in &records {
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
            .any(|option| option["id"] == "mode"));
        assert_eq!(records[1]["method"], "session/prompt");
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
    fn a_log_that_is_there_is_gone_on_with_and_not_written_over() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("wire.jsonl");
        Wire::open(&path, "calm-river").unwrap().announce().unwrap();
        Wire::open(&path, "brisk-ember")
            .unwrap()
            .announce()
            .unwrap();
        let records = records(&path);
        assert_eq!(records.len(), 2);
        assert_eq!(records[1]["sessionId"], "brisk-ember");
    }
}

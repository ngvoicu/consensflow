//! What no scenario of the goldens reaches. Each expected reading is what
//! Node's `devinReader` read of the same store and logs (Node 26), written
//! as `show` writes one; a failure V8 threw is said here in words of its own.

mod cells;
mod logs;
mod messages;
mod rows;

use std::fs;
use std::path::PathBuf;

use rusqlite::Connection;
use serde_json::{json, Value};

use super::*;
use crate::shared::record::reading::Settlement;

const SESSION: &str = "calm-river";

/// A session's store, and the launches' folder, under a root of their own.
struct Staged {
    root: tempfile::TempDir,
    env: Env,
    store: Connection,
}

impl Staged {
    /// A store whose session has no main chain yet.
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let under = |rest: &str| root.path().join(rest).to_string_lossy().into_owned();
        let env = Env::from_vars([
            ("HOME", under("")),
            ("XDG_DATA_HOME", under("data")),
            ("APPDATA", under("data")),
            ("CONSENSFLOW_HOME", under("home")),
        ]);
        fs::create_dir_all(root.path().join("data/devin/cli")).unwrap();
        let store = Connection::open(root.path().join("data/devin/cli/sessions.db")).unwrap();
        store
            .execute_batch(
                "CREATE TABLE sessions (id TEXT PRIMARY KEY, main_chain_id TEXT);
                 CREATE TABLE message_nodes (row_id INTEGER PRIMARY KEY, session_id TEXT,
                   node_id TEXT, parent_node_id TEXT, chat_message TEXT, created_at TEXT)",
            )
            .unwrap();
        store
            .execute("INSERT INTO sessions VALUES (?, NULL)", [SESSION])
            .unwrap();
        Self { root, env, store }
    }

    /// A row of `node` below `parent`, its message `message`: text as it
    /// is, any other value as its JSON.
    fn node(&self, node: Option<&str>, parent: Option<&str>, message: &Value) {
        let message = match message {
            Value::String(text) => text.clone(),
            other => other.to_string(),
        };
        self.store
            .execute(
                "INSERT INTO message_nodes (session_id, node_id, parent_node_id, chat_message, created_at)
                 VALUES (?, ?, ?, ?, '2026-10-04T10:00:00Z')",
                (SESSION, node, parent, message),
            )
            .unwrap();
    }

    /// Messages on a chain, each below the one before, the last its head.
    fn chain(&self, messages: &[Value]) {
        let nodes: Vec<String> = (1..=messages.len()).map(|at| format!("n-{at}")).collect();
        for (at, message) in messages.iter().enumerate() {
            let parent = at.checked_sub(1).map(|parent| nodes[parent].as_str());
            self.node(Some(&nodes[at]), parent, message);
        }
        self.head(nodes.last().unwrap());
    }

    fn head(&self, node: &str) {
        self.store
            .execute("UPDATE sessions SET main_chain_id = ?", [node])
            .unwrap();
    }

    fn rewrite(&self, node: &str, message: &Value) {
        self.store
            .execute(
                "UPDATE message_nodes SET chat_message = ? WHERE node_id = ?",
                (message.to_string(), node),
            )
            .unwrap();
    }

    fn launch(&self, launch: &str) -> PathBuf {
        self.root
            .path()
            .join("home/integrations/devin")
            .join(launch)
    }

    /// The wire log of `launch`, a line for each event.
    fn wire(&self, launch: &str, events: &[Value]) {
        let folder = self.launch(launch);
        fs::create_dir_all(&folder).unwrap();
        let lines: String = events.iter().map(|event| format!("{event}\n")).collect();
        fs::write(folder.join("wire.jsonl"), lines).unwrap();
    }

    fn reader(&self) -> Box<dyn Look + Send> {
        reader(SESSION, &self.env)
    }
}

fn look(reader: &mut Box<dyn Look + Send>) -> Arc<Reading> {
    reader.look(&Options::default(), 0)
}

/// A reading as the Node probe printed it: its reason, or its items and
/// what they say of the turn.
fn show(reading: &Reading) -> String {
    let record = match reading {
        Reading::Unknown(reason) => return format!("unknown: {reason}"),
        Reading::Known(record) => record,
    };
    let items: Vec<Value> = record
        .items
        .iter()
        .map(|item| json!([&*item.id, item.role, &*item.text, item.complete, item.at]))
        .collect();
    let state = match record.settlement {
        Settlement::Unknown => "unknown",
        Settlement::InFlight => "in-flight",
        Settlement::Settled => "settled",
    };
    format!(
        r#"{{"items":{},"inFlight":{},"asking":{},"failed":{},"settlement":"{state}"}}"#,
        Value::from(items),
        record.in_flight,
        record.asking,
        record.failed
    )
}

/// What one fresh look at the store and logs reads.
fn read_once(staged: &Staged) -> String {
    show(&look(&mut staged.reader()))
}

fn user(id: &str, client: Option<Value>) -> Value {
    let mut message = json!({ "message_id": id, "role": "user", "content": "Review T-1" });
    if let Some(client) = client {
        message["metadata"] = json!({ "extensions": { "chisel/client-message-id": client } });
    }
    message
}

fn reply(id: &str, content: Value) -> Value {
    json!({ "message_id": id, "role": "assistant", "content": content })
}

fn stopped(id: &str, content: &str) -> Value {
    let mut message = reply(id, json!(content));
    message["metadata"] = json!({ "finish_reason": "stop" });
    message
}

fn chunk(text: Option<Value>, stream: &str) -> Value {
    let mut content = json!({ "type": "text" });
    if let Some(text) = text {
        content["text"] = text;
    }
    json!({
        "sessionId": SESSION,
        "turnClientMessageId": "request-1",
        "update": {
            "sessionUpdate": "agent_message_chunk",
            "content": content,
            "_meta": { "cognition.ai/streamingMessageId": stream },
        },
    })
}

fn complete() -> Value {
    json!({ "sessionId": SESSION, "turnClientMessageId": "request-1", "cause": "complete" })
}

fn tool_call() -> Value {
    json!({ "sessionId": SESSION, "update": { "sessionUpdate": "tool_call" } })
}

const AT: &str = "2026-10-04T10:00:00Z";

/// The items of a user's request and a reply, `complete` or not.
fn asked_and_replied(reply: &str, complete: bool) -> String {
    format!(
        r#"[["u-1","user","Review T-1",true,"{AT}"],["a-1","assistant","{reply}",{complete},"{AT}"]]"#
    )
}

/// A user's request named `request-1` and a reply of `Done.`.
fn requested_done(staged: &Staged) {
    staged.chain(&[
        user("u-1", Some(json!("request-1"))),
        reply("a-1", json!("Done.")),
    ]);
}

fn settled_done() -> String {
    format!(
        r#"{{"items":{},"inFlight":false,"asking":false,"failed":false,"settlement":"settled"}}"#,
        asked_and_replied("Done.", true)
    )
}

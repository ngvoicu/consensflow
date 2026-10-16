//! Devin's store of its sessions, as the daemon reads it: `cli/sessions.db` in
//! Devin's data folder. A session is a row (`sessions`: its id and the node its
//! main chain ends in), and a message a node (`message_nodes`: the node, its
//! parent, and the message as JSON). The daemon opens it read-only and never
//! waits on a writer, so the stand-in writes in WAL mode, where a reader sees the
//! last whole write and a write waits for no reader.

use std::path::Path;
use std::time::Duration;

use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};

/// A session of the store, and the messages it has written.
pub struct Store {
    db: Connection,
    session: String,
    /// The node the main chain ends in.
    head: Option<String>,
    /// How many messages the session has.
    count: u64,
    /// What the session held when it was opened: whether a turn of it ended, and the
    /// user's message of a turn that never did (the one a quit cut), if its last is one.
    finished_a_turn: bool,
    cut: Option<String>,
}

/// A failure of the store, in words.
fn said(failed: rusqlite::Error) -> String {
    format!("Devin's store: {failed}")
}

impl Store {
    /// The store in `file` (made, with its folder, where it is not there), on the
    /// session `session`: a new one, or the one that is there, which goes on from
    /// where its chain ends.
    pub fn open(file: &Path, session: &str) -> Result<Self, String> {
        if let Some(folder) = file.parent() {
            std::fs::create_dir_all(folder).map_err(|failed| format!("{folder:?}: {failed}"))?;
        }
        let db = Connection::open(file).map_err(said)?;
        db.busy_timeout(Duration::from_secs(5)).map_err(said)?;
        db.execute_batch(
            "PRAGMA journal_mode = WAL;
             CREATE TABLE IF NOT EXISTS sessions (id TEXT PRIMARY KEY, main_chain_id TEXT);
             CREATE TABLE IF NOT EXISTS message_nodes (row_id INTEGER PRIMARY KEY, session_id TEXT,
               node_id TEXT, parent_node_id TEXT, chat_message TEXT, created_at TEXT);",
        )
        .map_err(said)?;
        db.execute(
            "INSERT OR IGNORE INTO sessions (id, main_chain_id) VALUES (?, NULL)",
            [session],
        )
        .map_err(said)?;
        let head: Option<String> = db
            .query_row(
                "SELECT main_chain_id FROM sessions WHERE id = ?",
                [session],
                |row| row.get(0),
            )
            .optional()
            .map_err(said)?
            .flatten();
        let messages = messages_of(&db, session)?;
        let finished_a_turn = messages.iter().any(ends_a_turn);
        // The last user's message, where nothing after it ended its turn.
        let cut = messages
            .iter()
            .rposition(|message| message["role"] == "user")
            .filter(|at| !messages[*at..].iter().any(ends_a_turn))
            .and_then(|at| messages[at]["content"].as_str().map(str::to_owned));
        Ok(Self {
            db,
            session: session.to_owned(),
            head,
            count: u64::try_from(messages.len()).unwrap_or(0),
            finished_a_turn,
            cut,
        })
    }

    /// Whether a turn of the session had ended when it was opened.
    pub fn finished_a_turn(&self) -> bool {
        self.finished_a_turn
    }

    /// The user's message of the turn a quit cut, which the session ended in when it
    /// was opened: given once.
    pub fn take_cut(&mut self) -> Option<String> {
        self.cut.take()
    }

    /// What a tool call came back with.
    pub fn tool(&mut self, text: &str) -> Result<(), String> {
        self.append(json!({ "role": "tool", "content": text, "metadata": {} }))
    }

    /// The user's message `text`, which the TUI names `client` in what it says to
    /// its agent (`turnClientMessageId` on the wire).
    pub fn user(&mut self, text: &str, client: &str) -> Result<(), String> {
        self.append(json!({
            "role": "user",
            "content": text,
            "metadata": { "extensions": { "chisel/client-message-id": client } },
        }))
    }

    /// The assistant's message `text`: its turn's end where `finished` (Devin's
    /// `finish_reason: stop`), and what a turn that was cancelled left where not.
    pub fn assistant(&mut self, text: &str, finished: bool) -> Result<(), String> {
        let metadata = if finished {
            json!({ "finish_reason": "stop" })
        } else {
            json!({})
        };
        self.append(json!({ "role": "assistant", "content": text, "metadata": metadata }))
    }

    /// A message as the node the main chain goes on in.
    fn append(&mut self, mut message: Value) -> Result<(), String> {
        self.cut = None;
        let at = jiff::Timestamp::now().to_string();
        self.count += 1;
        let node = format!("{}-node-{}", self.session, self.count);
        message["message_id"] = json!(format!("{}-message-{}", self.session, self.count));
        message["metadata"]["created_at"] = json!(at);
        let transaction = self.db.transaction().map_err(said)?;
        transaction
            .execute(
                "INSERT INTO message_nodes (session_id, node_id, parent_node_id, chat_message, created_at)
                 VALUES (?, ?, ?, ?, ?)",
                params![self.session, node, self.head, message.to_string(), at],
            )
            .map_err(said)?;
        transaction
            .execute(
                "UPDATE sessions SET main_chain_id = ? WHERE id = ?",
                params![node, self.session],
            )
            .map_err(said)?;
        transaction.commit().map_err(said)?;
        self.head = Some(node);
        Ok(())
    }
}

/// The messages of `session`, in the order they were written.
fn messages_of(db: &Connection, session: &str) -> Result<Vec<Value>, String> {
    let mut rows = db
        .prepare("SELECT chat_message FROM message_nodes WHERE session_id = ? ORDER BY row_id")
        .map_err(said)?;
    let messages = rows
        .query_map([session], |row| row.get::<_, String>(0))
        .map_err(said)?
        .filter_map(|message| serde_json::from_str(&message.ok()?).ok())
        .collect();
    Ok(messages)
}

/// Whether a message ended its turn: the assistant's, with Devin's `finish_reason: stop`.
fn ends_a_turn(message: &Value) -> bool {
    message["role"] == "assistant" && message["metadata"]["finish_reason"] == "stop"
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A node as another connection reads it: its id, its parent and its message.
    type Node = (String, Option<String>, Value);

    /// What another connection reads of the session: its head, and its nodes.
    fn read(file: &Path, session: &str) -> (Option<String>, Vec<Node>) {
        let db = Connection::open(file).unwrap();
        let head = db
            .query_row(
                "SELECT main_chain_id FROM sessions WHERE id = ?",
                [session],
                |row| row.get(0),
            )
            .unwrap();
        let mut rows = db
            .prepare("SELECT node_id, parent_node_id, chat_message FROM message_nodes WHERE session_id = ? ORDER BY row_id")
            .unwrap();
        let nodes = rows
            .query_map([session], |row| {
                let message: String = row.get(2)?;
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    serde_json::from_str(&message).unwrap(),
                ))
            })
            .unwrap()
            .map(Result::unwrap)
            .collect();
        (head, nodes)
    }

    #[test]
    fn a_session_is_a_chain_of_nodes_each_below_the_one_before_and_the_session_names_the_last() {
        let folder = tempfile::tempdir().unwrap();
        let file = folder
            .path()
            .join("data")
            .join("devin")
            .join("cli")
            .join("sessions.db");
        let mut store = Store::open(&file, "calm-river").unwrap();
        // Before any message the session is there, with no chain.
        assert_eq!(read(&file, "calm-river"), (None, vec![]));
        store.user("Reply with exactly: DONE", "client-1").unwrap();
        store.assistant("DONE", true).unwrap();
        let (head, nodes) = read(&file, "calm-river");
        assert_eq!(head.as_deref(), Some("calm-river-node-2"));
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[0].0, "calm-river-node-1");
        assert_eq!(nodes[0].1, None);
        assert_eq!(nodes[1].1.as_deref(), Some("calm-river-node-1"));
        let user = &nodes[0].2;
        assert_eq!(user["role"], "user");
        assert_eq!(user["content"], "Reply with exactly: DONE");
        assert_eq!(
            user["metadata"]["extensions"]["chisel/client-message-id"],
            "client-1"
        );
        assert_eq!(user["message_id"], "calm-river-message-1");
        assert!(user["metadata"]["created_at"].is_string());
        let assistant = &nodes[1].2;
        assert_eq!(assistant["role"], "assistant");
        assert_eq!(assistant["metadata"]["finish_reason"], "stop");
        assert_eq!(assistant["message_id"], "calm-river-message-2");
    }

    #[test]
    fn a_reply_that_was_cancelled_is_kept_with_no_end_to_its_turn() {
        let folder = tempfile::tempdir().unwrap();
        let file = folder.path().join("sessions.db");
        let mut store = Store::open(&file, "calm-river").unwrap();
        store.user("SLEEP 60 x", "client-1").unwrap();
        store.assistant("Interrupted", false).unwrap();
        let (_, nodes) = read(&file, "calm-river");
        assert!(nodes[1].2["metadata"].get("finish_reason").is_none());
    }

    #[test]
    fn a_session_that_is_there_goes_on_from_where_its_chain_ends_and_another_has_a_chain_of_its_own(
    ) {
        let folder = tempfile::tempdir().unwrap();
        let file = folder.path().join("sessions.db");
        Store::open(&file, "calm-river")
            .unwrap()
            .user("first", "client-1")
            .unwrap();
        let mut again = Store::open(&file, "calm-river").unwrap();
        again.user("second", "client-2").unwrap();
        let (head, nodes) = read(&file, "calm-river");
        assert_eq!(head.as_deref(), Some("calm-river-node-2"));
        assert_eq!(nodes[1].1.as_deref(), Some("calm-river-node-1"));
        assert_eq!(nodes[1].2["message_id"], "calm-river-message-2");
        let mut other = Store::open(&file, "brisk-ember").unwrap();
        other.user("elsewhere", "client-1").unwrap();
        assert_eq!(read(&file, "brisk-ember").1[0].1, None);
        assert_eq!(read(&file, "calm-river").1.len(), 2);
    }

    #[test]
    fn a_session_opened_again_says_whether_a_turn_of_it_ended_and_the_message_a_quit_cut() {
        let folder = tempfile::tempdir().unwrap();
        let file = folder.path().join("sessions.db");
        let fresh = Store::open(&file, "calm-river").unwrap();
        assert!(!fresh.finished_a_turn());
        // Its first turn, cut: a message and a call, and no end.
        let mut first = Store::open(&file, "calm-river").unwrap();
        first.user("SLEEP 600 FILE made.txt x", "client-1").unwrap();
        first.tool("Command running in background").unwrap();
        let mut cut = Store::open(&file, "calm-river").unwrap();
        assert!(!cut.finished_a_turn());
        assert_eq!(cut.take_cut().as_deref(), Some("SLEEP 600 FILE made.txt x"));
        assert_eq!(cut.take_cut(), None, "given once");
        // A turn that ended: none was cut, and a turn has ended since.
        cut.user("next", "client-2").unwrap();
        cut.assistant("done", true).unwrap();
        let mut ended = Store::open(&file, "calm-river").unwrap();
        assert!(ended.finished_a_turn());
        assert_eq!(ended.take_cut(), None);
        // A second turn cut after a first that ended.
        ended.user("SLEEP 600 again", "client-3").unwrap();
        let mut second = Store::open(&file, "calm-river").unwrap();
        assert!(second.finished_a_turn());
        assert_eq!(second.take_cut().as_deref(), Some("SLEEP 600 again"));
        // Anything written goes on from the cut: it is gone.
        let mut written = Store::open(&file, "calm-river").unwrap();
        written.user("else", "client-4").unwrap();
        assert_eq!(written.take_cut(), None);
    }

    #[test]
    fn what_is_written_is_there_for_a_reader_while_the_writer_is_open() {
        let folder = tempfile::tempdir().unwrap();
        let file = folder.path().join("sessions.db");
        let mut store = Store::open(&file, "calm-river").unwrap();
        store.user("hello", "client-1").unwrap();
        // Read-only, and never waiting on a writer, as the daemon reads it.
        let reader = Connection::open_with_flags(
            &file,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .unwrap();
        reader.busy_timeout(Duration::ZERO).unwrap();
        let seen: i64 = reader
            .query_row("SELECT count(*) FROM message_nodes", [], |row| row.get(0))
            .unwrap();
        assert_eq!(seen, 1);
        store.assistant("hi", true).unwrap();
        let seen: i64 = reader
            .query_row("SELECT count(*) FROM message_nodes", [], |row| row.get(0))
            .unwrap();
        assert_eq!(seen, 2);
    }
}

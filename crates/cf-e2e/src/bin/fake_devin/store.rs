//! Devin's store of its sessions, as the daemon reads it: `cli/sessions.db` in
//! Devin's data folder. A session is a row (`sessions`: its id and the node its
//! main chain ends in), and a message a node (`message_nodes`: the node, its
//! parent, and the message as JSON). What the TUI submits is a row of its history
//! (`prompt_history`), written at the Enter, before its turn takes it in, as Devin
//! 3000.11.92 writes it ([`submitted`]). The daemon opens it read-only and never
//! waits on a writer, so the stand-in writes in WAL mode, where a reader sees the
//! last whole write and a write waits for no reader. Its windows are many and its
//! data is scratch: no write waits for the disk, and every change that finds the
//! store held by another window is tried again for [`PATIENCE`].

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

/// How long a change of the store is tried again while another window holds it.
const PATIENCE: Duration = Duration::from_secs(30);

/// `change`, run again a moment after each time it finds the store held by another
/// window, for up to [`PATIENCE`]. SQLite's own wait is not enough: it calls no busy
/// handler for the change of a new file into WAL mode (the dry run's two workers,
/// 2026-10-15), and it gives up after five seconds of asking, which a lock held by
/// commits that wait for a slow disk outlasts (T-580: with every sync taking 300 ms,
/// the first round of the old store died on "database is locked").
fn patiently<T>(mut change: impl FnMut() -> rusqlite::Result<T>) -> rusqlite::Result<T> {
    let begun = std::time::Instant::now();
    loop {
        match change() {
            Err(rusqlite::Error::SqliteFailure(failure, _))
                if failure.code == rusqlite::ErrorCode::DatabaseBusy
                    && begun.elapsed() < PATIENCE =>
            {
                std::thread::sleep(Duration::from_millis(20));
            }
            done => return done,
        }
    }
}

/// The store made in `db`, in WAL mode with its tables, if it is not. Windows that
/// make it at the same moment (two workers given their tasks together) find it
/// locked. No write waits for the disk (`synchronous = OFF`, set before the change
/// into WAL mode, whose rollback journal it would otherwise sync): a commit that does
/// holds the store's lock for as long as the disk takes, which on a loaded runner is
/// seconds, and every other window waits for it.
fn made(db: &Connection) -> rusqlite::Result<()> {
    patiently(|| {
        db.execute_batch(
            "PRAGMA synchronous = OFF;
             PRAGMA journal_mode = WAL;
             CREATE TABLE IF NOT EXISTS sessions (id TEXT PRIMARY KEY, main_chain_id TEXT);
             CREATE TABLE IF NOT EXISTS message_nodes (row_id INTEGER PRIMARY KEY, session_id TEXT,
               node_id TEXT, parent_node_id TEXT, chat_message TEXT, created_at TEXT);
             CREATE TABLE IF NOT EXISTS prompt_history (id INTEGER PRIMARY KEY AUTOINCREMENT,
               content TEXT NOT NULL, timestamp INTEGER NOT NULL, session_id TEXT NOT NULL,
               is_shell INTEGER NOT NULL DEFAULT 0);",
        )
    })
}

/// The TUI of `session` submitted `text`: its history row in the store in `file`, at the
/// Enter, the second it is in, as Devin 3000.11.92 writes it whether a turn is at work or
/// not (T-102, probed live).
pub fn submitted(file: &Path, session: &str, text: &str) -> Result<(), String> {
    let db = Connection::open(file).map_err(said)?;
    db.busy_timeout(Duration::from_secs(5)).map_err(said)?;
    let second = jiff::Timestamp::now().as_second();
    patiently(|| {
        db.execute(
            "INSERT INTO prompt_history (content, timestamp, session_id) VALUES (?, ?, ?)",
            params![text, second, session],
        )
    })
    .map_err(said)?;
    Ok(())
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
        made(&db).map_err(said)?;
        let (head, messages) = patiently(|| {
            db.execute(
                "INSERT OR IGNORE INTO sessions (id, main_chain_id) VALUES (?, NULL)",
                [session],
            )?;
            let head: Option<String> = db
                .query_row(
                    "SELECT main_chain_id FROM sessions WHERE id = ?",
                    [session],
                    |row| row.get(0),
                )
                .optional()?
                .flatten();
            Ok((head, messages_of(&db, session)?))
        })
        .map_err(said)?;
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

    /// The words of the session's last user's message and of its last answer, as it held
    /// them when it was opened: what a window opened again on it replays.
    pub fn last_turn(&self) -> Result<(Option<String>, Option<String>), String> {
        let messages = messages_of(&self.db, &self.session).map_err(said)?;
        let last = |role: &str| {
            messages
                .iter()
                .rev()
                .find(|message| message["role"] == role)
                .and_then(|message| message["content"].as_str().map(str::to_owned))
        };
        Ok((last("user"), last("assistant")))
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

    /// The assistant's step that calls the tool `call` to run `command`, in the model's
    /// request `request` (Devin's `metadata.request_id`, a request of its own for each
    /// step), and what the call came back with.
    pub fn ran(
        &mut self,
        call: &str,
        command: &str,
        request: &str,
        output: &str,
    ) -> Result<(), String> {
        self.append(json!({
            "role": "assistant",
            "content": "",
            "tool_calls": [{ "id": call, "name": "exec", "arguments": { "command": command } }],
            "metadata": { "request_id": request, "finish_reason": null },
        }))?;
        self.append(json!({
            "role": "tool",
            "content": output,
            "tool_call_id": call,
            "metadata": {},
        }))
    }

    /// The assistant's message `text` that ends its turn as Devin 3000.11 stores one: in
    /// the model's request `request`, with no `finish_reason` (none of the 574 replies of
    /// T-652's store has one), so only the wire's end of that request proves it whole.
    pub fn answered(&mut self, text: &str, request: &str) -> Result<(), String> {
        self.append(json!({
            "role": "assistant",
            "content": text,
            "metadata": { "request_id": request, "finish_reason": null },
        }))
    }

    /// Devin's async compactor, in the middle of a turn: a system message that sums up
    /// the conversation begins a chain of its own (no parent), the turn's steps so far are
    /// copied below it, each a new node with the id Devin gave its message, and the main
    /// chain goes on from the copies. The user's message the turn answers is in none of
    /// it: it stays in the store, outside the main chain (T-652's store: the compactor's
    /// node 1159, `kept_turns` 81, a main chain of 190 messages none of them a user's).
    pub fn compact(&mut self) -> Result<(), String> {
        let messages = messages_of(&self.db, &self.session).map_err(said)?;
        let turn = messages
            .iter()
            .rposition(|message| message["role"] == "user")
            .map_or(0, |at| at + 1);
        let kept = &messages[turn..];
        self.head = None;
        self.append(json!({
            "role": "system",
            "content": "The conversation so far, summed up.",
            "metadata": {
                "extensions": {
                    "chisel/conversation_history": { "messages": [] },
                    "devin-rs/summary": { "source": "async_file_compactor", "kept_turns": kept.len() },
                },
            },
        }))?;
        for message in kept.iter().cloned() {
            self.append(message)?;
        }
        Ok(())
    }

    /// A message as the node the main chain goes on in: a new one, or a copy that
    /// keeps the id Devin gave the message.
    fn append(&mut self, mut message: Value) -> Result<(), String> {
        self.cut = None;
        let at = jiff::Timestamp::now().to_string();
        self.count += 1;
        let node = format!("{}-node-{}", self.session, self.count);
        if message.get("message_id").is_none() {
            message["message_id"] = json!(format!("{}-message-{}", self.session, self.count));
            message["metadata"]["created_at"] = json!(at);
        }
        let text = message.to_string();
        patiently(|| {
            let transaction = self.db.transaction()?;
            transaction.execute(
                "INSERT INTO message_nodes (session_id, node_id, parent_node_id, chat_message, created_at)
                 VALUES (?, ?, ?, ?, ?)",
                params![self.session, node, self.head, text, at],
            )?;
            transaction.execute(
                "UPDATE sessions SET main_chain_id = ? WHERE id = ?",
                params![node, self.session],
            )?;
            transaction.commit()
        })
        .map_err(said)?;
        self.head = Some(node);
        Ok(())
    }
}

/// The messages of `session`, in the order they were written.
fn messages_of(db: &Connection, session: &str) -> rusqlite::Result<Vec<Value>> {
    let mut rows =
        db.prepare("SELECT chat_message FROM message_nodes WHERE session_id = ? ORDER BY row_id")?;
    let messages = rows
        .query_map([session], |row| row.get::<_, String>(0))?
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
    fn a_compaction_begins_a_chain_of_its_own_with_the_turns_steps_copied_and_the_users_message_left_out(
    ) {
        let folder = tempfile::tempdir().unwrap();
        let file = folder.path().join("sessions.db");
        let mut store = Store::open(&file, "calm-river").unwrap();
        store.user("before", "client-1").unwrap();
        store.assistant("earlier", true).unwrap();
        store.user("ask", "client-2").unwrap();
        store
            .ran("call-1", "cf ask x", "client-2-request-1", "asked")
            .unwrap();
        store.compact().unwrap();
        store.answered("ASKED", "client-2-request-final").unwrap();
        let (head, nodes) = read(&file, "calm-river");
        let node = |id: &str| nodes.iter().find(|node| node.0 == id).unwrap();
        // The main chain, from its head up: the answer, the copied step and its tool's
        // answer, the summary, which has no parent.
        let mut chain = Vec::new();
        let mut at = head;
        while let Some(id) = at {
            let (_, parent, message) = node(&id);
            chain.push(message.clone());
            at = parent.clone();
        }
        chain.reverse();
        let roles: Vec<&str> = chain.iter().map(|m| m["role"].as_str().unwrap()).collect();
        assert_eq!(roles, ["system", "assistant", "tool", "assistant"]);
        assert_eq!(
            chain[0]["metadata"]["extensions"]["devin-rs/summary"]["source"],
            "async_file_compactor"
        );
        // The copies keep the ids Devin gave the messages.
        assert_eq!(chain[1]["message_id"], nodes[3].2["message_id"]);
        assert_eq!(chain[2]["message_id"], nodes[4].2["message_id"]);
        assert_eq!(chain[1]["metadata"]["request_id"], "client-2-request-1");
        let answer = &chain[3];
        assert_eq!(answer["content"], "ASKED");
        assert_eq!(answer["metadata"]["request_id"], "client-2-request-final");
        assert!(answer["metadata"]["finish_reason"].is_null());
        assert_eq!(nodes.len(), 9, "the user's messages stay in the store");
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

    #[test]
    fn a_store_another_window_holds_while_it_makes_it_is_made_once_that_window_lets_go() {
        let folder = tempfile::tempdir().unwrap();
        let file = folder.path().join("sessions.db");
        let holder = Connection::open(&file).unwrap();
        holder.execute_batch("BEGIN EXCLUSIVE").unwrap();
        // No busy handler: SQLite calls none for the change of a new file into WAL mode,
        // which is what the windows of the dry run met.
        let maker = Connection::open(&file).unwrap();
        maker.busy_timeout(Duration::ZERO).unwrap();
        let letting_go = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            holder.execute_batch("COMMIT").unwrap();
        });
        let begun = std::time::Instant::now();
        made(&maker).unwrap();
        assert!(
            begun.elapsed() >= Duration::from_millis(150),
            "it waited for the holder"
        );
        letting_go.join().unwrap();
        let tables: i64 = maker
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE name IN ('sessions', 'message_nodes')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(tables, 2);
    }

    #[test]
    fn a_store_is_written_with_no_wait_for_the_disk() {
        let folder = tempfile::tempdir().unwrap();
        let file = folder.path().join("sessions.db");
        let mut store = Store::open(&file, "calm-river").unwrap();
        store.user("hello", "client-1").unwrap();
        // 0 is OFF. A commit that waits for the disk holds the store's lock as long as the
        // disk takes, and on a loaded runner that has been seconds (T-580).
        let synchronous: i64 = store
            .db
            .query_row("PRAGMA synchronous", [], |row| row.get(0))
            .unwrap();
        assert_eq!(synchronous, 0);
    }

    #[test]
    fn a_store_another_window_holds_for_longer_than_sqlite_waits_is_waited_out() {
        let folder = tempfile::tempdir().unwrap();
        let file = folder.path().join("sessions.db");
        let mut writing = Store::open(&file, "calm-river").unwrap();
        // A window whose commit waits for a slow disk holds the write lock for seconds, and
        // SQLite's own wait gives up on it after five (dry run 2026-10-15, CI run 40195925291).
        let holder = Connection::open(&file).unwrap();
        holder.execute_batch("BEGIN IMMEDIATE").unwrap();
        let letting_go = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(6));
            holder.execute_batch("COMMIT").unwrap();
        });
        let begun = std::time::Instant::now();
        // One window opens the store while it is held, and one writes to it.
        let opening = std::thread::spawn({
            let file = file.clone();
            move || Store::open(&file, "brisk-ember")?.user("hello", "client-1")
        });
        writing.user("hello", "client-1").unwrap();
        opening.join().unwrap().unwrap();
        letting_go.join().unwrap();
        assert!(
            begun.elapsed() >= Duration::from_secs(6),
            "both waited for the holder"
        );
        assert_eq!(read(&file, "calm-river").1.len(), 1);
        assert_eq!(read(&file, "brisk-ember").1.len(), 1);
    }

    /// The count in the environment variable `name`, if it is set, else `default`.
    #[allow(clippy::disallowed_methods)] // A load test is told how much load by its environment.
    fn counted(name: &str, default: usize) -> usize {
        std::env::var(name)
            .ok()
            .and_then(|count| count.parse().ok())
            .unwrap_or(default)
    }

    #[test]
    fn windows_that_make_the_store_at_the_same_moment_each_open_it() {
        // The long run's two Devin workers were given their tasks in the same moment,
        // and the second window died on "database is locked" (dry run, 2026-10-15). The
        // race is the scheduler's to lose: one run of twenty rounds once met it in none of
        // them, so there are enough rounds that one meets it. A CI runner's Windows met it
        // again in round 43 of 200 (CI run 40195925291), in a run that was slow all
        // through, so groups of windows race at once, as the tests of a run do: set
        // CF_DEVIN_STORE_GROUPS and CF_DEVIN_STORE_ROUNDS to replay it under more load
        // (T-580 ran 12 groups of 8,000 rounds on Windows).
        let rounds = counted("CF_DEVIN_STORE_ROUNDS", 125);
        let groups = counted("CF_DEVIN_STORE_GROUPS", 4);
        let failed: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
        std::thread::scope(|scope| {
            for group in 0..groups {
                let failed = &failed;
                scope.spawn(move || {
                    for round in 0..rounds {
                        let folder = tempfile::tempdir().unwrap();
                        let file = folder.path().join("cli").join("sessions.db");
                        let start = std::sync::Arc::new(std::sync::Barrier::new(6));
                        let opened: Vec<Result<(), String>> = (0..6)
                            .map(|window| {
                                let (file, start) = (file.clone(), std::sync::Arc::clone(&start));
                                std::thread::spawn(move || {
                                    start.wait();
                                    let mut store =
                                        Store::open(&file, &format!("session-{window}"))?;
                                    store.user("hello", "client-1")
                                })
                            })
                            .collect::<Vec<_>>()
                            .into_iter()
                            .map(|thread| thread.join().unwrap())
                            .collect();
                        if !opened.iter().all(Result::is_ok) {
                            failed
                                .lock()
                                .unwrap()
                                .push(format!("group {group} round {round}: {opened:?}"));
                            return;
                        }
                    }
                });
            }
        });
        let failed = failed.into_inner().unwrap();
        assert!(failed.is_empty(), "{failed:#?}");
    }
}

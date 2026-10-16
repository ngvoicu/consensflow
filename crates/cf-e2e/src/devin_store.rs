//! Devin's store of its sessions, as a case reads it from the outside: where it
//! is on a machine, and the messages it holds. The stand-in Devin writes the same
//! file as Devin does (`fake-devin`'s store), and the daemon reads it as it reads
//! Devin's; a case that asks what a window was given, by the record that says it,
//! reads it here, whatever the daemon made of it. A message Devin rewrites is a
//! row of its own each time (a revision is a new node), and a chain Devin
//! copies is copied row by row, so a message is counted by the id Devin gave it:
//! once, however many rows hold it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{Connection, OpenFlags};
use serde_json::Value;

use crate::{Error, Result};

/// Where Devin keeps its store: `devin/cli/sessions.db` in its data folder, which on
/// Windows is `%APPDATA%` (`AppData/Roaming` in the home when that is not set) and
/// elsewhere `XDG_DATA_HOME` (`.local/share` in the home).
pub fn location(
    windows: bool,
    home: &str,
    app_data: Option<&str>,
    xdg_data_home: Option<&str>,
) -> PathBuf {
    let data = if windows {
        app_data.map_or_else(
            || Path::new(home).join("AppData").join("Roaming"),
            PathBuf::from,
        )
    } else {
        xdg_data_home.map_or_else(
            || Path::new(home).join(".local").join("share"),
            PathBuf::from,
        )
    };
    data.join("devin").join("cli").join("sessions.db")
}

/// A message of the store: the id Devin gave it, who it is from, and its words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub id: String,
    /// `user`, `assistant`, `tool` or `system`.
    pub role: String,
    pub text: String,
    /// The message as Devin stored it, whole: what it says besides its words
    /// (the tool calls of an assistant's, what a tool's came back with).
    pub raw: Value,
}

/// A tool an assistant's message calls: the id of the call, which the tool's own
/// message answers by, the tool's name, and the call as it was stored, whole (its
/// arguments among it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Call {
    pub id: String,
    pub name: String,
    pub text: String,
}

impl Call {
    /// Whether `words` are anywhere in the call as it was stored.
    pub fn says(&self, words: &str) -> bool {
        self.text.contains(words)
    }
}

impl Message {
    /// How many times `marker` is in its words.
    pub fn times(&self, marker: &str) -> usize {
        self.text.matches(marker).count()
    }

    /// The tools it calls, in the order it calls them: none for a message that
    /// calls none.
    pub fn calls(&self) -> Vec<Call> {
        let calls = self.raw["tool_calls"].as_array();
        calls
            .into_iter()
            .flatten()
            .map(|call| Call {
                id: call["id"].as_str().unwrap_or_default().to_owned(),
                name: call["name"].as_str().unwrap_or_default().to_owned(),
                text: call.to_string(),
            })
            .collect()
    }

    /// The call a tool's message answers, by the id of the call.
    pub fn answers(&self) -> Option<&str> {
        self.raw["tool_call_id"].as_str()
    }

    /// Whether `marker` is anywhere in the message as it was stored: its words, a
    /// tool's result, the arguments of a call, whatever Devin kept of it.
    pub fn holds(&self, marker: &str) -> bool {
        self.raw.to_string().contains(marker)
    }
}

/// The messages the store in `file` holds, of every session, each once, in the
/// order the first of its rows was written and as its last row says it. The store
/// is opened as the daemon opens it: read-only, and never waiting on a writer.
pub fn messages(file: &Path) -> Result<Vec<Message>> {
    read(file, None)
}

/// [`messages`] of the one session `session`.
pub fn messages_of(file: &Path, session: &str) -> Result<Vec<Message>> {
    read(file, Some(session))
}

/// The messages of `session`, or of every session, as [`messages`] says them.
fn read(file: &Path, session: Option<&str>) -> Result<Vec<Message>> {
    let said = |failed: rusqlite::Error| {
        Error::Daemon(format!("Devin's store {}: {failed}", file.display()))
    };
    let db = Connection::open_with_flags(
        file,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(said)?;
    db.busy_timeout(Duration::ZERO).map_err(said)?;
    let mut rows = db
        .prepare(
            "SELECT chat_message FROM message_nodes WHERE (?1 IS NULL OR session_id = ?1) \
             ORDER BY row_id",
        )
        .map_err(said)?;
    let written: Vec<String> = rows
        .query_map([session], |row| row.get(0))
        .map_err(said)?
        .collect::<std::result::Result<_, _>>()
        .map_err(said)?;
    let mut found: Vec<Message> = Vec::new();
    let mut at: HashMap<String, usize> = HashMap::new();
    for row in written {
        let Ok(message) = serde_json::from_str::<Value>(&row) else {
            continue;
        };
        let Some(id) = message["message_id"].as_str().map(str::to_owned) else {
            continue;
        };
        let message = Message {
            id: id.clone(),
            role: message["role"].as_str().unwrap_or_default().to_owned(),
            text: words(&message["content"]),
            raw: message,
        };
        match at.get(&id) {
            Some(&place) => found[place] = message,
            None => {
                at.insert(id, found.len());
                found.push(message);
            }
        }
    }
    Ok(found)
}

/// The words of a message's content: a string as it is, or the `text` parts of a
/// list, joined.
fn words(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter(|part| part["type"] == "text")
            .filter_map(|part| part["text"].as_str())
            .collect(),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn devins_store_is_in_the_data_folder_of_the_system_it_runs_on() {
        assert_eq!(
            location(false, "/h", None, None),
            Path::new("/h/.local/share/devin/cli/sessions.db")
        );
        assert_eq!(
            location(false, "/h", Some("/a"), Some("/x")),
            Path::new("/x/devin/cli/sessions.db")
        );
        assert_eq!(
            location(true, "/h", None, Some("/x")),
            Path::new("/h/AppData/Roaming/devin/cli/sessions.db")
        );
        assert_eq!(
            location(true, "/h", Some("/a"), Some("/x")),
            Path::new("/a/devin/cli/sessions.db")
        );
    }

    /// A store of these rows: a message each, as its JSON, of one session.
    fn store_of(rows: &[Value]) -> (tempfile::TempDir, PathBuf) {
        let rows: Vec<(&str, Value)> = rows.iter().map(|row| ("s", row.clone())).collect();
        store_in(&rows)
    }

    /// A store of these rows: the session of each, and its message as JSON.
    fn store_in(rows: &[(&str, Value)]) -> (tempfile::TempDir, PathBuf) {
        let folder = tempfile::tempdir().unwrap();
        let file = folder.path().join("sessions.db");
        let db = Connection::open(&file).unwrap();
        db.execute_batch(
            "CREATE TABLE message_nodes (row_id INTEGER PRIMARY KEY, session_id TEXT,
               node_id TEXT, parent_node_id TEXT, chat_message TEXT, created_at TEXT);",
        )
        .unwrap();
        for (session, row) in rows {
            db.execute(
                "INSERT INTO message_nodes (session_id, chat_message) VALUES (?, ?)",
                [*session, row.to_string().as_str()],
            )
            .unwrap();
        }
        (folder, file)
    }

    #[test]
    fn a_message_is_counted_once_by_its_id_as_its_last_row_says_it_whatever_the_rows_that_hold_it()
    {
        let (_folder, file) = store_of(&[
            json!({ "message_id": "a", "role": "user", "content": "first" }),
            json!({ "message_id": "b", "role": "assistant", "content": "draft" }),
            // The chain copied, and the assistant's message written again.
            json!({ "message_id": "a", "role": "user", "content": "first" }),
            json!({ "message_id": "b", "role": "assistant", "content": "final" }),
        ]);
        let found = messages(&file).unwrap();
        let said: Vec<(&str, &str, &str)> = found
            .iter()
            .map(|message| (&*message.id, &*message.role, &*message.text))
            .collect();
        assert_eq!(said, [("a", "user", "first"), ("b", "assistant", "final")]);
    }

    #[test]
    fn the_words_of_a_message_are_its_text_or_the_text_parts_of_its_list_and_a_row_that_is_no_message_is_none(
    ) {
        let (_folder, file) = store_of(&[
            json!({ "message_id": "a", "role": "user", "content": [
                { "type": "text", "text": "[ConsensFlow m-7 " },
                { "type": "image", "text": "no" },
                { "type": "text", "text": "· note]" },
            ] }),
            json!({ "message_id": "b", "role": "tool", "content": null }),
            json!({ "role": "user", "content": "no id" }),
        ]);
        let found = messages(&file).unwrap();
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].text, "[ConsensFlow m-7 · note]");
        assert_eq!(found[0].times("[ConsensFlow m-7 "), 1);
        assert_eq!(found[1].text, "");
    }

    #[test]
    fn the_messages_of_one_session_are_those_of_it_alone() {
        let (_folder, file) = store_in(&[
            (
                "one",
                json!({ "message_id": "a", "role": "user", "content": "first" }),
            ),
            (
                "two",
                json!({ "message_id": "b", "role": "user", "content": "other" }),
            ),
            (
                "one",
                json!({ "message_id": "c", "role": "assistant", "content": "second" }),
            ),
        ]);
        let of = |session: &str| -> Vec<String> {
            messages_of(&file, session)
                .unwrap()
                .into_iter()
                .map(|message| message.text)
                .collect()
        };
        assert_eq!(of("one"), ["first", "second"]);
        assert_eq!(of("two"), ["other"]);
        assert!(of("three").is_empty());
        assert_eq!(messages(&file).unwrap().len(), 3);
    }

    #[test]
    fn a_message_tells_the_tools_it_calls_the_call_it_answers_and_whatever_it_holds() {
        let (_folder, file) = store_of(&[
            json!({ "message_id": "a", "role": "assistant", "content": "", "tool_calls": [
                { "id": "call-1", "name": "run_command", "arguments": { "command": "echo x" } },
                { "id": "call-2", "name": "write" },
            ] }),
            json!({ "message_id": "b", "role": "tool", "tool_call_id": "call-1",
                   "content": [{ "type": "tool_result",
                                 "text": "Tool execution was rejected: User skipped this tool call" }] }),
            json!({ "message_id": "c", "role": "assistant", "content": "done" }),
        ]);
        let found = messages(&file).unwrap();
        let calls = found[0].calls();
        let named: Vec<(&str, &str)> = calls.iter().map(|call| (&*call.id, &*call.name)).collect();
        assert_eq!(named, [("call-1", "run_command"), ("call-2", "write")]);
        // The call is kept whole, and says what it was asked to do.
        assert!(calls[0].says("echo x") && !calls[1].says("echo x"));
        assert_eq!(found[0].answers(), None);
        assert_eq!(found[1].answers(), Some("call-1"));
        assert!(found[2].calls().is_empty());
        // What a tool came back with is not its words (the part is no text), and is in it.
        assert_eq!(found[1].text, "");
        assert!(found[1].holds("User skipped this tool call"));
        assert!(found[0].holds("echo x"));
        assert!(!found[2].holds("User skipped this tool call"));
    }

    #[test]
    fn a_store_that_is_not_there_is_an_error_that_names_it() {
        let folder = tempfile::tempdir().unwrap();
        let failed = messages(&folder.path().join("nowhere.db")).unwrap_err();
        assert!(failed.to_string().contains("nowhere.db"), "{failed}");
    }
}

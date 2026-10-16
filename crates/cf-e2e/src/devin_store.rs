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

/// How long a read waits on the store while SQLite holds it.
const HELD_FOR: Duration = Duration::from_secs(5);

/// A message of the store: the id Devin gave it, who it is from, and its words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub id: String,
    /// `user`, `assistant`, `tool` or `system`.
    pub role: String,
    pub text: String,
}

impl Message {
    /// How many times `marker` is in its words.
    pub fn times(&self, marker: &str) -> usize {
        self.text.matches(marker).count()
    }
}

/// The messages the store in `file` holds, of every session, each once, in the
/// order the first of its rows was written and as its last row says it. The store
/// is opened read-only, as the daemon opens it, but waits a moment where it is held:
/// a window of Devin's that opens or goes takes the store for as long as SQLite sets
/// its log right, and a case that read it then failed with "database is locked" (the
/// rig's Devin worker opened again after a restart, 2026-10-16), where the daemon's
/// look takes the record for unread and looks again.
pub fn messages(file: &Path) -> Result<Vec<Message>> {
    let said = |failed: rusqlite::Error| {
        Error::Daemon(format!("Devin's store {}: {failed}", file.display()))
    };
    let db = Connection::open_with_flags(
        file,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(said)?;
    db.busy_timeout(HELD_FOR).map_err(said)?;
    let mut rows = db
        .prepare("SELECT chat_message FROM message_nodes ORDER BY row_id")
        .map_err(said)?;
    let written: Vec<String> = rows
        .query_map([], |row| row.get(0))
        .map_err(said)?
        .collect::<std::result::Result<_, _>>()
        .map_err(said)?;
    let mut found: Vec<Message> = Vec::new();
    let mut at: HashMap<String, usize> = HashMap::new();
    for row in written {
        let Ok(message) = serde_json::from_str::<Value>(&row) else {
            continue;
        };
        let Some(id) = message["message_id"].as_str() else {
            continue;
        };
        let message = Message {
            id: id.to_owned(),
            role: message["role"].as_str().unwrap_or_default().to_owned(),
            text: words(&message["content"]),
        };
        match at.get(id) {
            Some(&place) => found[place] = message,
            None => {
                at.insert(id.to_owned(), found.len());
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

    /// A store of these rows: a message each, as its JSON.
    fn store_of(rows: &[Value]) -> (tempfile::TempDir, PathBuf) {
        let folder = tempfile::tempdir().unwrap();
        let file = folder.path().join("sessions.db");
        let db = Connection::open(&file).unwrap();
        db.execute_batch(
            "CREATE TABLE message_nodes (row_id INTEGER PRIMARY KEY, session_id TEXT,
               node_id TEXT, parent_node_id TEXT, chat_message TEXT, created_at TEXT);",
        )
        .unwrap();
        for row in rows {
            db.execute(
                "INSERT INTO message_nodes (session_id, chat_message) VALUES ('s', ?)",
                [row.to_string()],
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
    fn a_store_held_for_a_moment_is_read_once_it_is_let_go() {
        let (_folder, file) =
            store_of(&[json!({ "message_id": "a", "role": "user", "content": "first" })]);
        let writer = Connection::open(&file).unwrap();
        writer.execute_batch("BEGIN EXCLUSIVE").unwrap();
        let reading = std::thread::spawn({
            let file = file.clone();
            move || messages(&file)
        });
        std::thread::sleep(Duration::from_millis(300));
        writer.execute_batch("COMMIT").unwrap();
        let found = reading.join().unwrap().unwrap();
        assert_eq!(found.len(), 1, "read, not a locked store");
    }

    #[test]
    fn a_store_that_is_not_there_is_an_error_that_names_it() {
        let folder = tempfile::tempdir().unwrap();
        let failed = messages(&folder.path().join("nowhere.db")).unwrap_err();
        assert!(failed.to_string().contains("nowhere.db"), "{failed}");
    }
}

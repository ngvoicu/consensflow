//! What the chief said and did, from the daemon's copy of its conversation in the
//! ledger: its turns and its own edits, its last words, what it asked the owner,
//! what the owner typed into its window. And two looks at a chief while it runs,
//! each at a copy of a database in use: the question it left open in its
//! terminal, and the one its harness's own dialog holds (Devin's).
//!
//! Two things are kept that the code does not say by itself:
//!
//! - the chief's open question is its newest words that ended a turn with a
//!   question since the owner last typed into its window (ConsensFlow's own
//!   messages carry their header), and it stays open through later turns that
//!   ask nothing: a Codex chief asked the owner one, then waited for the answer to
//!   write its note (Windows, 2026-10-03);
//! - what a harness's turns say of the chief's edits is counted for Claude Code
//!   only: its Edit and Write results say so in words, other harnesses' do not.

use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde_json::Value;

use super::copy::on_copy;
use super::sentences::count_questions;
use crate::{js, Result};

/// The start of a message ConsensFlow delivers to a window: what the owner types
/// does not have it.
const CONSENSFLOW_HEADER: &str = "[ConsensFlow m-";

/// What a Claude Code chief's tool results say of an edit it made.
const EDITED: [&str; 2] = ["has been updated", "File created successfully"];

/// What a `cf ask` is answered with in a chief's window, where the human is not
/// asked on the board.
const ASK_REFUSED: &str = "ask the human here in your terminal";

/// An item of a conversation: what the ledger keeps of it.
struct Item {
    id: String,
    role: String,
    text: String,
    complete: bool,
}

/// The items of the conversations that `condition` (over `c`, the conversation, and
/// `p`, its participant) picks, with its parameters, in the order they happened.
fn items(db: &Connection, condition: &str, params: impl rusqlite::Params) -> Result<Vec<Item>> {
    let mut statement = db.prepare(&format!(
        "SELECT t.item_id, t.role, t.text, t.complete FROM transcript t
         JOIN conversation c ON c.id = t.conversation_id
         JOIN participant p ON p.id = c.participant_id
         WHERE {condition} ORDER BY t.conversation_id, t.seq"
    ))?;
    let found = statement
        .query_map(params, |row| {
            Ok(Item {
                id: row.get(0)?,
                role: row.get(1)?,
                text: row.get(2)?,
                complete: row.get::<_, i64>(3)? == 1,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(found)
}

/// The chief's own turns and edits.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(super) struct ChiefWindow {
    pub turns: i64,
    pub edits: Option<i64>,
    pub last_words: String,
    /// The messages that ended its turns: what it left the owner to read.
    pub turn_ends: Vec<String>,
    /// What the owner typed into its window, in characters.
    pub owner_messages: Vec<i64>,
    /// A `cf ask` it tried: refused, it is told to ask in its terminal.
    pub ask_refused: i64,
}

/// The chief's own turns and edits, from its conversations in the ledger. `harness`
/// is the chief's: only a Claude Code chief's edits are counted.
pub(super) fn window(db: &Connection, chief: i64, harness: Option<&str>) -> Result<ChiefWindow> {
    let items = items(db, "c.participant_id = ?1", [chief])?;
    let tools = || items.iter().filter(|item| item.role == "tool");
    let assistant: Vec<&Item> = items
        .iter()
        .filter(|item| item.role == "assistant")
        .collect();
    let count = |n: usize| i64::try_from(n).unwrap_or(i64::MAX);
    Ok(ChiefWindow {
        turns: count(assistant.len()),
        edits: (harness == Some("claude-code")).then(|| {
            count(
                tools()
                    .filter(|item| EDITED.iter().any(|said| item.text.contains(said)))
                    .count(),
            )
        }),
        last_words: assistant
            .last()
            .map(|item| js::first(&item.text, 1500))
            .unwrap_or_default(),
        turn_ends: assistant
            .iter()
            .filter(|item| item.complete)
            .map(|item| item.text.clone())
            .collect(),
        owner_messages: items
            .iter()
            .filter(|item| item.role == "user" && !item.text.starts_with(CONSENSFLOW_HEADER))
            .map(|item| count(js::length(&item.text)))
            .collect(),
        ask_refused: count(
            tools()
                .filter(|item| item.text.contains(ASK_REFUSED))
                .count(),
        ),
    })
}

/// What the chief wrote in its current conversation: the one the last switch
/// started.
pub(super) fn words_now(db: &Connection) -> Result<String> {
    let mut statement = db.prepare(
        "SELECT t.text FROM transcript t
         JOIN conversation c ON c.id = t.conversation_id
         JOIN participant p ON p.id = c.participant_id
         WHERE p.role = 'chief' AND c.ended_at IS NULL AND t.role = 'assistant'
         ORDER BY t.seq",
    )?;
    let words = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(words.join("\n"))
}

/// A question the chief left open in its terminal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenQuestion {
    /// The id its harness gave the message.
    pub id: String,
    pub text: String,
}

/// The chief's newest words that ended a turn with a question since the owner last
/// typed into its window, or none; read from a copy of the ledger `file`, which
/// the daemon may hold open.
pub fn chief_open_question(file: &Path) -> Result<Option<OpenQuestion>> {
    on_copy(file, |db| {
        let mut open = None;
        for item in items(db, "p.role = 'chief'", [])? {
            if item.role == "user" && !item.text.starts_with(CONSENSFLOW_HEADER) {
                open = None;
            } else if item.role == "assistant" && item.complete && count_questions(&item.text) > 0 {
                open = Some(OpenQuestion {
                    id: item.id,
                    text: item.text,
                });
            }
        }
        Ok(open)
    })
}

/// The questions a Devin chief's own dialog holds open: its newest call to its
/// question tool that no tool message has answered, read from a copy of Devin's
/// store, each question with its options; none when none is open. The chief's
/// session is its newest conversation in the ledger `file`; `env` says where
/// Devin keeps its store.
pub fn devin_chief_questions(file: &Path, env: &[(String, String)]) -> Result<Option<Vec<Value>>> {
    let session = on_copy(file, |db| {
        let mut statement = db.prepare(
            "SELECT c.native_session FROM conversation c
             JOIN participant p ON p.id = c.participant_id
             WHERE p.role = 'chief' ORDER BY c.id DESC LIMIT 1",
        )?;
        let mut rows = statement.query_map([], |row| row.get::<_, Option<String>>(0))?;
        Ok(rows.next().transpose()?.flatten())
    })?;
    let store = devin_data(env).join("cli").join("sessions.db");
    let Some(session) = session.filter(|session| !session.is_empty()) else {
        return Ok(None);
    };
    if !store.exists() {
        return Ok(None);
    }
    on_copy(&store, |db| {
        let mut statement = db.prepare(
            "SELECT chat_message FROM message_nodes WHERE session_id = ?1 ORDER BY row_id DESC",
        )?;
        let messages = statement
            .query_map([session], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(open_dialog(&messages))
    })
}

/// The questions the newest call of the question tool holds, in `messages` (the
/// newest first, each a JSON message), unless a tool message answered the call.
fn open_dialog(messages: &[String]) -> Option<Vec<Value>> {
    let mut answered: Vec<String> = Vec::new();
    for text in messages {
        let message: Value = serde_json::from_str(text).unwrap_or(Value::Null);
        if message["role"] == "tool" {
            if let Some(id) = message["tool_call_id"].as_str() {
                answered.push(id.to_owned());
            }
        }
        let call = (message["role"] == "assistant")
            .then(|| {
                message["tool_calls"]
                    .as_array()?
                    .iter()
                    .find(|call| call["name"] == "ask_user_question")
            })
            .flatten();
        if let Some(call) = call {
            let id = call["id"].as_str().unwrap_or_default();
            if answered.iter().any(|done| done == id) {
                return None;
            }
            return call["arguments"]["questions"].as_array().cloned();
        }
    }
    None
}

/// The last non-empty value of `name` in `env`, the environment of a window.
fn var<'e>(env: &'e [(String, String)], name: &str) -> Option<&'e str> {
    env.iter()
        .rev()
        .find(|(given, value)| given == name && !value.is_empty())
        .map(|(_, value)| value.as_str())
}

/// Whether the environment of a window is a Windows one: it says so (`OS`), or this
/// runs there.
pub fn on_windows(env: &[(String, String)]) -> bool {
    var(env, "OS").is_some_and(|os| os.to_ascii_lowercase().contains("windows")) || cfg!(windows)
}

/// Where Devin keeps its data, the folder its `cli/sessions.db` is in: on Windows
/// `%APPDATA%\devin` (Devin's docs), elsewhere the XDG place, `~/.local/share/devin`
/// unless `XDG_DATA_HOME` says another. `env` is the window's own.
pub fn devin_data(env: &[(String, String)]) -> PathBuf {
    let var = |name: &str| var(env, name);
    let home = || {
        var("HOME")
            .or_else(|| var("USERPROFILE"))
            .map(PathBuf::from)
            .or_else(|| cf_e2e::live::machine::home().ok())
            .unwrap_or_default()
    };
    if on_windows(env) {
        var("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| home().join("AppData").join("Roaming"))
            .join("devin")
    } else {
        var("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home().join(".local").join("share"))
            .join("devin")
    }
}

#[cfg(test)]
mod tests;

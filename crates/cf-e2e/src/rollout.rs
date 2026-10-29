//! A Codex thread's rollout, read as the suites that judge a thread read it: the
//! file `sessions/YYYY/MM/DD/rollout-<time>-<thread>.jsonl` below Codex's home,
//! a JSON record to a line. What a thread was told is its messages, each said by
//! someone: the developer, whose message is what the thread began with (written
//! once, with its first turn), and the user, whose messages are what the daemon
//! hands it. The stand-in Codex writes what the real one does of them, and the
//! live suites read the real one's.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::{files, Result};

/// The rollouts below the Codex home `home`, by their paths.
pub fn rollouts(home: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    collect(&home.join("sessions"), &mut found);
    found.sort();
    found
}

/// The rollout `thread` has below `home`, the first file whose name holds it.
pub fn of_thread(home: &Path, thread: &str) -> Option<PathBuf> {
    rollouts(home).into_iter().find(|path| {
        path.file_name()
            .is_some_and(|name| name.to_string_lossy().contains(thread))
    })
}

fn collect(folder: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, found);
        } else if path
            .extension()
            .is_some_and(|extension| extension == "jsonl")
            && path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("rollout-"))
        {
            found.push(path);
        }
    }
}

/// The text of the rollout at `path`, whole records only: Codex may be writing
/// the next while it is read.
pub fn read(path: &Path) -> Result<String> {
    files::read_finished_lines(path)
}

/// The records of the rollout `text`, oldest first; a line that is no record is
/// left out.
pub fn records(text: &str) -> Vec<Value> {
    text.lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

/// The thread a rollout's `text` is of, as its first record says it.
pub fn thread_of(text: &str) -> Option<String> {
    records(text)
        .first()
        .and_then(|record| record["payload"]["id"].as_str().map(str::to_owned))
}

/// What each message said by `role` (`developer` or `user`) says in the rollout
/// `text`, in order: the parts of a message joined.
pub fn said_by(text: &str, role: &str) -> Vec<String> {
    records(text)
        .iter()
        .filter(|record| {
            record["type"] == "response_item"
                && record["payload"]["type"] == "message"
                && record["payload"]["role"] == role
        })
        .map(|record| {
            record["payload"]["content"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|part| part["text"].as_str())
                .collect::<Vec<_>>()
                .join("")
        })
        .collect()
}

/// What the turn that took a message did before it, as a rollout says it.
#[derive(Debug, Clone, PartialEq)]
pub struct Turn {
    /// The message was written after a compaction its turn began with, and the seconds
    /// from the turn's start to the compaction's end.
    pub compacted: Option<f64>,
    /// The message is in the rollout.
    pub taken: bool,
}

/// The turn of the rollout `records` that took the message holding `words`: whether a
/// compaction came between its start and the message, as Codex 0.165 compacts a long
/// thread at the start of a turn (`task_started`, `compacted`, then the message).
pub fn turn_taking(records: &[Value], words: &str) -> Turn {
    let message = records.iter().position(|record| {
        record["type"] == "response_item"
            && record["payload"]["role"] == "user"
            && record["payload"].to_string().contains(words)
    });
    let Some(message) = message else {
        return Turn {
            compacted: None,
            taken: false,
        };
    };
    let began = records[..message].iter().rposition(|record| {
        record["type"] == "event_msg" && record["payload"]["type"] == "task_started"
    });
    let compacted = began.and_then(|began| {
        let compaction = records[began..message]
            .iter()
            .find(|record| record["type"] == "compacted")?;
        let from = seconds_of(records[began]["timestamp"].as_str()?)?;
        let to = seconds_of(compaction["timestamp"].as_str()?)?;
        Some(to - from)
    });
    Turn {
        compacted,
        taken: true,
    }
}

/// The seconds of an RFC 3339 time.
#[allow(clippy::cast_precision_loss)]
fn seconds_of(at: &str) -> Option<f64> {
    let at: jiff::Timestamp = at.parse().ok()?;
    Some(at.as_millisecond() as f64 / 1000.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROLLOUT: &str = concat!(
        r#"{"type":"session_meta","payload":{"id":"thread-one","originator":"codex-tui"}}"#,
        "\n",
        r#"{"type":"response_item","payload":{"type":"message","role":"developer","content":[{"type":"input_text","text":"the role "},{"type":"input_text","text":"as it began"}]}}"#,
        "\n",
        r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"first"}]}}"#,
        "\n",
        r#"{"type":"event_msg","payload":{"type":"item_completed","item":{"type":"UserMessage"}}}"#,
        "\n",
        r#"{"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"said"}]}}"#,
        "\n",
        "not a record\n",
        r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"second"}]}}"#,
        "\n",
    );

    #[test]
    fn the_messages_each_says_are_read_in_order_with_the_parts_of_one_joined() {
        assert_eq!(said_by(ROLLOUT, "developer"), ["the role as it began"]);
        assert_eq!(said_by(ROLLOUT, "user"), ["first", "second"]);
        assert_eq!(said_by(ROLLOUT, "assistant"), ["said"]);
        assert!(said_by(ROLLOUT, "system").is_empty());
        assert!(said_by("", "user").is_empty());
    }

    #[test]
    fn a_rollout_is_of_the_thread_its_first_record_names() {
        assert_eq!(thread_of(ROLLOUT).as_deref(), Some("thread-one"));
        assert_eq!(thread_of(""), None);
    }

    #[test]
    fn the_rollouts_of_a_home_are_found_below_its_sessions_by_their_names() {
        let home = tempfile::tempdir().unwrap();
        let day = home.path().join("sessions/2026/10/13");
        files::write(
            &day.join("rollout-2026-10-13T10-00-00-thread-one.jsonl"),
            ROLLOUT,
        )
        .unwrap();
        files::write(
            &day.join("rollout-2026-10-13T11-00-00-thread-two.jsonl"),
            "",
        )
        .unwrap();
        files::write(&day.join("notes.jsonl"), "").unwrap();
        files::write(&home.path().join("rollout-elsewhere.jsonl"), "").unwrap();
        let found = rollouts(home.path());
        assert_eq!(found.len(), 2, "{found:?}");
        let one = of_thread(home.path(), "thread-one").unwrap();
        assert_eq!(read(&one).unwrap(), ROLLOUT);
        assert_eq!(of_thread(home.path(), "thread-three"), None);
        assert!(rollouts(&home.path().join("none")).is_empty());
    }
}

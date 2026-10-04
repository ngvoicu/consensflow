//! The copy ConsensFlow keeps of each conversation, item by item, and what
//! the windows that had a task wrote.

use std::collections::HashMap;

use cf_base::time;
use cf_proto::ledger::{TaskTranscript, TranscriptItem};
use rusqlite::{params, OptionalExtension};
use serde_json::Value;

use super::known_conversation;
use crate::model::{cut, LedgerError};
use crate::store::Store;

/// The most of one tool's output that is copied: it can run to megabytes,
/// and the tool can be run again. Words are copied whole (the human's, an
/// agent's, ConsensFlow's): a chief switched in reads them in `cf history`.
pub const TRANSCRIPT_ITEM_MAX: usize = 64_000;

const TRANSCRIPT_ROLES: [&str; 4] = ["user", "assistant", "tool", "custom"];

/// The header every delivery opens with, naming its message: `[ConsensFlow m-12 `.
const HEADER: &str = "[ConsensFlow m-";

/// The copy of a window's conversation, one row per item as the harness's
/// own record has them: what is new is added, and an item still being
/// written is brought up to date. `from` is the position of the first item
/// given, so a caller may pass only the tail. An item is read as the
/// harness's record wrote it: one with no id is passed over, and a role or
/// a time it does not know is `custom`, or none. How many rows changed.
pub(crate) fn copy_transcript(
    store: &mut Store,
    conversation_id: i64,
    items: &[Value],
    from: i64,
) -> Result<usize, LedgerError> {
    store.write(|store| {
        known_conversation(store, conversation_id)?;
        let copied_at = store.at();
        let mut upsert = store.db.prepare_cached(
            "INSERT INTO transcript (conversation_id, item_id, seq, role, text, complete, at, copied_at)
       VALUES (?, ?, ?, ?, ?, ?, ?, ?)
       ON CONFLICT (conversation_id, item_id) DO UPDATE
         SET seq = excluded.seq, text = excluded.text, complete = excluded.complete,
             at = excluded.at, copied_at = excluded.copied_at
         WHERE transcript.text != excluded.text OR transcript.complete != excluded.complete",
        )?;
        let mut written = 0;
        for (index, item) in (0_i64..).zip(items) {
            let Some(id) = item
                .get("id")
                .and_then(Value::as_str)
                .filter(|id| !id.is_empty())
            else {
                continue;
            };
            let text = item.get("text").and_then(Value::as_str).unwrap_or_default();
            let role = item
                .get("role")
                .and_then(Value::as_str)
                .filter(|role| TRANSCRIPT_ROLES.contains(role))
                .unwrap_or("custom");
            let text = if role == "tool" {
                cut(text, TRANSCRIPT_ITEM_MAX)
            } else {
                text.to_string()
            };
            let at = item
                .get("at")
                .and_then(Value::as_str)
                .filter(|at| time::parse(at).is_some());
            written += upsert.execute(params![
                conversation_id,
                id,
                from + index,
                role,
                text,
                i64::from(item.get("complete") != Some(&Value::Bool(false))),
                at,
                copied_at,
            ])?;
        }
        Ok(written)
    })
}

/// One copied item, as the transcript table holds it.
struct Copied {
    conversation_id: i64,
    item_id: String,
    role: String,
    text: String,
    complete: i64,
    at: Option<String>,
}

/// What the windows that had a task wrote: the copied items of each window
/// the task was given to, in the order it was (a reassigned task's first
/// window, then the one that took it), whole, the last `limit` of them (all
/// of them when no limit is given). `cf task get --transcript` reads it.
pub(crate) fn transcript(
    store: &Store,
    project_id: i64,
    number: i64,
    limit: Option<usize>,
) -> Result<TaskTranscript, LedgerError> {
    let task = store.task_row(project_id, number)?;
    let mut windows = store
        .db
        .prepare(
            "SELECT recipient_id FROM message WHERE task_id = ? AND kind = 'task'
       GROUP BY recipient_id ORDER BY MIN(id)",
        )?
        .query_map([task.id], |row| row.get::<_, i64>("recipient_id"))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if windows.is_empty() {
        windows.extend(task.assignee_id);
    }
    let mut rows = Vec::new();
    for participant_id in windows {
        rows.extend(part_of(store, task.id, participant_id)?);
    }
    let total = rows.len();
    let first = limit.map_or(0, |limit| total.saturating_sub(limit));
    Ok(TaskTranscript {
        total,
        items: rows
            .into_iter()
            .skip(first)
            .map(|row| TranscriptItem {
                id: row.item_id,
                conversation: row.conversation_id,
                role: row.role,
                text: row.text,
                complete: row.complete == 1,
                at: row.at,
            })
            .collect(),
    })
}

/// The part of a window's copy that is task `task_id`'s. A window may hold
/// more than one task (the chief's own after its other work, a follow-up
/// given with --after): each task message that arrived in it (a brief, a
/// resume, a reopen), known by its header, turns the window to that task
/// until the next one does. A copy that shows none of the task's headers is
/// the task's whole.
fn part_of(store: &Store, task_id: i64, participant_id: i64) -> Result<Vec<Copied>, LedgerError> {
    let copied = store
        .db
        .prepare(
            "SELECT t.conversation_id, t.item_id, t.role, t.text, t.complete, t.at
       FROM transcript t JOIN conversation c ON c.id = t.conversation_id
       WHERE c.participant_id = ? ORDER BY t.conversation_id, t.seq",
        )?
        .query_map([participant_id], |row| {
            Ok(Copied {
                conversation_id: row.get("conversation_id")?,
                item_id: row.get("item_id")?,
                role: row.get("role")?,
                text: row.get("text")?,
                complete: row.get("complete")?,
                at: row.get("at")?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let task_of: HashMap<i64, i64> = store
        .db
        .prepare(
            "SELECT id, task_id FROM message
         WHERE recipient_id = ? AND kind = 'task' AND task_id IS NOT NULL",
        )?
        .query_map([participant_id], |row| {
            Ok((row.get("id")?, row.get("task_id")?))
        })?
        .collect::<rusqlite::Result<_>>()?;
    let mut current = None;
    let mut shown = false;
    let mut part = Vec::with_capacity(copied.len());
    for row in &copied {
        let header = if row.role == "user" {
            header_of(&row.text)
        } else {
            None
        };
        if let Some(turned) = header.and_then(|message| task_of.get(&message).copied()) {
            current = Some(turned);
            shown |= turned == task_id;
        }
        part.push(current == Some(task_id));
    }
    if !shown {
        return Ok(copied);
    }
    Ok(copied
        .into_iter()
        .zip(part)
        .filter_map(|(row, in_part)| in_part.then_some(row))
        .collect())
}

/// The message a delivery's header names: the first `[ConsensFlow m-` in
/// the text followed by digits and a space, as JavaScript's
/// `/\[ConsensFlow m-(\d+) /` found it. A number too long for an id names none.
fn header_of(text: &str) -> Option<i64> {
    let mut from = 0;
    while let Some(found) = text[from..].find(HEADER) {
        let digits_at = from + found + HEADER.len();
        let digits = text[digits_at..]
            .bytes()
            .take_while(u8::is_ascii_digit)
            .count();
        if digits > 0 && text[digits_at + digits..].starts_with(' ') {
            return text[digits_at..digits_at + digits].parse().ok();
        }
        from += found + 1;
    }
    None
}

/// The first item ConsensFlow's copy of the participant's current
/// conversation shows it was given (a user item) that contains `text`, or
/// none: a delivery's header there proves the delivery arrived.
pub(crate) fn copied_item_with(
    store: &Store,
    participant_id: i64,
    text: &str,
) -> Result<Option<String>, LedgerError> {
    Ok(store
        .db
        .query_row(
            "SELECT t.item_id FROM transcript t JOIN conversation c ON c.id = t.conversation_id
       WHERE c.participant_id = ? AND c.ended_at IS NULL AND t.role = 'user'
         AND instr(t.text, ?) > 0
       ORDER BY t.seq LIMIT 1",
            params![participant_id, text],
            |row| row.get("item_id"),
        )
        .optional()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_first_whole_header_as_the_javascript_pattern_did() {
        assert_eq!(header_of("[ConsensFlow m-12 from @chief]"), Some(12));
        assert_eq!(
            header_of("[ConsensFlow m-x] then [ConsensFlow m-7 here"),
            Some(7),
            "past one that is not whole"
        );
        assert_eq!(header_of("[ConsensFlow m-12x] [ConsensFlow m-3 "), Some(3));
        assert_eq!(
            header_of("[ConsensFlow m-12]"),
            None,
            "no space after the number"
        );
        assert_eq!(
            header_of("[ConsensFlow m-٣ "),
            None,
            "ASCII digits only, as JavaScript's \\d"
        );
        assert_eq!(
            header_of("[ConsensFlow m-99999999999999999999 "),
            None,
            "a number no id can be"
        );
    }
}

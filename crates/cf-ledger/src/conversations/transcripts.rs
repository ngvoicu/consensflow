//! The copy ConsensFlow keeps of each conversation, item by item, and what
//! the windows that had a task wrote.

use std::borrow::Cow;
use std::collections::HashMap;

use cf_base::time;
use cf_proto::ledger::{HistoryItem, TaskTranscript, TranscriptItem};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::{known_conversation, FIELD_MAX};
use crate::model::{cut, LedgerError};
use crate::openings::text_of;
use crate::store::Store;

/// The most of one tool's output that is copied: it can run to megabytes,
/// and the tool can be run again. Words are copied whole (the human's, an
/// agent's, ConsensFlow's): a chief switched in reads them in `cf history`.
pub const TRANSCRIPT_ITEM_MAX: usize = 64_000;

const TRANSCRIPT_ROLES: [&str; 4] = ["user", "assistant", "tool", "custom"];

/// The header every delivery opens with, naming its message: `[ConsensFlow m-12 `.
const HEADER: &str = "[ConsensFlow m-";

/// How many hexadecimal digits of its hash end an id the copy cut ([`kept_id`]).
const ID_HASH: usize = 32;

/// The copy of a window's conversation, one row per item as the harness's
/// own record has them: what is new is added, and an item still being
/// written is brought up to date. `from` is the position of the first item
/// given, so a caller may pass only the tail. An item is read as the
/// harness's record wrote it: one with no id is passed over, and a role or
/// a time it does not know is `custom`, or none. An id or a time longer than
/// [`FIELD_MAX`] characters is kept within it: the id as [`kept_id`] says, the
/// time as none. An item that says it is a
/// compaction's summary (`compaction: true`) is marked so when it is first
/// copied; one copied before the mark stays as it was. An item written again
/// changed is its next revision. How many rows changed.
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
            "INSERT INTO transcript (conversation_id, item_id, seq, role, text, complete, at, copied_at, compaction)
       VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
       ON CONFLICT (conversation_id, item_id) DO UPDATE
         SET seq = excluded.seq, text = excluded.text, complete = excluded.complete,
             at = excluded.at, copied_at = excluded.copied_at, revision = transcript.revision + 1
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
            let id = kept_id(id);
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
                .filter(|at| within(at) && time::parse(at).is_some());
            let changed = upsert.execute(params![
                conversation_id,
                id,
                from + index,
                role,
                text,
                i64::from(item.get("complete") != Some(&Value::Bool(false))),
                at,
                copied_at,
                i64::from(item.get("compaction") == Some(&Value::Bool(true))),
            ])?;
            if changed > 0 {
                write_parts(&store.db, conversation_id, &id, &text)?;
            }
            written += changed;
        }
        Ok(written)
    })
}

/// Whether `text` is no longer than [`FIELD_MAX`] characters.
fn within(text: &str) -> bool {
    text.chars().nth(FIELD_MAX).is_none()
}

/// An item's id as the copy keeps it: as it is, where it is no longer than
/// [`FIELD_MAX`] characters; else its first characters, "…" and the first
/// [`ID_HASH`] hexadecimal digits of the SHA-256 of the whole of it,
/// [`FIELD_MAX`] characters in all. The same id is kept the same each time it
/// is copied, so a copy made again finds its row; two that begin alike are
/// kept apart.
fn kept_id(id: &str) -> Cow<'_, str> {
    if within(id) {
        return Cow::Borrowed(id);
    }
    Cow::Owned(marked(id, id.as_bytes(), 0))
}

/// Id `id` cut as [`kept_id`] cuts one, its hash of `whole` (its bytes), and
/// of `count` too where that is not 0.
fn marked(id: &str, whole: &[u8], count: u32) -> String {
    let mut hash = Sha256::new();
    hash.update(whole);
    if count > 0 {
        hash.update(count.to_le_bytes());
    }
    let digits: String = hash
        .finalize()
        .iter()
        .take(ID_HASH / 2)
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let head: String = id.chars().take(FIELD_MAX - 1 - ID_HASH).collect();
    format!("{head}…{digits}")
}

/// Cuts each id of an item an older build copied longer than [`FIELD_MAX`]
/// characters to the one the copy keeps of it ([`kept_id`]): what migration
/// 0019 does past its SQL, before it splits the long items into parts, which
/// name their item by its id. An id that is no UTF-8 is cut as it reads, its
/// hash of its bytes as they are. Where another of the conversation's items
/// holds that id already, as only an id written to be one could, its hash is
/// of the id and a count, the first that none holds.
pub(crate) fn cut_copied(db: &Connection) -> rusqlite::Result<()> {
    let long = db
        .prepare("SELECT rowid FROM transcript WHERE octet_length(item_id) > ?")?
        .query_map([i64::try_from(FIELD_MAX).unwrap_or(i64::MAX)], |row| {
            row.get::<_, i64>(0)
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut read = db.prepare("SELECT item_id FROM transcript WHERE rowid = ?")?;
    let mut write = db.prepare("UPDATE OR IGNORE transcript SET item_id = ?2 WHERE rowid = ?1")?;
    for row in long {
        let whole = read.query_row([row], |found| Ok(found.get_ref(0)?.as_bytes()?.to_vec()))?;
        let id = text_of(&whole);
        if within(&id) {
            continue;
        }
        let mut count = 0;
        while write.execute(params![row, marked(&id, &whole, count)])? == 0 {
            count += 1;
        }
    }
    Ok(())
}

/// The parts of a text longer than one ([`TRANSCRIPT_ITEM_MAX`] characters
/// each, the last fewer), or none for a text no longer than that.
fn parts_of(text: &str) -> Vec<&str> {
    let starts: Vec<usize> = text
        .char_indices()
        .step_by(TRANSCRIPT_ITEM_MAX)
        .map(|(at, _)| at)
        .collect();
    if starts.len() < 2 {
        return Vec::new();
    }
    let ends = starts.iter().skip(1).copied().chain([text.len()]);
    starts
        .iter()
        .zip(ends)
        .map(|(&start, end)| &text[start..end])
        .collect()
}

/// Keeps item `item_id`'s text in parts as well, where it is longer than one,
/// so that a reader of a long item reads a part of it and no more: each part
/// that changed is written, and the parts past the text's last are taken away
/// (all of them from a text no longer than a part).
fn write_parts(
    db: &Connection,
    conversation_id: i64,
    item_id: &str,
    text: &str,
) -> rusqlite::Result<()> {
    let parts = parts_of(text);
    let mut write = db.prepare_cached(
        "INSERT INTO transcript_part (conversation_id, item_id, part, text) VALUES (?, ?, ?, ?)
       ON CONFLICT (conversation_id, item_id, part) DO UPDATE SET text = excluded.text
         WHERE transcript_part.text != excluded.text",
    )?;
    for (part, piece) in (0_i64..).zip(&parts) {
        write.execute(params![conversation_id, item_id, part, piece])?;
    }
    db.prepare_cached(
        "DELETE FROM transcript_part WHERE conversation_id = ? AND item_id = ? AND part >= ?",
    )?
    .execute(params![
        conversation_id,
        item_id,
        i64::try_from(parts.len()).unwrap_or(i64::MAX)
    ])?;
    Ok(())
}

/// Splits into parts every item copied before the copy kept long items in
/// parts too: what migration 0019 does past its SQL, once. Only a text of
/// more bytes than a part's characters can be longer than a part, and only
/// those are read, one at a time. Each part is written by its item's row, so
/// the item's id is kept as it was; and a text that is no UTF-8 (neither
/// Node's ledger nor this one writes one, each writing its language's own
/// strings) is split as it reads, U+FFFD for what is none, never refused.
pub(crate) fn split_copied(db: &Connection) -> rusqlite::Result<()> {
    let long = db
        .prepare("SELECT rowid FROM transcript WHERE octet_length(text) > ?")?
        .query_map(
            [i64::try_from(TRANSCRIPT_ITEM_MAX).unwrap_or(i64::MAX)],
            |row| row.get::<_, i64>(0),
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut read = db.prepare("SELECT text FROM transcript WHERE rowid = ?")?;
    let mut write = db.prepare(
        "INSERT INTO transcript_part (conversation_id, item_id, part, text)
       SELECT conversation_id, item_id, ?2, ?3 FROM transcript WHERE rowid = ?1",
    )?;
    for row in long {
        let text = read.query_row([row], |found| Ok(text_of(found.get_ref(0)?.as_bytes()?)))?;
        for (part, piece) in (0_i64..).zip(parts_of(&text)) {
            write.execute(params![row, part, piece])?;
        }
    }
    Ok(())
}

/// The items of a conversation's copy, in order.
pub(super) fn items_of(
    store: &Store,
    conversation_id: i64,
) -> Result<Vec<HistoryItem>, LedgerError> {
    Ok(store
        .db
        .prepare_cached(
            "SELECT item_id, role, text, complete, at, compaction FROM transcript
       WHERE conversation_id = ? ORDER BY seq",
        )?
        .query_map([conversation_id], |row| {
            Ok(HistoryItem {
                id: row.get("item_id")?,
                role: row.get("role")?,
                text: row.get("text")?,
                complete: row.get::<_, i64>("complete")? == 1,
                at: row.get("at")?,
                compaction: row.get::<_, i64>("compaction")? == 1,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?)
}

/// One copied item, as the transcript table holds it.
struct Copied {
    conversation_id: i64,
    item_id: String,
    role: String,
    text: String,
    complete: i64,
    at: Option<String>,
    compaction: i64,
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
                compaction: row.compaction == 1,
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
            "SELECT t.conversation_id, t.item_id, t.role, t.text, t.complete, t.at, t.compaction
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
                compaction: row.get("compaction")?,
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

/// Whether the participant's windows hold words the human typed after the
/// delivery of message `after`, copied since `since` (when that message was
/// queued): a user item no delivery made, which is one with no header of
/// ConsensFlow's, that comes after the message's own item in its window's
/// record. What a harness put in its record before the message (Codex's
/// `# AGENTS.md instructions`, written when its first turn begins and so first
/// copied with the message) is no news after it, nor is the summary a
/// compaction writes in the user's place, which the human never typed. An item
/// copied again as it was keeps the time it was first copied, so it is not new
/// either.
pub(crate) fn typed_since(
    store: &Store,
    participant_id: i64,
    since: &str,
    after: i64,
) -> Result<bool, LedgerError> {
    let items = store
        .db
        .prepare(
            "SELECT t.conversation_id, t.seq, t.text FROM transcript t
       JOIN conversation c ON c.id = t.conversation_id
       WHERE c.participant_id = ? AND t.role = 'user' AND t.compaction = 0 AND t.copied_at > ?",
        )?
        .query_map(params![participant_id, since], |row| {
            Ok((
                row.get::<_, i64>("conversation_id")?,
                row.get::<_, i64>("seq")?,
                row.get::<_, String>("text")?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    // Where the message itself sits in each window's record, where it does.
    let delivered_at = |conversation: i64| {
        items
            .iter()
            .filter(|(of, _, text)| *of == conversation && header_of(text) == Some(after))
            .map(|(_, seq, _)| *seq)
            .min()
    };
    Ok(items.iter().any(|(conversation, seq, text)| {
        header_of(text).is_none() && delivered_at(*conversation).is_none_or(|at| *seq > at)
    }))
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

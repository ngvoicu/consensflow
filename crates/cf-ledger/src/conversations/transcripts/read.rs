//! What the copy gives back: a conversation's items, a task's transcript, and
//! the words a delivery or the human is known by.

use std::collections::HashMap;

use cf_proto::ledger::{HistoryItem, ItemDetail, TaskTranscript, TranscriptItem};
use rusqlite::params;

use super::drawn::detail_of;
use crate::model::LedgerError;
use crate::store::Store;

/// The header every delivery opens with, naming its message: `[ConsensFlow m-12 `.
const HEADER: &str = "[ConsensFlow m-";

/// The items of a conversation's copy, in order.
pub(crate) fn items_of(
    store: &Store,
    conversation_id: i64,
) -> Result<Vec<HistoryItem>, LedgerError> {
    Ok(store
        .db
        .prepare_cached(
            "SELECT item_id, role, text, complete, at, compaction FROM transcript
       WHERE conversation_id = ? AND drawn_only = 0 ORDER BY seq",
        )?
        .query_map([conversation_id], |row| {
            Ok(HistoryItem {
                id: row.get("item_id")?,
                role: row.get("role")?,
                text: row.get("text")?,
                complete: row.get::<_, i64>("complete")? == 1,
                at: row.get("at")?,
                compaction: row.get::<_, i64>("compaction")? == 1,
                detail: None,
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
    detail: Option<String>,
}

/// What the windows that had a task wrote: the copied items of each window
/// the task was given to, in the order it was (a reassigned task's first
/// window, then the one that took it), whole, the last `limit` of them (all
/// of them when no limit is given). `cf task get --transcript` reads it. A
/// task whose brief reached no window has no transcript of its own, and says
/// so (`reached`): what its window wrote was another task's.
pub(crate) fn transcript(
    store: &Store,
    project_id: i64,
    number: i64,
    limit: Option<usize>,
) -> Result<TaskTranscript, LedgerError> {
    let task = store.task_row(project_id, number)?;
    let windows = store
        .db
        .prepare(
            "SELECT recipient_id FROM message WHERE task_id = ? AND kind = 'task'
       GROUP BY recipient_id ORDER BY MIN(id)",
        )?
        .query_map([task.id], |row| row.get::<_, i64>("recipient_id"))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut rows = Vec::new();
    let mut reached = false;
    for participant_id in windows {
        if let Some(part) = part_of(store, task.id, participant_id)? {
            reached = true;
            rows.extend(part);
        }
    }
    let total = rows.len();
    let first = limit.map_or(0, |limit| total.saturating_sub(limit));
    Ok(TaskTranscript {
        reached,
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
                // A task's transcript says what a call was, never the change it made.
                detail: detail_of(row.detail.as_deref()).map(|detail| ItemDetail {
                    diff: None,
                    ..detail
                }),
            })
            .collect(),
    })
}

/// The part of a window's copy that is task `task_id`'s. A window may hold
/// more than one task (the chief's own after its other work, a follow-up
/// given with --after): each task message that arrived in it (a brief, a
/// resume, a reopen), known by its header, turns the window to that task
/// until the next one does. A copy that shows none of the task's headers is
/// the task's whole where a task message of it was delivered there, and none
/// where none was: the task never reached that window, and what it wrote was
/// for others (the second review, 2026-10-16: the chief's own step, done before
/// its brief came back, was shown the chief's whole conversation).
fn part_of(
    store: &Store,
    task_id: i64,
    participant_id: i64,
) -> Result<Option<Vec<Copied>>, LedgerError> {
    let copied = store
        .db
        .prepare(
            "SELECT t.conversation_id, t.item_id, t.role, t.text, t.complete, t.at, t.compaction,
         t.detail
       FROM transcript t JOIN conversation c ON c.id = t.conversation_id
       WHERE c.participant_id = ? AND t.drawn_only = 0 ORDER BY t.conversation_id, t.seq",
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
                detail: row.get("detail")?,
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
        let delivered = store
            .db
            .prepare(
                "SELECT 1 FROM message WHERE task_id = ? AND recipient_id = ? AND kind = 'task'
                   AND state IN ('delivered', 'read')",
            )?
            .exists(params![task_id, participant_id])?;
        return Ok(delivered.then_some(copied));
    }
    Ok(Some(
        copied
            .into_iter()
            .zip(part)
            .filter_map(|(row, in_part)| in_part.then_some(row))
            .collect(),
    ))
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
/// none.
pub(crate) fn copied_item_with(
    store: &Store,
    participant_id: i64,
    text: &str,
) -> Result<Option<String>, LedgerError> {
    Ok(copied_items_with(store, participant_id, text)?
        .into_iter()
        .next()
        .map(|(item, _)| item))
}

/// Every item ConsensFlow's copy of the participant's current conversation
/// shows it was given (a user item) that contains `text`, in their order: its
/// id and its words. A delivery's header proves it arrived where it begins
/// the item as the paste does, which the caller reads in the words.
pub(crate) fn copied_items_with(
    store: &Store,
    participant_id: i64,
    text: &str,
) -> Result<Vec<(String, String)>, LedgerError> {
    Ok(store
        .db
        .prepare(
            "SELECT t.item_id, t.text FROM transcript t JOIN conversation c ON c.id = t.conversation_id
       WHERE c.participant_id = ? AND c.ended_at IS NULL AND t.role = 'user'
         AND instr(t.text, ?) > 0
       ORDER BY t.seq",
        )?
        .query_map(params![participant_id, text], |row| {
            Ok((row.get("item_id")?, row.get("text")?))
        })?
        .collect::<rusqlite::Result<_>>()?)
}

/// Every item ConsensFlow's copy of the participant's current conversation shows
/// it was given that contains `text` and was first copied at `since` or after, in
/// order: its id and its words. What the window was given from then on, never an
/// earlier copy of the same words, or a header quoted before (Calliope's review of
/// 2026-10-21, round 12); which of them is a paste is the reader's to tell, as of
/// [`copied_items_with`]. An item copied again as it was keeps the time it was
/// first copied.
pub(crate) fn copied_items_since(
    store: &Store,
    participant_id: i64,
    text: &str,
    since: &str,
) -> Result<Vec<(String, String)>, LedgerError> {
    Ok(store
        .db
        .prepare(
            "SELECT t.item_id, t.text FROM transcript t JOIN conversation c ON c.id = t.conversation_id
       WHERE c.participant_id = ? AND c.ended_at IS NULL AND t.role = 'user'
         AND t.compaction = 0 AND instr(t.text, ?) > 0 AND t.copied_at >= ?
       ORDER BY t.seq",
        )?
        .query_map(params![participant_id, text, since], |row| {
            Ok((row.get("item_id")?, row.get("text")?))
        })?
        .collect::<rusqlite::Result<_>>()?)
}

/// Whether the participant's windows hold words the human typed after the
/// delivery of message `after`, copied since `since` (when that message was
/// queued): see [`typed_at`].
pub(crate) fn typed_since(
    store: &Store,
    participant_id: i64,
    since: &str,
    after: i64,
) -> Result<bool, LedgerError> {
    Ok(typed_at(store, participant_id, since, None, after)?.is_some())
}

/// When the human last typed to the participant's windows: the time the newest
/// of their words was copied, among those copied since `since` and, where `until`
/// is given, not after it. Words are a user item no delivery made, which is one
/// with no header of ConsensFlow's, that comes after the item of message `after` in
/// its window's record. What a harness put in its record before the message (Codex's
/// `# AGENTS.md instructions`, written when its first turn begins and so first
/// copied with the message) is no news after it, nor is the summary a compaction
/// writes in the user's place, which the human never typed, nor a user's item of the
/// harness's own words (Claude Code's `<task-notification>`), which is copied as the
/// harness's (`custom`). An item copied again as it was keeps the time it was first
/// copied, so it is not new either. None where there are none.
pub(crate) fn typed_at(
    store: &Store,
    participant_id: i64,
    since: &str,
    until: Option<&str>,
    after: i64,
) -> Result<Option<String>, LedgerError> {
    let items = store
        .db
        .prepare(
            "SELECT t.conversation_id, t.seq, t.text, t.copied_at FROM transcript t
       JOIN conversation c ON c.id = t.conversation_id
       WHERE c.participant_id = ?1 AND t.role = 'user' AND t.compaction = 0 AND t.copied_at > ?2
         AND (?3 IS NULL OR t.copied_at <= ?3)",
        )?
        .query_map(params![participant_id, since, until], |row| {
            Ok((
                row.get::<_, i64>("conversation_id")?,
                row.get::<_, i64>("seq")?,
                row.get::<_, String>("text")?,
                row.get::<_, String>("copied_at")?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    // Where the message itself sits in each window's record, where it does.
    let delivered_at = |conversation: i64| {
        items
            .iter()
            .filter(|(of, _, text, _)| *of == conversation && header_of(text) == Some(after))
            .map(|(_, seq, _, _)| *seq)
            .min()
    };
    Ok(items
        .iter()
        .filter(|(conversation, seq, text, _)| {
            header_of(text).is_none() && delivered_at(*conversation).is_none_or(|at| *seq > at)
        })
        .map(|(_, _, _, copied_at)| copied_at.clone())
        .max())
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

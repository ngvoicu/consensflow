//! The copy ConsensFlow keeps of each conversation, item by item, and what
//! the windows that had a task wrote.

use std::collections::HashMap;

use cf_base::time;
use cf_proto::ledger::{HistoryItem, ItemDetail, TaskTranscript, TranscriptItem};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;

use super::{known_conversation, ID_SHOWN};
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

/// The copy of a window's conversation, one row per item as the harness's
/// own record has them: what is new is added, and an item still being
/// written is brought up to date. `from` is the position of the first item
/// given, so a caller may pass only the tail. An item is read as the
/// harness's record wrote it: one with no id is passed over, and its id, its
/// identity, is kept whole however long; a role it does not know is `custom`;
/// its time is kept as a store keeps one ([`time::canonical`]), none where it
/// is no time. An item that says it is a
/// compaction's summary (`compaction: true`) is marked so when it is first
/// copied; one copied before the mark stays as it was. An item whose role is
/// another than it was copied with (a record a daemon read as the user's, which a
/// later one reads as the harness's own: Claude Code's companion, T-296) is
/// brought to the role it has, as an item whose text changed is. An item is copied at its
/// place in the record (`from` + its index) unless an older item of the copy holds that
/// place, as it does where the record shrank (a resumed window rewrote it): it goes after
/// every item the copy has then, so the copy is in the order the items came and the last
/// ones are the newest (T-1118). An item the copy has keeps its place, or moves to a later
/// one, never an earlier one. What the history viewer
/// reads of an item besides its text is written with its row ([`write_head`]),
/// and its text in parts where it is long ([`write_parts`]); an item written
/// again changed is its next revision.
///
/// What the history draws of an item besides its words (`detail`, chief-harness CHF-16) is
/// kept with it ([`Drawn`]), and compared as its words are: an item that says something
/// else of it is written again, its words the same or not. An item only the history shows
/// (`drawnOnly`) is kept so, and no reader of the conversation's words reads it; one that
/// would stand before an item copied already is not copied, since its row would stand
/// after it (the history goes by rows): an item a reader made of a record an older build
/// copied, read again from the start. A model's thinking is cut as a tool's output is. How
/// many rows changed.
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
            "INSERT INTO transcript (conversation_id, item_id, seq, role, text, complete, at, copied_at, compaction, detail, drawn_only)
       VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
       ON CONFLICT (conversation_id, item_id) DO UPDATE
         SET seq = excluded.seq, role = excluded.role, text = excluded.text,
             complete = excluded.complete, at = excluded.at, copied_at = excluded.copied_at,
             detail = excluded.detail, drawn_only = excluded.drawn_only
         WHERE transcript.text != excluded.text OR transcript.complete != excluded.complete
            OR transcript.role != excluded.role OR transcript.detail IS NOT excluded.detail
            OR transcript.drawn_only != excluded.drawn_only
       RETURNING rowid, compaction",
        )?;
        // The place after every item the copy has: where an item goes that the record
        // shows at a place an older one holds.
        let mut next: i64 = store.db.query_row(
            "SELECT COALESCE(MAX(seq), -1) + 1 FROM transcript WHERE conversation_id = ?",
            [conversation_id],
            |row| row.get(0),
        )?;
        let mut known = store
            .db
            .prepare_cached("SELECT seq FROM transcript WHERE conversation_id = ? AND item_id = ?")?;
        let mut holder = store.db.prepare_cached(
            "SELECT 1 FROM transcript WHERE conversation_id = ? AND seq = ? AND item_id != ?",
        )?;
        let mut written = 0;
        let misplaced = misplaced(&store.db, conversation_id, items)?;
        for ((index, item), misplaced) in (0_i64..).zip(items).zip(misplaced) {
            let Some(id) = item
                .get("id")
                .and_then(Value::as_str)
                .filter(|id| !id.is_empty())
            else {
                continue;
            };
            if misplaced {
                continue;
            }
            let text = item.get("text").and_then(Value::as_str).unwrap_or_default();
            let role = item
                .get("role")
                .and_then(Value::as_str)
                .filter(|role| TRANSCRIPT_ROLES.contains(role))
                .unwrap_or("custom");
            let drawn = Drawn::of(item);
            let thinking = drawn.as_ref().is_some_and(|drawn| drawn.kind == "thinking");
            let text = if role == "tool" || thinking {
                cut(text, TRANSCRIPT_ITEM_MAX)
            } else {
                text.to_string()
            };
            let complete = item.get("complete") != Some(&Value::Bool(false));
            let at = item
                .get("at")
                .and_then(Value::as_str)
                .and_then(time::canonical);
            let wanted = from + index;
            let place = match known
                .query_row(params![conversation_id, id], |row| row.get::<_, i64>(0))
                .optional()?
            {
                Some(held) if wanted <= held => held,
                _ if holder.exists(params![conversation_id, wanted, id])? => next,
                _ => wanted,
            };
            next = next.max(place + 1);
            let changed = upsert
                .query_row(
                    params![
                        conversation_id,
                        id,
                        place,
                        role,
                        text,
                        i64::from(complete),
                        at,
                        copied_at,
                        i64::from(item.get("compaction") == Some(&Value::Bool(true))),
                        drawn.as_ref().map(Drawn::json),
                        i64::from(item.get("drawnOnly") == Some(&Value::Bool(true))),
                    ],
                    |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
                )
                .optional()?;
            let Some((row, compaction)) = changed else {
                continue;
            };
            let parts = parts_of(&text, id.len() > ID_IN_ROW);
            write_head(
                &store.db,
                &Head {
                    item: row,
                    conversation_id,
                    role,
                    bytes: text.len(),
                    parts: parts.len(),
                    id_shown: shown_id(id),
                    complete,
                    at,
                    compaction,
                },
            )?;
            write_parts(&store.db, row, &parts)?;
            write_drawn(&store.db, row, drawn.as_ref())?;
            written += 1;
        }
        Ok(written)
    })
}

/// The most bytes of an item's id its text is read past: the text of an item
/// whose id is longer is kept in parts however short, so that no reader of the
/// history goes through the id, which the item's row holds before its text, to
/// reach it. An id a page shows whole ([`ID_SHOWN`] characters) is no longer.
pub(crate) const ID_IN_ROW: usize = 4 * ID_SHOWN;

/// How many of the last characters of its text each part holds again before
/// it, what a search that goes on in the next part reads of it: as many as a
/// page shows before what a search found, more than a phrase it may hold.
pub(crate) const PART_TAIL: usize = 2_000;

/// What the history viewer reads of the item at row `item` but its text, as
/// migration 0020's `transcript_head` holds it.
struct Head<'a> {
    item: i64,
    conversation_id: i64,
    role: &'a str,
    bytes: usize,
    parts: usize,
    id_shown: String,
    complete: bool,
    at: Option<String>,
    compaction: i64,
}

/// Writes `head`, with its row: its id as a page shows it and its mark of a
/// compaction's summary as they were when it was first copied, which they stay,
/// as its row's do; its role, its text's size, how many parts it is kept in,
/// whether it is complete and its time as they are now; and the next revision
/// of it where it was written before.
fn write_head(db: &Connection, head: &Head<'_>) -> rusqlite::Result<()> {
    db.prepare_cached(
        "INSERT INTO transcript_head (item, conversation_id, role, bytes, parts, id_shown, complete, at, compaction)
       VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
       ON CONFLICT (item) DO UPDATE
         SET role = excluded.role, bytes = excluded.bytes, parts = excluded.parts,
             complete = excluded.complete, at = excluded.at,
             revision = transcript_head.revision + 1",
    )?
    .execute(params![
        head.item,
        head.conversation_id,
        head.role,
        number(head.bytes),
        number(head.parts),
        head.id_shown,
        i64::from(head.complete),
        head.at,
        head.compaction
    ])?;
    Ok(())
}

/// An item's id, where it has one that is not empty.
fn id_of(item: &Value) -> Option<&str> {
    item.get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
}

/// Which of `items` are only the history's and would stand out of their place: not copied
/// yet, before an item that is. The others are copied as ever.
fn misplaced(
    db: &Connection,
    conversation_id: i64,
    items: &[Value],
) -> rusqlite::Result<Vec<bool>> {
    let mut copied =
        db.prepare_cached("SELECT 1 FROM transcript WHERE conversation_id = ? AND item_id = ?")?;
    let mut misplaced = vec![false; items.len()];
    let mut later_copied = false;
    for (at, item) in items.iter().enumerate().rev() {
        let Some(id) = id_of(item) else {
            continue;
        };
        let present = copied.exists(params![conversation_id, id])?;
        let drawn_only = item.get("drawnOnly") == Some(&Value::Bool(true));
        misplaced[at] = drawn_only && !present && later_copied;
        later_copied |= present;
    }
    Ok(misplaced)
}

/// What the history draws of an item besides its words, as the copy keeps it: what the
/// item's `detail` says, each field read as the ledger takes it (a kind or a status it does
/// not know is none, a header cut to [`HEADER_MAX`] characters, a name to [`NAME_MAX`], a
/// change to [`TRANSCRIPT_ITEM_MAX`]). None where the item says no kind the ledger knows:
/// it draws as a plain one, of its role.
#[derive(Debug, PartialEq)]
struct Drawn {
    kind: &'static str,
    name: Option<String>,
    header: Option<String>,
    status: Option<&'static str>,
    took: Option<u64>,
    tokens: Option<u64>,
    diff: Option<String>,
}

/// The kinds of item the history draws.
const KINDS: [&str; 6] = ["prompt", "text", "thinking", "tool", "compaction", "notice"];

/// How a tool's call may stand.
const STATUSES: [&str; 3] = ["running", "ok", "failed"];

/// The most characters of a call's header the copy keeps: as many as a page shows of an
/// id ([`ID_SHOWN`]).
pub(crate) const HEADER_MAX: usize = ID_SHOWN;

/// The most characters of a tool's name the copy keeps.
pub(crate) const NAME_MAX: usize = 64;

impl Drawn {
    fn of(item: &Value) -> Option<Self> {
        let detail = item.get("detail")?;
        let word = |name: &str, words: &[&'static str]| {
            let said = detail.get(name).and_then(Value::as_str)?;
            words.iter().copied().find(|word| *word == said)
        };
        let text = |name: &str, max: usize| {
            detail
                .get(name)
                .and_then(Value::as_str)
                .map(|text| text.chars().take(max).collect::<String>())
        };
        Some(Self {
            kind: word("kind", &KINDS)?,
            name: text("name", NAME_MAX),
            header: text("header", HEADER_MAX),
            status: word("status", &STATUSES),
            took: detail.get("tookMs").and_then(Value::as_u64),
            tokens: detail.get("tokens").and_then(Value::as_u64),
            diff: detail
                .get("diff")
                .and_then(Value::as_str)
                .map(|diff| cut(diff, TRANSCRIPT_ITEM_MAX)),
        })
    }

    /// As the item's row keeps it: JSON of what it says, in one order, so that the same
    /// is the same text.
    fn json(&self) -> String {
        serde_json::to_string(&self.detail()).unwrap_or_default()
    }

    /// As a reader of the ledger hands it out.
    fn detail(&self) -> ItemDetail {
        ItemDetail {
            kind: self.kind.to_owned(),
            name: self.name.clone(),
            header: self.header.clone(),
            status: self.status.map(str::to_owned),
            took_ms: self.took,
            tokens: self.tokens,
            diff: self.diff.clone(),
        }
    }
}

/// What an item's row keeps of what the history draws of it, read back: none for none, or
/// for JSON no build wrote.
pub(crate) fn detail_of(json: Option<&str>) -> Option<ItemDetail> {
    serde_json::from_str(json?).ok()
}

/// Writes what the history draws of the item at row `item` besides its words where the
/// viewer reads it: its short fields in its head, and the change an edit made in a row of
/// its own (taken away where there is none). An item that says none draws as a plain one.
fn write_drawn(db: &Connection, item: i64, drawn: Option<&Drawn>) -> rusqlite::Result<()> {
    let diff = drawn.and_then(|drawn| drawn.diff.as_deref());
    db.prepare_cached(
        "UPDATE transcript_head SET kind = ?2, name = ?3, header = ?4, status = ?5, took = ?6,
           tokens = ?7, diff_bytes = ?8
         WHERE item = ?1",
    )?
    .execute(params![
        item,
        drawn.map(|drawn| drawn.kind),
        drawn.and_then(|drawn| drawn.name.as_deref()),
        drawn.and_then(|drawn| drawn.header.as_deref()),
        drawn.and_then(|drawn| drawn.status),
        drawn
            .and_then(|drawn| drawn.took)
            .map(|took| i64::try_from(took).unwrap_or(i64::MAX)),
        drawn
            .and_then(|drawn| drawn.tokens)
            .map(|tokens| i64::try_from(tokens).unwrap_or(i64::MAX)),
        number(diff.map_or(0, str::len)),
    ])?;
    match diff {
        Some(diff) => db
            .prepare_cached(
                "INSERT INTO transcript_diff (item, text) VALUES (?, ?)
           ON CONFLICT (item) DO UPDATE SET text = excluded.text
             WHERE transcript_diff.text != excluded.text",
            )?
            .execute(params![item, diff])?,
        None => db
            .prepare_cached("DELETE FROM transcript_diff WHERE item = ?")?
            .execute([item])?,
    };
    Ok(())
}

/// `count` as SQLite takes a number.
fn number(count: usize) -> i64 {
    i64::try_from(count).unwrap_or(i64::MAX)
}

/// An item's id as a page shows it: whole, where it is no longer than
/// [`ID_SHOWN`] characters; else its first [`ID_SHOWN`] characters and "…".
fn shown_id(id: &str) -> String {
    match id.char_indices().nth(ID_SHOWN) {
        Some((end, _)) => format!("{}…", &id[..end]),
        None => id.to_owned(),
    }
}

/// Writes the head of every item copied before the copy wrote heads, from its
/// row: what migration 0020 does past its SQL, before it splits the long items
/// into parts, which hang from their heads. Its time is kept as a store keeps
/// one, and written back to its row where that is not how it was written: as
/// the copy writes it, none where it is no time. An id or a time that is no
/// UTF-8 (which no build writes) is read as it reads, U+FFFD for what is none.
/// How many rows' times were written back.
pub(crate) fn head_copied(db: &Connection) -> rusqlite::Result<usize> {
    let mut heads = db.prepare(
        "SELECT rowid, conversation_id, item_id, role, octet_length(text), complete, at, compaction
       FROM transcript",
    )?;
    let mut rows = heads.query([])?;
    let mut retimed = Vec::new();
    while let Some(row) = rows.next()? {
        let item: i64 = row.get(0)?;
        let given = match row.get_ref(6)? {
            rusqlite::types::ValueRef::Text(at) => Some(text_of(at)),
            _ => None,
        };
        let at = given.as_deref().and_then(time::canonical);
        if at != given {
            retimed.push((item, at.clone()));
        }
        let role: String = row.get(3)?;
        write_head(
            db,
            &Head {
                item,
                conversation_id: row.get(1)?,
                role: &role,
                bytes: row.get::<_, i64>(4)?.unsigned_abs() as usize,
                parts: 0,
                id_shown: shown_id(&text_of(row.get_ref(2)?.as_bytes()?)),
                complete: row.get::<_, i64>(5)? == 1,
                at,
                compaction: row.get(7)?,
            },
        )?;
    }
    let mut retime = db.prepare("UPDATE transcript SET at = ?2 WHERE rowid = ?1")?;
    for (item, at) in &retimed {
        retime.execute(params![item, at])?;
    }
    Ok(retimed.len())
}

/// The parts `text` is kept in: [`TRANSCRIPT_ITEM_MAX`] characters each, the
/// last fewer, where it is longer than one; else the whole of it, one part,
/// where it must be kept in parts all the same (`kept`, the item's id being
/// longer than [`ID_IN_ROW`]); else none.
fn parts_of(text: &str, kept: bool) -> Vec<&str> {
    let starts: Vec<usize> = text
        .char_indices()
        .step_by(TRANSCRIPT_ITEM_MAX)
        .map(|(at, _)| at)
        .collect();
    if starts.len() < 2 {
        return if kept { vec![text] } else { Vec::new() };
    }
    let ends = starts.iter().skip(1).copied().chain([text.len()]);
    starts
        .iter()
        .zip(ends)
        .map(|(&start, end)| &text[start..end])
        .collect()
}

/// The end of a part its row holds before it: its last [`PART_TAIL`] characters.
fn tail_of(part: &str) -> &str {
    let start = part
        .char_indices()
        .rev()
        .nth(PART_TAIL - 1)
        .map_or(0, |(at, _)| at);
    &part[start..]
}

/// Keeps the text of the item at row `item` in `parts` ([`parts_of`]), so that
/// a reader reads a part of it and no more: each part that changed is written,
/// with its end, and the parts past the last are taken away (all of them where
/// there are none).
fn write_parts(db: &Connection, item: i64, parts: &[&str]) -> rusqlite::Result<()> {
    let mut write = db.prepare_cached(
        "INSERT INTO transcript_part (item, part, tail, text) VALUES (?, ?, ?, ?)
       ON CONFLICT (item, part) DO UPDATE SET tail = excluded.tail, text = excluded.text
         WHERE transcript_part.text != excluded.text",
    )?;
    for (part, piece) in (0_i64..).zip(parts) {
        write.execute(params![item, part, tail_of(piece), piece])?;
    }
    db.prepare_cached("DELETE FROM transcript_part WHERE item = ? AND part >= ?")?
        .execute(params![item, number(parts.len())])?;
    Ok(())
}

/// Keeps in parts every item copied before the copy kept them: what migration
/// 0020 does past its SQL, once, after it wrote their heads, and writes how many
/// to each head. Only a text of more bytes than a part's characters can be
/// longer than a part, and only those, and the items of ids longer than
/// [`ID_IN_ROW`], are read, one at a time. A text that is no UTF-8 (neither
/// Node's ledger nor this one writes one, each writing its language's own
/// strings) is split as it reads, U+FFFD for what is none, never refused.
pub(crate) fn split_copied(db: &Connection) -> rusqlite::Result<()> {
    let long = db
        .prepare(
            "SELECT rowid, octet_length(item_id) > ?2 FROM transcript
       WHERE octet_length(text) > ?1 OR octet_length(item_id) > ?2",
        )?
        .query_map(
            params![number(TRANSCRIPT_ITEM_MAX), number(ID_IN_ROW)],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, bool>(1)?)),
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut read = db.prepare("SELECT text FROM transcript WHERE rowid = ?")?;
    let mut write =
        db.prepare("INSERT INTO transcript_part (item, part, tail, text) VALUES (?, ?, ?, ?)")?;
    let mut count = db.prepare("UPDATE transcript_head SET parts = ?2 WHERE item = ?1")?;
    for (row, kept) in long {
        let text = read.query_row([row], |found| Ok(text_of(found.get_ref(0)?.as_bytes()?)))?;
        let parts = parts_of(&text, kept);
        for (part, piece) in (0_i64..).zip(&parts) {
            write.execute(params![row, part, tail_of(piece), piece])?;
        }
        count.execute(params![row, number(parts.len())])?;
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

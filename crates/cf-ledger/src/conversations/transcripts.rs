//! The copy ConsensFlow keeps of each conversation, item by item, and what
//! the windows that had a task wrote.

mod drawn;
mod parts;
mod read;

use cf_base::time;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;

use self::drawn::{write_drawn, Drawn};
use self::parts::{parts_of, shown_id, write_head, write_parts, Head};
use super::{known_conversation, ID_SHOWN};
use crate::model::{cut, LedgerError};
use crate::store::Store;

pub(crate) use self::drawn::{HEADER_MAX, NAME_MAX};
pub(crate) use self::parts::{head_copied, split_copied};
pub(crate) use self::read::{
    copied_item_with, copied_items_since, copied_items_with, items_of, transcript, typed_at,
    typed_since,
};

/// The most of one tool's output that is copied: it can run to megabytes,
/// and the tool can be run again. Words are copied whole (the human's, an
/// agent's, ConsensFlow's): a chief switched in reads them in `cf history`.
pub const TRANSCRIPT_ITEM_MAX: usize = 64_000;

const TRANSCRIPT_ROLES: [&str; 4] = ["user", "assistant", "tool", "custom"];

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
            let Some(id) = id_of(item) else {
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

/// `count` as SQLite takes a number.
pub(super) fn number(count: usize) -> i64 {
    i64::try_from(count).unwrap_or(i64::MAX)
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

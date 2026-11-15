//! The head and the parts the copy keeps of an item for the history viewer to
//! read (migration 0020), and the same written for the items copied before.

use cf_base::time;
use rusqlite::{params, Connection};

use super::{number, ID_IN_ROW, PART_TAIL, TRANSCRIPT_ITEM_MAX};
use crate::conversations::ID_SHOWN;
use crate::text::text_of;

/// What the history viewer reads of the item at row `item` but its text, as
/// migration 0020's `transcript_head` holds it.
pub(super) struct Head<'a> {
    pub(super) item: i64,
    pub(super) conversation_id: i64,
    pub(super) role: &'a str,
    pub(super) bytes: usize,
    pub(super) parts: usize,
    pub(super) id_shown: String,
    pub(super) complete: bool,
    pub(super) at: Option<String>,
    pub(super) compaction: i64,
}

/// Writes `head`, with its row: its id as a page shows it and its mark of a
/// compaction's summary as they were when it was first copied, which they stay,
/// as its row's do; its role, its text's size, how many parts it is kept in,
/// whether it is complete and its time as they are now; and the next revision
/// of it where it was written before.
pub(super) fn write_head(db: &Connection, head: &Head<'_>) -> rusqlite::Result<()> {
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

/// An item's id as a page shows it: whole, where it is no longer than
/// [`ID_SHOWN`] characters; else its first [`ID_SHOWN`] characters and "…".
pub(super) fn shown_id(id: &str) -> String {
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
pub(super) fn parts_of(text: &str, kept: bool) -> Vec<&str> {
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
pub(super) fn write_parts(db: &Connection, item: i64, parts: &[&str]) -> rusqlite::Result<()> {
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

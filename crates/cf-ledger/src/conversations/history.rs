//! A participant's whole history, as the history viewer reads it, a page at a
//! time: the items copied of every conversation it had, the current one too,
//! as one timeline, read in order from where the viewer is (`read_history`),
//! with no heading of any conversation (chief-harness, CHF-11): a conversation
//! is the place its items stand at, and one with none is not there; a long
//! item's text, a part at a time (`read_history_part`); and the parties of a
//! message a delivery names, to tell whether it is the participant's
//! (`message_parties`).
//!
//! What a read costs is bounded, whatever the size of the history or of one
//! item in it. It reaches what it reads through the indexes of migration 0020
//! and scans nothing: one conversation after another, each one's items in the
//! order they were first copied, from a place a copy made again never moves
//! ([`HistoryKey`]), until the page is full ([`HistoryAsk::room`]) or the read
//! has gone through as much as one may ([`HistoryAsk::scan`]): each
//! conversation it comes to, each row and each part it looks up, and each byte
//! of text is charged before it is read, and each field as what it is. Of a
//! conversation it reads its id alone, from its index. A tool's output is not
//! even come to when the tools are left out; a long item is read a part at a
//! time, from the parts its copy keeps, never whole. Of an item it reads its
//! text and its head (migration 0020's `transcript_head`): its role, its size,
//! its parts, its id as a page shows it (never the id itself), and the fields
//! its row holds past its id and its text, never from there. Of the participant
//! it reads what an index holds of it, and of its project that it is there. A
//! row holds its values one after another, and reading one goes through those
//! before it, however long: so every query of the history reads a row only up
//! to fields that are short, and no further (each listed in the plan test
//! below). Every field it reads is short whatever an older build wrote: each
//! write keeps it within its ceiling ([`crate::ID_SHOWN`], a time as a store
//! keeps one) and migration 0020 brought what was past it within it. No value
//! is loaded whole that may be long and is not shown: a message's body.
//!
//! Every read moves, however little it may go through: the first step it
//! takes (a conversation, a row, a part) it takes, so that it shows an entry or
//! passes one, and the next read goes on from there. That step is one part at
//! most: a search that goes on inside an item reads the end of the part before
//! it ([`TAIL_BYTES`]), never the whole of it.

mod queries;
mod reading;
mod text;

use cf_proto::ledger::{HistoryFrom, HistoryKey, HistoryRead, MessageParties, TextPart};
use rusqlite::{params, OptionalExtension};

use self::queries::{
    CURRENT_CONVERSATION, MESSAGE_PARTIES, NEWEST_ITEM, PART_HEAD, PART_TEXT, PROJECT_THERE,
    ROW_TEXT, WHOSE,
};
use self::reading::Reading;
use self::text::chars;
use super::transcripts::number;
use crate::model::{quoted, LedgerError};
use crate::store::Store;
use crate::text::text_of;

/// The most of an item's text a page shows, and a part holds: in characters,
/// as many as a tool's output is copied of, which a frame always carries. An
/// item longer than that is kept in parts of it as well.
pub const HISTORY_PART: usize = crate::conversations::TRANSCRIPT_ITEM_MAX;

/// The most one read goes through, in bytes of text and of fields, with each
/// conversation, each row and each part it comes to charged as some more
/// ([`CONVERSATION_COST`], [`ROW_COST`], [`PART_COST`]): a read that gets there
/// stops, and says where, so the next one goes on from it. Its first step it
/// takes whatever this is.
pub const HISTORY_SCAN_BYTES: usize = 16 * 1024 * 1024;

/// The most characters a search holds. A phrase no longer than a part and one
/// character spans two parts at most, so each part searched with the end of
/// the one before finds it wherever it is; this is far below that, and a
/// search for more is refused, never searched short.
pub const HISTORY_FIND_MAX: usize = 1_000;

/// What a read of the history is asked for: from where and which way, the
/// items that hold `find` alone (in any case), the tools' outputs or not, how
/// many bytes of items fill a page (`room`, as their JSON), and
/// the most it goes through (`scan`, [`HISTORY_SCAN_BYTES`]).
#[derive(Debug, Clone, Copy)]
pub struct HistoryAsk<'a> {
    pub from: HistoryFrom,
    pub find: Option<&'a str>,
    pub tools: bool,
    pub room: usize,
    pub scan: usize,
}

/// One read of participant `handle`'s history, as [`HistoryAsk`] says: the
/// items in the order the read went (newest first, unless it reads after a
/// place), across conversations. A search for more than [`HISTORY_FIND_MAX`]
/// characters is refused. Nothing is written.
pub(crate) fn read_history(
    store: &Store,
    project_id: i64,
    handle: &str,
    ask: &HistoryAsk<'_>,
) -> Result<HistoryRead, LedgerError> {
    let participant = whose(store, project_id, handle)?;
    let asked = ask.find.map_or(0, |find| find.chars().count());
    if asked > HISTORY_FIND_MAX {
        return Err(LedgerError::refused_with(
            "invalid-text",
            format!("a search holds at most 1,000 characters; this one has {asked}"),
            400,
        ));
    }
    let mut read = Reading::new(store, &participant, ask);
    read.run()?;
    Ok(read.into_read())
}

/// The participant a read is of: its id.
pub(super) struct Whose {
    pub(super) id: i64,
}

/// Participant `handle` of project `project_id`, still in it, as a read uses
/// it. Neither the project's row nor the participant's is read: each holds
/// fields a read does not use, which an older build may have written long, and
/// a value of a row is reached through the ones before it.
fn whose(store: &Store, project_id: i64, handle: &str) -> Result<Whose, LedgerError> {
    let project = store
        .db
        .prepare_cached(PROJECT_THERE)?
        .exists([project_id])?;
    if !project {
        return Err(LedgerError::refused_with(
            "unknown-project",
            format!("no project {project_id}"),
            404,
        ));
    }
    let found = store
        .db
        .prepare_cached(WHOSE)?
        .query_row(params![project_id, handle], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, bool>(1)?))
        })
        .optional()?;
    let Some((id, here)) = found else {
        return Err(LedgerError::refused_with(
            "unknown-participant",
            format!("{} is not in project {project_id}", quoted(handle)),
            404,
        ));
    };
    if !here {
        return Err(LedgerError::refused_with(
            "member-left",
            format!("@{handle} left the staff"),
            409,
        ));
    }
    Ok(Whose { id })
}

/// The part of the item at row `row` of conversation `conversation`, the
/// participant `handle`'s, from character `from` to the end of the part its
/// copy keeps it in (or of its text, where it is no longer than a part), where
/// the next begins, and the item's revision (from its head): what one part is,
/// and read alone, by the item's place and never by its id. An item that is
/// not the participant's is refused as one that is not there.
pub(crate) fn read_history_part(
    store: &Store,
    project_id: i64,
    handle: &str,
    conversation: i64,
    row: i64,
    from: usize,
) -> Result<TextPart, LedgerError> {
    let participant = whose(store, project_id, handle)?;
    let (parts, revision): (i64, i64) = store
        .db
        .prepare_cached(PART_HEAD)?
        .query_row(params![participant.id, conversation, row], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .optional()?
        .ok_or_else(|| {
            LedgerError::refused_with(
                "unknown-item",
                format!("no item at row {row} of {handle}'s conversation {conversation}"),
                404,
            )
        })?;
    let parts = parts.unsigned_abs() as usize;
    if parts > 0 {
        let index = (from / HISTORY_PART).min(parts - 1);
        let text: String = store
            .db
            .prepare_cached(PART_TEXT)?
            .query_row(params![row, number(index)], |found| found.get(0))
            .optional()?
            .unwrap_or_default();
        return Ok(TextPart {
            text: chars(&text, from - index * HISTORY_PART, HISTORY_PART).to_owned(),
            from,
            next: (index + 1 < parts).then_some((index + 1) * HISTORY_PART),
            revision,
        });
    }
    let text = store
        .db
        .prepare_cached(ROW_TEXT)?
        .query_row([row], |found| Ok(text_of(found.get_ref(0)?.as_bytes()?)))?;
    let part = chars(&text, from, HISTORY_PART);
    let next = from + part.chars().count();
    Ok(TextPart {
        text: part.to_owned(),
        from,
        next: (next < text.chars().count()).then_some(next),
        revision,
    })
}

/// Message `id`'s parties, as far as the history page needs to tell whether a
/// delivery names one of a participant's: the fields its row holds before its
/// body, and its sender's handle. The body is never read, so that however long
/// it is no more of the row is.
pub(crate) fn message_parties(
    store: &Store,
    id: i64,
) -> Result<Option<MessageParties>, LedgerError> {
    Ok(store
        .db
        .prepare_cached(MESSAGE_PARTIES)?
        .query_row([id], |row| {
            Ok(MessageParties {
                id,
                project_id: row.get("project_id")?,
                recipient_id: row.get("recipient_id")?,
                sender: row.get("sender")?,
            })
        })
        .optional()?)
}

/// Where the one item of participant `handle`'s history that may still be written stands:
/// the newest of the conversation it has now. Every other item is behind a later one, or in
/// a conversation that ended, and is not being written any more, whatever its reader said
/// of it; none where the conversation has no item. Two lookups by an index each, whatever
/// the history holds, and no text read: of the conversation's row only what is short.
pub(crate) fn live_item(
    store: &Store,
    project_id: i64,
    handle: &str,
) -> Result<Option<HistoryKey>, LedgerError> {
    let participant = whose(store, project_id, handle)?;
    let Some(conversation) = store
        .db
        .prepare_cached(CURRENT_CONVERSATION)?
        .query_row([participant.id], |row| row.get::<_, i64>(0))
        .optional()?
    else {
        return Ok(None);
    };
    let newest = store
        .db
        .prepare_cached(NEWEST_ITEM)?
        .query_row([conversation], |row| row.get::<_, i64>(0))
        .optional()?;
    Ok(newest.map(|row| HistoryKey {
        conversation,
        row,
        part: 0,
    }))
}

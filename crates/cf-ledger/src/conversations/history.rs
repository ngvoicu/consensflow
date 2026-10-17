//! A participant's whole history, as the history viewer reads it, a page at a
//! time: every conversation it had, the current one too, headed with the agent
//! it ran on and what ended it as far as the ledger's log says, and the items
//! copied of them, read in order from where the viewer is (`read_history`); a
//! long item's text, a part at a time (`read_history_part`); and the opening
//! of a message a delivery names, for its line (`message_opening`).
//!
//! What a read costs is bounded, whatever the size of the history or of one
//! item in it. It reaches what it reads through the indexes of migration 0019
//! and scans nothing: one conversation after another, each one's items in the
//! order they were first copied, from a place a copy made again never moves
//! ([`HistoryKey`]), until the page is full ([`HistoryAsk::room`], the headings
//! with the items) or the read has gone through as much as one may
//! ([`HistoryAsk::scan`]): each conversation it comes to, each row and each
//! part it looks up, and each byte of text is charged before it is read, and
//! each field as what it is. A tool's output is not even come to when the
//! tools are left out; a heading is a few lookups of the log, and counts the
//! tools' outputs only so far; a long item is read a part at a time, from the
//! parts its copy keeps, never whole. Of an item it reads its text and its head
//! (migration 0019's `transcript_head`): its role, its size, its parts, its id
//! as a page shows it (never the id itself), and the fields its row holds past
//! its id and its text, never from there. Of the participant it reads what an
//! index holds of it, and of its project that it is there. A row holds its
//! values one after another, and reading one goes through those before it,
//! however long: so every query of the history reads a row only up to fields
//! that are short, and no further (each listed in the plan test below).
//! Every field it reads is short whatever an older build wrote: each write keeps
//! it within its ceiling ([`crate::FIELD_MAX`], [`crate::ID_SHOWN`],
//! [`crate::NATIVE_SESSION_MAX`], [`crate::HEADING_EVENT_MAX`], a time as a store
//! keeps one) and migration 0019 brought what was past it within it. No value
//! is loaded whole that may be long and is not shown: a conversation's role, a
//! message's body.
//!
//! Every read moves, however little it may go through: the first step it
//! takes (a conversation, a row, a part) it takes, so that it shows an entry or
//! passes one, and the next read goes on from there. That step is one part at
//! most: a search that goes on inside an item reads the end of the part before
//! it ([`TAIL_BYTES`]), never the whole of it.
//!
//! An event an older build wrote whose data is no JSON, or holds no whole
//! number where a conversation's id is, says nothing of any conversation here,
//! as the indexes of the log read it.

use cf_proto::ledger::{
    ConversationEnd, ConversationView, HistoryEntry, HistoryFrom, HistoryItem, HistoryKey,
    HistoryRead, HistoryRow, MessageOpening, ParticipantConversation, Scanned, TextCut, TextPart,
};
use rusqlite::types::ValueRef;
use rusqlite::{params, OptionalExtension, Params, Row, Statement, StatementStatus};
use serde_json::Value;

use super::transcripts::PART_TAIL;
use super::{FIELD_MAX, ID_SHOWN, NATIVE_SESSION_MAX};
use crate::model::{quoted, LedgerError};
use crate::openings::{opening_of, text_of};
use crate::store::Store;

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

/// The most bytes a conversation's fields a heading shows hold: its harness and
/// times, [`FIELD_MAX`] each (a time a store keeps is a few dozen), and its
/// native session, [`NATIVE_SESSION_MAX`] characters as JavaScript counts them.
const CONVERSATION_FIELDS: usize = 3 * FIELD_MAX + 3 * NATIVE_SESSION_MAX;

/// The most bytes a row's fields hold besides its text: its id as a page shows
/// it ([`ID_SHOWN`] characters and "…") and its time, as a store keeps one (its
/// role is one of four words).
const ROW_FIELDS: usize = 4 * ID_SHOWN + 3 + 64;

/// The most of a message's body its delivery's line reads: the line says its
/// first line, cut at 160 characters, which this holds behind any blank lines
/// a body could reasonably open with.
pub const MESSAGE_OPENING: usize = 4_096;

/// What reading a message's opening is charged, of what a read of the history
/// may go through: the opening's bytes, and the lookups of its row as a row's.
pub const OPENING_COST: usize = MESSAGE_OPENING + ROW_COST;

/// What a read is charged for each conversation it comes to, besides its rows,
/// its fields and the events its heading reads the data of: finding it, and
/// what else its heading takes (the lookups of the log by its indexes, and the
/// count of its tools' outputs).
const CONVERSATION_COST: usize = 4_096;

/// What a read is charged for each row it comes to, besides its text and its fields.
const ROW_COST: usize = 256;

/// What a read is charged for each lookup of a part of an item's text,
/// besides the bytes of it it reads: one by the item's row and the part's place.
const PART_COST: usize = 64;

/// How much of the part before it a search that goes on inside an item reads,
/// in bytes at most: the end its row holds before its text, as many characters
/// as a page shows before what it finds and more than the end of a phrase it
/// may hold ([`HISTORY_FIND_MAX`]).
const TAIL_BYTES: usize = 4 * PART_TAIL;

/// What the log says a conversation is, as the indexes of 0019 read it: none
/// where the event's data is no JSON. A lookup says it so, word for word, for
/// SQLite to find it in the index.
const CONVERSATION_OF: &str =
    "CASE WHEN json_valid(data) THEN json_extract(data, '$.conversation') END";

/// What the log says a participant is, as the index of 0019 reads it.
const PARTICIPANT_OF: &str =
    "CASE WHEN json_valid(data) THEN json_extract(data, '$.participant') END";

/// The items of conversation `?1` from row `?2` the way a read goes
/// (`newest_first`, the row itself too where `with_it`), as their heads say
/// them, gone through by the index of what the read shows (every item, or none
/// of the tools' outputs). The items' own rows are not read: each holds its
/// role past its id, and the rest past its text.
fn rows_from(tools: bool, newest_first: bool, with_it: bool) -> String {
    format!(
        "SELECT item AS row_id, role, bytes, parts, id_shown, complete, at, compaction, revision,
         octet_length(id_shown) + ifnull(octet_length(at), 0) AS field_bytes
       FROM transcript_head INDEXED BY {}
       WHERE conversation_id = ?1{} AND item {}{} ?2 ORDER BY item {}",
        if tools {
            "transcript_head_conversation_index"
        } else {
            "transcript_head_said_index"
        },
        if tools { "" } else { " AND role != 'tool'" },
        if newest_first { "<" } else { ">" },
        if with_it { "=" } else { "" },
        if newest_first { "DESC" } else { "ASC" },
    )
}

/// The participant's (`?1`) conversation next to `?2` the way a read goes
/// (`newest_first`), found by its index: the fields its heading shows, which
/// its row holds before the role it was given, which can be long, and their
/// size.
fn conversation_from(newest_first: bool) -> String {
    format!(
        "SELECT id, harness, native_session, started_at, ended_at,
         octet_length(harness) + ifnull(octet_length(native_session), 0)
           + octet_length(started_at) + ifnull(octet_length(ended_at), 0) AS bytes
       FROM conversation INDEXED BY conversation_participant_index
       WHERE participant_id = ?1 AND id {} ?2 ORDER BY id {} LIMIT 1",
        if newest_first { "<" } else { ">" },
        if newest_first { "DESC" } else { "ASC" },
    )
}

/// Whether there is project `?1`, by its row's id alone, none of its row read.
const PROJECT_THERE: &str = "SELECT 1 FROM project WHERE id = ?";

/// Participant `?2` of project `?1`: its id, whether it left, its role and its
/// agent, all from the index that holds them, never from its row, which holds
/// the roles it was given before its agent.
const WHOSE: &str = "SELECT id, left_at IS NULL, role, agent FROM participant
       INDEXED BY participant_history_index WHERE project_id = ?1 AND handle = ?2";

/// The head of the item at row `?3` of conversation `?2` of participant `?1`:
/// how many parts its text is kept in, and which copy of it its row holds;
/// none where it is not the participant's.
const PART_HEAD: &str = "SELECT h.parts, h.revision FROM transcript_head h
         JOIN conversation c ON c.id = h.conversation_id
       WHERE c.participant_id = ?1 AND h.conversation_id = ?2 AND h.item = ?3";

/// How many of conversation `?1`'s tools' outputs there are, `?2` at most:
/// counted in the index that holds them alone.
const TOOLS_OF: &str = "SELECT COUNT(*) FROM (SELECT 1 FROM transcript_head
         INDEXED BY transcript_head_tool_index WHERE conversation_id = ?1 AND role = 'tool' LIMIT ?2)";

/// The size in bytes of part `?2` of the item at row `?1`, from its row's
/// header alone.
const PART_SIZE: &str =
    "SELECT octet_length(text) FROM transcript_part WHERE item = ?1 AND part = ?2";

/// Part `?2` of the item at row `?1`, its text: read past the part's end alone
/// ([`PART_TAIL`] characters).
const PART_TEXT: &str = "SELECT text FROM transcript_part WHERE item = ?1 AND part = ?2";

/// The end of part `?2` of the item at row `?1`, which its row holds before its text.
const PART_END: &str = "SELECT tail FROM transcript_part WHERE item = ?1 AND part = ?2";

/// The text of the item at row `?1`, one kept in no parts: read past its id
/// alone, which is no longer than
/// [`ID_IN_ROW`](super::transcripts::ID_IN_ROW) bytes, as one whose id is
/// longer is kept in parts.
const ROW_TEXT: &str = "SELECT text FROM transcript WHERE rowid = ?1";

/// The first (or, `order` says, the last) event that began conversation `?1`
/// or took a window to it; with `before`, the first before event `?2`.
fn activation_of(order: &str, before: bool) -> String {
    format!(
        "SELECT id FROM event INDEXED BY event_activation_index
       WHERE kind IN ('conversation.started', 'conversation.followed')
         AND {CONVERSATION_OF} = ?1{} {order} LIMIT 1",
        if before { " AND id < ?2" } else { "" }
    )
}

/// The first Switch chief of project `?1` after event `?2`, and its data.
const SWITCH_AFTER: &str =
    "SELECT id, data, octet_length(data) AS bytes FROM event INDEXED BY event_switch_index
       WHERE kind = 'chief.switched' AND project_id = ?1 AND id > ?2 ORDER BY id LIMIT 1";

/// The first `conversation.ended` of conversation `?1` after event `?2`.
fn ended_after() -> String {
    format!(
        "SELECT id FROM event INDEXED BY event_ended_index
       WHERE kind = 'conversation.ended' AND {CONVERSATION_OF} = ?1 AND id > ?2
       ORDER BY id LIMIT 1"
    )
}

/// The first of participant `?2`'s conversations of project `?1` to begin or
/// be gone to after event `?3`: which event, of what kind, the conversation
/// the log names, and the size of its data.
fn went_after() -> String {
    format!(
        "SELECT id, kind, {CONVERSATION_OF}, octet_length(data) FROM event
         INDEXED BY event_participant_index
       WHERE kind IN ('conversation.started', 'conversation.followed')
         AND project_id = ?1 AND {PARTICIPANT_OF} = ?2 AND id > ?3
       ORDER BY id LIMIT 1"
    )
}

/// Message `?1`'s fields a delivery's line shows, each of which its row holds
/// before its body, and its sender's handle and its task's number, which theirs
/// hold before anything long.
const MESSAGE_FIELDS: &str = "SELECT m.project_id, m.recipient_id, s.handle AS sender, m.kind, t.number AS task_number
       FROM message m LEFT JOIN participant s ON s.id = m.sender_id LEFT JOIN task t ON t.id = m.task_id
       WHERE m.id = ?";

/// How many of a conversation's tools' outputs its heading counts, at most.
const TOOLS_COUNTED: usize = 1_000;

/// How much of an item's text a search shows before the words it found.
const BEFORE_A_MATCH: usize = 2_000;

/// What the page says of an entry besides its fields: a delivery's line, the
/// part it shows, the place it stands.
const ROW_SLACK: usize = 600;

/// What a read of the history is asked for: from where and which way, the
/// items that hold `find` alone (in any case), the tools' outputs or not, how
/// many bytes of entries and headings fill a page (`room`, as their JSON), and
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
/// entries in the order the read went (newest first, unless it reads after a
/// place), and the conversations it took an entry of, headed. A search for
/// more than [`HISTORY_FIND_MAX`] characters is refused. Nothing is written.
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
    let (newest_first, from) = match ask.from {
        HistoryFrom::Newest => (true, None),
        HistoryFrom::Before(key) => (true, Some(key)),
        HistoryFrom::After(key) => (false, Some(key)),
    };
    let mut read = Reading {
        store,
        participant: &participant,
        newest_first,
        needle: ask.find.filter(|find| !find.is_empty()).map(folded),
        tools: ask.tools,
        room: ask.room,
        scan: ask.scan,
        conversations: Vec::new(),
        entries: Vec::new(),
        used: 0,
        scanned: Scanned::default(),
        from,
        resume: from,
        full: false,
        bounded: false,
    };
    // From the place's own conversation, or from the newest.
    let mut past = match from {
        None => i64::MAX,
        Some(key) if newest_first => key.conversation.saturating_add(1),
        Some(key) => key.conversation.saturating_sub(1),
    };
    while read.may(CONVERSATION_COST + CONVERSATION_FIELDS) {
        let Some(conversation) = read.next_conversation(past)? else {
            break;
        };
        past = conversation.id;
        let within = from.filter(|key| key.conversation == conversation.id);
        read.conversation(&conversation, within)?;
        if read.full || read.bounded {
            break;
        }
    }
    let more = read.full || read.bounded;
    Ok(HistoryRead {
        participant: participant.id,
        charged: read.charged(),
        conversations: read.conversations,
        entries: read.entries,
        resume: if more { read.resume } else { None },
        bounded: read.bounded,
        scanned: read.scanned,
    })
}

/// The participant a read is of, as much of it as a read uses: its id, its
/// project, its handle (the one it was asked by), its role and its agent.
struct Whose<'a> {
    id: i64,
    project_id: i64,
    handle: &'a str,
    role: String,
    agent: Option<String>,
}

/// Participant `handle` of project `project_id`, still in it, as a read uses
/// it. Neither the project's row nor the participant's is read: each holds
/// fields a read does not use, which an older build may have written long, and
/// a value of a row is reached through the ones before it.
fn whose<'a>(store: &Store, project_id: i64, handle: &'a str) -> Result<Whose<'a>, LedgerError> {
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
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, bool>(1)?,
                row.get::<_, String>(2)?,
                text_or_none(row.get_ref(3)?),
            ))
        })
        .optional()?;
    let Some((id, here, role, agent)) = found else {
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
    Ok(Whose {
        id,
        project_id,
        handle,
        role,
        agent,
    })
}

/// A read as it goes: the entries taken and the headings of their
/// conversations, the room they take, what it went through, the place it began
/// at and that of the last entry it is done with, and why it stopped.
struct Reading<'a> {
    store: &'a Store,
    participant: &'a Whose<'a>,
    newest_first: bool,
    needle: Option<String>,
    tools: bool,
    room: usize,
    scan: usize,
    conversations: Vec<ParticipantConversation>,
    entries: Vec<HistoryEntry>,
    used: usize,
    scanned: Scanned,
    from: Option<HistoryKey>,
    resume: Option<HistoryKey>,
    full: bool,
    bounded: bool,
}

/// A row as a read comes to it, before it reads any of its text: where it
/// stands, its item as its head says it (its text yet to be read), which copy
/// of it the row holds, its text's size in bytes, and how many parts its text
/// is kept in (none: it is read from its row).
struct Met {
    key: HistoryKey,
    item: HistoryItem,
    revision: i64,
    bytes: usize,
    parts: usize,
}

impl Reading<'_> {
    /// What the read has been charged so far.
    fn charged(&self) -> usize {
        self.scanned.bytes
            + self.scanned.fields
            + self.scanned.rows * ROW_COST
            + self.scanned.parts * PART_COST
            + self.scanned.conversations * CONVERSATION_COST
    }

    /// Whether the read may go through `more` (as it is charged) and stay
    /// within its bound; where it may not, it is bounded. Until it has taken
    /// an entry or passed one it always may, so that the first step it takes
    /// is never held by its bound and every read moves.
    fn may(&mut self, more: usize) -> bool {
        let moved = !self.entries.is_empty() || self.resume != self.from;
        if moved && self.charged() + more > self.scan {
            self.bounded = true;
        }
        !self.bounded
    }

    /// The one row `sql` gives for `params`, if any, with the steps it took counted.
    fn one<T>(
        &mut self,
        sql: &str,
        params: impl Params,
        map: impl FnOnce(&Row<'_>) -> rusqlite::Result<T>,
    ) -> Result<Option<T>, LedgerError> {
        let mut statement = self.store.db.prepare_cached(sql)?;
        statement.reset_status(StatementStatus::VmStep);
        let found = statement.query_row(params, map).optional();
        self.scanned.steps += steps(&statement);
        Ok(found?)
    }

    /// The participant's conversation next to `past` the way the read goes,
    /// with the fields its heading shows, short whatever an older build wrote
    /// ([`CONVERSATION_FIELDS`]) and charged as what they are, and nothing else
    /// of its row: not the role it was given, which can be long. None where
    /// there is no more.
    fn next_conversation(&mut self, past: i64) -> Result<Option<ConversationView>, LedgerError> {
        let sql = conversation_from(self.newest_first);
        let participant_id = self.participant.id;
        let found = self.one(&sql, params![participant_id, past], |row| {
            let text = |name: &str| -> rusqlite::Result<Option<String>> {
                Ok(text_or_none(row.get_ref(name)?))
            };
            Ok((
                ConversationView {
                    id: row.get("id")?,
                    participant_id,
                    harness: text("harness")?.unwrap_or_default(),
                    native_session: text("native_session")?,
                    started_at: text("started_at")?.unwrap_or_default(),
                    ended_at: text("ended_at")?,
                },
                row.get::<_, i64>("bytes")?,
            ))
        })?;
        let Some((conversation, bytes)) = found else {
            return Ok(None);
        };
        self.scanned.conversations += 1;
        self.scanned.fields += bytes.unsigned_abs() as usize;
        Ok(Some(conversation))
    }

    /// Goes through `conversation` from `within`, the place the read began at
    /// where that is in it (the item itself too, where a search stopped inside
    /// it): its items the way the read goes, each taken or, in a search,
    /// passed; and its heading in their place, where the read came to it and
    /// it shows none.
    fn conversation(
        &mut self,
        conversation: &ConversationView,
        within: Option<HistoryKey>,
    ) -> Result<(), LedgerError> {
        let id = conversation.id;
        let (bound, with_it) = match within {
            Some(place) => (place.row, place.part > 0),
            None if self.newest_first => (i64::MAX, false),
            None => (0, false),
        };
        let sql = rows_from(self.tools, self.newest_first, with_it);
        let needle = self.needle.clone();
        let store = self.store;
        let mut statement = store.db.prepare_cached(&sql)?;
        statement.reset_status(StatementStatus::VmStep);
        let mut rows = statement.query(params![id, bound])?;
        let mut any = false;
        while self.may(ROW_COST + ROW_FIELDS) {
            let Some(row) = rows.next()? else {
                break;
            };
            self.scanned.rows += 1;
            self.scanned.fields += row.get::<_, i64>("field_bytes")?.unsigned_abs() as usize;
            any = true;
            let met = Met {
                key: HistoryKey {
                    conversation: id,
                    row: row.get("row_id")?,
                    part: 0,
                },
                item: HistoryItem {
                    id: text_or_none(row.get_ref("id_shown")?).unwrap_or_default(),
                    role: row.get("role")?,
                    text: String::new(),
                    complete: row.get::<_, i64>("complete")? == 1,
                    at: text_or_none(row.get_ref("at")?),
                    compaction: row.get::<_, i64>("compaction")? == 1,
                },
                revision: row.get("revision")?,
                bytes: row.get::<_, i64>("bytes")?.unsigned_abs() as usize,
                parts: row.get::<_, i64>("parts")?.unsigned_abs() as usize,
            };
            let from_part = within
                .filter(|place| place.row == met.key.row)
                .map_or(0, |place| place.part);
            let going = match &needle {
                None => self.show(conversation, met)?,
                Some(needle) => self.search(conversation, met, from_part, needle)?,
            };
            if !going {
                break;
            }
        }
        drop(rows);
        self.scanned.steps += steps(&statement);
        if self.full || self.bounded {
            return Ok(());
        }
        if needle.is_some() {
            // Through the whole conversation: the next read goes on past it.
            let edge = if self.newest_first { 0 } else { i64::MAX };
            self.resume = Some(HistoryKey {
                conversation: id,
                row: edge,
                part: 0,
            });
        } else if !any && within.is_none() {
            let heading = HistoryKey {
                conversation: id,
                row: 0,
                part: 0,
            };
            self.take(conversation, HistoryEntry::Heading(heading))?;
        }
        Ok(())
    }

    /// Takes the item `met`: whole, or its first part where it is kept in
    /// parts. An item the page has no room for is not read: its JSON is no
    /// shorter than its text.
    fn show(&mut self, conversation: &ConversationView, met: Met) -> Result<bool, LedgerError> {
        let first = self.part_bytes(&met, 0)?;
        let reading = first.unwrap_or(met.bytes);
        if !self.entries.is_empty() && self.used + reading + ROW_SLACK > self.room {
            self.full = true;
            return Ok(false);
        }
        if !self.may(reading) {
            return Ok(false);
        }
        let row = match first {
            None => self.whole_row(met)?,
            Some(_) => {
                let text = self.read_part(&met, 0)?;
                long_row(met, text, 0)
            }
        };
        self.take(conversation, HistoryEntry::Item(row))
    }

    /// Takes the item `met` if it holds `needle` (folded), from part
    /// `from_part` of it where the read before stopped inside it; else passes
    /// it. An item read from its row is read whole; one kept in parts a part at
    /// a time, each with the end of the one before, where the words may begin
    /// (the end of the part before the first it reads, where it goes on inside
    /// the item, read by itself: [`TAIL_BYTES`]). Says whether the read goes on.
    fn search(
        &mut self,
        conversation: &ConversationView,
        met: Met,
        from_part: usize,
        needle: &str,
    ) -> Result<bool, LedgerError> {
        if self.part_bytes(&met, 0)?.is_none() {
            if !self.may(met.bytes) {
                return Ok(false);
            }
            let key = met.key;
            let row = self.whole_row(met)?;
            if folded(&row.item.text).contains(needle) {
                return self.take(conversation, HistoryEntry::Item(row));
            }
            self.resume = Some(key);
            return Ok(true);
        }
        let reach = needle.chars().count().saturating_sub(1);
        // What the read has of the text before the part it searches: where it
        // begins, in characters of the item, and its text.
        let mut previous: Option<(usize, String)> = None;
        if let Some(before) = from_part.checked_sub(1) {
            if !self.may(TAIL_BYTES) {
                return Ok(false);
            }
            previous = self.read_tail(&met, before)?;
        }
        let mut part = from_part;
        while let Some(bytes) = self.part_bytes(&met, part)? {
            if !self.may(bytes) {
                return Ok(false);
            }
            let text = self.read_part(&met, part)?;
            let tail = previous
                .as_ref()
                .map_or("", |(_, before)| last_chars(before, reach));
            if let Some(at) = first_match(&format!("{tail}{text}"), needle) {
                let at = part * HISTORY_PART + at - tail.chars().count();
                let (from, shown) = around(previous.as_ref(), &text, part, at);
                return self.take(conversation, HistoryEntry::Item(long_row(met, shown, from)));
            }
            previous = Some((part * HISTORY_PART, text));
            part += 1;
            // Searched so far: a read that stops before the next part goes on from it.
            self.resume = Some(HistoryKey { part, ..met.key });
        }
        self.resume = Some(met.key);
        Ok(true)
    }

    /// The size in bytes of part `part` of item `met`, where its head says its
    /// copy keeps it in parts and has that one; none past them, and for an item
    /// read from its row. The lookup is charged.
    fn part_bytes(&mut self, met: &Met, part: usize) -> Result<Option<usize>, LedgerError> {
        if part >= met.parts {
            return Ok(None);
        }
        self.scanned.parts += 1;
        let found = self.one(PART_SIZE, params![met.key.row, number(part)], |row| {
            row.get::<_, i64>(0)
        })?;
        Ok(found.map(|bytes| bytes.unsigned_abs() as usize))
    }

    /// Part `part` of item `met`'s text, read and charged, its lookup too.
    fn read_part(&mut self, met: &Met, part: usize) -> Result<String, LedgerError> {
        self.scanned.parts += 1;
        let text = self
            .one(PART_TEXT, params![met.key.row, number(part)], |row| {
                row.get::<_, String>(0)
            })?
            .unwrap_or_default();
        self.scanned.bytes += text.len();
        Ok(text)
    }

    /// The end of part `part` of item `met` (a part before another, so a whole
    /// one), as its row holds it before its text, which is not read: its last
    /// [`PART_TAIL`] characters ([`TAIL_BYTES`] at most), charged, its lookup
    /// too. Where they begin, in characters of the item, and their text; none
    /// where the item has no such part.
    fn read_tail(
        &mut self,
        met: &Met,
        part: usize,
    ) -> Result<Option<(usize, String)>, LedgerError> {
        self.scanned.parts += 1;
        let Some(tail) = self.one(PART_END, params![met.key.row, number(part)], |row| {
            Ok(text_of(row.get_ref(0)?.as_bytes()?))
        })?
        else {
            return Ok(None);
        };
        self.scanned.bytes += tail.len();
        let from = (part + 1) * HISTORY_PART - tail.chars().count();
        Ok(Some((from, tail)))
    }

    /// Item `met`, read from its row ([`ROW_TEXT`]): its text whole, and charged.
    fn whole_row(&mut self, met: Met) -> Result<HistoryRow, LedgerError> {
        let text = self
            .one(ROW_TEXT, [met.key.row], |row| {
                Ok(text_of(row.get_ref(0)?.as_bytes()?))
            })?
            .ok_or(rusqlite::Error::QueryReturnedNoRows)?;
        self.scanned.bytes += text.len();
        Ok(HistoryRow {
            key: met.key,
            item: HistoryItem { text, ..met.item },
            cut: None,
        })
    }

    /// Takes `entry`, of `conversation`, if the page has room for it (its
    /// first entry always): the item's JSON and a little more for what the
    /// page says of it, and the conversation's heading where this is the first
    /// of the conversation's entries the page takes. Says whether it took it;
    /// the page is full where it did not.
    fn take(
        &mut self,
        conversation: &ConversationView,
        entry: HistoryEntry,
    ) -> Result<bool, LedgerError> {
        let new = self
            .conversations
            .last()
            .is_none_or(|last| last.conversation.id != conversation.id);
        let heading = if new {
            Some(self.heading(conversation)?)
        } else {
            None
        };
        let cost = match &entry {
            HistoryEntry::Item(row) => json_len(&row.item) + ROW_SLACK,
            HistoryEntry::Heading(_) => 0,
        } + heading
            .as_ref()
            .map_or(0, |heading| json_len(heading) + ROW_SLACK);
        if !self.entries.is_empty() && self.used + cost > self.room {
            self.full = true;
            return Ok(false);
        }
        self.used += cost;
        self.conversations.extend(heading);
        self.resume = Some(entry.key());
        self.entries.push(entry);
        Ok(true)
    }

    /// `conversation`, headed: the agent it began on, what ended it, and how
    /// many tools' outputs it holds (counted where they are left out, which is
    /// what the count says, and only so far). The log says the first two, each
    /// in a few lookups: a chief's conversation began on the agent the first
    /// switch after it began was from, or on the one the chief has now; a
    /// member's, on the member's.
    fn heading(
        &mut self,
        conversation: &ConversationView,
    ) -> Result<ParticipantConversation, LedgerError> {
        let id = conversation.id;
        let participant = self.participant;
        let first = self.activation(id, "ORDER BY id")?;
        let switched = match first.filter(|_| participant.role == "chief") {
            Some(first) => self.switch_after(first)?,
            None => None,
        };
        let agent = match switched {
            Some((_, data)) => text(&data["from"]["agent"]),
            None => participant.agent.clone(),
        };
        let ended_by = match conversation.ended_at {
            Some(_) => match self.activation(id, "ORDER BY id DESC")? {
                Some(last) => self.what_ended(id, last)?,
                None => None,
            },
            None => None,
        };
        let tools = if self.tools {
            0
        } else {
            self.one(TOOLS_OF, params![id, number(TOOLS_COUNTED)], |row| {
                row.get::<_, i64>(0)
            })?
            .unwrap_or(0)
            .unsigned_abs() as usize
        };
        Ok(ParticipantConversation {
            conversation: conversation.clone(),
            agent,
            ended_by,
            tools,
            more_tools: tools >= TOOLS_COUNTED,
        })
    }

    /// The first (or, `order` says, the last) event that began conversation
    /// `id` or took a window to it.
    fn activation(&mut self, id: i64, order: &str) -> Result<Option<i64>, LedgerError> {
        self.one(&activation_of(order, false), [id], |row| row.get(0))
    }

    /// The first Switch chief of the project after event `after`, and what it
    /// says: nothing (null) where its data is no JSON. Its data is short
    /// ([`crate::HEADING_EVENT_MAX`]), and charged as what it is.
    fn switch_after(&mut self, after: i64) -> Result<Option<(i64, Value)>, LedgerError> {
        let found = self.one(
            SWITCH_AFTER,
            params![self.participant.project_id, after],
            |row| {
                let data = text_or_none(row.get_ref("data")?).unwrap_or_default();
                Ok((
                    row.get("id")?,
                    serde_json::from_str(&data).unwrap_or(Value::Null),
                    row.get::<_, i64>("bytes")?,
                ))
            },
        )?;
        Ok(found.map(|(id, data, bytes)| {
            self.scanned.fields += bytes.unsigned_abs() as usize;
            (id, data)
        }))
    }

    /// What ended conversation `id`, which last began or was gone to at event
    /// `last`: the first after it of a `conversation.ended` of it, which says
    /// no more; a Switch chief, for the chief's; and another of the
    /// participant's conversations beginning (a window that opened fresh), or
    /// its window going to one, one it had before (a `/resume`) or one it had
    /// not (`/clear`, `/new`, or a `/resume` of a session the ledger never saw).
    fn what_ended(&mut self, id: i64, last: i64) -> Result<Option<ConversationEnd>, LedgerError> {
        let ended: Option<i64> = self.one(&ended_after(), params![id, last], |row| row.get(0))?;
        let switched = if self.participant.role == "chief" {
            self.switch_after(last)?
        } else {
            None
        };
        // Where it went: a conversation the log names by a whole number, or
        // none; its data, short, charged as what it is.
        let went: Option<(i64, String, Option<i64>)> = self
            .one(
                &went_after(),
                params![self.participant.project_id, self.participant.handle, last],
                |row| {
                    let to = match row.get_ref(2)? {
                        ValueRef::Integer(to) => Some(to),
                        _ => None,
                    };
                    Ok((row.get(0)?, row.get(1)?, to, row.get::<_, i64>(3)?))
                },
            )?
            .map(|(at, kind, to, bytes)| {
                self.scanned.fields += bytes.unsigned_abs() as usize;
                (at, kind, to)
            });
        let first = [
            ended,
            switched.as_ref().map(|(at, _)| *at),
            went.as_ref().map(|(at, ..)| *at),
        ]
        .into_iter()
        .flatten()
        .min();
        if first.is_none() || first == ended {
            return Ok(None);
        }
        if let Some((_, data)) = switched.filter(|(at, _)| Some(*at) == first) {
            let to = &data["to"];
            return Ok(Some(ConversationEnd::Switch {
                harness: text(&to["harness"]).unwrap_or_default(),
                agent: text(&to["agent"]),
            }));
        }
        let Some((at, kind, to)) = went else {
            return Ok(None);
        };
        if kind == "conversation.started" {
            return Ok(Some(ConversationEnd::Fresh));
        }
        let resumed = match to {
            Some(to) => self
                .one(&activation_of("", true), params![to, at], |row| {
                    row.get::<_, i64>(0)
                })?
                .is_some(),
            None => false,
        };
        Ok(Some(ConversationEnd::Window { resumed }))
    }
}

/// Item `met`, kept in parts, showing `text`, the part of it from character
/// `from`, of the copy of it its head says: a cut of it, but where that is the
/// whole of its text (its one part, from its start), as an item kept in parts
/// for its long id alone is.
fn long_row(met: Met, text: String, from: usize) -> HistoryRow {
    let whole = met.parts == 1 && from == 0;
    HistoryRow {
        key: met.key,
        item: HistoryItem { text, ..met.item },
        cut: (!whole).then_some(TextCut {
            from,
            bytes: met.bytes,
            revision: met.revision,
        }),
    }
}

/// The steps SQLite's machine took for `statement` since they were last counted.
fn steps(statement: &Statement<'_>) -> usize {
    usize::try_from(statement.reset_status(StatementStatus::VmStep)).unwrap_or(0)
}

/// `count` as SQLite takes a number.
fn number(count: usize) -> i64 {
    i64::try_from(count).unwrap_or(i64::MAX)
}

fn json_len(value: &impl serde::Serialize) -> usize {
    serde_json::to_vec(value).map_or(0, |json| json.len())
}

fn text(value: &Value) -> Option<String> {
    value.as_str().map(str::to_owned)
}

/// The part a search shows of a long item it found its words in at character
/// `at`: from [`BEFORE_A_MATCH`] characters before them, as much as a part
/// holds, within part `part` (`text`) and what the read has of the text before
/// it (`previous`: the character it begins at, and its text). Where it begins,
/// and its text.
fn around(
    previous: Option<&(usize, String)>,
    text: &str,
    part: usize,
    at: usize,
) -> (usize, String) {
    let (start, joined) = match previous {
        Some((start, before)) => (*start, format!("{before}{text}")),
        None => (part * HISTORY_PART, text.to_owned()),
    };
    let from = at.saturating_sub(BEFORE_A_MATCH).max(start);
    (from, chars(&joined, from - start, HISTORY_PART).to_owned())
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

/// Message `id`, as far as a delivery's line names it: the fields its row
/// holds before its body, and the first [`MESSAGE_OPENING`] bytes of the body
/// alone, read through SQLite's incremental blob reads, so that however long
/// the body is no more of it is read.
pub(crate) fn message_opening(
    store: &Store,
    id: i64,
) -> Result<Option<MessageOpening>, LedgerError> {
    let Some(mut opening) = store
        .db
        .prepare_cached(MESSAGE_FIELDS)?
        .query_row([id], |row| {
            Ok(MessageOpening {
                id,
                project_id: row.get("project_id")?,
                recipient_id: row.get("recipient_id")?,
                sender: row.get("sender")?,
                kind: row.get("kind")?,
                task_number: row.get("task_number")?,
                opening: String::new(),
            })
        })
        .optional()?
    else {
        return Ok(None);
    };
    opening.opening = opening_of(&store.db, c"message", c"body", id, MESSAGE_OPENING)?;
    Ok(Some(opening))
}

/// A text value as text, U+FFFD for what is no UTF-8 in it; none for null.
fn text_or_none(value: ValueRef<'_>) -> Option<String> {
    match value {
        ValueRef::Text(bytes) => Some(text_of(bytes)),
        _ => None,
    }
}

/// `count` characters of `text` from character `from` (fewer at its end).
fn chars(text: &str, from: usize, count: usize) -> &str {
    let mut at = text.char_indices().map(|(at, _)| at).skip(from);
    let start = at.next().unwrap_or(text.len());
    let end = at.nth(count.saturating_sub(1)).unwrap_or(text.len());
    &text[start..end.max(start)]
}

/// The last `count` characters of `text` (all of it, where it is shorter).
fn last_chars(text: &str, count: usize) -> &str {
    if count == 0 {
        return "";
    }
    text.char_indices()
        .rev()
        .nth(count - 1)
        .map_or(text, |(at, _)| &text[at..])
}

/// `text` in lower case, a character at a time, by Unicode's own mapping: what
/// a search compares, so that "É" holds "é". It is no case folding: "ß" does
/// not hold "ss", a final "ς" is not "σ", and "İ" is "i̇".
pub(crate) fn folded(text: &str) -> String {
    text.chars().flat_map(char::to_lowercase).collect()
}

/// Where `text` first holds `needle` (folded), in characters of `text`.
fn first_match(text: &str, needle: &str) -> Option<usize> {
    // Each folded character's place in the text it came from.
    let mut lowered = String::with_capacity(text.len());
    let mut came_from = Vec::with_capacity(text.len());
    for (place, character) in text.chars().enumerate() {
        for lower in character.to_lowercase() {
            for _ in 0..lower.len_utf8() {
                came_from.push(place);
            }
            lowered.push(lower);
        }
    }
    lowered.find(needle).map(|at| came_from[at])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// How SQLite goes about `sql`, a line a step.
    fn plan(db: &rusqlite::Connection, sql: &str) -> Vec<String> {
        let mut explained = db.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap();
        let count = explained.parameter_count();
        explained
            .query_map(rusqlite::params_from_iter(vec![1; count]), |row| row.get(3))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }

    /// Every query of the history, and how SQLite goes about each: by a key or
    /// an index, never a scan; the participant and a conversation's tools'
    /// outputs from an index alone (the participant's row holds its roles
    /// before its agent); an item's fields from its head, never its row (which
    /// holds its role past its id). Each other reads a row only up to a field
    /// that is short, and charged where it can be more than a few bytes.
    #[test]
    fn every_query_of_the_history_goes_by_a_key_or_an_index_and_reads_no_row_it_need_not() {
        let db = rusqlite::Connection::open_in_memory().unwrap();
        crate::schema::migrate(&db, &crate::schema::MIGRATIONS).unwrap();
        let head = |index: &str, to: &str| {
            format!("SEARCH transcript_head USING INDEX {index} (conversation_id=? AND rowid{to}?)")
        };
        let part = "SEARCH transcript_part USING INDEX sqlite_autoindex_transcript_part_1 (item=? AND part=?)";
        let activation = "SEARCH event USING INDEX event_activation_index (<expr>=?)";
        let by_row = |table: &str| format!("SEARCH {table} USING INTEGER PRIMARY KEY (rowid=?)");
        let queries: Vec<(String, Vec<String>)> = vec![
            (PROJECT_THERE.into(), vec![by_row("project")]),
            (
                WHOSE.into(),
                vec!["SEARCH participant USING COVERING INDEX participant_history_index (project_id=? AND handle=?)".into()],
            ),
            (
                conversation_from(true),
                vec!["SEARCH conversation USING INDEX conversation_participant_index (participant_id=? AND rowid<?)".into()],
            ),
            (
                conversation_from(false),
                vec!["SEARCH conversation USING INDEX conversation_participant_index (participant_id=? AND rowid>?)".into()],
            ),
            (rows_from(false, true, false), vec![head("transcript_head_said_index", "<")]),
            // SQLite says a bound the row itself is within as one it is not.
            (rows_from(false, false, true), vec![head("transcript_head_said_index", ">")]),
            (rows_from(true, true, true), vec![head("transcript_head_conversation_index", "<")]),
            (rows_from(true, false, false), vec![head("transcript_head_conversation_index", ">")]),
            (
                TOOLS_OF.into(),
                vec![
                    "CO-ROUTINE (subquery-1)".into(),
                    "SEARCH transcript_head USING COVERING INDEX transcript_head_tool_index (conversation_id=?)".into(),
                    "SCAN (subquery-1)".into(),
                ],
            ),
            (PART_SIZE.into(), vec![part.into()]),
            (PART_TEXT.into(), vec![part.into()]),
            (PART_END.into(), vec![part.into()]),
            (ROW_TEXT.into(), vec![by_row("transcript")]),
            (PART_HEAD.into(), vec![by_row("h"), by_row("c")]),
            (activation_of("ORDER BY id", false), vec![activation.into()]),
            (activation_of("ORDER BY id DESC", false), vec![activation.into()]),
            (
                activation_of("", true),
                vec!["SEARCH event USING INDEX event_activation_index (<expr>=? AND rowid<?)".into()],
            ),
            (
                SWITCH_AFTER.into(),
                vec!["SEARCH event USING INDEX event_switch_index (project_id=? AND rowid>?)".into()],
            ),
            (
                ended_after(),
                vec!["SEARCH event USING INDEX event_ended_index (<expr>=? AND rowid>?)".into()],
            ),
            (
                went_after(),
                vec!["SEARCH event USING INDEX event_participant_index (project_id=? AND <expr>=? AND rowid>?)".into()],
            ),
            (
                MESSAGE_FIELDS.into(),
                vec![by_row("m"), format!("{} LEFT-JOIN", by_row("s")), format!("{} LEFT-JOIN", by_row("t"))],
            ),
        ];
        for (sql, expected) in queries {
            assert_eq!(plan(&db, &sql), expected, "{sql}");
        }
    }
}

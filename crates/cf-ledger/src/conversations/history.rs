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
//! ([`HistoryAsk::scan`]): each conversation it comes to, each row, and each
//! byte of text is charged before it is read, and each field as what it is. A
//! tool's output is not even come to when the tools are left out; a heading is
//! a few lookups of the log, and counts the tools' outputs only so far; a long
//! item is read a part at a time, from the parts its copy keeps, never whole.
//! Every other field it reads is short whatever an older build wrote: each
//! write keeps it within its ceiling ([`crate::FIELD_MAX`],
//! [`crate::NATIVE_SESSION_MAX`], [`crate::HEADING_EVENT_MAX`]) and migration
//! 0019 cut what was past it. No value is loaded whole that may be long and is
//! not shown: a conversation's role, a message's body.
//!
//! Every read moves, however little it may go through: the first step it
//! takes (a conversation, a row, a part) it takes, so that it shows an entry or
//! passes one, and the next read goes on from there.
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

use super::{FIELD_MAX, NATIVE_SESSION_MAX};
use crate::model::LedgerError;
use crate::openings::{opening_of, text_of};
use crate::store::Store;
use crate::views::ParticipantRow;

/// The most of an item's text a page shows, and a part holds: in characters,
/// as many as a tool's output is copied of, which a frame always carries. An
/// item longer than that is kept in parts of it as well.
pub const HISTORY_PART: usize = crate::conversations::TRANSCRIPT_ITEM_MAX;

/// The most one read goes through, in bytes of text and of fields, with each
/// conversation and each row it comes to charged as some more
/// ([`CONVERSATION_COST`], [`ROW_COST`]): a read that gets there stops, and
/// says where, so the next one goes on from it. Its first step it takes
/// whatever this is.
pub const HISTORY_SCAN_BYTES: usize = 16 * 1024 * 1024;

/// The most characters a search holds. A phrase no longer than a part and one
/// character spans two parts at most, so each part searched with the end of
/// the one before finds it wherever it is; this is far below that, and a
/// search for more is refused, never searched short.
pub const HISTORY_FIND_MAX: usize = 1_000;

/// The most bytes a conversation's fields a heading shows hold: its harness and
/// times, [`FIELD_MAX`] characters each, and its native session,
/// [`NATIVE_SESSION_MAX`] characters as JavaScript counts them.
const CONVERSATION_FIELDS: usize = 3 * 4 * FIELD_MAX + 3 * NATIVE_SESSION_MAX;

/// The most bytes a row's fields hold besides its text: its item's id and its
/// time, [`FIELD_MAX`] characters each (its role is one of four words).
const ROW_FIELDS: usize = 2 * 4 * FIELD_MAX;

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

/// What the log says a conversation is, as the indexes of 0019 read it: none
/// where the event's data is no JSON. A lookup says it so, word for word, for
/// SQLite to find it in the index.
const CONVERSATION_OF: &str =
    "CASE WHEN json_valid(data) THEN json_extract(data, '$.conversation') END";

/// What the log says a participant is, as the index of 0019 reads it.
const PARTICIPANT_OF: &str =
    "CASE WHEN json_valid(data) THEN json_extract(data, '$.participant') END";

/// The parts of the item at row `?2` of conversation `?1`, by its row: its id,
/// which can be long, is never read out to look them up.
const OF_THE_ROW: &str =
    "conversation_id = ?1 AND item_id = (SELECT item_id FROM transcript WHERE rowid = ?2)";

/// The fields of the long item at row `?2` of conversation `?1` that its row
/// holds after its text, its revision too, from the index that holds them: its
/// row holds them only past the whole of its text.
const LONG_ITEM_FIELDS: &str =
    "SELECT complete, at, compaction, revision FROM transcript INDEXED BY transcript_item_index
       WHERE conversation_id = ?1 AND item_id = (SELECT item_id FROM transcript WHERE rowid = ?2)";

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
    let participant = store.participant_by_handle(project_id, handle)?;
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

/// A read as it goes: the entries taken and the headings of their
/// conversations, the room they take, what it went through, the place it began
/// at and that of the last entry it is done with, and why it stopped.
struct Reading<'a> {
    store: &'a Store,
    participant: &'a ParticipantRow,
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
/// stands, its item's id and its role, and its text's size in bytes.
struct Met {
    key: HistoryKey,
    item_id: String,
    role: String,
    bytes: usize,
}

impl Reading<'_> {
    /// What the read has been charged so far.
    fn charged(&self) -> usize {
        self.scanned.bytes
            + self.scanned.fields
            + self.scanned.rows * ROW_COST
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
        let sql = format!(
            "SELECT id, harness, native_session, started_at, ended_at,
         octet_length(harness) + ifnull(octet_length(native_session), 0)
           + octet_length(started_at) + ifnull(octet_length(ended_at), 0) AS bytes
       FROM conversation INDEXED BY conversation_participant_index
       WHERE participant_id = ?1 AND id {} ?2 ORDER BY id {} LIMIT 1",
            if self.newest_first { "<" } else { ">" },
            if self.newest_first { "DESC" } else { "ASC" },
        );
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
        // Its fields' sizes are read past its text without reading it.
        let sql = format!(
            "SELECT rowid AS row_id, item_id, role, octet_length(text) AS bytes,
         octet_length(item_id) + ifnull(octet_length(at), 0) AS field_bytes
       FROM transcript INDEXED BY {}
       WHERE conversation_id = ?1{} AND rowid {}{} ?2 ORDER BY rowid {}",
            if self.tools {
                "transcript_copied_index"
            } else {
                "transcript_said_index"
            },
            if self.tools {
                ""
            } else {
                " AND role != 'tool'"
            },
            if self.newest_first { "<" } else { ">" },
            if with_it { "=" } else { "" },
            if self.newest_first { "DESC" } else { "ASC" },
        );
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
                item_id: text_or_none(row.get_ref("item_id")?).unwrap_or_default(),
                role: row.get("role")?,
                bytes: row.get::<_, i64>("bytes")?.unsigned_abs() as usize,
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

    /// Takes the item `met`: whole, or its first part where it is long. An
    /// item the page has no room for is not read: its JSON is no shorter than
    /// its text.
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
                self.long_row(met, text, 0)?
            }
        };
        self.take(conversation, HistoryEntry::Item(row))
    }

    /// Takes the item `met` if it holds `needle` (folded), from part
    /// `from_part` of it where the read before stopped inside it; else passes
    /// it. An item no longer than a part is read whole; a long one a part at a
    /// time, each with the end of the one before, where the words may begin.
    /// Says whether the read goes on.
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
        let mut previous = None;
        if let Some(before) = from_part.checked_sub(1) {
            let bytes = self.part_bytes(&met, before)?.unwrap_or(0);
            if !self.may(bytes) {
                return Ok(false);
            }
            previous = Some(self.read_part(&met, before)?);
        }
        let mut part = from_part;
        while let Some(bytes) = self.part_bytes(&met, part)? {
            if !self.may(bytes) {
                return Ok(false);
            }
            let text = self.read_part(&met, part)?;
            let tail = previous
                .as_deref()
                .map_or("", |before| last_chars(before, reach));
            if let Some(at) = first_match(&format!("{tail}{text}"), needle) {
                let at = part * HISTORY_PART + at - tail.chars().count();
                let (from, shown) = around(previous.as_deref(), &text, part, at);
                let row = self.long_row(met, shown, from)?;
                return self.take(conversation, HistoryEntry::Item(row));
            }
            previous = Some(text);
            part += 1;
            // Searched so far: a read that stops before the next part goes on from it.
            self.resume = Some(HistoryKey { part, ..met.key });
        }
        self.resume = Some(met.key);
        Ok(true)
    }

    /// The size in bytes of part `part` of item `met`, where its copy keeps it
    /// in parts and has that one: none for an item no longer than a part.
    fn part_bytes(&mut self, met: &Met, part: usize) -> Result<Option<usize>, LedgerError> {
        if met.bytes <= HISTORY_PART {
            return Ok(None);
        }
        let found = self.one(
            &format!(
                "SELECT octet_length(text) FROM transcript_part
       WHERE {OF_THE_ROW} AND part = ?3"
            ),
            params![met.key.conversation, met.key.row, number(part)],
            |row| row.get::<_, i64>(0),
        )?;
        Ok(found.map(|bytes| bytes.unsigned_abs() as usize))
    }

    /// Part `part` of item `met`'s text, read and charged.
    fn read_part(&mut self, met: &Met, part: usize) -> Result<String, LedgerError> {
        let text = self
            .one(
                &format!(
                    "SELECT text FROM transcript_part
       WHERE {OF_THE_ROW} AND part = ?3"
                ),
                params![met.key.conversation, met.key.row, number(part)],
                |row| row.get::<_, String>(0),
            )?
            .unwrap_or_default();
        self.scanned.bytes += text.len();
        Ok(text)
    }

    /// Item `met`, no longer than a part, as its row holds it: read and charged.
    fn whole_row(&mut self, met: Met) -> Result<HistoryRow, LedgerError> {
        let item = self
            .one(
                "SELECT text, complete, at, compaction FROM transcript WHERE rowid = ?1",
                [met.key.row],
                |row| {
                    Ok(HistoryItem {
                        id: met.item_id,
                        role: met.role,
                        text: text_of(row.get_ref("text")?.as_bytes()?),
                        complete: row.get::<_, i64>("complete")? == 1,
                        at: text_or_none(row.get_ref("at")?),
                        compaction: row.get::<_, i64>("compaction")? == 1,
                    })
                },
            )?
            .ok_or(rusqlite::Error::QueryReturnedNoRows)?;
        self.scanned.bytes += item.text.len();
        Ok(HistoryRow {
            key: met.key,
            item,
            cut: None,
        })
    }

    /// Long item `met` showing `text`, the part of it from character `from`,
    /// of the copy of it its revision says: its fields from the index that
    /// holds them, never from its row, which holds them past the whole of its
    /// text.
    fn long_row(&mut self, met: Met, text: String, from: usize) -> Result<HistoryRow, LedgerError> {
        let (complete, at, compaction, revision) = self
            .one(
                LONG_ITEM_FIELDS,
                params![met.key.conversation, met.key.row],
                |row| {
                    Ok((
                        row.get::<_, i64>("complete")? == 1,
                        text_or_none(row.get_ref("at")?),
                        row.get::<_, i64>("compaction")? == 1,
                        row.get::<_, i64>("revision")?,
                    ))
                },
            )?
            .unwrap_or((true, None, false, 0));
        Ok(HistoryRow {
            key: met.key,
            item: HistoryItem {
                id: met.item_id,
                role: met.role,
                text,
                complete,
                at,
                compaction,
            },
            cut: Some(TextCut {
                from,
                bytes: met.bytes,
                revision,
            }),
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
            self.one(
                "SELECT COUNT(*) FROM (SELECT 1 FROM transcript INDEXED BY transcript_tool_index
         WHERE conversation_id = ?1 AND role = 'tool' LIMIT ?2)",
                params![id, number(TOOLS_COUNTED)],
                |row| row.get::<_, i64>(0),
            )?
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
        self.one(
            &format!(
                "SELECT id FROM event INDEXED BY event_activation_index
       WHERE kind IN ('conversation.started', 'conversation.followed')
         AND {CONVERSATION_OF} = ?1 {order} LIMIT 1"
            ),
            [id],
            |row| row.get(0),
        )
    }

    /// The first Switch chief of the project after event `after`, and what it
    /// says: nothing (null) where its data is no JSON. Its data is short
    /// ([`crate::HEADING_EVENT_MAX`]), and charged as what it is.
    fn switch_after(&mut self, after: i64) -> Result<Option<(i64, Value)>, LedgerError> {
        let found = self.one(
            "SELECT id, data, octet_length(data) AS bytes FROM event INDEXED BY event_switch_index
       WHERE kind = 'chief.switched' AND project_id = ?1 AND id > ?2 ORDER BY id LIMIT 1",
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
        let ended: Option<i64> = self.one(
            &format!(
                "SELECT id FROM event INDEXED BY event_ended_index
       WHERE kind = 'conversation.ended' AND {CONVERSATION_OF} = ?1 AND id > ?2
       ORDER BY id LIMIT 1"
            ),
            params![id, last],
            |row| row.get(0),
        )?;
        let switched = if self.participant.role == "chief" {
            self.switch_after(last)?
        } else {
            None
        };
        // Where it went: a conversation the log names by a whole number, or
        // none; its data, short, charged as what it is.
        let went: Option<(i64, String, Option<i64>)> = self
            .one(
                &format!(
                    "SELECT id, kind, {CONVERSATION_OF}, octet_length(data) FROM event
         INDEXED BY event_participant_index
       WHERE kind IN ('conversation.started', 'conversation.followed')
         AND project_id = ?1 AND {PARTICIPANT_OF} = ?2 AND id > ?3
       ORDER BY id LIMIT 1"
                ),
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
                .one(
                    &format!(
                        "SELECT id FROM event INDEXED BY event_activation_index
       WHERE kind IN ('conversation.started', 'conversation.followed')
         AND {CONVERSATION_OF} = ?1 AND id < ?2 LIMIT 1"
                    ),
                    params![to, at],
                    |row| row.get::<_, i64>(0),
                )?
                .is_some(),
            None => false,
        };
        Ok(Some(ConversationEnd::Window { resumed }))
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
/// holds, within part `part` (`text`) and the one before it where the read
/// has it. Where it begins, and its text.
fn around(previous: Option<&str>, text: &str, part: usize, at: usize) -> (usize, String) {
    let (start, joined) = match previous {
        Some(before) => ((part - 1) * HISTORY_PART, format!("{before}{text}")),
        None => (part * HISTORY_PART, text.to_owned()),
    };
    let from = at.saturating_sub(BEFORE_A_MATCH).max(start);
    (from, chars(&joined, from - start, HISTORY_PART).to_owned())
}

/// The part of the item at row `row` of conversation `conversation`, the
/// participant `handle`'s, from character `from` to the end of the part its
/// copy keeps it in (or of its text, where it is no longer than a part), where
/// the next begins, and the item's revision: what one part is, and read alone,
/// by the item's place and never by its id. An item that is not the
/// participant's is refused as one that is not there.
pub(crate) fn read_history_part(
    store: &Store,
    project_id: i64,
    handle: &str,
    conversation: i64,
    row: i64,
    from: usize,
) -> Result<TextPart, LedgerError> {
    let participant = store.participant_by_handle(project_id, handle)?;
    let bytes: i64 = store
        .db
        .query_row(
            "SELECT octet_length(t.text) FROM transcript t JOIN conversation c ON c.id = t.conversation_id
       WHERE c.participant_id = ? AND t.conversation_id = ? AND t.rowid = ?",
            params![participant.id, conversation, row],
            |row| row.get(0),
        )
        .optional()?
        .ok_or_else(|| {
            LedgerError::refused_with(
                "unknown-item",
                format!("no item at row {row} of {handle}'s conversation {conversation}"),
                404,
            )
        })?;
    let part_of = |part: usize| -> Result<Option<String>, LedgerError> {
        Ok(store
            .db
            .prepare_cached(&format!(
                "SELECT text FROM transcript_part WHERE {OF_THE_ROW} AND part = ?3"
            ))?
            .query_row(params![conversation, row, number(part)], |found| {
                found.get(0)
            })
            .optional()?)
    };
    let kept = |part: usize| -> Result<bool, LedgerError> {
        Ok(store
            .db
            .prepare_cached(&format!(
                "SELECT 1 FROM transcript_part WHERE {OF_THE_ROW} AND part = ?3"
            ))?
            .exists(params![conversation, row, number(part)])?)
    };
    let long = bytes.unsigned_abs() as usize > HISTORY_PART && kept(0)?;
    if long {
        let index = from / HISTORY_PART;
        let text = part_of(index)?.unwrap_or_default();
        let next = kept(index + 1)?;
        // Its revision from the index that holds it, never from its row, which
        // holds it past the whole of its text.
        let revision = store
            .db
            .prepare_cached(LONG_ITEM_FIELDS)?
            .query_row(params![conversation, row], |found| found.get("revision"))?;
        return Ok(TextPart {
            text: chars(&text, from % HISTORY_PART, HISTORY_PART).to_owned(),
            from,
            next: next.then_some((index + 1) * HISTORY_PART),
            revision,
        });
    }
    let (text, revision) = store.db.query_row(
        "SELECT text, revision FROM transcript WHERE rowid = ?",
        [row],
        |found| Ok((text_of(found.get_ref(0)?.as_bytes()?), found.get(1)?)),
    )?;
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
        .prepare_cached(
            "SELECT m.project_id, m.recipient_id, s.handle AS sender, m.kind, t.number AS task_number
       FROM message m LEFT JOIN participant s ON s.id = m.sender_id LEFT JOIN task t ON t.id = m.task_id
       WHERE m.id = ?",
        )?
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

    #[test]
    fn a_long_item_s_fields_and_revision_are_read_from_their_index_alone_never_past_its_text() {
        let db = rusqlite::Connection::open_in_memory().unwrap();
        crate::schema::migrate(&db, &crate::schema::MIGRATIONS).unwrap();
        let plan: Vec<String> = db
            .prepare(&format!("EXPLAIN QUERY PLAN {LONG_ITEM_FIELDS}"))
            .unwrap()
            .query_map(params![1, 1], |row| row.get(3))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert!(
            plan.iter()
                .any(|step| step.contains("USING COVERING INDEX transcript_item_index")),
            "{plan:?}"
        );
    }
}

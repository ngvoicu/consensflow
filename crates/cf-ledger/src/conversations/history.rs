//! A participant's whole history, as the history viewer reads it, a page at a
//! time: every conversation it had, the current one too, with the agent it ran
//! on and what ended it as far as the ledger's log says, and the items copied
//! of them, read in order from where the viewer is (`read_history`); and a long
//! item's text, a part at a time (`read_history_part`).
//!
//! A read goes through as little as it can: one conversation after another,
//! each in its own order (the transcript's index), from a place that never
//! moves under it ([`HistoryKey`]), until the page is full or it has gone
//! through as much text as a read may ([`HistoryAsk::scan`]); a tool's output
//! is left out by the query itself when the tools are not asked for, so its
//! text is never read; and a search is the query's too (`cf_holds`), which
//! hands over only what holds the words.

use std::collections::{HashMap, HashSet};

use cf_proto::ledger::{
    ConversationEnd, HistoryFrom, HistoryItem, HistoryKey, HistoryRead, HistoryRow,
    ParticipantConversation, Scanned, TextCut, TextPart,
};
use rusqlite::functions::FunctionFlags;
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde_json::Value;

use crate::model::LedgerError;
use crate::store::Store;
use crate::views::{conversation_view, ParticipantRow};

/// The most of an item's text a page shows, and a part holds: in characters,
/// as many as a tool's output is copied of, which a frame always carries.
pub const HISTORY_PART: usize = crate::conversations::TRANSCRIPT_ITEM_MAX;

/// The most text one read goes through, in bytes: a search over a long
/// history stops there, and says where, so the next read goes on from it.
pub const HISTORY_SCAN_BYTES: usize = 16 * 1024 * 1024;

/// How much of an item's text a search shows before the words it found.
const BEFORE_A_MATCH: usize = 2_000;

/// What a read of the history is asked for: from where and which way, the
/// items that hold `find` alone (in any case), the tools' outputs or not, how
/// many bytes of items fill a page (`room`, as their JSON), and the most text
/// it goes through (`scan`).
#[derive(Debug, Clone, Copy)]
pub struct HistoryAsk<'a> {
    pub from: HistoryFrom,
    pub find: Option<&'a str>,
    pub tools: bool,
    pub room: usize,
    pub scan: usize,
}

/// A row's place and fields, as the read's queries give them.
const ROW_FIELDS: &str = "rowid AS row_id, seq, item_id, role, complete, at, compaction";

/// One read of participant `handle`'s history, as [`HistoryAsk`] says. The
/// conversations are every one the participant had, oldest first; the rows are
/// in the order the read went (newest first, unless it reads after a place).
/// Nothing is written.
pub(crate) fn read_history(
    store: &Store,
    project_id: i64,
    handle: &str,
    ask: &HistoryAsk<'_>,
) -> Result<HistoryRead, LedgerError> {
    let participant = store.participant_by_handle(project_id, handle)?;
    let conversations = headings(store, project_id, &participant, ask.tools)?;
    let mut ids: Vec<i64> = conversations
        .iter()
        .map(|conversation| conversation.conversation.id)
        .collect();
    let (newest_first, from) = match ask.from {
        HistoryFrom::Newest => (true, None),
        HistoryFrom::Before(key) => (true, Some(key)),
        HistoryFrom::After(key) => (false, Some(key)),
    };
    if newest_first {
        ids.reverse();
    }
    // The conversations the read goes through: from the one its place is in.
    let ids = ids.into_iter().filter(|id| match from {
        None => true,
        Some(key) if newest_first => *id <= key.conversation,
        Some(key) => *id >= key.conversation,
    });
    let needle = ask.find.filter(|find| !find.is_empty()).map(folded);
    let mut read = Reading {
        rows: Vec::new(),
        used: 0,
        scanned: Scanned::default(),
        resume: None,
        full: false,
        bounded: false,
    };
    for id in ids {
        // Within the place's own conversation, from the place; in any other, all of it.
        let bound = match from {
            Some(key) if key.conversation == id => (key.seq, key.row),
            _ if newest_first => (i64::MAX, i64::MAX),
            _ => (i64::MIN, i64::MIN),
        };
        match &needle {
            None => read.whole(store, id, bound, newest_first, ask)?,
            Some(needle) => read.search(store, id, bound, newest_first, needle, ask)?,
        }
        if read.full || read.bounded {
            break;
        }
    }
    let more = read.full || read.bounded;
    Ok(HistoryRead {
        participant: participant.id,
        conversations,
        rows: read.rows,
        resume: if more { read.resume } else { None },
        bounded: read.bounded,
        scanned: read.scanned,
    })
}

/// A read as it goes: the rows taken, the room they take, what it went
/// through, the place of the last row it is done with, and why it stopped.
struct Reading {
    rows: Vec<HistoryRow>,
    used: usize,
    scanned: Scanned,
    resume: Option<HistoryKey>,
    full: bool,
    bounded: bool,
}

impl Reading {
    /// The items of conversation `id` past `bound`, read whole until the page
    /// is full or the read has gone through as much text as it may.
    fn whole(
        &mut self,
        store: &Store,
        id: i64,
        bound: (i64, i64),
        newest_first: bool,
        ask: &HistoryAsk<'_>,
    ) -> Result<(), LedgerError> {
        let sql = format!(
            "SELECT {ROW_FIELDS}, text FROM transcript
       WHERE conversation_id = ?1 AND (seq, rowid) {} (?2, ?3){}
       ORDER BY seq {order}, rowid {order}",
            if newest_first { "<" } else { ">" },
            if ask.tools { "" } else { " AND role != 'tool'" },
            order = if newest_first { "DESC" } else { "ASC" },
        );
        let mut statement = store.db.prepare_cached(&sql)?;
        let mut rows = statement.query(params![id, bound.0, bound.1])?;
        while let Some(row) = rows.next()? {
            let text: String = row.get("text")?;
            self.scanned.rows += 1;
            self.scanned.bytes += text.len();
            let shown = shown_row(row, id, text, None)?;
            if !self.take(shown, ask.room) {
                return Ok(());
            }
            if self.scanned.bytes >= ask.scan {
                self.bounded = true;
                return Ok(());
            }
        }
        Ok(())
    }

    /// The items of conversation `id` past `bound` that hold `needle`: the
    /// query tells which, and only those are read whole.
    fn search(
        &mut self,
        store: &Store,
        id: i64,
        bound: (i64, i64),
        newest_first: bool,
        needle: &str,
        ask: &HistoryAsk<'_>,
    ) -> Result<(), LedgerError> {
        let sql = format!(
            "SELECT rowid AS row_id, seq, octet_length(text) AS bytes, cf_holds(text, ?4) AS hit
       FROM transcript
       WHERE conversation_id = ?1 AND (seq, rowid) {} (?2, ?3){}
       ORDER BY seq {order}, rowid {order}",
            if newest_first { "<" } else { ">" },
            if ask.tools { "" } else { " AND role != 'tool'" },
            order = if newest_first { "DESC" } else { "ASC" },
        );
        let mut statement = store.db.prepare_cached(&sql)?;
        let mut rows = statement.query(params![id, bound.0, bound.1, needle])?;
        while let Some(row) = rows.next()? {
            self.scanned.rows += 1;
            self.scanned.bytes += row.get::<_, i64>("bytes")?.unsigned_abs() as usize;
            let key = HistoryKey {
                conversation: id,
                seq: row.get("seq")?,
                row: row.get("row_id")?,
            };
            if row.get::<_, bool>("hit")? {
                let found = store
                    .db
                    .prepare_cached(&format!(
                        "SELECT {ROW_FIELDS}, text FROM transcript WHERE rowid = ?"
                    ))?
                    .query_row([key.row], |found| {
                        let text: String = found.get("text")?;
                        Ok(shown_row(found, id, text, Some(needle)))
                    })??;
                if !self.take(found, ask.room) {
                    return Ok(());
                }
            } else {
                self.resume = Some(key);
            }
            if self.scanned.bytes >= ask.scan {
                self.bounded = true;
                return Ok(());
            }
        }
        Ok(())
    }

    /// Takes `row` if the page has room for it (its first row always): what
    /// it takes is its item's JSON and a little more for what the page says of
    /// it. Says whether it took it; the page is full when it did not.
    fn take(&mut self, row: HistoryRow, room: usize) -> bool {
        let cost = serde_json::to_vec(&row.item).map_or(0, |json| json.len()) + ROW_SLACK;
        if !self.rows.is_empty() && self.used + cost > room {
            self.full = true;
            return false;
        }
        self.used += cost;
        self.resume = Some(row.key);
        self.rows.push(row);
        true
    }
}

/// What the page says of an item besides its fields: a delivery's line, the
/// part it shows, a heading for its conversation.
const ROW_SLACK: usize = 600;

/// A row as a read gives it: its text whole, or the part a page shows, from
/// the start or (for a search) around the first place it holds `needle`.
fn shown_row(
    row: &Row<'_>,
    conversation: i64,
    text: String,
    needle: Option<&str>,
) -> Result<HistoryRow, LedgerError> {
    let key = HistoryKey {
        conversation,
        seq: row.get("seq")?,
        row: row.get("row_id")?,
    };
    let bytes = text.len();
    let length = text.chars().count();
    let (text, cut) = if length <= HISTORY_PART {
        (text, None)
    } else {
        let at = needle
            .and_then(|needle| first_match(&text, needle))
            .unwrap_or(0);
        let from = at.saturating_sub(BEFORE_A_MATCH).min(length - HISTORY_PART);
        (
            chars(&text, from, HISTORY_PART).to_owned(),
            Some(TextCut { from, bytes }),
        )
    };
    Ok(HistoryRow {
        key,
        item: HistoryItem {
            id: row.get("item_id")?,
            role: row.get("role")?,
            text,
            complete: row.get::<_, i64>("complete")? == 1,
            at: row.get("at")?,
            compaction: row.get::<_, i64>("compaction")? == 1,
        },
        cut,
    })
}

/// The part of item `item` of conversation `conversation`, the participant
/// `handle`'s, from character `from`: [`HISTORY_PART`] characters at most,
/// and where the next begins. An item that is not the participant's is
/// refused as one that is not there.
pub(crate) fn read_history_part(
    store: &Store,
    project_id: i64,
    handle: &str,
    conversation: i64,
    item: &str,
    from: usize,
) -> Result<TextPart, LedgerError> {
    let participant = store.participant_by_handle(project_id, handle)?;
    let text: String = store
        .db
        .query_row(
            "SELECT t.text FROM transcript t JOIN conversation c ON c.id = t.conversation_id
       WHERE c.participant_id = ? AND t.conversation_id = ? AND t.item_id = ?",
            params![participant.id, conversation, item],
            |row| row.get(0),
        )
        .optional()?
        .ok_or_else(|| {
            LedgerError::refused_with(
                "unknown-item",
                format!("no item {item} in {handle}'s conversation {conversation}"),
                404,
            )
        })?;
    let part = chars(&text, from, HISTORY_PART);
    let next = from + part.chars().count();
    Ok(TextPart {
        text: part.to_owned(),
        from,
        next: (next < text.chars().count()).then_some(next),
    })
}

/// `count` characters of `text` from character `from` (fewer at its end).
fn chars(text: &str, from: usize, count: usize) -> &str {
    let mut at = text.char_indices().map(|(at, _)| at).skip(from);
    let start = at.next().unwrap_or(text.len());
    let end = at.nth(count.saturating_sub(1)).unwrap_or(text.len());
    &text[start..end.max(start)]
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

/// Registers `cf_holds(text, needle)` on `db`: whether `text`, folded, holds
/// `needle`, which is folded already. What a search of the history runs in its
/// query, Unicode's lower case where SQLite's own `lower` and `LIKE` fold ASCII
/// alone.
pub(crate) fn register_search(db: &Connection) -> rusqlite::Result<()> {
    db.create_scalar_function(
        "cf_holds",
        2,
        FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
        |context| {
            let text = context.get_raw(0).as_str().unwrap_or_default();
            let needle = context.get_raw(1).as_str().unwrap_or_default();
            Ok(folded(text).contains(needle))
        },
    )
}

/// Every conversation of `participant`, oldest first (in the order each began),
/// headed: the agent it ran on, what ended it, and how many tools' outputs it
/// holds (counted only where they are left out, which is what the count says).
fn headings(
    store: &Store,
    project_id: i64,
    participant: &ParticipantRow,
    tools: bool,
) -> Result<Vec<ParticipantConversation>, LedgerError> {
    let conversations = store
        .db
        .prepare("SELECT * FROM conversation WHERE participant_id = ? ORDER BY id")?
        .query_map([participant.id], conversation_view)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let own: HashSet<i64> = conversations
        .iter()
        .map(|conversation| conversation.id)
        .collect();
    let told = told(
        store,
        project_id,
        &own,
        participant.role == "chief",
        participant.agent.as_deref(),
    )?;
    let mut counted = store.db.prepare_cached(
        "SELECT COUNT(*) FROM transcript WHERE conversation_id = ? AND role = 'tool'",
    )?;
    conversations
        .into_iter()
        .map(|conversation| {
            let id = conversation.id;
            let tools = if tools {
                0
            } else {
                counted
                    .query_row([id], |row| row.get::<_, i64>(0))?
                    .unsigned_abs() as usize
            };
            Ok(ParticipantConversation {
                agent: told
                    .agents
                    .get(&id)
                    .cloned()
                    .unwrap_or_else(|| participant.agent.clone()),
                ended_by: conversation
                    .ended_at
                    .as_ref()
                    .and_then(|_| told.ends.get(&id).cloned()),
                tools,
                conversation,
            })
        })
        .collect()
}

/// What the log says of a participant's conversations, by id: the agent each
/// began on, and what ended each the last time it ended.
struct Told {
    agents: HashMap<i64, Option<String>>,
    ends: HashMap<i64, ConversationEnd>,
}

/// Reads the project's log in order for the conversations `own`. One begins
/// (`conversation.started`) or the window goes to one (`conversation.followed`),
/// and the participant's conversation before it ends: in a window that opened
/// on a fresh one, or in the window that went to another, which is one of its
/// own it had before (a `/resume`) or one it had not (`/clear`, `/new`, or a
/// `/resume` of a session the ledger never saw). A `chief.switched` ends the
/// chief's: each switch says the agent the chief ran on until it, so a
/// conversation of the `chief`'s began on the agent the next switch after it
/// was from, or on the agent it has `now`. A `conversation.ended` ends one and
/// says no more.
fn told(
    store: &Store,
    project_id: i64,
    own: &HashSet<i64>,
    chief: bool,
    now: Option<&str>,
) -> Result<Told, LedgerError> {
    let events = store
        .db
        .prepare(
            "SELECT kind, data FROM event WHERE project_id = ? AND kind IN
         ('conversation.started', 'conversation.followed', 'conversation.ended', 'chief.switched')
       ORDER BY id",
        )?
        .query_map([project_id], |row| {
            Ok((row.get::<_, String>("kind")?, row.get::<_, String>("data")?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let events: Vec<(String, Value)> = events
        .into_iter()
        .filter(|(kind, _)| chief || kind != "chief.switched")
        .map(|(kind, data)| (kind, serde_json::from_str(&data).unwrap_or(Value::Null)))
        .collect();
    let text = |value: &Value| value.as_str().map(str::to_owned);
    let switched_from: Vec<Option<String>> = events
        .iter()
        .filter(|(kind, _)| kind == "chief.switched")
        .map(|(_, data)| text(&data["from"]["agent"]))
        .collect();
    let mut told = Told {
        agents: HashMap::new(),
        ends: HashMap::new(),
    };
    let mut switches = 0;
    let mut current: Option<i64> = None;
    for (kind, data) in &events {
        if kind == "chief.switched" {
            if let Some(ended) = current.take() {
                let to = &data["to"];
                let end = ConversationEnd::Switch {
                    harness: text(&to["harness"]).unwrap_or_default(),
                    agent: text(&to["agent"]),
                };
                told.ends.insert(ended, end);
            }
            switches += 1;
            continue;
        }
        let Some(id) = data["conversation"].as_i64().filter(|id| own.contains(id)) else {
            continue;
        };
        if kind == "conversation.ended" {
            if current == Some(id) {
                current = None;
            }
            told.ends.remove(&id);
            continue;
        }
        let seen = told.agents.contains_key(&id);
        if let Some(before) = current.filter(|before| *before != id) {
            let end = if kind == "conversation.started" {
                ConversationEnd::Fresh
            } else {
                ConversationEnd::Window { resumed: seen }
            };
            told.ends.insert(before, end);
        }
        if !seen {
            let agent = match switched_from.get(switches) {
                Some(from) => from.clone(),
                None => now.map(str::to_owned),
            };
            told.agents.insert(id, agent);
        }
        current = Some(id);
    }
    Ok(told)
}

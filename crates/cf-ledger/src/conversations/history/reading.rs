//! One read of the history as it goes: what it is charged, the conversations,
//! items and parts it goes through, and the page it fills.

use cf_proto::ledger::{
    HistoryFrom, HistoryItem, HistoryKey, HistoryRead, HistoryRow, ItemDetail, Scanned, TextCut,
};
use rusqlite::{params, OptionalExtension, Params, Row, Statement, StatementStatus};

use super::queries::{
    conversation_from, rows_from, DIFF_TEXT, PART_END, PART_SIZE, PART_TEXT, ROW_TEXT,
};
use super::text::{chars, first_match, folded, last_chars, text_or_none};
use super::{HistoryAsk, Whose, HISTORY_PART};
use crate::conversations::transcripts::{number, HEADER_MAX, NAME_MAX, PART_TAIL};
use crate::conversations::ID_SHOWN;
use crate::model::LedgerError;
use crate::store::Store;
use crate::text::text_of;

/// The most bytes a row's fields hold besides its text: its id as a page shows
/// it ([`ID_SHOWN`] characters and "…") and its time, as a store keeps one (its
/// role is one of four words), and what the history draws of it ([`DRAWN_FIELDS`]).
const ROW_FIELDS: usize = 4 * ID_SHOWN + 3 + 64 + DRAWN_FIELDS;

/// The most bytes what the history draws of an item holds in its head (migration 0032): a
/// call's header ([`HEADER_MAX`] characters) and its tool's name ([`NAME_MAX`]), its kind
/// and its status (a word each), the time it took and a compaction's tokens (a number
/// each). The change an edit made is read apart, and charged as a part is.
const DRAWN_FIELDS: usize = 4 * HEADER_MAX + 4 * NAME_MAX + 2 * 16 + 2 * 20;

/// What a read is charged for each conversation it comes to, besides its rows:
/// finding it by its index, and going to its items by theirs. One that shows
/// no item is charged this alone, so a read goes through as many of them as
/// its bound holds and says where the next goes on.
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

/// How much of an item's text a search shows before the words it found.
const BEFORE_A_MATCH: usize = 2_000;

/// What the page says of an entry besides its fields: where a delivery
/// begins, the part it shows, the place it stands.
const ROW_SLACK: usize = 600;

/// A read as it goes: the items taken, the room they take, what it went
/// through, the place it began at and that of the last item it is done with,
/// and why it stopped.
pub(super) struct Reading<'a> {
    store: &'a Store,
    participant: &'a Whose,
    newest_first: bool,
    needle: Option<String>,
    tools: bool,
    room: usize,
    scan: usize,
    entries: Vec<HistoryRow>,
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

impl<'a> Reading<'a> {
    /// A read of `participant`'s history as `ask` says, before its first step: `ask.find`
    /// is what it searches for (folded), none where it is empty.
    pub(super) fn new(store: &'a Store, participant: &'a Whose, ask: &HistoryAsk<'_>) -> Self {
        let (newest_first, from) = match ask.from {
            HistoryFrom::Newest => (true, None),
            HistoryFrom::Before(key) => (true, Some(key)),
            HistoryFrom::After(key) => (false, Some(key)),
        };
        Self {
            store,
            participant,
            newest_first,
            needle: ask.find.filter(|find| !find.is_empty()).map(folded),
            tools: ask.tools,
            room: ask.room,
            scan: ask.scan,
            entries: Vec::new(),
            used: 0,
            scanned: Scanned::default(),
            from,
            resume: from,
            full: false,
            bounded: false,
        }
    }

    /// Goes through the participant's conversations, one after another from the place
    /// the read began at, until the page is full, the read is bounded, or there are no
    /// more.
    pub(super) fn run(&mut self) -> Result<(), LedgerError> {
        let from = self.from;
        // From the place's own conversation, or from the newest.
        let mut past = match from {
            None => i64::MAX,
            Some(key) if self.newest_first => key.conversation.saturating_add(1),
            Some(key) => key.conversation.saturating_sub(1),
        };
        while self.may(CONVERSATION_COST) {
            let Some(conversation) = self.next_conversation(past)? else {
                break;
            };
            past = conversation;
            let within = from.filter(|key| key.conversation == conversation);
            self.conversation(conversation, within)?;
            if self.full || self.bounded {
                break;
            }
        }
        Ok(())
    }

    /// What the read came to, once it has gone: the page of items, the place a next read
    /// goes on from (none where it went through all there is), and what it was charged.
    pub(super) fn into_read(self) -> HistoryRead {
        let more = self.full || self.bounded;
        HistoryRead {
            participant: self.participant.id,
            charged: self.charged(),
            entries: self.entries,
            resume: if more { self.resume } else { None },
            bounded: self.bounded,
            scanned: self.scanned,
        }
    }

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
    /// an item or passed one it always may, so that the first step it takes
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

    /// The id of the participant's conversation next to `past` the way the
    /// read goes, and nothing else of it; none where there is no more.
    fn next_conversation(&mut self, past: i64) -> Result<Option<i64>, LedgerError> {
        let sql = conversation_from(self.newest_first);
        let found = self.one(&sql, params![self.participant.id, past], |row| row.get(0))?;
        if found.is_some() {
            self.scanned.conversations += 1;
        }
        Ok(found)
    }

    /// Goes through conversation `id` from `within`, the place the read began
    /// at where that is in it (the item itself too, where a search stopped
    /// inside it): its items the way the read goes, each taken or, in a
    /// search, passed. One that shows none is passed: a read that stops there
    /// goes on past it.
    fn conversation(&mut self, id: i64, within: Option<HistoryKey>) -> Result<(), LedgerError> {
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
        while self.may(ROW_COST + ROW_FIELDS) {
            let Some(row) = rows.next()? else {
                break;
            };
            self.scanned.rows += 1;
            self.scanned.fields += row.get::<_, i64>("field_bytes")?.unsigned_abs() as usize;
            let diff_bytes = row.get::<_, i64>("diff_bytes")?.unsigned_abs() as usize;
            let mut met = Met {
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
                    detail: drawn_of(row)?,
                },
                revision: row.get("revision")?,
                bytes: row.get::<_, i64>("bytes")?.unsigned_abs() as usize,
                parts: row.get::<_, i64>("parts")?.unsigned_abs() as usize,
            };
            if diff_bytes > 0 {
                // The change an edit made, from its own row: charged as a part is.
                if !self.may(PART_COST + diff_bytes) {
                    break;
                }
                self.scanned.parts += 1;
                let diff = self.one(DIFF_TEXT, [met.key.row], |row| {
                    Ok(text_of(row.get_ref(0)?.as_bytes()?))
                })?;
                self.scanned.bytes += diff.as_ref().map_or(0, String::len);
                if let Some(detail) = &mut met.item.detail {
                    detail.diff = diff;
                }
            }
            let from_part = within
                .filter(|place| place.row == met.key.row)
                .map_or(0, |place| place.part);
            let going = match &needle {
                None => self.show(met)?,
                Some(needle) => self.search(met, from_part, needle)?,
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
        // Through the whole conversation: the next read goes on past it.
        let edge = if self.newest_first { 0 } else { i64::MAX };
        self.resume = Some(HistoryKey {
            conversation: id,
            row: edge,
            part: 0,
        });
        Ok(())
    }

    /// Takes the item `met`: whole, or its first part where it is kept in
    /// parts. An item the page has no room for is not read: its JSON is no
    /// shorter than its text.
    fn show(&mut self, met: Met) -> Result<bool, LedgerError> {
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
        self.take(row)
    }

    /// Takes the item `met` if it holds `needle` (folded), from part
    /// `from_part` of it where the read before stopped inside it; else passes
    /// it. An item read from its row is read whole; one kept in parts a part at
    /// a time, each with the end of the one before, where the words may begin
    /// (the end of the part before the first it reads, where it goes on inside
    /// the item, read by itself: [`TAIL_BYTES`]). Says whether the read goes on.
    fn search(&mut self, met: Met, from_part: usize, needle: &str) -> Result<bool, LedgerError> {
        // A call's header, read with its head, holds the words: it is shown as a read
        // with no search shows it (chief-harness CHF-20), its text not searched.
        let header = met
            .item
            .detail
            .as_ref()
            .and_then(|drawn| drawn.header.as_deref());
        if from_part == 0 && header.is_some_and(|header| folded(header).contains(needle)) {
            return self.show(met);
        }
        if self.part_bytes(&met, 0)?.is_none() {
            if !self.may(met.bytes) {
                return Ok(false);
            }
            let key = met.key;
            let row = self.whole_row(met)?;
            if folded(&row.item.text).contains(needle) {
                return self.take(row);
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
                return self.take(long_row(met, shown, from));
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

    /// Takes item `row` if the page has room for it (its first item always):
    /// its JSON and a little more for what the page says of it. Says whether
    /// it took it; the page is full where it did not.
    fn take(&mut self, row: HistoryRow) -> Result<bool, LedgerError> {
        let cost = json_len(&row.item) + ROW_SLACK;
        if !self.entries.is_empty() && self.used + cost > self.room {
            self.full = true;
            return Ok(false);
        }
        self.used += cost;
        self.resume = Some(row.key);
        self.entries.push(row);
        Ok(true)
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

/// What the history draws of the item a head `row` says besides its words: none where it
/// says no kind (an item copied before migration 0032, or of which its reader said nothing
/// more than its role does). The change an edit made is read apart.
fn drawn_of(row: &Row<'_>) -> rusqlite::Result<Option<ItemDetail>> {
    let Some(kind) = text_or_none(row.get_ref("kind")?) else {
        return Ok(None);
    };
    let count = |name: &str| -> rusqlite::Result<Option<u64>> {
        Ok(row.get::<_, Option<i64>>(name)?.map(i64::unsigned_abs))
    };
    Ok(Some(ItemDetail {
        kind,
        name: text_or_none(row.get_ref("name")?),
        header: text_or_none(row.get_ref("header")?),
        status: text_or_none(row.get_ref("status")?),
        took_ms: count("took")?,
        tokens: count("tokens")?,
        diff: None,
    }))
}

/// The steps SQLite's machine took for `statement` since they were last counted.
fn steps(statement: &Statement<'_>) -> usize {
    usize::try_from(statement.reset_status(StatementStatus::VmStep)).unwrap_or(0)
}

fn json_len(value: &impl serde::Serialize) -> usize {
    serde_json::to_vec(value).map_or(0, |json| json.len())
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

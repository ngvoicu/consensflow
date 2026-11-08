//! The human's view of a participant's whole history, from its window's
//! History button: one timeline of the items copied of every conversation it
//! had, oldest to newest and the current one too, a page at a time. Nothing
//! heads a conversation (chief-harness, CHF-11): no harness, model, agent,
//! time or end of one is said, and an item says only the conversation and the
//! row it stands at, the place a page goes on from and its long text is read
//! by. A conversation that shows no item is not there.
//!
//! A page is one read of the ledger's ([`HistoryRead`]), as much as one frame
//! of the app's bridge carries ([`PAGE_BYTES`]). It goes on from a place, the
//! oldest item shown or the newest, that does not move under it, so a page
//! never repeats an item another showed nor skips one, whatever is added,
//! grows or is copied again meanwhile. A delivery ConsensFlow made to this
//! participant is marked with where it begins in the item's text, after what
//! the human typed with it (the viewer draws it whole, in one block); only the
//! message it names being this participant's makes it one, and nothing of the
//! message is read for that but its parties. A long item shows a part, and is
//! read whole a part at a time, by its place.
//!
//! Only the one item that may still be written says so: a reader marks an assistant's
//! message complete when it is the turn's last answer, and no other (every message before
//! it, Claude Code's of a call alone and Pi's before a step, stays unfinished for good), so
//! an item behind a later one, or in a conversation that ended, is shown complete however
//! its reader left it, and an assistant's message that ended with no words is not shown.
//! Nor is any item being written while its participant's window is not in a turn (at rest,
//! paused, closed): the caller then gives no item that may be.
//!
//! Every item fits a page, the first too: it shows a part of its text at
//! most, and every other text a page shows of it is [`SHOWN_FIELD`] characters
//! at most.

use cf_base::refusal::Refusal;
use cf_base::text::utf16_len;
use cf_ledger::{
    HistoryFrom, HistoryKey, HistoryRead, HistoryRow, ItemDetail, TextCut, PAGE_BYTES,
};
use serde::Serialize;

use super::entries::message_id;
use super::text::{delivered, typed_before};

/// How many bytes of items a page holds: the frame, less what the page says
/// besides them.
pub const HISTORY_ROOM: usize = PAGE_BYTES - 1_024;

/// The most characters a page shows of a text that is no item's text: an
/// item's id and its time. The ledger gives none longer (an id as a page shows
/// it is [`cf_ledger::ID_SHOWN`] characters and "…"); a longer one shows that
/// many and "…".
pub const SHOWN_FIELD: usize = 256;

/// Whether message `id` is one of this project's and this participant's, sent
/// to it or by it. A header may name any number, and only the participant's own
/// messages make a delivery, so the lookup says no for another's and for one
/// not on record.
pub type Owns<'a> = &'a dyn Fn(i64) -> Result<bool, Refusal>;

/// Where the one item that may still be written stands (the ledger's `live_item`, asked
/// while the participant's window is in a turn): none where it is not, or has no item in
/// its conversation now.
pub type Live = Option<HistoryKey>;

/// One page of a participant's history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WholeHistoryPage {
    /// The items, oldest first, across conversations.
    pub items: Vec<ShownItem>,
    /// Where the page before this one goes on from: the oldest item shown,
    /// or where a search stopped; none where there is nothing older.
    pub older: Option<HistoryKey>,
    /// Where the page after this one goes on from; none on the most recent.
    pub newer: Option<HistoryKey>,
    /// The read stopped at the most one goes through, before the page was full:
    /// a search, or a read past conversations that show no item.
    pub partial: bool,
}

/// An item as a page shows it: its place, the conversation and the row it
/// stands at (which `history.part` reads it by, and a page goes on from), and
/// the rest of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ShownItem {
    pub conversation: i64,
    pub row: i64,
    pub id: String,
    pub role: String,
    /// Whole, or the part `cut` says.
    pub text: String,
    /// Whether the item is written: the page says it is unfinished only of the one item
    /// that may be ([`whole_history_page`]), whatever its reader said.
    pub complete: bool,
    pub at: Option<String>,
    /// The summary a compaction wrote in the user's place.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub compaction: bool,
    /// A delivery ConsensFlow made to this participant: where it begins.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delivery: Option<Delivery>,
    /// The text shown is a part of a longer one, which the page reads whole.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cut: Option<TextCut>,
    /// What the history draws of it besides its words (chief-harness CHF-16), where its
    /// reader said it: none for an item copied before, which draws as a plain one.
    #[serde(flatten)]
    pub detail: Option<ItemDetail>,
}

/// Where in the item's text a delivery begins (in UTF-16 code units): what
/// comes before it is what the human typed with it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Delivery {
    pub at: usize,
}

/// The page of `read`, the ledger's read from `from`. Its items are taken in
/// the order the read went while the page has room for them, the first always.
/// `owns` says whether a message a delivery names is one this participant was
/// sent or sent: a header that names any other is the item's text, as it is.
/// The lookup is by the message's row alone, one for each row the read has
/// already charged; the page charges the read nothing for it.
///
/// `live` is where the one item that may still be written stands. Of the rest the page
/// says they are complete, and an assistant's message with no words is gone through and
/// not shown: it was a call's alone, or begun and never written, and the turn went on.
pub fn whole_history_page(
    read: &HistoryRead,
    from: HistoryFrom,
    owns: Owns<'_>,
    live: Live,
) -> Result<WholeHistoryPage, Refusal> {
    let mut items: Vec<ShownItem> = Vec::new();
    let mut used = 0;
    // How many of the read's entries the page went through: those it shows, and those it
    // passed over, so that the places it goes on from are past both.
    let mut gone_through = 0;
    for row in &read.entries {
        let written = live == Some(HistoryKey { part: 0, ..row.key }) && !row.item.complete;
        if !written && said_nothing(row) {
            gone_through += 1;
            continue;
        }
        let mut item = shown(row, owns)?;
        item.complete |= !written;
        let cost = bytes(&item) + 1;
        if !items.is_empty() && used + cost > HISTORY_ROOM {
            break;
        }
        used += cost;
        items.push(item);
        gone_through += 1;
    }
    let taken = &read.entries[..gone_through];
    let trimmed = taken.len() < read.entries.len();
    // Where the next page each way goes on from: past the last item taken where
    // the page was full, else where the read stopped; and back from the first.
    let last_taken = taken.last().map(|row| row.key);
    let first_taken = taken.first().map(|row| row.key);
    let onward = if trimmed { last_taken } else { read.resume };
    let (older, newer) = match from {
        HistoryFrom::Newest => (onward, None),
        HistoryFrom::Before(place) => (onward, first_taken.or(Some(place))),
        HistoryFrom::After(place) => (first_taken.or(Some(place)), onward),
    };
    if !matches!(from, HistoryFrom::After(_)) {
        items.reverse();
    }
    Ok(WholeHistoryPage {
        items,
        older,
        newer,
        partial: read.bounded && !trimmed,
    })
}

/// Where item `row` holds a delivery the page marks, where the
/// delivery begins in the text (after what the human typed with it, none of it
/// where only Claude's own placeholders and wrapper opening precede the header:
/// `typed_before`) and the id the header names, as the ledger holds one (none
/// where no message could be it): what is neither a tool's output, an agent's
/// words nor a compaction's summary, shown from its start, and holds a
/// delivery's header.
fn folded(row: &HistoryRow) -> Option<(usize, Option<i64>)> {
    let item = &row.item;
    let said = item.role == "tool" || item.role == "assistant" || item.compaction;
    let from_the_start = row.cut.is_none_or(|cut| cut.from == 0);
    delivered(&item.text)
        .filter(|_| !said && from_the_start)
        .map(|header| {
            let at = if typed_before(&item.text, &header).is_empty() {
                0
            } else {
                header.at
            };
            (at, message_id(header.id).1)
        })
}

/// An item as a page shows it: a delivery this participant was sent or sent,
/// marked with where it begins.
fn shown(row: &HistoryRow, owns: Owns<'_>) -> Result<ShownItem, Refusal> {
    let item = &row.item;
    let delivery = match folded(row) {
        Some((at, Some(ledger_id))) if owns(ledger_id)? => Some(Delivery {
            at: utf16_len(&item.text[..at]),
        }),
        _ => None,
    };
    Ok(ShownItem {
        conversation: row.key.conversation,
        row: row.key.row,
        id: field(&item.id),
        role: item.role.clone(),
        text: item.text.clone(),
        complete: item.complete,
        at: item.at.as_deref().map(field),
        compaction: item.compaction,
        delivery,
        cut: row.cut,
        detail: item.detail.clone(),
    })
}

/// Whether `row` is an assistant's message with no words to show: not a part of a longer
/// text, whose first part may be blank and whose words come after it.
fn said_nothing(row: &HistoryRow) -> bool {
    row.item.role == "assistant" && row.item.text.trim().is_empty() && row.cut.is_none()
}

/// `text` as a page shows a text that is no item's own: [`SHOWN_FIELD`]
/// characters at most, and "…" past them (so an id the ledger cut so shows
/// as it gave it).
fn field(text: &str) -> String {
    match text.char_indices().nth(SHOWN_FIELD) {
        Some((end, _)) => format!("{}…", &text[..end]),
        None => text.to_owned(),
    }
}

/// The size of `value` as the page receives it.
fn bytes(value: &impl Serialize) -> usize {
    serde_json::to_vec(value).map_or(0, |json| json.len())
}

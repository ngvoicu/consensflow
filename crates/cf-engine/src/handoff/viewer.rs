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
//! grows or is copied again meanwhile. A delivery ConsensFlow made to this participant says its outcome in
//! one line, as `cf history` says it, from the opening of the message it names,
//! each opening charged against what the read may still go through; its whole
//! text is there too, for the page to open on a click. A long item shows a
//! part, and is read whole a part at a time, by its place.
//!
//! Every item fits a page, the first too: it shows a part of its text at
//! most, and every other text a page shows of it is [`SHOWN_FIELD`] characters
//! at most.

use cf_base::refusal::Refusal;
use cf_base::text::utf16_len;
use cf_ledger::{
    HistoryFrom, HistoryKey, HistoryRead, HistoryRow, MessageOpening, TextCut, OPENING_COST,
    PAGE_BYTES,
};
use serde::Serialize;

use super::entries::{message_id, outcome_line};
use super::text::{delivered, typed_before};
use super::Gist;

/// How many bytes of items a page holds: the frame, less what the page says
/// besides them.
pub const HISTORY_ROOM: usize = PAGE_BYTES - 1_024;

/// The most characters a page shows of a text that is no item's text: an
/// item's id and its time. The ledger gives none longer (an id as a page shows
/// it is [`cf_ledger::ID_SHOWN`] characters and "…"); a longer one shows that
/// many and "…".
pub const SHOWN_FIELD: usize = 256;

/// A message's opening by its id, in this project and this participant's
/// only: what a delivery's line is said from. A line may name any number, and
/// only the participant's own messages are read out, so the lookup says none
/// for another's.
pub type Openings<'a> = &'a dyn Fn(i64) -> Result<Option<MessageOpening>, Refusal>;

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
    /// A search stopped at the most a read goes through, before the page was full.
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
    pub complete: bool,
    pub at: Option<String>,
    /// The summary a compaction wrote in the user's place.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub compaction: bool,
    /// A delivery ConsensFlow made to this participant, in one line.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delivery: Option<Delivery>,
    /// The text shown is a part of a longer one, which the page reads whole.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cut: Option<TextCut>,
}

/// A delivery's line, and where in the item's text the delivery begins (in
/// UTF-16 code units): what comes before it is what the human typed with it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Delivery {
    pub line: String,
    pub at: usize,
}

/// The page of `read`, the ledger's read from `from`. Its items are taken in
/// the order the read went while the page has room for them, the first always.
/// `opening` gives the opening of a message a delivery names, where it is one
/// this participant was sent or sent: a header that names any other is the
/// item's text, as it is. Each opening read is charged [`OPENING_COST`]
/// against `left`, what the read may still go through: an item whose opening
/// does not fit ends the page before it, as one the page has no room for
/// does, but the first, which `left` holds room for.
pub fn whole_history_page(
    read: &HistoryRead,
    from: HistoryFrom,
    opening: Openings<'_>,
    left: usize,
) -> Result<WholeHistoryPage, Refusal> {
    let mut items: Vec<ShownItem> = Vec::new();
    let mut used = 0;
    let mut spent = 0;
    for row in &read.entries {
        let folds = folded(row).is_some_and(|(_, (_, ledger_id))| ledger_id.is_some());
        let reading = if folds { OPENING_COST } else { 0 };
        if !items.is_empty() && spent + reading > left {
            break;
        }
        spent += reading;
        let item = shown(row, opening)?;
        let cost = bytes(&item) + 1;
        if !items.is_empty() && used + cost > HISTORY_ROOM {
            break;
        }
        used += cost;
        items.push(item);
    }
    let taken = &read.entries[..items.len()];
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

/// Where item `row` holds a delivery the page says in one line, where the
/// delivery begins in the text (after what the human typed with it, none of it
/// where only Claude's own placeholders and wrapper opening precede the header:
/// `typed_before`) and the id the header names, as a line writes it and as the
/// ledger holds one (none where no message could be it): what is neither a
/// tool's output, an agent's words nor a compaction's summary, shown from its
/// start, and holds a delivery's header.
fn folded(row: &HistoryRow) -> Option<(usize, (String, Option<i64>))> {
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
            (at, message_id(header.id))
        })
}

/// An item as a page shows it: a delivery this participant was sent or sent,
/// as its line, said from the message's opening.
fn shown(row: &HistoryRow, opening: Openings<'_>) -> Result<ShownItem, Refusal> {
    let item = &row.item;
    let delivery = match folded(row) {
        Some((at, (id, Some(ledger_id)))) => opening(ledger_id)?.map(|found| Delivery {
            line: outcome_line(&id, &Gist::from(&found)),
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
    })
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

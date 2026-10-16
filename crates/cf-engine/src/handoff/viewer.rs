//! The human's view of a participant's whole history, from its window's
//! History button: every conversation it had, oldest to newest and the current
//! one too, each with its harness, agent, times and what ended it as far as
//! the ledger knows, and the items copied of it, a page at a time.
//!
//! A page is one read of the ledger's ([`HistoryRead`]), as much as one frame
//! of the app's bridge carries ([`PAGE_BYTES`]), each conversation's heading
//! counted with its items. It goes on from a place, the oldest entry shown or
//! the newest, that does not move under it, so a page never repeats an item
//! another showed nor skips one, whatever is added, grows or is copied again
//! meanwhile; a conversation that shows no item is an entry of its own, its
//! heading. A delivery ConsensFlow made to this participant says its outcome in
//! one line, as `cf history` says it, from the opening of the message it names,
//! each opening charged against what the read may still go through; its whole
//! text is there too, for the page to open on a click. A long item shows a
//! part, and is read whole a part at a time, by its place.
//!
//! Every entry fits a page, the first too: an item shows a part of its text at
//! most, and every other text a page shows of it or of its conversation is
//! [`SHOWN_FIELD`] characters at most.

use std::collections::HashMap;

use cf_base::refusal::Refusal;
use cf_base::text::utf16_len;
use cf_ledger::{
    ConversationEnd, HistoryEntry, HistoryFrom, HistoryKey, HistoryRead, HistoryRow,
    MessageOpening, ParticipantConversation, TextCut, OPENING_COST, PAGE_BYTES,
};
use serde::Serialize;

use super::entries::{message_id, outcome_line};
use super::text::delivered;
use super::Gist;

/// How many bytes of items and headings a page holds: the frame, less what
/// the page says besides them.
pub const HISTORY_ROOM: usize = PAGE_BYTES - 1_024;

/// The most characters a page shows of a text that is no item's text: an
/// item's id, its time, a conversation's harness, agent and times, and what
/// ended it. The ledger gives none longer (an id as a page shows it is
/// [`cf_ledger::ID_SHOWN`] characters and "…", a name [`cf_ledger::FIELD_MAX`]
/// bytes); a longer one shows that many and "…".
pub const SHOWN_FIELD: usize = 256;

/// A message's opening by its id, in this project and this participant's
/// only: what a delivery's line is said from. A line may name any number, and
/// only the participant's own messages are read out, so the lookup says none
/// for another's.
pub type Openings<'a> = &'a dyn Fn(i64) -> Result<Option<MessageOpening>, Refusal>;

/// One page of a participant's history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WholeHistoryPage {
    /// The conversations the page holds items of, oldest first.
    pub conversations: Vec<ShownConversation>,
    /// Where the page before this one goes on from: the oldest item shown,
    /// or where a search stopped; none where there is nothing older.
    pub older: Option<HistoryKey>,
    /// Where the page after this one goes on from; none on the most recent.
    pub newer: Option<HistoryKey>,
    /// A search stopped at the most a read goes through, before the page was full.
    pub partial: bool,
}

/// A conversation as a page shows it: its heading, and its items on the page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShownConversation {
    pub id: i64,
    pub harness: String,
    pub agent: Option<String>,
    pub started_at: String,
    /// None for the conversation going on now.
    pub ended_at: Option<String>,
    /// What ended it, where the ledger knows.
    pub ended_by: Option<ConversationEnd>,
    /// How many of its tools' outputs are left out: all of them, unless asked
    /// for, counted so far and no further (`more_tools_left_out`: that many, or more).
    pub tools_left_out: usize,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub more_tools_left_out: bool,
    pub items: Vec<ShownItem>,
}

/// An item as a page shows it: its place in its conversation (`row`, which
/// `history.part` reads it by), and the rest of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ShownItem {
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

/// The page of `read`, the ledger's read from `from`. Its entries are taken in
/// the order the read went while the page has room for them and for the
/// heading of each conversation they are of, the first always; a heading the
/// read took in place of a conversation's items (one that shows none, as the
/// one going on now with nothing copied yet) shows alone. `opening` gives the
/// opening of a message a delivery names, where it is one this participant
/// was sent or sent: a header that names any other is the item's text, as it
/// is. Each opening read is charged [`OPENING_COST`] against `left`, what the
/// read may still go through: an entry whose opening does not fit ends the
/// page before it, as one the page has no room for does, but the first, which
/// `left` holds room for.
pub fn whole_history_page(
    read: &HistoryRead,
    from: HistoryFrom,
    opening: Openings<'_>,
    left: usize,
) -> Result<WholeHistoryPage, Refusal> {
    let headings: HashMap<i64, &ParticipantConversation> = read
        .conversations
        .iter()
        .map(|conversation| (conversation.conversation.id, conversation))
        .collect();
    let heading = |id: i64| headings.get(&id).copied();
    let mut taken: Vec<(HistoryKey, Option<ShownItem>)> = Vec::new();
    let mut used = 0;
    let mut spent = 0;
    for entry in &read.entries {
        let key = entry.key();
        let folds = match entry {
            HistoryEntry::Item(row) => {
                folded(row).is_some_and(|(_, (_, ledger_id))| ledger_id.is_some())
            }
            HistoryEntry::Heading(_) => false,
        };
        let reading = if folds { OPENING_COST } else { 0 };
        if !taken.is_empty() && spent + reading > left {
            break;
        }
        spent += reading;
        let item = match entry {
            HistoryEntry::Item(row) => Some(shown(row, opening)?),
            HistoryEntry::Heading(_) => None,
        };
        let heads = taken
            .last()
            .is_none_or(|(taken, _)| taken.conversation != key.conversation);
        let headed = if heads {
            heading(key.conversation).map_or(0, |conversation| bytes(&head(conversation)) + 1)
        } else {
            0
        };
        let cost = item.as_ref().map_or(0, |item| bytes(item) + 1) + headed;
        if !taken.is_empty() && used + cost > HISTORY_ROOM {
            break;
        }
        used += cost;
        taken.push((key, item));
    }
    let trimmed = taken.len() < read.entries.len();
    // Where the next page each way goes on from: past the last row taken where
    // the page was full, else where the read stopped; and back from the first.
    let last_taken = taken.last().map(|(key, _)| *key);
    let first_taken = taken.first().map(|(key, _)| *key);
    let onward = if trimmed { last_taken } else { read.resume };
    let (older, newer) = match from {
        HistoryFrom::Newest => (onward, None),
        HistoryFrom::Before(place) => (onward, first_taken.or(Some(place))),
        HistoryFrom::After(place) => (first_taken.or(Some(place)), onward),
    };
    if !matches!(from, HistoryFrom::After(_)) {
        taken.reverse();
    }
    // Oldest first, each conversation with its entries on the page.
    let mut conversations: Vec<ShownConversation> = Vec::new();
    for (key, item) in taken {
        if conversations
            .last()
            .is_none_or(|shown| shown.id != key.conversation)
        {
            let Some(conversation) = heading(key.conversation) else {
                continue;
            };
            conversations.push(head(conversation));
        }
        if let (Some(shown), Some(item)) = (conversations.last_mut(), item) {
            shown.items.push(item);
        }
    }
    Ok(WholeHistoryPage {
        conversations,
        older,
        newer,
        partial: read.bounded && !trimmed,
    })
}

/// A conversation's heading, with no items.
fn head(conversation: &ParticipantConversation) -> ShownConversation {
    let view = &conversation.conversation;
    ShownConversation {
        id: view.id,
        harness: field(&view.harness),
        agent: conversation.agent.as_deref().map(field),
        started_at: field(&view.started_at),
        ended_at: view.ended_at.as_deref().map(field),
        ended_by: conversation.ended_by.as_ref().map(|end| match end {
            ConversationEnd::Switch { harness, agent } => ConversationEnd::Switch {
                harness: field(harness),
                agent: agent.as_deref().map(field),
            },
            other => other.clone(),
        }),
        tools_left_out: conversation.tools,
        more_tools_left_out: conversation.more_tools,
        items: Vec::new(),
    }
}

/// Where item `row` holds a delivery the page says in one line, its header's
/// place in the text and the id the header names, as a line writes it and as
/// the ledger holds one (none where no message could be it): what is neither a
/// tool's output, an agent's words nor a compaction's summary, shown from its
/// start, and holds a delivery's header.
fn folded(row: &HistoryRow) -> Option<(usize, (String, Option<i64>))> {
    let item = &row.item;
    let said = item.role == "tool" || item.role == "assistant" || item.compaction;
    let from_the_start = row.cut.is_none_or(|cut| cut.from == 0);
    delivered(&item.text)
        .filter(|_| !said && from_the_start)
        .map(|header| (header.at, message_id(header.id)))
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

/// `text` as a page shows a text that is no item's: [`SHOWN_FIELD`]
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

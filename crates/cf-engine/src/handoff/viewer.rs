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
//! one line, as `cf history` says it; its whole text is there too, for the page
//! to open on a click. A long item shows a part, and is read whole a part at a
//! time.

use std::collections::HashMap;

use cf_base::refusal::Refusal;
use cf_base::text::utf16_len;
use cf_ledger::{
    ConversationEnd, HistoryEntry, HistoryFrom, HistoryKey, HistoryRead, HistoryRow,
    ParticipantConversation, TextCut, PAGE_BYTES,
};
use serde::Serialize;

use super::entries::{named_message, outcome_line};
use super::text::delivered;
use super::Lookup;

/// How many bytes of items and headings a page holds: the frame, less what
/// the page says besides them.
pub const HISTORY_ROOM: usize = PAGE_BYTES - 1_024;

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

/// An item as a page shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ShownItem {
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
/// one going on now with nothing copied yet) shows alone. `message` gives a
/// message a delivery names, where it is one this participant was sent or
/// sent: a header that names any other is the item's text, as it is.
pub fn whole_history_page(
    read: &HistoryRead,
    from: HistoryFrom,
    message: Lookup<'_>,
) -> Result<WholeHistoryPage, Refusal> {
    let headings: HashMap<i64, &ParticipantConversation> = read
        .conversations
        .iter()
        .map(|conversation| (conversation.conversation.id, conversation))
        .collect();
    let heading = |id: i64| headings.get(&id).copied();
    let mut taken: Vec<(HistoryKey, Option<ShownItem>)> = Vec::new();
    let mut used = 0;
    for entry in &read.entries {
        let key = entry.key();
        let item = match entry {
            HistoryEntry::Item(row) => Some(shown(row, message)?),
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
        harness: view.harness.clone(),
        agent: conversation.agent.clone(),
        started_at: view.started_at.clone(),
        ended_at: view.ended_at.clone(),
        ended_by: conversation.ended_by.clone(),
        tools_left_out: conversation.tools,
        more_tools_left_out: conversation.more_tools,
        items: Vec::new(),
    }
}

/// An item as a page shows it: what is neither a tool's output, an agent's
/// words nor a compaction's summary, shown from its start, and holds the
/// header of a delivery this participant was sent or sent, is that delivery.
fn shown(row: &HistoryRow, message: Lookup<'_>) -> Result<ShownItem, Refusal> {
    let item = &row.item;
    let said = item.role == "tool" || item.role == "assistant" || item.compaction;
    let from_the_start = row.cut.is_none_or(|cut| cut.from == 0);
    let header = delivered(&item.text).filter(|_| !said && from_the_start);
    let delivery = match header {
        None => None,
        Some(header) => {
            let (id, found) = named_message(header.id, message)?;
            found.map(|m| Delivery {
                line: outcome_line(&id, &m),
                at: utf16_len(&item.text[..header.at]),
            })
        }
    };
    Ok(ShownItem {
        id: item.id.clone(),
        role: item.role.clone(),
        text: item.text.clone(),
        complete: item.complete,
        at: item.at.clone(),
        compaction: item.compaction,
        delivery,
        cut: row.cut,
    })
}

/// The size of `value` as the page receives it.
fn bytes(value: &impl Serialize) -> usize {
    serde_json::to_vec(value).map_or(0, |json| json.len())
}

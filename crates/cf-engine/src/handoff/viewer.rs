//! The human's view of a participant's whole history, from its window's
//! History button: every conversation it had, oldest to newest and the current
//! one too, each with its harness, agent, times and what ended it as far as
//! the ledger knows, and the items copied of it, a page at a time.
//!
//! A page is one read of the ledger's ([`HistoryRead`]), as much as one frame
//! of the app's bridge carries ([`PAGE_BYTES`]). It goes on from a place, the
//! oldest item shown or the newest, that does not move under it, so a page
//! never repeats an item another showed nor skips one, whatever is added or
//! grows meanwhile. A delivery ConsensFlow made to this participant says its
//! outcome in one line, as `cf history` says it; its whole text is there too,
//! for the page to open on a click. A long item shows a part, and is read whole
//! a part at a time.

use cf_base::refusal::Refusal;
use cf_base::text::utf16_len;
use cf_ledger::{
    ConversationEnd, HistoryFrom, HistoryKey, HistoryRead, HistoryRow, ParticipantConversation,
    TextCut, PAGE_BYTES,
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
    /// How many of its tools' outputs are left out: all of them, unless asked for.
    pub tools_left_out: usize,
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

/// The page of `read`, the ledger's read from `from` (a search's, if
/// `searching`). Its rows are taken in the order the read went while the page
/// has room for them, the first always. `message` gives a message a delivery
/// names, where it is one this participant was sent or sent: a header that
/// names any other is the item's text, as it is. Without a search, a
/// conversation the page's span holds that shows no item (the one going on
/// now, with nothing copied yet) shows its heading alone.
pub fn whole_history_page(
    read: &HistoryRead,
    from: HistoryFrom,
    searching: bool,
    message: Lookup<'_>,
) -> Result<WholeHistoryPage, Refusal> {
    let heading = |id: i64| {
        read.conversations
            .iter()
            .find(|conversation| conversation.conversation.id == id)
    };
    let mut taken: Vec<(HistoryKey, ShownItem)> = Vec::new();
    let mut used = 0;
    for row in &read.rows {
        let item = shown(row, message)?;
        let heads = taken
            .last()
            .is_none_or(|(key, _)| key.conversation != row.key.conversation);
        let headed = if heads {
            heading(row.key.conversation).map_or(0, |conversation| bytes(&head(conversation)) + 1)
        } else {
            0
        };
        let cost = bytes(&item) + 1 + headed;
        if !taken.is_empty() && used + cost > HISTORY_ROOM {
            break;
        }
        used += cost;
        taken.push((row.key, item));
    }
    let trimmed = taken.len() < read.rows.len();
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
    // Without a search, every conversation between the oldest shown and the
    // newest, and past them where nothing is older or newer.
    let low = match (older, taken.first()) {
        (None, _) | (_, None) => i64::MIN,
        (Some(_), Some((key, _))) => key.conversation,
    };
    let high = match (newer, taken.last()) {
        (None, _) | (_, None) => i64::MAX,
        (Some(_), Some((key, _))) => key.conversation,
    };
    let mut conversations: Vec<ShownConversation> = Vec::new();
    for conversation in &read.conversations {
        let id = conversation.conversation.id;
        let items: Vec<ShownItem> = taken
            .iter()
            .filter(|(key, _)| key.conversation == id)
            .map(|(_, item)| item.clone())
            .collect();
        let spanned = !searching && low <= id && id <= high;
        if items.is_empty() && !spanned {
            continue;
        }
        conversations.push(ShownConversation {
            items,
            ..head(conversation)
        });
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

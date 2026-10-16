//! The human's view of a participant's whole history, from its window's
//! History button: every conversation it had, oldest to newest and the current
//! one too, each with its harness, agent, times and what ended it as far as
//! the ledger knows, and every item copied of it. Paged as `cf history` is,
//! newest first and each page in the order things were said, a page as much as
//! one frame of the app's bridge carries ([`PAGE_BYTES`]). A delivery
//! ConsensFlow made says its outcome in one line, as `cf history` says it; its
//! whole text is there too, for the page to open on a click.

use cf_base::refusal::Refusal;
use cf_base::text::utf16_len;
use cf_ledger::model::cut;
use cf_ledger::{
    ConversationEnd, HistoryItem, ParticipantConversation, PAGE_BYTES, TRANSCRIPT_ITEM_MAX,
};
use serde::Serialize;

use super::entries::delivery_line;
use super::pages::{pack, Size};
use super::text::delivered;
use super::Lookup;

/// Room a page keeps for what it says besides its conversations.
const FRAME_BYTES: usize = 1_024;

/// One page of a participant's whole history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WholeHistoryPage {
    /// 1 for the most recent; 0, of 0, when nothing is shown.
    pub page: usize,
    pub pages: usize,
    /// The conversations the page holds items of, oldest first.
    pub conversations: Vec<ShownConversation>,
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
    /// How many of its tool outputs are left out: all of them, unless asked for.
    pub tools_left_out: usize,
    /// It has items on an older page.
    pub earlier: bool,
    /// It has items on a newer page.
    pub later: bool,
    pub items: Vec<ShownItem>,
}

/// An item as a page shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ShownItem {
    pub id: String,
    pub role: String,
    /// Whole, or cut where it would not fit a page on its own.
    pub text: String,
    pub complete: bool,
    pub at: Option<String>,
    /// The summary a compaction wrote in the user's place.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub compaction: bool,
    /// A delivery ConsensFlow made, in one line.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delivery: Option<Delivery>,
}

/// A delivery's line, and where in the item's text the delivery begins (in
/// UTF-16 code units): what comes before it is what the human typed with it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Delivery {
    pub line: String,
    pub at: usize,
}

/// What goes on a page: an item of conversation `conversation` (its place in
/// the list), or its heading alone where none of its items is shown.
struct Piece {
    conversation: usize,
    item: Option<ShownItem>,
    bytes: usize,
}

/// Page `page` of the whole history of a participant, its `conversations` as
/// the ledger reads them (oldest first). Page 1 holds the most recent items.
/// `find` keeps only the items that contain it, in any case, in their text or
/// a delivery's line (an empty one is none); a conversation none of whose items
/// is shown shows with its heading alone, unless something is searched for.
/// `tools` shows the tools' outputs. A page that is not there is refused as
/// `cf history` refuses it, `no-such-page`.
pub fn whole_history_page(
    conversations: &[ParticipantConversation],
    message: Lookup<'_>,
    page: f64,
    find: Option<&str>,
    tools: bool,
) -> Result<WholeHistoryPage, Refusal> {
    let needle = find.filter(|find| !find.is_empty()).map(str::to_lowercase);
    let headings: Vec<ShownConversation> = conversations
        .iter()
        .map(|conversation| heading(conversation, tools))
        .collect();
    let heading_bytes: Vec<usize> = headings.iter().map(|heading| bytes(heading) + 1).collect();
    let room = PAGE_BYTES - FRAME_BYTES;
    let mut pieces = Vec::new();
    for (place, conversation) in conversations.iter().enumerate() {
        let before = pieces.len();
        for item in &conversation.items {
            if item.role == "tool" && !tools {
                continue;
            }
            let shown = shown(item, message)?;
            if needle
                .as_ref()
                .is_some_and(|needle| !contains(&shown, needle))
            {
                continue;
            }
            let shown = fitted(shown, room - heading_bytes[place]);
            pieces.push(Piece {
                conversation: place,
                bytes: bytes(&shown) + 1,
                item: Some(shown),
            });
        }
        if pieces.len() == before && needle.is_none() {
            pieces.push(Piece {
                conversation: place,
                item: None,
                bytes: 0,
            });
        }
    }
    let room = Size {
        bytes: room,
        lines: 0,
    };
    let pages = pack(pieces, room, |piece, newer| {
        let heading = match newer {
            Some(newer) if newer.conversation == piece.conversation => 0,
            _ => heading_bytes[piece.conversation],
        };
        Size {
            bytes: piece.bytes + heading,
            lines: 0,
        }
    });
    if pages.is_empty() {
        return Ok(WholeHistoryPage {
            page: 0,
            pages: 0,
            conversations: Vec::new(),
        });
    }
    let count = pages.len();
    let in_range = page.is_finite() && page.fract() == 0.0 && page >= 1.0 && page <= count as f64;
    if !in_range {
        let how_many = if count == 1 {
            "is 1 page".to_owned()
        } else {
            format!("are {count} pages")
        };
        return Err(Refusal::new("no-such-page", format!("there {how_many}")));
    }
    let at = page as usize - 1;
    // Pages run newest first: the one after is older.
    let first_of = |at: usize| pages.get(at).and_then(|page| page.first());
    let last_of = |at: usize| pages.get(at).and_then(|page| page.last());
    let older = last_of(at + 1).map(|piece| piece.conversation);
    let newer = at
        .checked_sub(1)
        .and_then(first_of)
        .map(|piece| piece.conversation);
    let mut shown: Vec<ShownConversation> = Vec::new();
    for piece in pages.into_iter().nth(at).unwrap_or_default() {
        if shown.last().map(|last| last.id)
            != Some(conversations[piece.conversation].conversation.id)
        {
            let mut heading = headings[piece.conversation].clone();
            heading.earlier = older == Some(piece.conversation) && shown.is_empty();
            shown.push(heading);
        }
        if let Some(item) = piece.item {
            if let Some(last) = shown.last_mut() {
                last.items.push(item);
            }
        }
    }
    if let (Some(last), Some(newer)) = (shown.last_mut(), newer) {
        last.later = conversations[newer].conversation.id == last.id;
    }
    Ok(WholeHistoryPage {
        page: at + 1,
        pages: count,
        conversations: shown,
    })
}

/// A conversation's heading, with no items yet.
fn heading(conversation: &ParticipantConversation, tools: bool) -> ShownConversation {
    let view = &conversation.conversation;
    let tools_left_out = if tools {
        0
    } else {
        conversation
            .items
            .iter()
            .filter(|item| item.role == "tool")
            .count()
    };
    ShownConversation {
        id: view.id,
        harness: view.harness.clone(),
        agent: conversation.agent.clone(),
        started_at: view.started_at.clone(),
        ended_at: view.ended_at.clone(),
        ended_by: conversation.ended_by.clone(),
        tools_left_out,
        earlier: false,
        later: false,
        items: Vec::new(),
    }
}

/// An item as a page shows it: what is neither a tool's output, an agent's
/// words nor a compaction's summary, and holds a delivery's header, is a
/// delivery, as `cf history` reads it.
fn shown(item: &HistoryItem, message: Lookup<'_>) -> Result<ShownItem, Refusal> {
    let said = item.role == "tool" || item.role == "assistant" || item.compaction;
    let delivery = match delivered(&item.text).filter(|_| !said) {
        Some(header) => Some(Delivery {
            line: delivery_line(header.id, message)?,
            at: utf16_len(&item.text[..header.at]),
        }),
        None => None,
    };
    Ok(ShownItem {
        id: item.id.clone(),
        role: item.role.clone(),
        text: item.text.clone(),
        complete: item.complete,
        at: item.at.clone(),
        compaction: item.compaction,
        delivery,
    })
}

/// Whether `needle`, lowercase, is in the item's text or its delivery's line.
fn contains(item: &ShownItem, needle: &str) -> bool {
    item.text.to_lowercase().contains(needle)
        || item
            .delivery
            .as_ref()
            .is_some_and(|delivery| delivery.line.to_lowercase().contains(needle))
}

/// The item, its text cut at [`TRANSCRIPT_ITEM_MAX`] characters where it takes
/// more than `room` bytes on its own: at six bytes a character (a control
/// character, escaped) that is 384 KB, so it fits.
fn fitted(mut item: ShownItem, room: usize) -> ShownItem {
    if bytes(&item) + 1 > room {
        item.text = cut(&item.text, TRANSCRIPT_ITEM_MAX);
    }
    item
}

/// The size of `value` as the page receives it.
fn bytes(value: &impl Serialize) -> usize {
    serde_json::to_vec(value).map_or(0, |json| json.len())
}

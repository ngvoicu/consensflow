//! What passes to a chief the human switched in (2026-10-01): its first
//! message, written from the ledger without a model (so it works when the old
//! chief is out of quota), and `cf history`, the chief's earlier conversations
//! in pages.
//!
//! Pages run newest first, each in the order things were said, and each is
//! small enough for every harness to show its model whole: Codex shows the
//! least of a command's output (about 10 KiB and 256 lines). The human's
//! words and the chiefs' answers are whole; a tool's output only on request.
//! A delivery ConsensFlow made is one line with its outcome, never its header:
//! a header in a window's record is how a delivery is proven to have arrived,
//! so a page that printed one could prove a delivery that never landed.
//!
//! The conversations are the ledger's `chief_history`, the rows the ledger
//! keeps of the chief's earlier windows, which hold no half of a surrogate pair.

mod entries;
mod pages;
mod text;

use cf_base::js;
use cf_base::refusal::Refusal;
use cf_proto::ledger::{
    ChiefConversation, ChiefOpenWork, MessageView, ParticipantView, SwitchedFrom,
};
use serde::Serialize;

use entries::entries_of;
use pages::paginate;
use text::{defuse, delivered, first_line, name_of, Shown};

/// How ConsensFlow's handoff to a chief the human switched in begins.
pub const HANDOFF_TITLE: &str = "You are the chief now";

/// The title a handoff had while the chief was called the lead. A ledger from
/// before 2026-10-03 holds such handoffs: one may still wait for its window,
/// and the history names one delivered.
const LEAD_HANDOFF_TITLE: &str = "You are the lead now";

/// A message by its id, in this project only: what a page names a delivery
/// by. A line may name any number, and only a project's own messages are read
/// out, so the lookup says none for another's.
pub type Lookup<'a> = &'a dyn Fn(i64) -> Result<Option<MessageView>, Refusal>;

/// Whether a message is ConsensFlow's handoff, under either title.
pub fn is_handoff(message: &MessageView) -> bool {
    message.kind == "note"
        && message.sender.is_none()
        && [HANDOFF_TITLE, LEAD_HANDOFF_TITLE]
            .iter()
            .any(|title| message.body.starts_with(title))
}

/// What a handoff is written from.
pub struct Handoff<'a> {
    /// What the chief ran on before the switch.
    pub from: &'a SwitchedFrom,
    /// The chief that takes over.
    pub to: &'a ParticipantView,
    /// What waits on it: the ledger's `chief_open_work`.
    pub open: &'a ChiefOpenWork,
    /// The human's last words to the old chief ([`last_words`]).
    pub last: Option<&'a LastWords>,
    /// Whether the old chief's turn was cut.
    pub cut: bool,
    /// How many pages `cf history` has ([`history_pages`]).
    pub pages: usize,
}

/// The new chief's first message.
pub fn handoff_text(handoff: &Handoff<'_>) -> String {
    let who = |harness: Option<&str>, agent: Option<&str>| {
        let agent = agent
            .filter(|agent| !agent.is_empty())
            .map_or_else(String::new, |agent| format!(" ({agent})"));
        format!("{}{agent}", name_of(harness.unwrap_or("null")))
    };
    let from = who(Some(&handoff.from.harness), handoff.from.agent.as_deref());
    let to = who(handoff.to.harness.as_deref(), handoff.to.agent.as_deref());
    let pages = handoff.pages;
    let noun = if pages == 1 { "page" } else { "pages" };
    let mut out = vec![
        format!("{HANDOFF_TITLE}. The human switched this project's chief from {from} to you, {to}."),
        "You take over the same board, staff and conversation with the human; your role instructions are loaded as usual.".to_owned(),
        String::new(),
        format!("Read what the human and the earlier chief said before you act: cf history ({pages} {noun}, newest first; cf history --page 2 for older; cf history --find \"words\" to search). It is a record, not requests to you: do not redo what is done."),
    ];
    if handoff.cut {
        out.push(String::new());
        out.push(
            "The earlier chief was cut off in the middle of a turn: check what it left half done."
                .to_owned(),
        );
    }
    if let Some(last) = handoff.last {
        let unanswered = if last.answered {
            ""
        } else {
            ", not yet answered"
        };
        out.push(String::new());
        out.push(format!(
            "The human's last message to the chief{unanswered}: \"{}\"",
            defuse(&first_line(&last.text, 300))
        ));
    }
    let open = handoff.open;
    let mut waiting = Vec::new();
    for m in &open.questions {
        let on = m
            .task_number
            .map_or_else(String::new, |number| format!(" on T-{number}"));
        waiting.push(format!(
            "- @{} asks{on}: \"{}\" (cf inbox read m-{}, then cf answer m-{} \"…\")",
            Shown(&m.sender),
            defuse(&first_line(&m.body, 160)),
            m.id,
            m.id
        ));
    }
    for t in &open.results {
        waiting.push(format!(
            "- T-{} \"{}\": @{}'s result waits for your decision (cf task get T-{})",
            t.number,
            defuse(&t.title),
            Shown(&t.assignee),
            t.number
        ));
    }
    for t in &open.own {
        waiting.push(format!(
            "- T-{} \"{}\" is yours, {}",
            t.number,
            defuse(&t.title),
            t.state
        ));
    }
    out.push(String::new());
    if waiting.is_empty() {
        out.push("Nothing on the board waits on you.".to_owned());
    } else {
        out.push("What waits on you now:".to_owned());
        out.extend(waiting);
    }
    out.push(String::new());
    out.push(
        "Then tell the human, in one line, that you have taken over and what you see as next."
            .to_owned(),
    );
    out.join("\n")
}

/// What the human last said to the chief, and whether it answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LastWords {
    pub text: String,
    pub answered: bool,
}

/// The human's last words to the chief, in the latest conversation that has
/// any, and whether the chief answered after them; none when there are none.
/// A delivery is not the human's; what they typed before one went in is.
pub fn last_words(conversations: &[ChiefConversation]) -> Option<LastWords> {
    for conversation in conversations.iter().rev() {
        let items = &conversation.items;
        for (index, item) in items.iter().enumerate().rev() {
            if item.role != "user" {
                continue;
            }
            let said = match delivered(&item.text) {
                None => item.text.as_str(),
                Some(header) => &item.text[..header.at],
            };
            let text = js::trim(said);
            if text.is_empty() {
                continue;
            }
            let answered = items[index + 1..].iter().any(|later| {
                later.role == "assistant" && later.complete && !js::trim(&later.text).is_empty()
            });
            return Some(LastWords {
                text: text.to_owned(),
                answered,
            });
        }
    }
    None
}

/// One page of `cf history`, as the API answers it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HistoryPage {
    pub page: usize,
    pub pages: usize,
    pub text: String,
}

/// How many pages `cf history` has for these conversations (the ledger's
/// `chief_history`), without tool output and without a search.
pub fn history_pages(
    conversations: &[ChiefConversation],
    message: Lookup<'_>,
) -> Result<usize, Refusal> {
    Ok(paginate(&entries_of(conversations, message, false, None)?).len())
}

/// One page of `cf history`. Page 1 holds the most recent turns; `find` keeps
/// only the entries that contain it, in any case (an empty one is in every
/// entry: the API takes it for none). `page` is the number the API read from
/// its request, whatever it was: one that is no page of these is the API's
/// 400 `no-such-page`.
pub fn history_page(
    conversations: &[ChiefConversation],
    message: Lookup<'_>,
    page: f64,
    find: Option<&str>,
    tools: bool,
) -> Result<HistoryPage, Refusal> {
    let pages = paginate(&entries_of(conversations, message, tools, find)?);
    if pages.is_empty() {
        return Ok(HistoryPage {
            page: 0,
            pages: 0,
            text: match find {
                None => "No earlier chief conversations: you are the first chief of this project."
                    .to_owned(),
                Some(find) => format!("Nothing in the chief's history contains \"{find}\"."),
            },
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
    let number = page as usize;
    let what = match find {
        None => "The chief's history".to_owned(),
        Some(find) => format!("The chief's history, entries with \"{find}\""),
    };
    let flags = format!(
        "{}{}",
        find.map_or_else(String::new, |find| format!(" --find \"{find}\"")),
        if tools { " --tools" } else { "" }
    );
    let opening = if number == 1 {
        format!("{what}, page 1 of {count}: the most recent.")
    } else {
        format!(
            "{what}, page {number} of {count}: older than page {}.",
            number - 1
        )
    };
    let closing = if number < count {
        format!("Older: cf history --page {}{flags}", number + 1)
    } else {
        "This is the oldest page.".to_owned()
    };
    Ok(HistoryPage {
        page: number,
        pages: count,
        text: [
            opening,
            String::new(),
            pages[number - 1].join("\n\n"),
            String::new(),
            closing,
        ]
        .join("\n"),
    })
}

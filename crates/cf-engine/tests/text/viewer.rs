//! The history viewer's pages (the History button on a window's head): every
//! conversation of a participant, the current one too, paged newest first
//! within one frame of the app's bridge, searched in any case, the tools'
//! outputs left out unless asked for, a delivery said in one line, and a
//! compaction's summary named as one.

use cf_base::refusal::Refusal;
use cf_engine::handoff::{whole_history_page, WholeHistoryPage};
use cf_ledger::{PAGE_BYTES, TRANSCRIPT_ITEM_MAX};
use cf_proto::ledger::{
    ConversationEnd, ConversationView, HistoryItem, MessageView, ParticipantConversation,
};
use serde_json::json;

use super::support::message;

/// The messages a delivery's line may name.
fn messages(id: i64) -> Result<Option<MessageView>, Refusal> {
    Ok(match id {
        5 => Some(message(5, "result", Some("zeus"), Some(1), "Parser done")),
        _ => None,
    })
}

/// Conversation `n` of the chief, on `harness` and `agent`, with its items each
/// a role and its text; `ended_by` none for the one going on now. A role of
/// `summary` is the summary a compaction wrote, a `user` item marked.
fn conversation(
    n: i64,
    harness: &str,
    agent: &str,
    ended_by: Option<ConversationEnd>,
    items: Vec<(&str, String)>,
) -> ParticipantConversation {
    ParticipantConversation {
        conversation: ConversationView {
            id: n,
            participant_id: 2,
            harness: harness.to_owned(),
            native_session: Some(format!("s-{n}")),
            started_at: format!("2026-10-1{n}T09:00:00.000Z"),
            ended_at: ended_by
                .as_ref()
                .map(|_| format!("2026-10-1{n}T17:00:00.000Z")),
        },
        agent: Some(agent.to_owned()),
        ended_by,
        items: items
            .into_iter()
            .enumerate()
            .map(|(index, (role, text))| HistoryItem {
                id: format!("{n}-{index}"),
                role: if role == "summary" { "user" } else { role }.to_owned(),
                text,
                complete: true,
                at: Some(format!("2026-10-1{n}T10:{:02}:00.000Z", index % 60)),
                compaction: role == "summary",
            })
            .collect(),
    }
}

/// What a switch to Codex on maia says ended a conversation.
fn switched() -> Option<ConversationEnd> {
    Some(ConversationEnd::Switch {
        harness: "codex".to_owned(),
        agent: Some("maia".to_owned()),
    })
}

/// Page `number` of `conversations`.
fn page(
    conversations: &[ParticipantConversation],
    number: usize,
    find: Option<&str>,
    tools: bool,
) -> WholeHistoryPage {
    whole_history_page(conversations, &messages, number as f64, find, tools).unwrap()
}

/// Every page, newest first.
fn every_page(conversations: &[ParticipantConversation], tools: bool) -> Vec<WholeHistoryPage> {
    let first = page(conversations, 1, None, tools);
    (1..=first.pages)
        .map(|number| page(conversations, number, None, tools))
        .collect()
}

/// The size of the answer that carries `page`, as the bridge carries it.
fn answered(page: &WholeHistoryPage) -> usize {
    serde_json::to_vec(&json!({ "ok": true, "history": page }))
        .unwrap()
        .len()
}

/// The ids of a page's items, in order.
fn ids(page: &WholeHistoryPage) -> Vec<String> {
    page.conversations
        .iter()
        .flat_map(|conversation| conversation.items.iter().map(|item| item.id.clone()))
        .collect()
}

#[test]
fn pages_run_newest_first_each_in_order_within_a_frame_a_conversation_headed_on_each_it_is_on() {
    let long = |n: usize| format!("{n} {}", "word ".repeat(10_000));
    let mut before: Vec<(&str, String)> = (0..30).map(|n| ("assistant", long(n))).collect();
    before[15] = ("summary", long(15));
    let after: Vec<(&str, String)> = (30..60).map(|n| ("user", long(n))).collect();
    let history = [
        conversation(1, "claude-code", "chief", switched(), before),
        conversation(2, "codex", "maia", None, after),
    ];
    let pages = every_page(&history, false);
    assert!(pages.len() >= 6, "{} pages", pages.len());
    for (at, shown) in pages.iter().enumerate() {
        assert_eq!((shown.page, shown.pages), (at + 1, pages.len()));
        assert!(answered(shown) <= PAGE_BYTES, "{} bytes", answered(shown));
        // A conversation is headed once on a page.
        let headed: Vec<i64> = shown.conversations.iter().map(|c| c.id).collect();
        let mut once = headed.clone();
        once.dedup();
        assert_eq!(headed, once);
        // It says where it goes on: on the page older than this, and the one newer.
        let first = shown.conversations.first().unwrap();
        let last = shown.conversations.last().unwrap();
        let older = pages
            .get(at + 1)
            .map(|older| older.conversations.last().unwrap().id);
        let newer = at
            .checked_sub(1)
            .map(|newer| pages[newer].conversations[0].id);
        assert_eq!(first.earlier, older == Some(first.id), "page {}", at + 1);
        assert_eq!(last.later, newer == Some(last.id), "page {}", at + 1);
    }
    // Page 1 ends with the newest item; every item is on one page, once, in order.
    assert_eq!(ids(&pages[0]).last().unwrap(), "2-29");
    let read: Vec<String> = pages.iter().rev().flat_map(ids).collect();
    let all: Vec<String> = history
        .iter()
        .flat_map(|conversation| conversation.items.iter().map(|item| item.id.clone()))
        .collect();
    assert_eq!(read, all);
    // The summary is named as one, a user's item that is the harness's own.
    let summary = pages
        .iter()
        .flat_map(|shown| &shown.conversations)
        .flat_map(|conversation| &conversation.items)
        .find(|item| item.id == "1-15")
        .unwrap();
    assert_eq!((summary.role.as_str(), summary.compaction), ("user", true));
}

#[test]
fn heads_each_conversation_with_its_harness_agent_times_and_what_ended_it_the_current_one_with_none(
) {
    let history = [
        conversation(
            1,
            "claude-code",
            "chief",
            switched(),
            vec![
                ("user", "Ship it".to_owned()),
                ("summary", "This session is being continued".to_owned()),
            ],
        ),
        conversation(
            2,
            "codex",
            "maia",
            Some(ConversationEnd::Window { resumed: false }),
            vec![("assistant", "Shipped".to_owned())],
        ),
        conversation(3, "codex", "maia", None, vec![("tool", "ok".to_owned())]),
    ];
    let shown = serde_json::to_value(page(&history, 1, None, false)).unwrap();
    assert_eq!(
        shown,
        json!({
            "page": 1,
            "pages": 1,
            "conversations": [
                {
                    "id": 1, "harness": "claude-code", "agent": "chief",
                    "startedAt": "2026-10-11T09:00:00.000Z", "endedAt": "2026-10-11T17:00:00.000Z",
                    "endedBy": { "by": "switch", "harness": "codex", "agent": "maia" },
                    "toolsLeftOut": 0, "earlier": false, "later": false,
                    "items": [
                        { "id": "1-0", "role": "user", "text": "Ship it", "complete": true, "at": "2026-10-11T10:00:00.000Z" },
                        { "id": "1-1", "role": "user", "text": "This session is being continued", "complete": true, "at": "2026-10-11T10:01:00.000Z", "compaction": true },
                    ],
                },
                {
                    "id": 2, "harness": "codex", "agent": "maia",
                    "startedAt": "2026-10-12T09:00:00.000Z", "endedAt": "2026-10-12T17:00:00.000Z",
                    "endedBy": { "by": "window", "resumed": false },
                    "toolsLeftOut": 0, "earlier": false, "later": false,
                    "items": [
                        { "id": "2-0", "role": "assistant", "text": "Shipped", "complete": true, "at": "2026-10-12T10:00:00.000Z" },
                    ],
                },
                // The one going on now, its one item a tool's output left out: its heading alone.
                {
                    "id": 3, "harness": "codex", "agent": "maia",
                    "startedAt": "2026-10-13T09:00:00.000Z", "endedAt": null, "endedBy": null,
                    "toolsLeftOut": 1, "earlier": false, "later": false,
                    "items": [],
                },
            ],
        })
    );
}

#[test]
fn says_a_delivery_in_its_line_and_where_it_begins_and_no_summary_or_agent_s_quote_is_one() {
    let header = "[ConsensFlow m-5 · T-1 · result from @zeus]\nParser done";
    let history = [conversation(
        1,
        "pi",
        "chief",
        None,
        vec![
            ("user", format!("héllo 🙂{header}")),
            ("custom", header.to_owned()),
            ("summary", format!("Summed up: {header}")),
            ("assistant", format!("I quoted {header}")),
            ("user", "[ConsensFlow m-99 · note]\ngone".to_owned()),
        ],
    )];
    let shown = page(&history, 1, None, false);
    let items = &shown.conversations[0].items;
    let line = "· m-5: @zeus's result on T-1 (cf task get T-1)";
    let delivery = items[0].delivery.as_ref().unwrap();
    // What the human typed with it is "héllo 🙂": eight UTF-16 units, as the page counts.
    assert_eq!((delivery.line.as_str(), delivery.at), (line, 8));
    assert_eq!(items[0].text, format!("héllo 🙂{header}"), "the text whole");
    assert_eq!(items[1].delivery.as_ref().unwrap().at, 0);
    assert!(items[2].delivery.is_none() && items[2].compaction);
    assert!(items[3].delivery.is_none());
    assert_eq!(
        items[4].delivery.as_ref().unwrap().line,
        "· m-99: a message ConsensFlow delivered (no longer on record)"
    );
}

#[test]
fn searches_in_any_case_in_the_text_and_a_delivery_s_line_and_leaves_the_tools_out_unless_asked() {
    let history = [
        conversation(
            1,
            "claude-code",
            "chief",
            switched(),
            vec![
                ("user", "Run the PARSER tests".to_owned()),
                ("tool", "ok 12 passed".to_owned()),
                (
                    "user",
                    "[ConsensFlow m-5 · T-1 · result from @zeus]\nAll green".to_owned(),
                ),
            ],
        ),
        conversation(
            2,
            "codex",
            "maia",
            None,
            vec![("assistant", "noted".to_owned())],
        ),
    ];
    let found = |find: &str, tools: bool| ids(&page(&history, 1, Some(find), tools));
    assert_eq!(found("parser", false), ["1-0"]);
    // "cf task get T-1" is in the delivery's line alone.
    assert_eq!(found("CF TASK GET t-1", false), ["1-2"]);
    // A search heads only the conversations it found something in.
    let searched = page(&history, 1, Some("noted"), false);
    assert_eq!(searched.conversations.len(), 1);
    assert_eq!(searched.conversations[0].id, 2);
    // A tool's output only when asked for, in a search too.
    let off = page(&history, 1, Some("passed"), false);
    assert_eq!((off.page, off.pages, off.conversations.len()), (0, 0, 0));
    assert_eq!(found("passed", true), ["1-1"]);
    let shown = page(&history, 1, None, false);
    assert_eq!(shown.conversations[0].tools_left_out, 1);
    assert_eq!(ids(&shown), ["1-0", "1-2", "2-0"]);
    let with_tools = page(&history, 1, None, true);
    assert_eq!(with_tools.conversations[0].tools_left_out, 0);
    assert_eq!(ids(&with_tools), ["1-0", "1-1", "1-2", "2-0"]);
    // An empty search is none.
    assert_eq!(
        ids(&page(&history, 1, Some(""), false)),
        ["1-0", "1-2", "2-0"]
    );
}

#[test]
fn cuts_an_item_too_long_for_a_frame_alone_and_keeps_every_page_within_one() {
    let huge = "x".repeat(2_000_000);
    // Each control character is six bytes of JSON.
    let escaped = "\u{1}".repeat(300_000);
    let mut many = vec![
        conversation(1, "claude-code", "chief", switched(), vec![("user", huge)]),
        conversation(
            2,
            "claude-code",
            "chief",
            switched(),
            vec![("assistant", escaped)],
        ),
    ];
    // Three thousand conversations of one word each: their headings fill pages too,
    // and take more of one than their items do.
    for n in 3..3_003 {
        many.push(conversation(
            n,
            "codex",
            "maia",
            Some(ConversationEnd::Fresh),
            vec![("assistant", "ok".to_owned())],
        ));
    }
    let pages = every_page(&many, false);
    for shown in &pages {
        assert!(answered(shown) <= PAGE_BYTES, "{} bytes", answered(shown));
    }
    let oldest = &pages.last().unwrap().conversations;
    let first = &oldest[0].items[0];
    assert!(
        first.text.ends_with("… (2000000 characters; cut here)"),
        "{}",
        &first.text[first.text.len() - 40..]
    );
    assert!(first.text.starts_with(&"x".repeat(TRANSCRIPT_ITEM_MAX)));
    let read: Vec<String> = pages.iter().rev().flat_map(ids).collect();
    assert_eq!(read.len(), 3_002, "every item, once");
}

#[test]
fn refuses_a_page_that_is_not_there_as_cf_history_does_and_says_none_of_nothing() {
    let history = [conversation(
        1,
        "pi",
        "chief",
        None,
        vec![("user", "hi".to_owned())],
    )];
    for asked in [0.0, 2.0, 1.5, f64::NAN, -1.0] {
        let refused = whole_history_page(&history, &messages, asked, None, false).unwrap_err();
        assert_eq!(
            (refused.code, refused.status, refused.message.as_str()),
            ("no-such-page", 400, "there is 1 page"),
            "page {asked}"
        );
    }
    let none = whole_history_page(&[], &messages, 1.0, None, false).unwrap();
    assert_eq!((none.page, none.pages, none.conversations.len()), (0, 0, 0));
}

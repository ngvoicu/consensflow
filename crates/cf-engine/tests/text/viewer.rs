//! The history viewer's pages (the History button on a window's head): one
//! read of the ledger's, taken while a frame of the app's bridge has room, set
//! out by conversation with their headings, and the places the next pages go
//! on from; a delivery to this participant said in one line, any other header
//! left as text; a long item's part as the read cut it.

use cf_base::refusal::Refusal;
use cf_engine::handoff::{whole_history_page, WholeHistoryPage};
use cf_ledger::{HistoryFrom, PAGE_BYTES};
use cf_proto::ledger::{
    ConversationEnd, ConversationView, HistoryEntry, HistoryItem, HistoryKey, HistoryRead,
    HistoryRow, MessageView, ParticipantConversation, Scanned, TextCut,
};
use serde_json::json;

use super::support::message;

/// The messages this participant was sent or sent, as the daemon looks them up
/// for it: m-5 is its, and any other number names none of its.
fn messages(id: i64) -> Result<Option<MessageView>, Refusal> {
    Ok((id == 5).then(|| message(5, "result", Some("zeus"), Some(1), "Parser done")))
}

/// Conversation `n` of the chief, on `harness` and `agent`; `ended_by` none for the current one.
fn heading(
    n: i64,
    harness: &str,
    agent: &str,
    ended_by: Option<ConversationEnd>,
    tools: usize,
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
        tools,
        more_tools: false,
    }
}

/// An item of conversation `conversation`, the `seq`th copied: a role
/// (`summary`, a compaction's) and its text.
fn row(conversation: i64, seq: i64, role: &str, text: &str) -> HistoryRow {
    HistoryRow {
        key: HistoryKey {
            conversation,
            row: conversation * 100 + seq,
            part: 0,
        },
        item: HistoryItem {
            id: format!("{conversation}-{seq}"),
            role: if role == "summary" { "user" } else { role }.to_owned(),
            text: text.to_owned(),
            complete: true,
            at: Some(format!("2026-10-1{conversation}T10:{seq:02}:00.000Z")),
            compaction: role == "summary",
        },
        cut: None,
    }
}

/// A read of the ledger's that gave `entries`, and goes on from `resume`.
fn read_entries(
    conversations: Vec<ParticipantConversation>,
    entries: Vec<HistoryEntry>,
    resume: Option<HistoryKey>,
) -> HistoryRead {
    HistoryRead {
        participant: 2,
        conversations,
        entries,
        resume,
        bounded: false,
        scanned: Scanned::default(),
    }
}

/// A read of the ledger's that gave the items `rows`, and goes on from `resume`.
fn read(
    conversations: Vec<ParticipantConversation>,
    rows: Vec<HistoryRow>,
    resume: Option<HistoryKey>,
) -> HistoryRead {
    let entries = rows.into_iter().map(HistoryEntry::Item).collect();
    read_entries(conversations, entries, resume)
}

/// The heading of conversation `conversation`, as a read takes it in place of its items.
fn heading_entry(conversation: i64) -> HistoryEntry {
    HistoryEntry::Heading(HistoryKey {
        conversation,
        row: 0,
        part: 0,
    })
}

fn switched() -> Option<ConversationEnd> {
    Some(ConversationEnd::Switch {
        harness: "codex".to_owned(),
        agent: Some("maia".to_owned()),
    })
}

fn page(read: &HistoryRead, from: HistoryFrom) -> WholeHistoryPage {
    whole_history_page(read, from, &messages).unwrap()
}

/// The ids of a page's items, in order.
fn ids(page: &WholeHistoryPage) -> Vec<String> {
    page.conversations
        .iter()
        .flat_map(|conversation| conversation.items.iter().map(|item| item.id.clone()))
        .collect()
}

#[test]
fn heads_each_conversation_with_its_harness_agent_times_and_end_and_the_current_one_with_none() {
    let conversations = vec![
        heading(1, "claude-code", "chief", switched(), 0),
        heading(
            2,
            "codex",
            "maia",
            Some(ConversationEnd::Window { resumed: false }),
            0,
        ),
        heading(3, "codex", "maia", None, 1),
    ];
    // The most recent page, newest first as the ledger reads it: the current
    // conversation holds a tool's output alone, left out, so its heading is
    // taken in place of its items.
    let entries = vec![
        heading_entry(3),
        HistoryEntry::Item(row(2, 0, "assistant", "Shipped")),
        HistoryEntry::Item(row(1, 1, "summary", "This session is being continued")),
        HistoryEntry::Item(row(1, 0, "user", "Ship it")),
    ];
    let shown = serde_json::to_value(page(
        &read_entries(conversations, entries, None),
        HistoryFrom::Newest,
    ))
    .unwrap();
    assert_eq!(
        shown,
        json!({
            "conversations": [
                {
                    "id": 1, "harness": "claude-code", "agent": "chief",
                    "startedAt": "2026-10-11T09:00:00.000Z", "endedAt": "2026-10-11T17:00:00.000Z",
                    "endedBy": { "by": "switch", "harness": "codex", "agent": "maia" },
                    "toolsLeftOut": 0,
                    "items": [
                        { "id": "1-0", "role": "user", "text": "Ship it", "complete": true, "at": "2026-10-11T10:00:00.000Z" },
                        { "id": "1-1", "role": "user", "text": "This session is being continued", "complete": true, "at": "2026-10-11T10:01:00.000Z", "compaction": true },
                    ],
                },
                {
                    "id": 2, "harness": "codex", "agent": "maia",
                    "startedAt": "2026-10-12T09:00:00.000Z", "endedAt": "2026-10-12T17:00:00.000Z",
                    "endedBy": { "by": "window", "resumed": false },
                    "toolsLeftOut": 0,
                    "items": [
                        { "id": "2-0", "role": "assistant", "text": "Shipped", "complete": true, "at": "2026-10-12T10:00:00.000Z" },
                    ],
                },
                {
                    "id": 3, "harness": "codex", "agent": "maia",
                    "startedAt": "2026-10-13T09:00:00.000Z", "endedAt": null, "endedBy": null,
                    "toolsLeftOut": 1,
                    "items": [],
                },
            ],
            "older": null,
            "newer": null,
            "partial": false,
        })
    );
}

#[test]
fn says_a_delivery_to_this_participant_in_its_line_and_leaves_any_other_header_as_text() {
    let header = "[ConsensFlow m-5 · T-1 · result from @zeus]\nParser done";
    // Another participant's message, quoted in this one's window.
    let theirs = "[ConsensFlow m-9 · T-2 · result from @hera]\nSecret plans";
    let rows = vec![
        row(1, 5, "user", "[ConsensFlow m-99 · note]\ngone"),
        row(1, 4, "user", theirs),
        row(1, 3, "assistant", &format!("I quoted {header}")),
        row(1, 2, "summary", &format!("Summed up: {header}")),
        row(1, 1, "custom", header),
        row(1, 0, "user", &format!("héllo 🙂{header}")),
    ];
    let shown = page(
        &read(vec![heading(1, "pi", "chief", None, 0)], rows, None),
        HistoryFrom::Newest,
    );
    let items = &shown.conversations[0].items;
    let line = "· m-5: @zeus's result on T-1 (cf task get T-1)";
    let delivery = items[0].delivery.as_ref().unwrap();
    // What the human typed with it is "héllo 🙂": eight UTF-16 units, as the page counts.
    assert_eq!((delivery.line.as_str(), delivery.at), (line, 8));
    assert_eq!(items[0].text, format!("héllo 🙂{header}"), "the text whole");
    assert_eq!(items[1].delivery.as_ref().unwrap().at, 0);
    assert!(items[2].delivery.is_none() && items[2].compaction);
    assert!(items[3].delivery.is_none(), "an agent's quote");
    assert!(
        items[4].delivery.is_none(),
        "another's message: no line, nothing of it"
    );
    assert_eq!(items[4].text, theirs);
    assert!(
        items[5].delivery.is_none(),
        "a message not on record is text too"
    );
}

#[test]
fn takes_rows_while_a_frame_has_room_and_says_where_the_next_pages_go_on_from() {
    // Five items of 200 KB, newest first: two fit a frame, and the rest are the next page's.
    let big = "x".repeat(200_000);
    let rows: Vec<HistoryRow> = (0..5).rev().map(|seq| row(1, seq, "user", &big)).collect();
    let resume = Some(rows[4].key);
    let all = read(
        vec![heading(1, "pi", "chief", None, 0)],
        rows.clone(),
        resume,
    );
    let newest = page(&all, HistoryFrom::Newest);
    assert_eq!(ids(&newest), ["1-3", "1-4"]);
    assert!(
        serde_json::to_vec(&json!({ "ok": true, "history": newest }))
            .unwrap()
            .len()
            <= PAGE_BYTES
    );
    assert_eq!(
        newest.older,
        Some(rows[1].key),
        "from the oldest shown, not past what was read"
    );
    assert_eq!(newest.newer, None, "the most recent");
    // A page read before a place goes back to it; one read after goes on from where the read stopped.
    let before = page(&all, HistoryFrom::Before(rows[0].key));
    assert_eq!(before.newer, Some(rows[0].key));
    let some = read(
        vec![heading(1, "pi", "chief", None, 0)],
        rows[3..].to_vec(),
        None,
    );
    let after = page(&some, HistoryFrom::After(rows[2].key));
    assert_eq!((after.older, after.newer), (Some(rows[3].key), None));
}

#[test]
fn counts_each_conversation_s_heading_against_the_frame_as_well_as_its_items() {
    // Three thousand conversations of one word each: their headings take more of a
    // frame than their items do.
    let conversations: Vec<ParticipantConversation> = (1..=3_000)
        .map(|n| heading(n, "codex", "maia", Some(ConversationEnd::Fresh), 0))
        .collect();
    let rows: Vec<HistoryRow> = (1..=3_000)
        .rev()
        .map(|n| row(n, 0, "assistant", "ok"))
        .collect();
    let resume = rows.last().map(|row| row.key);
    let shown = page(&read(conversations, rows, resume), HistoryFrom::Newest);
    let size = serde_json::to_vec(&json!({ "ok": true, "history": shown }))
        .unwrap()
        .len();
    assert!(size <= PAGE_BYTES, "{size} bytes");
    assert!(ids(&shown).len() < 3_000, "the rest are older pages'");
}

#[test]
fn counts_the_heading_of_a_conversation_that_shows_no_item_against_the_frame_and_goes_on_past_it() {
    // The reviewers' case: 10,000 conversations of tools' outputs alone, left
    // out, each a heading and nothing more. Their headings fill a frame, and
    // the next page goes on from the oldest one shown.
    let conversations: Vec<ParticipantConversation> = (1..=10_000)
        .map(|n| heading(n, "codex", "maia", Some(ConversationEnd::Fresh), 3))
        .collect();
    let entries: Vec<HistoryEntry> = (1..=10_000).rev().map(heading_entry).collect();
    let resume = entries.last().map(HistoryEntry::key);
    let shown = page(
        &read_entries(conversations, entries, resume),
        HistoryFrom::Newest,
    );
    let size = serde_json::to_vec(&json!({ "ok": true, "history": shown }))
        .unwrap()
        .len();
    assert!(size <= PAGE_BYTES, "{size} bytes");
    let headed = shown.conversations.len();
    assert!(headed > 100 && headed < 10_000, "{headed} headings");
    assert!(shown
        .conversations
        .iter()
        .all(|conversation| conversation.items.is_empty()));
    let oldest = shown.conversations[0].id;
    assert_eq!(
        shown.older,
        Some(HistoryKey {
            conversation: oldest,
            row: 0,
            part: 0
        }),
        "on from the oldest heading shown"
    );
}

#[test]
fn a_search_shows_the_conversations_it_found_in_and_says_where_it_stopped_short() {
    let conversations = vec![
        heading(1, "pi", "chief", switched(), 0),
        heading(2, "codex", "maia", None, 0),
    ];
    let mut stopped = read(
        conversations,
        vec![row(1, 3, "user", "Run the PARSER tests")],
        Some(HistoryKey {
            conversation: 1,
            row: 101,
            part: 0,
        }),
    );
    stopped.bounded = true;
    let found = page(&stopped, HistoryFrom::Newest);
    assert_eq!(
        found.conversations.len(),
        1,
        "no heading of the one it found nothing in"
    );
    assert!(found.partial);
    assert_eq!(
        found.older, stopped.resume,
        "on from where it stopped, not from the hit"
    );
}

#[test]
fn a_part_of_a_long_item_shows_as_the_read_cut_it_and_a_delivery_only_from_its_start() {
    let header = "[ConsensFlow m-5 · T-1 · result from @zeus]\nParser done";
    let mut start = row(1, 1, "user", header);
    start.cut = Some(TextCut {
        from: 0,
        bytes: 900_000,
    });
    let mut middle = row(1, 0, "user", header);
    middle.cut = Some(TextCut {
        from: 70_000,
        bytes: 900_000,
    });
    let shown = page(
        &read(
            vec![heading(1, "pi", "chief", None, 0)],
            vec![start, middle],
            None,
        ),
        HistoryFrom::Newest,
    );
    let items = &shown.conversations[0].items;
    assert!(items[1].delivery.is_some());
    assert_eq!(
        items[1].cut,
        Some(TextCut {
            from: 0,
            bytes: 900_000
        })
    );
    assert!(
        items[0].delivery.is_none(),
        "a part from its middle is text"
    );
    let said = serde_json::to_value(&items[0]).unwrap();
    assert_eq!(said["cut"], json!({ "from": 70_000, "bytes": 900_000 }));
}

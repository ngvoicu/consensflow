//! The history viewer's pages (the History button on a window's head): one
//! read of the ledger's, taken while a frame of the app's bridge has room, as
//! one stream of items oldest first with no heading of any conversation (each
//! item says the conversation and row it stands at), and the places the next
//! pages go on from; a delivery to this participant marked with where it
//! begins and nothing of its words, and charged nothing; any other header left
//! as text; a long item's part as the read cut it; and the first item within a
//! page too, whatever it holds.

use cf_base::refusal::Refusal;
use cf_engine::handoff::{
    whole_history_page, ShownItem, WholeHistoryPage, HISTORY_ROOM, SHOWN_FIELD,
};
use cf_ledger::{HistoryFrom, HISTORY_PART, PAGE_BYTES};
use cf_proto::ledger::{HistoryItem, HistoryKey, HistoryRead, HistoryRow, Scanned, TextCut};
use serde_json::json;

/// The messages this participant was sent or sent, as the daemon looks them up
/// for it: m-5 is its, and any other number names none of its.
fn owns(id: i64) -> Result<bool, Refusal> {
    Ok(id == 5)
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
            detail: None,
        },
        cut: None,
    }
}

/// A read of the ledger's that gave the items `rows`, and goes on from `resume`.
fn read(rows: Vec<HistoryRow>, resume: Option<HistoryKey>) -> HistoryRead {
    HistoryRead {
        participant: 2,
        entries: rows,
        resume,
        bounded: false,
        scanned: Scanned::default(),
        charged: 0,
    }
}

fn page(read: &HistoryRead, from: HistoryFrom) -> WholeHistoryPage {
    whole_history_page(read, from, &owns, None).unwrap()
}

/// The ids of a page's items, in order.
fn ids(page: &WholeHistoryPage) -> Vec<String> {
    page.items.iter().map(|item| item.id.clone()).collect()
}

#[test]
fn sets_out_the_items_of_every_conversation_oldest_first_in_one_stream_and_heads_none() {
    // The most recent page, newest first as the ledger reads it, across three
    // conversations (the current one holds a tool's output alone, left out, so
    // the read took nothing of it).
    let rows = vec![
        row(2, 0, "assistant", "Shipped"),
        row(1, 1, "summary", "This session is being continued"),
        row(1, 0, "user", "Ship it"),
    ];
    let shown = serde_json::to_value(page(&read(rows, None), HistoryFrom::Newest)).unwrap();
    assert_eq!(
        shown,
        json!({
            "items": [
                { "conversation": 1, "row": 100, "id": "1-0", "role": "user", "text": "Ship it", "complete": true, "at": "2026-10-11T10:00:00.000Z" },
                { "conversation": 1, "row": 101, "id": "1-1", "role": "user", "text": "This session is being continued", "complete": true, "at": "2026-10-11T10:01:00.000Z", "compaction": true },
                { "conversation": 2, "row": 200, "id": "2-0", "role": "assistant", "text": "Shipped", "complete": true, "at": "2026-10-12T10:00:00.000Z" },
            ],
            "older": null,
            "newer": null,
            "partial": false,
        })
    );
}

#[test]
fn marks_a_delivery_to_this_participant_and_leaves_any_other_header_as_text() {
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
    let shown = page(&read(rows, None), HistoryFrom::Newest);
    let items = &shown.items;
    // What the human typed with it is "héllo 🙂": eight UTF-16 units, as the page counts.
    assert_eq!(items[0].delivery.as_ref().unwrap().at, 8);
    assert_eq!(items[0].text, format!("héllo 🙂{header}"), "the text whole");
    assert_eq!(items[1].delivery.as_ref().unwrap().at, 0);
    assert!(items[2].delivery.is_none() && items[2].compaction);
    assert!(items[3].delivery.is_none(), "an agent's quote");
    assert!(
        items[4].delivery.is_none(),
        "another's message: no mark, nothing of it"
    );
    assert_eq!(items[4].text, theirs);
    assert!(
        items[5].delivery.is_none(),
        "a message not on record is text too"
    );
}

#[test]
fn types_nothing_for_what_claude_put_ahead_of_a_delivery_and_keeps_the_words_a_human_typed() {
    let message = "[ConsensFlow m-5 · T-1 · result from @zeus]\nParser done";
    let wrapped = |ahead: &str| {
        format!("{ahead}<pasted_content id=\"f920\">\n{message}\n</pasted_content id=\"f920\">\n")
    };
    let rows = vec![
        // The placeholders of the images Claude attached to a result that starts with their
        // paths, the header at once, or behind a blank line and the wrapper's opening.
        row(1, 3, "user", &format!("[Image #1] [Image #2]{message}")),
        row(1, 2, "user", &wrapped("[Image #3]\n\n")),
        row(1, 1, "user", &wrapped("\n\n")),
        // The human's own words, a placeholder among them, are what they typed.
        row(1, 0, "user", &format!("[Image #1]look at this{message}")),
    ];
    let shown = page(&read(rows, None), HistoryFrom::Newest);
    let at = |index: usize| shown.items[index].delivery.as_ref().map(|d| d.at);
    assert_eq!(
        [at(0), at(1), at(2), at(3)],
        [Some(22), Some(0), Some(0), Some(0)]
    );
}

#[test]
fn takes_rows_while_a_frame_has_room_and_says_where_the_next_pages_go_on_from() {
    // Five items of 200 KB, newest first: two fit a frame, and the rest are the next page's.
    let big = "x".repeat(200_000);
    let rows: Vec<HistoryRow> = (0..5).rev().map(|seq| row(1, seq, "user", &big)).collect();
    let resume = Some(rows[4].key);
    let all = read(rows.clone(), resume);
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
    let some = read(rows[3..].to_vec(), None);
    let after = page(&some, HistoryFrom::After(rows[2].key));
    assert_eq!((after.older, after.newer), (Some(rows[3].key), None));
}

#[test]
fn takes_the_items_of_thousands_of_conversations_while_a_frame_has_room() {
    // Ten thousand conversations of one word each: more of them than a frame holds.
    let rows: Vec<HistoryRow> = (1..=10_000)
        .rev()
        .map(|n| row(n, 0, "assistant", "ok"))
        .collect();
    let resume = rows.last().map(|row| row.key);
    let shown = page(&read(rows, resume), HistoryFrom::Newest);
    let size = serde_json::to_vec(&json!({ "ok": true, "history": shown }))
        .unwrap()
        .len();
    assert!(size <= PAGE_BYTES, "{size} bytes");
    let taken = shown.items.len();
    assert!(taken > 1_000 && taken < 10_000, "{taken} items");
    assert_eq!(
        shown.older,
        Some(HistoryKey {
            conversation: shown.items[0].conversation,
            row: shown.items[0].row,
            part: 0
        }),
        "on from the oldest shown"
    );
}

#[test]
fn a_read_that_passed_only_conversations_that_show_nothing_is_an_empty_page_that_goes_on_from_where_it_stopped(
) {
    // The reviewers' case: 10,000 conversations of tools' outputs alone, left
    // out. The read took nothing before its bound, and stopped past the 4,000th.
    let mut passed = read(
        Vec::new(),
        Some(HistoryKey {
            conversation: 6_000,
            row: 0,
            part: 0,
        }),
    );
    passed.bounded = true;
    let shown = page(&passed, HistoryFrom::Newest);
    assert!(shown.items.is_empty());
    assert_eq!(shown.older, passed.resume, "on from where it stopped");
    assert!(shown.partial);
    assert_eq!(
        serde_json::to_value(&shown).unwrap(),
        json!({ "items": [], "older": { "conversation": 6_000, "row": 0 }, "newer": null, "partial": true })
    );
}

#[test]
fn a_search_shows_what_it_found_and_says_where_it_stopped_short() {
    let mut stopped = read(
        vec![row(1, 3, "user", "Run the PARSER tests")],
        Some(HistoryKey {
            conversation: 1,
            row: 101,
            part: 0,
        }),
    );
    stopped.bounded = true;
    let found = page(&stopped, HistoryFrom::Newest);
    assert_eq!(ids(&found), ["1-3"]);
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
        revision: 3,
    });
    let mut middle = row(1, 0, "user", header);
    middle.cut = Some(TextCut {
        from: 70_000,
        bytes: 900_000,
        revision: 4,
    });
    let shown = page(&read(vec![start, middle], None), HistoryFrom::Newest);
    let items = &shown.items;
    assert!(items[1].delivery.is_some());
    assert_eq!(
        items[1].cut,
        Some(TextCut {
            from: 0,
            bytes: 900_000,
            revision: 3,
        })
    );
    assert!(
        items[0].delivery.is_none(),
        "a part from its middle is text"
    );
    let said = serde_json::to_value(&items[0]).unwrap();
    assert_eq!(
        said["cut"],
        json!({ "from": 70_000, "bytes": 900_000, "revision": 4 }),
        "with the copy of its item the part is of"
    );
}

#[test]
fn a_page_of_deliveries_holds_as_many_items_as_its_room_allows() {
    let header = "[ConsensFlow m-5 · T-1 · result from @zeus]\nParser done";
    // Newest first: three deliveries, a word between two of them.
    let rows = vec![
        row(1, 3, "user", header),
        row(1, 2, "user", "a word"),
        row(1, 1, "user", header),
        row(1, 0, "user", header),
    ];
    let all = read(rows, None);
    // Reading a delivery costs the page nothing: the four are all there.
    let shown = page(&all, HistoryFrom::Newest);
    assert_eq!(ids(&shown), ["1-0", "1-1", "1-2", "1-3"]);
    assert_eq!(shown.older, None, "nothing left to go on to");
    assert!(shown
        .items
        .iter()
        .all(|item| item.id == "1-2" || item.delivery.is_some()));
    // Nor is a long page of them: it ends where its room does, and not before.
    let body = format!(
        "[ConsensFlow m-5 · T-1 · result from @zeus]\n{}",
        "y".repeat(9_000)
    );
    let rows: Vec<HistoryRow> = (0..200)
        .rev()
        .map(|seq| row(1, seq, "user", &body))
        .collect();
    let resume = rows.last().map(|row| row.key);
    let long = read(rows.clone(), resume);
    let shown = page(&long, HistoryFrom::Newest);
    let taken = shown.items.len();
    assert!(taken > 40 && taken < 200, "{taken} items");
    let size = |items: &[ShownItem]| {
        items
            .iter()
            .map(|item| serde_json::to_vec(item).unwrap().len() + 1)
            .sum::<usize>()
    };
    let next = page(&read(vec![rows[taken].clone()], None), HistoryFrom::Newest);
    assert!(
        size(&shown.items) + size(&next.items) > HISTORY_ROOM,
        "the next delivery would have fitted: {taken} items of {}",
        size(&shown.items)
    );
}

#[test]
fn a_delivery_says_where_it_begins_and_nothing_of_its_words() {
    let header = "[ConsensFlow m-5 · T-1 · result from @zeus]\nParser done";
    let rows = vec![
        row(1, 1, "user", &format!("héllo 🙂{header}")),
        row(1, 0, "user", header),
    ];
    let shown = page(&read(rows, None), HistoryFrom::Newest);
    let said = serde_json::to_value(&shown.items).unwrap();
    // What the human typed with it is "héllo 🙂": eight UTF-16 units, as the page counts.
    assert_eq!(said[0]["delivery"], json!({ "at": 0 }));
    assert_eq!(said[1]["delivery"], json!({ "at": 8 }));
    assert!(
        !said.to_string().contains("\"line\""),
        "no line of the delivery's outcome: {said}"
    );
}

/// An assistant's message its reader did not complete (not the turn's last answer).
fn unfinished(conversation: i64, seq: i64, text: &str) -> HistoryRow {
    let mut unfinished = row(conversation, seq, "assistant", text);
    unfinished.item.complete = false;
    unfinished
}

fn page_with(read: &HistoryRead, from: HistoryFrom, live: Option<HistoryKey>) -> WholeHistoryPage {
    whole_history_page(read, from, &owns, live).unwrap()
}

/// The `(id, complete)` of a page's items.
fn flags(page: &WholeHistoryPage) -> Vec<(String, bool)> {
    page.items
        .iter()
        .map(|item| (item.id.clone(), item.complete))
        .collect()
}

#[test]
fn says_an_item_is_unfinished_only_of_the_one_that_may_still_be_written() {
    // Newest first: the newest is being written; behind it the messages of the turn's
    // earlier steps, which a reader never completes, and the prompt.
    let rows = vec![
        unfinished(1, 4, "Working on"),
        unfinished(1, 3, "Looking at the file."),
        row(1, 2, "user", "Run it"),
        unfinished(0, 9, "Cut off in the conversation before"),
    ];
    let live = Some(rows[0].key);
    let shown = page_with(&read(rows.clone(), None), HistoryFrom::Newest, live);
    assert_eq!(
        flags(&shown),
        [("0-9", true), ("1-2", true), ("1-3", true), ("1-4", false)]
            .map(|(id, complete)| (id.to_owned(), complete))
    );
    // Where nothing may be written (no item in the conversation now), none is unfinished.
    let none = page_with(&read(rows, None), HistoryFrom::Newest, None);
    assert!(none.items.iter().all(|item| item.complete));
    // A search that stopped inside the newest item's text names a part of it: it is the same item.
    let mut inside = unfinished(1, 4, "Working on");
    inside.key.part = 2;
    let found = page_with(&read(vec![inside], None), HistoryFrom::Newest, live);
    assert!(!found.items[0].complete);
}

#[test]
fn passes_over_an_assistant_s_message_with_no_words_and_goes_on_from_past_it() {
    // Newest first: the newest has just begun, and is being written; behind it a message of
    // a call alone, an answer's blank and the prompt.
    let rows = vec![
        unfinished(1, 5, ""),
        unfinished(1, 4, ""),
        row(1, 3, "assistant", "Done."),
        row(1, 2, "assistant", "  \n"),
        row(1, 1, "user", "Run it"),
    ];
    let live = Some(rows[0].key);
    let shown = page_with(&read(rows.clone(), None), HistoryFrom::Newest, live);
    assert_eq!(
        flags(&shown),
        [("1-1", true), ("1-3", true), ("1-5", false)]
            .map(|(id, complete)| (id.to_owned(), complete)),
        "the newest, being written, is shown even with no words"
    );
    // Where the newest is not being written, nothing of it is shown.
    let ended = page_with(&read(rows.clone(), None), HistoryFrom::Newest, None);
    assert_eq!(ids(&ended), ["1-1", "1-3"]);
    // A page that only passed over messages goes on from where its read did, and the page
    // after it from the place it began at.
    let resume = Some(rows[4].key);
    let blank = vec![unfinished(1, 4, ""), row(1, 2, "assistant", "")];
    let before = page_with(
        &read(blank.clone(), resume),
        HistoryFrom::Before(rows[0].key),
        None,
    );
    assert!(before.items.is_empty());
    assert_eq!((before.older, before.newer), (resume, Some(blank[0].key)));
    // What it passed over is not counted against the room it has for the rest.
    let big = "x".repeat(200_000);
    let mut many: Vec<HistoryRow> = (0..5)
        .rev()
        .map(|seq| row(1, seq * 2 + 1, "user", &big))
        .collect();
    for seq in 0..5 {
        many.insert(seq * 2, unfinished(1, 100 + seq as i64, ""));
    }
    let shown = page_with(
        &read(many.clone(), Some(many[9].key)),
        HistoryFrom::Newest,
        None,
    );
    assert_eq!(
        ids(&shown).len(),
        2,
        "two items of 200 KB fill a frame, as without them"
    );
    assert_eq!(
        shown.older,
        Some(many[4].key),
        "from the last it went through: the blank behind the oldest shown is not read again"
    );
}

#[test]
fn the_first_item_fits_a_page_whatever_it_holds() {
    // The reviewers' case and more: an id of a mebibyte, and as long a time; a
    // text a part long, each character one that JSON writes in six bytes.
    let huge = "h".repeat(1 << 20);
    let mut first = row(1, 0, "user", &"\u{0}".repeat(HISTORY_PART));
    first.item.id = huge.clone();
    first.item.at = Some(huge.clone());
    let shown = page(&read(vec![first], None), HistoryFrom::Newest);
    let size = serde_json::to_vec(&json!({ "ok": true, "history": shown }))
        .unwrap()
        .len();
    assert!(size <= PAGE_BYTES, "{size} bytes");
    let item = &shown.items[0];
    assert_eq!(
        item.text.chars().count(),
        HISTORY_PART,
        "its text as the read gave it"
    );
    let cut = format!("{}…", "h".repeat(SHOWN_FIELD));
    assert_eq!(
        (item.id.as_str(), item.at.as_deref()),
        (cut.as_str(), Some(cut.as_str()))
    );
}

#[test]
fn says_what_the_history_draws_of_an_item_and_nothing_of_one_its_reader_said_nothing_more_of() {
    // What the readers say of an item besides its words (chief-harness CHF-16): a call's
    // header, end, time and change, a thinking's kind; none for an item copied before.
    let mut call = row(1, 1, "tool", "ls: no-such-folder: No such file");
    call.item.detail = Some(cf_proto::ledger::ItemDetail {
        kind: "tool".into(),
        name: Some("Bash".into()),
        header: Some("$ ls no-such-folder".into()),
        status: Some("failed".into()),
        took_ms: Some(1_200),
        tokens: None,
        diff: None,
    });
    let mut thought = row(1, 0, "custom", "The file first.");
    thought.item.detail = Some(cf_proto::ledger::ItemDetail {
        kind: "thinking".into(),
        ..Default::default()
    });
    let rows = vec![call, thought, row(0, 9, "user", "go")];
    let shown = serde_json::to_value(page(&read(rows, None), HistoryFrom::Newest)).unwrap();
    assert_eq!(
        shown["items"],
        json!([
            { "conversation": 0, "row": 9, "id": "0-9", "role": "user", "text": "go", "complete": true, "at": "2026-10-10T10:09:00.000Z" },
            { "conversation": 1, "row": 100, "id": "1-0", "role": "custom", "text": "The file first.", "complete": true, "at": "2026-10-11T10:00:00.000Z", "kind": "thinking" },
            { "conversation": 1, "row": 101, "id": "1-1", "role": "tool", "text": "ls: no-such-folder: No such file", "complete": true, "at": "2026-10-11T10:01:00.000Z",
              "kind": "tool", "name": "Bash", "header": "$ ls no-such-folder", "status": "failed", "tookMs": 1_200 },
        ])
    );
}

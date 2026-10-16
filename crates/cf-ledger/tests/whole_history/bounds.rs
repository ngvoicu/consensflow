//! What one read of the history costs: the same whatever the history holds
//! besides the page. A page of the newest items takes as many of SQLite's
//! steps beside 2,000 conversations and 100,000 tools' outputs it leaves out as
//! beside none; and headings take a page's room as items do, so a page of
//! conversations that show no item (tools' outputs alone, left out) holds what
//! a page holds, and the next goes on past them, each heading once.

use std::collections::HashSet;
use std::time::Instant;

use cf_ledger::{
    HistoryAsk, HistoryEntry, HistoryFrom, HistoryRead, Ledger, MessageOpening, NewMember, NewNote,
    Scanned, MESSAGE_OPENING,
};
use serde_json::Value;

use super::reads::{ask, ids, ROOM};
use super::{id_of, item, ledger_in, project};

/// The chief's history: `earlier` conversations of one tool's output each,
/// then (after a member's `others`) the one it is in now, ten items of the
/// user's and `tools` tools' outputs after them. The ledger, the project, and
/// the chief.
fn history(
    dir: &tempfile::TempDir,
    earlier: usize,
    others: usize,
    tools: usize,
) -> (Ledger, i64, i64) {
    let mut ledger = ledger_in(dir);
    let project = project(&mut ledger);
    let chief = id_of(&project, "chief");
    for at in 0..earlier {
        let conversation = ledger
            .start_conversation(chief, "claude-code")
            .expect("a conversation");
        ledger
            .copy_transcript(
                conversation.id,
                &[item(&format!("t-{at}"), "tool", "ok")],
                0,
            )
            .expect("copied");
    }
    if others > 0 {
        let member = ledger
            .add_member(
                project.id,
                &NewMember {
                    agent: "zeus".into(),
                    harness: "pi".into(),
                    designer: false,
                    roles: vec!["worker".into()],
                    tier: "standard".into(),
                },
            )
            .expect("a member");
        for _ in 0..others {
            ledger
                .start_conversation(member.id, "pi")
                .expect("a conversation");
        }
    }
    let now = ledger
        .start_conversation(chief, "claude-code")
        .expect("a conversation");
    let mut copied: Vec<Value> = (0..10)
        .map(|at| item(&format!("u-{at}"), "user", &"u".repeat(100)))
        .collect();
    copied.extend((0..tools).map(|at| item(&format!("t-{at}"), "tool", "ok")));
    ledger.copy_transcript(now.id, &copied, 0).expect("copied");
    (ledger, project.id, chief)
}

/// The newest page, tools left out, with room for the ten items of the user's and their heading alone.
fn newest(ledger: &Ledger, project: i64) -> HistoryRead {
    let read = ledger
        .read_history(
            project,
            "chief",
            &ask(HistoryFrom::Newest, None, false, 10 * 760 + 1_000),
        )
        .expect("read");
    assert_eq!(ids(&read).len(), 10, "{:?}", read.scanned);
    read
}

#[test]
fn a_page_takes_the_same_steps_beside_2_000_conversations_and_100_000_tools_outputs_left_out() {
    let small = tempfile::tempdir().expect("a folder");
    let (ledger, project, _) = history(&small, 0, 0, 1_000);
    let alone = newest(&ledger, project).scanned;
    // Beside the chief's 2,000 earlier conversations and their log, a member's
    // 2,000 between them and the chief's now.
    let large = tempfile::tempdir().expect("a folder");
    let (ledger, project, _) = history(&large, 2_000, 2_000, 100_000);
    let read = newest(&ledger, project);
    let beside = read.scanned;
    assert_eq!(
        (
            read.conversations[0].tools,
            read.conversations[0].more_tools
        ),
        (1_000, true),
        "counted so far, and said to be more"
    );
    // The page beside them went as far as the earlier conversation's heading,
    // and no further: one conversation more, one row no more.
    assert_eq!(
        (beside.conversations, beside.rows, beside.bytes),
        (alone.conversations + 1, alone.rows, alone.bytes),
        "{alone:?} {beside:?}"
    );
    // SQLite's steps are the same for the same rows and plan: beside it all,
    // the page takes those of the one heading more and no others.
    assert!(
        beside.steps < alone.steps + 1_000,
        "the steps grew with what the page leaves out: {alone:?} {beside:?}"
    );
}

#[test]
fn a_read_never_loads_the_role_a_conversation_was_given() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger = ledger_in(&dir);
    let project = project(&mut ledger);
    let chief = id_of(&project, "chief");
    // The first instructions of a harness that keeps them, as long as a chief's: 12 KB.
    let role = "Read the board before anything else.\n".repeat(330);
    for _ in 0..50 {
        let conversation = ledger
            .start_conversation(chief, "claude-code")
            .expect("a conversation");
        ledger
            .set_conversation_role_text(conversation.id, &role)
            .expect("given its role");
    }
    // A read that loaded one value as long as the role whole would fail.
    ledger.limit_value_length(8_000).expect("held");
    let page = ledger
        .read_history(
            project.id,
            "chief",
            &ask(HistoryFrom::Newest, None, false, ROOM),
        )
        .expect("a page");
    assert_eq!(page.entries.len(), 50);
    let searched = ledger
        .read_history(
            project.id,
            "chief",
            &ask(HistoryFrom::Newest, Some("nowhere"), false, ROOM),
        )
        .expect("a search");
    assert_eq!(
        (searched.scanned.conversations, searched.resume),
        (50, None)
    );
}

#[test]
fn a_message_s_opening_is_its_first_bytes_cut_at_a_character_and_its_body_is_never_loaded_whole() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger = ledger_in(&dir);
    let project = project(&mut ledger);
    // A megabyte, a character of four bytes across where the opening ends.
    let before = format!("{}x", "é".repeat(MESSAGE_OPENING / 2 - 2));
    let body = format!("{before}{}", "🙂".repeat(250_000));
    let note = ledger
        .note(
            project.id,
            &NewNote {
                from: Some("human".into()),
                to: "chief".into(),
                body,
                task: None,
            },
        )
        .expect("a note");
    // A read that loaded the body whole would fail.
    ledger.limit_value_length(64 * 1024).expect("held");
    let opening = ledger
        .message_opening(note.id)
        .expect("read")
        .expect("there");
    assert_eq!(
        opening,
        MessageOpening {
            id: note.id,
            project_id: project.id,
            recipient_id: id_of(&project, "chief"),
            sender: Some("human".into()),
            kind: "note".into(),
            task_number: None,
            opening: before,
        }
    );
    assert!(ledger
        .message_opening(note.id + 1_000)
        .expect("read")
        .is_none());
}

#[test]
fn headings_fill_a_page_as_items_do_and_the_next_page_goes_on_past_them_each_once() {
    // A thousand conversations of tools' outputs alone, left out: each shows as
    // its heading. A room of 20,000 bytes, a twentieth or so of them a page.
    let dir = tempfile::tempdir().expect("a folder");
    let (ledger, project, _) = history(&dir, 1_000, 0, 0);
    let room = 20_000;
    let mut headed: Vec<i64> = Vec::new();
    let mut items = 0;
    let mut pages = 0;
    let mut from = HistoryFrom::Newest;
    loop {
        let read = ledger
            .read_history(project, "chief", &ask(from, None, false, room))
            .expect("read");
        pages += 1;
        assert!(pages < 200, "no end");
        assert!(
            read.conversations.len() < 40,
            "a page heads what its room holds: {}",
            read.conversations.len()
        );
        for entry in &read.entries {
            match entry {
                HistoryEntry::Heading(key) => headed.push(key.conversation),
                HistoryEntry::Item(_) => items += 1,
            }
        }
        let Some(place) = read.resume else { break };
        from = HistoryFrom::Before(place);
    }
    assert_eq!(items, 10);
    assert!(pages > 20, "{pages} pages");
    let once: HashSet<i64> = headed.iter().copied().collect();
    assert_eq!(
        (headed.len(), once.len()),
        (1_000, 1_000),
        "every heading once"
    );
    let newest = ledger
        .read_history(project, "chief", &ask(HistoryFrom::Newest, None, false, 1))
        .expect("read")
        .entries[0]
        .key();
    assert!(
        !once.contains(&newest.conversation),
        "the one with items shows them instead"
    );
}

/// What a page takes for a chief of 50,000 items and 200 MB of text in five
/// conversations, two items in five tools' outputs, a 24 MiB item of an
/// agent's in the newest, and 10,000 conversations of tools' outputs alone
/// before them all. The daemon reads on its own thread (the ledger holds its
/// file alone, one connection), so this is how long it waits for each. Run in
/// release: `cargo test --release -p cf-ledger --test whole_history -- --ignored --nocapture`.
#[test]
#[ignore = "builds a 230 MB ledger; run by hand, in release"]
fn a_page_of_a_long_history_takes_what_a_page_takes() {
    let dir = tempfile::tempdir().expect("a folder");
    let started = Instant::now();
    let (mut ledger, project, chief) = history(&dir, 10_000, 0, 0);
    let mut conversations = Vec::new();
    for _ in 0..5 {
        let conversation = ledger
            .start_conversation(chief, "claude-code")
            .expect("a conversation")
            .id;
        conversations.push(conversation);
        for batch in 0..10 {
            let copied: Vec<Value> = (0..1_000)
                .map(|at| {
                    let n = batch * 1_000 + at;
                    if n % 5 < 2 {
                        item(
                            &format!("t-{n}"),
                            "tool",
                            &format!("{n} {}", "t".repeat(8_000)),
                        )
                    } else {
                        item(
                            &format!("i-{n}"),
                            "user",
                            &format!("{n} word {}", "w".repeat(1_320)),
                        )
                    }
                })
                .collect();
            ledger
                .copy_transcript(conversation, &copied, i64::from(batch) * 1_000)
                .expect("copied");
        }
    }
    let newest = conversations[4];
    let long = format!("{} the NEEDLE", "a".repeat(24 * 1024 * 1024));
    ledger
        .copy_transcript(newest, &[item("long", "assistant", &long)], 10_000)
        .expect("copied");
    println!(
        "built: 10,000 conversations, 50,000 items and a 24 MiB one in {:?}",
        started.elapsed()
    );
    let time = |what: &str, ask: HistoryAsk<'_>| {
        let started = Instant::now();
        let read = ledger.read_history(project, "chief", &ask).expect("read");
        let took = started.elapsed();
        let Scanned {
            conversations,
            rows,
            parts,
            bytes,
            fields,
            steps,
        } = read.scanned;
        println!(
            "{what}: {took:?}, {} entries; {conversations} conversations, {rows} rows, {parts} part lookups, {bytes} bytes of text and {fields} of fields, {steps} steps gone through",
            read.entries.len(),
        );
        assert!(took.as_millis() < 1_000, "{what} took {took:?}");
        read
    };
    let first = time(
        "newest, tools off",
        ask(HistoryFrom::Newest, None, false, ROOM),
    );
    time(
        "newest, tools on",
        ask(HistoryFrom::Newest, None, true, ROOM),
    );
    let middle = first.resume.expect("more");
    time("older", ask(HistoryFrom::Before(middle), None, false, ROOM));
    // The words of the 50,000 items, searched from before the 24 MiB one.
    let before_long = HistoryFrom::Before(first.entries[0].key());
    time(
        "a word in no item",
        ask(before_long, Some("nowhere"), false, ROOM),
    );
    time(
        "a word in every item",
        ask(before_long, Some("WORD"), false, ROOM),
    );
    let inside = time(
        "the 24 MiB item's last words, first read",
        ask(HistoryFrom::Newest, Some("needle"), false, ROOM),
    );
    time(
        "the 24 MiB item's last words, from where the first stopped",
        ask(
            HistoryFrom::Before(inside.resume.expect("inside")),
            Some("needle"),
            false,
            ROOM,
        ),
    );
    let oldest_big = cf_ledger::HistoryKey {
        conversation: conversations[0],
        row: 0,
        part: 0,
    };
    time(
        "a page of the 10,000 conversations of tools' outputs, headings alone",
        ask(HistoryFrom::Before(oldest_big), None, false, ROOM),
    );
    let long_row = first.entries[0].key().row;
    let started = Instant::now();
    let last = ledger
        .read_history_part(project, "chief", newest, long_row, 393 * 64_000)
        .expect("a part");
    println!(
        "the 24 MiB item's last part: {:?} ({} bytes)",
        started.elapsed(),
        last.text.len()
    );
    let started = Instant::now();
    let mut parts = 0;
    let mut from = Some(0);
    while let Some(at) = from {
        from = ledger
            .read_history_part(project, "chief", newest, long_row, at)
            .expect("a part")
            .next;
        parts += 1;
    }
    println!(
        "the 24 MiB item whole, {parts} parts: {:?}",
        started.elapsed()
    );
}

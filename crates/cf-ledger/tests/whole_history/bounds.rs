//! What one read of the history costs: the same whatever the history holds
//! besides the page. A page of the newest items takes as many of SQLite's
//! steps beside 100,000 tools' outputs it leaves out as beside none; a
//! conversation that shows no item (tools' outputs alone, left out) is one
//! lookup of its id, charged, and shows nothing, so a read goes on past
//! thousands of them to the items before them, as far as its bound, and the
//! next goes on from where it stopped, each item once.

use std::time::Instant;

use cf_ledger::{
    HistoryAsk, HistoryFrom, HistoryRead, Ledger, MessageOpening, NewMember, NewNote, Scanned,
    HISTORY_SCAN_BYTES, MESSAGE_OPENING,
};
use serde_json::Value;

use super::reads::{ask, ids, LOADED_AT_MOST, ROOM};
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
                    model: None,
                    effort: None,
                    model_name: Some("zeus".into()),
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

/// The newest page, tools left out, with room for the ten items of the user's alone.
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

/// The most steps SQLite's machine takes for a conversation that shows no
/// item: finding its id and finding it has no item to show, measured at 28.
const STEPS_AN_EMPTY_CONVERSATION: usize = 40;

#[test]
fn a_page_takes_the_same_steps_beside_100_000_tools_outputs_left_out_and_one_lookup_a_conversation_with_none_shown(
) {
    let small = tempfile::tempdir().expect("a folder");
    let (ledger, project, _) = history(&small, 0, 0, 1_000);
    let alone = newest(&ledger, project).scanned;
    // Beside 100,000 tools' outputs and a member's 2,000 conversations, which
    // are not the chief's: the same rows, the same bytes, the same steps.
    let large = tempfile::tempdir().expect("a folder");
    let (ledger, project, _) = history(&large, 0, 2_000, 100_000);
    let beside = newest(&ledger, project).scanned;
    assert_eq!(
        (
            beside.conversations,
            beside.rows,
            beside.bytes,
            beside.fields
        ),
        (alone.conversations, alone.rows, alone.bytes, alone.fields),
        "{alone:?} {beside:?}"
    );
    assert!(
        beside.steps < alone.steps + 1_000,
        "the steps grew with what the page leaves out: {alone:?} {beside:?}"
    );
    // Beside the chief's 2,000 earlier conversations of a tool's output each,
    // the page has room for one more item and looks for it through them all:
    // each is one lookup more and no row, and there is nothing older.
    let earlier = tempfile::tempdir().expect("a folder");
    let (ledger, project, _) = history(&earlier, 2_000, 0, 1_000);
    let read = newest(&ledger, project);
    let past = read.scanned;
    assert_eq!(
        (past.conversations, past.rows, past.bytes, past.fields),
        (
            alone.conversations + 2_000,
            alone.rows,
            alone.bytes,
            alone.fields
        ),
        "{alone:?} {past:?}"
    );
    assert!(
        past.steps <= alone.steps + 2_000 * STEPS_AN_EMPTY_CONVERSATION,
        "{alone:?} {past:?}"
    );
    assert_eq!((read.resume, read.bounded), (None, false));
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
    assert_eq!(
        (page.entries.len(), page.scanned.conversations, page.resume),
        (0, 50, None)
    );
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
fn pages_of_one_item_go_across_conversations_that_show_none_and_show_each_item_once_in_order() {
    // A thousand conversations of tools' outputs alone, left out, and every
    // hundredth one with an item of the user's too. A room of one item a page: each page goes on past the conversations between
    // items, and every item comes once, in the order it was copied.
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger = ledger_in(&dir);
    let project = project(&mut ledger);
    let chief = id_of(&project, "chief");
    let mut copied = Vec::new();
    for at in 0..1_000 {
        let conversation = ledger
            .start_conversation(chief, "claude-code")
            .expect("a conversation");
        let mut items = vec![item(&format!("t-{at}"), "tool", "ok")];
        if at % 100 == 0 {
            items.push(item(&format!("u-{at}"), "user", "said"));
            copied.push(format!("u-{at}"));
        }
        ledger
            .copy_transcript(conversation.id, &items, 0)
            .expect("copied");
    }
    let mut shown: Vec<String> = Vec::new();
    let mut pages = 0;
    let mut from = HistoryFrom::Newest;
    loop {
        let read = ledger
            .read_history(project.id, "chief", &ask(from, None, false, 1))
            .expect("read");
        pages += 1;
        assert!(pages < 50, "no end");
        assert!(read.entries.len() <= 1, "{} items", read.entries.len());
        shown.extend(ids(&read));
        let Some(place) = read.resume else { break };
        from = HistoryFrom::Before(place);
    }
    shown.reverse();
    assert_eq!(shown, copied);
    assert_eq!(pages, copied.len(), "the last finds nothing older");
}

/// The most steps SQLite's machine takes for each conversation, row and part a
/// read comes to: 170 at the most when a conversation's heading read the log,
/// measured, and fewer since.
const STEPS_A_ROW: usize = 200;

/// What a page takes for a chief of 50,000 items and 200 MB of text in five
/// conversations, two items in five tools' outputs, a 24 MiB item of an
/// agent's in the newest, and 10,000 conversations of tools' outputs alone
/// before them all. The daemon reads on its own thread (the ledger holds its
/// file alone, one connection), so this is how long it waits for each, printed
/// for whoever runs it; what it asserts is what each read went through (its
/// charge, its steps, no long value loaded), which no machine's load moves.
/// Run in release: `cargo test --release -p cf-ledger --test whole_history -- --ignored --nocapture`.
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
    // A read that loaded a long item's text whole would fail, the value too long.
    ledger.limit_value_length(LOADED_AT_MOST).expect("held");
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
        // What the read went through is what it was let go through, and its
        // steps are as many as the rows and parts it came to, whatever the
        // history holds beside them: the same on any machine, as the time is
        // not (in debug, or beside a build, it passes a second).
        assert!(
            read.charged <= HISTORY_SCAN_BYTES,
            "{what} was charged {} bytes",
            read.charged
        );
        assert!(
            steps <= STEPS_A_ROW * (conversations + rows + parts).max(1),
            "{what} took {steps} steps for {conversations} conversations, {rows} rows and {parts} parts"
        );
        read
    };
    // A read that fills its page is not cut short by what it may go through:
    // that is a charge that grew, or a bound that fell, where the page was
    // held to what it takes.
    let fills = |what: &str, read: &HistoryRead| {
        assert!(
            !read.bounded,
            "{what} stopped at what a read may go through"
        );
    };
    let first = time(
        "newest, tools off",
        ask(HistoryFrom::Newest, None, false, ROOM),
    );
    fills("newest, tools off", &first);
    let tools = time(
        "newest, tools on",
        ask(HistoryFrom::Newest, None, true, ROOM),
    );
    fills("newest, tools on", &tools);
    let middle = first.resume.expect("more");
    let older = time("older", ask(HistoryFrom::Before(middle), None, false, ROOM));
    fills("older", &older);
    // The words of the 50,000 items, searched from before the 24 MiB one.
    let before_long = HistoryFrom::Before(first.entries[0].key);
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
    // The 10,000 conversations of tools' outputs show nothing: the read goes
    // through as many as its bound holds, and says where the next goes on.
    let none_shown = time(
        "the 10,000 conversations of tools' outputs, nothing shown",
        ask(HistoryFrom::Before(oldest_big), None, false, ROOM),
    );
    // Before the five, the ten items of the conversation the chief was in then,
    // and none of the 10,000 before it: the read goes through as many as its
    // bound holds, and says where the next goes on.
    assert_eq!(ids(&none_shown).len(), 10, "{:?}", none_shown.scanned);
    assert!(
        none_shown.bounded && none_shown.resume.is_some(),
        "{:?}",
        none_shown.scanned
    );
    let long_row = first.entries[0].key.row;
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

#[test]
fn a_read_through_conversations_that_show_no_item_shows_none_and_reads_none_of_their_fields() {
    // CHF-11: 10,000 conversations of a tool's output each, left out, before the
    // one the chief is in now. A read from the oldest item of that one shows
    // nothing of them: it reads only each one's id, charged less than a
    // heading's lookups and fields were (4,096 and 2,304 bytes a conversation),
    // so it goes through more of them before its bound.
    let dir = tempfile::tempdir().expect("a folder");
    let (ledger, project, _) = history(&dir, 10_000, 0, 0);
    let newest = ledger
        .read_history(
            project,
            "chief",
            &ask(HistoryFrom::Newest, None, false, 1_000_000),
        )
        .expect("read");
    let oldest = newest.entries.last().expect("an item").key;
    let read = ledger
        .read_history(
            project,
            "chief",
            &ask(HistoryFrom::Before(oldest), None, false, 1_000_000),
        )
        .expect("read");
    assert!(read.entries.is_empty(), "{} entries", read.entries.len());
    assert!(read.bounded && read.resume.is_some(), "{:?}", read.scanned);
    assert_eq!(read.scanned.fields, 0, "{:?}", read.scanned);
    assert!(
        read.scanned.conversations > HISTORY_SCAN_BYTES / (4_096 + 2_304),
        "{:?}",
        read.scanned
    );
}

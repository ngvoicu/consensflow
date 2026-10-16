//! How a read of the history goes: from a place that does not move, so the
//! pages never repeat or skip an item, whatever is added or grows meanwhile;
//! through a page and no more, and never through a tool's text it leaves out;
//! a search through no more than its bound, and on from where it stopped until
//! it finds; and a long item a part at a time, each part the participant's own.

use std::time::Instant;

use cf_ledger::{
    HistoryAsk, HistoryFrom, HistoryRead, Ledger, NewMember, HISTORY_PART, HISTORY_SCAN_BYTES,
    PAGE_BYTES,
};
use serde_json::{json, Value};

use super::{id_of, item, ledger_in, project};

/// A page's room, as the viewer asks: the frame, less what the page says besides.
const ROOM: usize = PAGE_BYTES - 1_024;

/// A project whose chief has one conversation: the ledger, the project and the conversation.
fn chief(dir: &tempfile::TempDir) -> (Ledger, i64, i64) {
    let mut ledger = ledger_in(dir);
    let project = project(&mut ledger);
    let conversation = ledger
        .start_conversation(id_of(&project, "chief"), "claude-code")
        .expect("a conversation");
    (ledger, project.id, conversation.id)
}

fn ask(from: HistoryFrom, find: Option<&str>, tools: bool, room: usize) -> HistoryAsk<'_> {
    HistoryAsk {
        from,
        find,
        tools,
        room,
        scan: HISTORY_SCAN_BYTES,
    }
}

/// The ids of the items a read gave, in the order it gave them.
fn ids(read: &HistoryRead) -> Vec<String> {
    read.rows.iter().map(|row| row.item.id.clone()).collect()
}

/// Every page older than `first`, each read from where the one before stopped:
/// the ids of their items, newest first. A read that never gets to the end is
/// stopped at `most` reads.
fn older(
    ledger: &Ledger,
    project: i64,
    first: &HistoryRead,
    find: Option<&str>,
    most: usize,
) -> Vec<String> {
    let mut seen = Vec::new();
    let mut resume = first.resume;
    for _ in 0..most {
        let Some(place) = resume else {
            return seen;
        };
        let read = ledger
            .read_history(
                project,
                "chief",
                &ask(HistoryFrom::Before(place), find, false, ROOM),
            )
            .expect("read");
        seen.extend(ids(&read));
        resume = read.resume;
    }
    panic!("no end after {most} reads: {seen:?}");
}

/// `n` items of the user's, each `size` bytes, `i-0` the first.
fn items(n: usize, size: usize) -> Vec<Value> {
    (0..n)
        .map(|at| {
            item(
                &format!("i-{at}"),
                "user",
                &format!("{at:>6}{}", "x".repeat(size - 6)),
            )
        })
        .collect()
}

#[test]
fn pages_go_on_from_the_oldest_item_shown_whatever_is_added_or_grows_meanwhile() {
    let dir = tempfile::tempdir().expect("a folder");
    let (mut ledger, project, conversation) = chief(&dir);
    // Thirty items of 50 KB: ten or so to a page.
    let mut copied = items(30, 50_000);
    ledger
        .copy_transcript(conversation, &copied, 0)
        .expect("copied");
    let first = ledger
        .read_history(
            project,
            "chief",
            &ask(HistoryFrom::Newest, None, false, ROOM),
        )
        .expect("read");
    let shown = ids(&first);
    assert!(shown.len() > 3 && shown.len() < 30, "{shown:?}");
    // Meanwhile the newest item grows past what a page shows of it, an item
    // the page shows grows too, and two more come.
    copied[29]["text"] = json!("y".repeat(600_000));
    copied[25]["text"] = json!("z".repeat(530_000));
    copied.extend(items(32, 50_000).split_off(30));
    ledger
        .copy_transcript(conversation, &copied, 0)
        .expect("copied");
    let rest = older(&ledger, project, &first, None, 50);
    let mut every: Vec<String> = shown.into_iter().chain(rest).collect();
    every.reverse();
    let expected: Vec<String> = (0..30).map(|at| format!("i-{at}")).collect();
    assert_eq!(
        every, expected,
        "each item once, in order, none of the new ones"
    );
}

#[test]
fn an_item_whose_place_repeats_another_s_is_neither_repeated_nor_skipped() {
    let dir = tempfile::tempdir().expect("a folder");
    let (mut ledger, project, conversation) = chief(&dir);
    ledger
        .copy_transcript(conversation, &items(4, 10), 0)
        .expect("copied");
    // A record that shrank, copied again from its second item: `x` takes the
    // place `i-1` has, which keeps it, its text unchanged.
    ledger
        .copy_transcript(conversation, &[item("x", "user", "in i-1's place")], 1)
        .expect("copied");
    // A room that takes one row: a page between every two items.
    let first = ledger
        .read_history(project, "chief", &ask(HistoryFrom::Newest, None, false, 1))
        .expect("read");
    let mut seen = ids(&first);
    let mut resume = first.resume;
    // Five items, a read each: more reads than that never end.
    for _ in 0..10 {
        let Some(place) = resume else { break };
        let read = ledger
            .read_history(
                project,
                "chief",
                &ask(HistoryFrom::Before(place), None, false, 1),
            )
            .expect("read");
        seen.extend(ids(&read));
        resume = read.resume;
    }
    assert_eq!(seen, ["i-3", "i-2", "x", "i-1", "i-0"]);
    // And the other way, after each place.
    let oldest = ledger
        .read_history(
            project,
            "chief",
            &ask(HistoryFrom::Newest, None, false, ROOM),
        )
        .expect("read")
        .rows
        .last()
        .expect("the oldest")
        .key;
    let mut after = vec!["i-0".to_owned()];
    let mut place = Some(oldest);
    for _ in 0..10 {
        let Some(from) = place else { break };
        let read = ledger
            .read_history(
                project,
                "chief",
                &ask(HistoryFrom::After(from), None, false, 1),
            )
            .expect("read");
        after.extend(ids(&read));
        place = read.resume;
    }
    assert_eq!(after, ["i-0", "i-1", "x", "i-2", "i-3"]);
}

#[test]
fn a_read_goes_through_a_page_and_no_more_and_never_through_a_tool_s_text_it_leaves_out() {
    let dir = tempfile::tempdir().expect("a folder");
    let (mut ledger, project, conversation) = chief(&dir);
    // A thousand items: the user's of 100 bytes, and between them tools' outputs of 100 KB.
    let copied: Vec<Value> = (0..1_000)
        .map(|at| {
            if at % 2 == 0 {
                item(&format!("u-{at}"), "user", &"u".repeat(100))
            } else {
                item(&format!("t-{at}"), "tool", &"t".repeat(100_000))
            }
        })
        .collect();
    ledger
        .copy_transcript(conversation, &copied, 0)
        .expect("copied");
    let without = ledger
        .read_history(
            project,
            "chief",
            &ask(HistoryFrom::Newest, None, false, ROOM),
        )
        .expect("read");
    assert_eq!(without.rows.len(), 500, "the user's items all fit");
    assert_eq!(
        (without.scanned.rows, without.scanned.bytes),
        (500, 500 * 100),
        "no tool's row, nor its text"
    );
    assert_eq!(without.conversations[0].tools, 500, "and they are counted");
    let with = ledger
        .read_history(
            project,
            "chief",
            &ask(HistoryFrom::Newest, None, true, ROOM),
        )
        .expect("read");
    assert!(with.scanned.rows < 20, "{:?}", with.scanned);
    assert!(
        with.scanned.bytes < ROOM + 100_000,
        "a page and one item more at most: {:?}",
        with.scanned
    );
    assert!(with.resume.is_some());
}

#[test]
fn a_search_goes_through_no_more_than_its_bound_and_older_goes_on_from_there_to_what_it_finds() {
    let dir = tempfile::tempdir().expect("a folder");
    let (mut ledger, project, conversation) = chief(&dir);
    // The oldest item holds the words, in a case of its own; 200 items of 100 KB come after it.
    let mut copied = vec![item("found", "user", "Ünïcode says Héllo")];
    copied.extend((1..=200).map(|at| item(&format!("i-{at}"), "user", &"x".repeat(100_000))));
    ledger
        .copy_transcript(conversation, &copied, 0)
        .expect("copied");
    let find = Some("üNÏCODE SAYS hÉLLO");
    let first = ledger
        .read_history(
            project,
            "chief",
            &ask(HistoryFrom::Newest, find, false, ROOM),
        )
        .expect("read");
    assert!(
        first.rows.is_empty() && first.bounded,
        "{:?}",
        first.scanned
    );
    assert!(
        first.scanned.bytes >= HISTORY_SCAN_BYTES
            && first.scanned.bytes < HISTORY_SCAN_BYTES + 100_000,
        "{:?}",
        first.scanned
    );
    assert_eq!(older(&ledger, project, &first, find, 5), ["found"]);
}

#[test]
fn a_long_item_shows_a_part_its_first_or_where_a_search_found_and_reads_whole_a_part_at_a_time() {
    let dir = tempfile::tempdir().expect("a folder");
    let (mut ledger, project, conversation) = chief(&dir);
    let whole = format!(
        "Ünïcode {}🙂 and at the end, the NEEDLE",
        "é".repeat(HISTORY_PART * 3)
    );
    ledger
        .copy_transcript(conversation, &[item("long", "assistant", &whole)], 0)
        .expect("copied");
    let read = ledger
        .read_history(
            project,
            "chief",
            &ask(HistoryFrom::Newest, None, false, ROOM),
        )
        .expect("read");
    let row = &read.rows[0];
    let cut = row.cut.expect("a part");
    assert_eq!((cut.from, cut.bytes), (0, whole.len()));
    assert_eq!(row.item.text.chars().count(), HISTORY_PART);
    assert!(whole.starts_with(&row.item.text));
    // A search shows the part where it found its words.
    let found = ledger
        .read_history(
            project,
            "chief",
            &ask(HistoryFrom::Newest, Some("needle"), false, ROOM),
        )
        .expect("read");
    let row = &found.rows[0];
    assert!(row.cut.expect("a part").from > 0);
    assert!(
        row.item.text.ends_with("the NEEDLE"),
        "{}",
        &row.item.text[..40]
    );
    // The whole of it, a part at a time.
    let mut read_whole = String::new();
    let mut from = Some(0);
    for _ in 0..10 {
        let Some(at) = from else { break };
        let part = ledger
            .read_history_part(project, "chief", conversation, "long", at)
            .expect("a part");
        assert_eq!(part.from, at);
        read_whole.push_str(&part.text);
        from = part.next;
    }
    assert!(from.is_none(), "every part read");
    assert_eq!(read_whole, whole);
    // Another participant's history holds no item of the chief's.
    ledger
        .add_member(
            project,
            &NewMember {
                agent: "zeus".into(),
                harness: "pi".into(),
                designer: false,
                roles: vec!["worker".into()],
                tier: "standard".into(),
            },
        )
        .expect("a member");
    let refused = ledger
        .read_history_part(project, "zeus", conversation, "long", 0)
        .expect_err("not zeus's");
    assert!(refused.to_string().contains("no item long"), "{refused}");
}

/// What a page takes for a chief of 50,000 items and 200 MB of text, in five
/// conversations, two items in five tools' outputs. The daemon reads on its
/// own thread (the ledger holds its file alone, one connection), so this is
/// how long it waits while the page is read. Run in release:
/// `cargo test --release -p cf-ledger --test whole_history -- --ignored --nocapture`.
#[test]
#[ignore = "builds a 200 MB ledger; run by hand, in release"]
fn a_page_of_a_long_history_takes_what_a_page_takes() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger = ledger_in(&dir);
    let project = project(&mut ledger);
    let chief = id_of(&project, "chief");
    let started = Instant::now();
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
    println!("built: 50,000 items in {:?}", started.elapsed());
    let time = |what: &str, ask: HistoryAsk<'_>| {
        let started = Instant::now();
        let read = ledger
            .read_history(project.id, "chief", &ask)
            .expect("read");
        let took = started.elapsed();
        println!(
            "{what}: {took:?}, {} rows shown, {} rows and {} bytes gone through",
            read.rows.len(),
            read.scanned.rows,
            read.scanned.bytes
        );
        assert!(took.as_millis() < 1_000, "{what} took {took:?}");
        read
    };
    let newest = time(
        "newest, tools off",
        ask(HistoryFrom::Newest, None, false, ROOM),
    );
    time(
        "newest, tools on",
        ask(HistoryFrom::Newest, None, true, ROOM),
    );
    let middle = newest.resume.expect("more");
    time("older", ask(HistoryFrom::Before(middle), None, false, ROOM));
    time(
        "a word in no item",
        ask(HistoryFrom::Newest, Some("nowhere"), false, ROOM),
    );
    time(
        "a word in every item",
        ask(HistoryFrom::Newest, Some("WORD"), false, ROOM),
    );
    let started = Instant::now();
    ledger
        .read_history_part(project.id, "chief", conversations[0], "i-2", 0)
        .expect("a part");
    println!("a part: {:?}", started.elapsed());
}

//! How a read of the history goes: from a place that does not move, so the
//! pages never repeat or skip an item, whatever is added, grows or is copied
//! again meanwhile; through a page and no more, and never through a tool's
//! text it leaves out; a search through no more than its bound, inside a long
//! item too, and on from where it stopped until it finds; and a long item a
//! part at a time, each part read alone and the participant's own.

use cf_ledger::{
    HistoryAsk, HistoryEntry, HistoryFrom, HistoryRead, Ledger, NewMember, HISTORY_PART,
    HISTORY_SCAN_BYTES, PAGE_BYTES,
};
use serde_json::{json, Value};

use super::pages::{fetched, SLACK};
use super::{id_of, item, ledger_in, project};

/// A page's room, as the viewer asks: the frame, less what the page says besides.
pub(super) const ROOM: usize = PAGE_BYTES - 1_024;

/// A project whose chief has one conversation: the ledger, the project and the conversation.
pub(super) fn chief(dir: &tempfile::TempDir) -> (Ledger, i64, i64) {
    let mut ledger = ledger_in(dir);
    let project = project(&mut ledger);
    let conversation = ledger
        .start_conversation(id_of(&project, "chief"), "claude-code")
        .expect("a conversation");
    (ledger, project.id, conversation.id)
}

pub(super) fn ask(
    from: HistoryFrom,
    find: Option<&str>,
    tools: bool,
    room: usize,
) -> HistoryAsk<'_> {
    HistoryAsk {
        from,
        find,
        tools,
        room,
        scan: HISTORY_SCAN_BYTES,
    }
}

/// The ids of the items a read gave, in the order it gave them.
pub(super) fn ids(read: &HistoryRead) -> Vec<String> {
    read.entries
        .iter()
        .filter_map(|entry| match entry {
            HistoryEntry::Item(row) => Some(row.item.id.clone()),
            HistoryEntry::Heading(_) => None,
        })
        .collect()
}

/// The row the chief's item `id` stands at, as a page says it: found on the
/// pages read back from the most recent, an entry a page.
pub(super) fn row_of(ledger: &Ledger, project: i64, id: &str) -> i64 {
    let mut from = HistoryFrom::Newest;
    for _ in 0..100 {
        let read = ledger
            .read_history(project, "chief", &ask(from, None, true, 1))
            .expect("read");
        for entry in &read.entries {
            if let HistoryEntry::Item(row) = entry {
                if row.item.id == id {
                    return row.key.row;
                }
            }
        }
        from = HistoryFrom::Before(read.resume.expect("the item, before the end"));
    }
    panic!("no item {id} in 100 entries");
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
fn a_record_that_shrank_copied_again_with_a_shown_item_changed_repeats_nothing_and_skips_nothing() {
    // The reviewers' case: the most recent page shows the newest items; the
    // window's record shrinks (a resumed window rewrote it) and is copied again
    // from its start, the newest item changed and now first, so that its place
    // in the record is the oldest; Older then goes on.
    let dir = tempfile::tempdir().expect("a folder");
    let (mut ledger, project, conversation) = chief(&dir);
    ledger
        .copy_transcript(conversation, &items(21, 100), 0)
        .expect("copied");
    let first = ledger
        .read_history(
            project,
            "chief",
            &ask(HistoryFrom::Newest, None, false, 8 * 760),
        )
        .expect("read");
    let shown = ids(&first);
    assert_eq!(shown.first().map(String::as_str), Some("i-20"));
    assert!(shown.len() > 2 && shown.len() < 21, "{shown:?}");
    let mut rewritten = items(21, 100);
    rewritten[20]["text"] = json!("i-20, changed");
    let newest = rewritten.remove(20);
    ledger
        .copy_transcript(conversation, &[newest], 0)
        .expect("copied again");
    let rest = older(&ledger, project, &first, None, 30);
    let mut every: Vec<String> = shown.into_iter().chain(rest).collect();
    every.reverse();
    let expected: Vec<String> = (0..21).map(|at| format!("i-{at}")).collect();
    assert_eq!(every, expected, "each item once, where it was first copied");
}

#[test]
fn an_item_copied_into_another_s_place_stands_where_it_was_first_copied_neither_repeated_nor_skipped(
) {
    let dir = tempfile::tempdir().expect("a folder");
    let (mut ledger, project, conversation) = chief(&dir);
    ledger
        .copy_transcript(conversation, &items(4, 10), 0)
        .expect("copied");
    // A record that shrank, copied again from its second item: `x` takes the
    // place `i-1` has, which keeps it, its text unchanged. `x` was copied last.
    ledger
        .copy_transcript(conversation, &[item("x", "user", "in i-1's place")], 1)
        .expect("copied");
    // A room that takes one entry: a page between every two items.
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
    assert_eq!(seen, ["x", "i-3", "i-2", "i-1", "i-0"]);
    // And the other way, after each place.
    let oldest = ledger
        .read_history(
            project,
            "chief",
            &ask(HistoryFrom::Newest, None, false, ROOM),
        )
        .expect("read")
        .entries
        .last()
        .expect("the oldest")
        .key();
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
    assert_eq!(after, ["i-0", "i-1", "i-2", "i-3", "x"]);
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
    assert_eq!(ids(&without).len(), 500, "the user's items all fit");
    assert_eq!(
        (without.scanned.rows, without.scanned.bytes),
        (500, 500 * 100),
        "no tool's row, nor its text"
    );
    assert_eq!(
        (
            without.conversations[0].tools,
            without.conversations[0].more_tools
        ),
        (500, false),
        "and they are counted"
    );
    let with = ledger
        .read_history(
            project,
            "chief",
            &ask(HistoryFrom::Newest, None, true, ROOM),
        )
        .expect("read");
    assert!(with.scanned.rows < 40, "{:?}", with.scanned);
    let shown: usize = with
        .entries
        .iter()
        .map(|entry| match entry {
            HistoryEntry::Item(row) => row.item.text.len(),
            HistoryEntry::Heading(_) => 0,
        })
        .sum();
    assert_eq!(
        with.scanned.bytes, shown,
        "the text it shows and no more: an item the page has no room for is not read"
    );
    assert!(with.resume.is_some());
}

#[test]
fn a_search_goes_through_no_more_than_its_bound_and_older_goes_on_from_there_to_what_it_finds() {
    // Items of a part or less, each read whole; and items of two parts, each
    // read a part at a time.
    search_to_the_bound(400, 50_000);
    search_to_the_bound(300, HISTORY_PART + 6_000);
}

/// The oldest item holds the words, in a case of its own; `count` items of
/// `size` bytes come after it, more than one search goes through. A search
/// stops at its bound, and Older goes on from there to the words.
fn search_to_the_bound(count: usize, size: usize) {
    let dir = tempfile::tempdir().expect("a folder");
    let (mut ledger, project, conversation) = chief(&dir);
    let mut copied = vec![item("found", "user", "Ünïcode says Héllo")];
    copied.extend((1..=count).map(|at| item(&format!("i-{at}"), "user", &"x".repeat(size))));
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
        first.entries.is_empty() && first.bounded,
        "{:?}",
        first.scanned
    );
    assert!(
        first.scanned.bytes <= HISTORY_SCAN_BYTES
            && first.scanned.bytes > HISTORY_SCAN_BYTES - 200_000,
        "to its bound and never past it: {:?}",
        first.scanned
    );
    assert_eq!(older(&ledger, project, &first, find, 5), ["found"]);
}

#[test]
fn a_search_inside_an_item_longer_than_its_bound_stops_at_the_bound_and_goes_on_inside_the_item() {
    // The reviewers' item: 24 MiB of an agent's words, the words at its very end.
    let dir = tempfile::tempdir().expect("a folder");
    let (mut ledger, project, conversation) = chief(&dir);
    let whole = format!("{} the NEEDLE", "w".repeat(24 * 1024 * 1024));
    ledger
        .copy_transcript(
            conversation,
            &[
                item("before", "user", "Write it all out"),
                item("long", "assistant", &whole),
            ],
            0,
        )
        .expect("copied");
    let find = Some("needle");
    let first = ledger
        .read_history(
            project,
            "chief",
            &ask(HistoryFrom::Newest, find, false, ROOM),
        )
        .expect("read");
    assert!(first.entries.is_empty() && first.bounded);
    assert!(
        first.scanned.bytes <= HISTORY_SCAN_BYTES,
        "never past its bound, whatever one item's size: {:?}",
        first.scanned
    );
    let inside = first.resume.expect("a place inside the item");
    assert!(inside.part > 0, "{inside:?}");
    let found = ledger
        .read_history(
            project,
            "chief",
            &ask(HistoryFrom::Before(inside), find, false, ROOM),
        )
        .expect("read");
    assert_eq!(ids(&found), ["long"]);
    assert!(found.scanned.bytes <= HISTORY_SCAN_BYTES);
    let HistoryEntry::Item(row) = &found.entries[0] else {
        panic!("an item");
    };
    assert!(row.item.text.ends_with("the NEEDLE"));
    assert_eq!(row.cut.expect("a part").bytes, whole.len());
    // And on past it, to the item before.
    let rest = older(&ledger, project, &found, find, 5);
    assert!(rest.is_empty(), "{rest:?}");
}

#[test]
fn every_read_moves_however_little_it_may_go_through_and_older_reaches_the_oldest() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger = ledger_in(&dir);
    let project = project(&mut ledger);
    let chief = id_of(&project, "chief");
    // A conversation of a tool's output alone; one of short items and a long
    // one of four parts of characters of four bytes, its words in its last
    // (the reviewers' widest parts); and the one going on now.
    let conversation = |ledger: &mut Ledger, items: &[Value]| {
        let id = ledger
            .start_conversation(chief, "claude-code")
            .expect("a conversation")
            .id;
        ledger.copy_transcript(id, items, 0).expect("copied");
        id
    };
    let tools = conversation(&mut ledger, &[item("t-0", "tool", "ok")]);
    let long = format!("{} the NEEDLE", "🙂".repeat(HISTORY_PART * 3));
    conversation(
        &mut ledger,
        &[
            item("w-0", "user", "a needle"),
            item("w-1", "assistant", &long),
            item("w-2", "user", "after"),
        ],
    );
    conversation(&mut ledger, &[item("n-0", "user", "now")]);
    // Each read may go through a byte: it takes its first step all the same,
    // and no more, so Older goes on an entry or a part at a time to the end.
    let walk = |find: Option<&str>, tools: bool| {
        let mut seen = Vec::new();
        let mut from = HistoryFrom::Newest;
        for _ in 0..50 {
            let read = ledger
                .read_history(
                    project.id,
                    "chief",
                    &HistoryAsk {
                        scan: 1,
                        ..ask(from, find, tools, ROOM)
                    },
                )
                .expect("a read");
            // One part at most, 256,000 bytes of these, and the end of the
            // one before it read by itself, 8,000 bytes at most.
            assert!(
                read.scanned.bytes <= 4 * HISTORY_PART + 8_000,
                "a part and the end of the one before at most: {:?}",
                read.scanned
            );
            // A place inside the long item is at one of its four parts: one
            // gone all through is passed, never stood at past its last.
            assert!(
                read.resume.is_none_or(|key| key.part < 4),
                "{:?}",
                read.resume
            );
            seen.extend(read.entries.iter().map(|entry| match entry {
                HistoryEntry::Item(row) => row.item.id.clone(),
                HistoryEntry::Heading(key) => format!("heading {}", key.conversation),
            }));
            let Some(place) = read.resume else {
                return seen;
            };
            from = HistoryFrom::Before(place);
        }
        panic!("no end after 50 reads: {seen:?}");
    };
    let heading = format!("heading {tools}");
    assert_eq!(walk(None, false), ["n-0", "w-2", "w-1", "w-0", &heading]);
    assert_eq!(walk(None, true), ["n-0", "w-2", "w-1", "w-0", "t-0"]);
    assert_eq!(walk(Some("needle"), false), ["w-1", "w-0"]);
    assert!(walk(Some("nowhere"), false).is_empty());
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
    let HistoryEntry::Item(row) = &read.entries[0] else {
        panic!("an item");
    };
    let cut = row.cut.expect("a part");
    assert_eq!((cut.from, cut.bytes), (0, whole.len()));
    assert_eq!(row.item.text.chars().count(), HISTORY_PART);
    assert!(whole.starts_with(&row.item.text));
    assert!(row.item.complete && row.item.at.is_none() && row.item.role == "assistant");
    // A search shows the part where it found its words.
    let found = ledger
        .read_history(
            project,
            "chief",
            &ask(HistoryFrom::Newest, Some("needle"), false, ROOM),
        )
        .expect("read");
    let HistoryEntry::Item(row) = &found.entries[0] else {
        panic!("an item");
    };
    assert!(row.cut.expect("a part").from > 0);
    assert!(
        row.item.text.ends_with("the NEEDLE"),
        "{}",
        &row.item.text[..40]
    );
    // The whole of it, a part at a time, by its place.
    let place = row_of(&ledger, project, "long");
    let mut read_whole = String::new();
    let mut from = Some(0);
    for _ in 0..10 {
        let Some(at) = from else { break };
        let part = ledger
            .read_history_part(project, "chief", conversation, place, at)
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
        .read_history_part(project, "zeus", conversation, place, 0)
        .expect_err("not zeus's");
    assert!(
        refused
            .to_string()
            .contains(&format!("no item at row {place} of zeus's")),
        "{refused}"
    );
}

#[test]
fn a_search_heads_only_the_conversations_it_found_its_words_in_and_a_page_heads_one_with_nothing() {
    let dir = tempfile::tempdir().expect("a folder");
    let (mut ledger, project, conversation) = chief(&dir);
    ledger
        .copy_transcript(conversation, &[item("found", "user", "the alpha")], 0)
        .expect("copied");
    let chief = ledger
        .read_history(
            project,
            "chief",
            &ask(HistoryFrom::Newest, None, false, ROOM),
        )
        .expect("read")
        .conversations[0]
        .conversation
        .participant_id;
    // The window opens fresh: the one going on now holds nothing yet.
    let now = ledger
        .start_conversation(chief, "claude-code")
        .expect("a conversation");
    let found = ledger
        .read_history(
            project,
            "chief",
            &ask(HistoryFrom::Newest, Some("ALPHA"), false, ROOM),
        )
        .expect("read");
    assert_eq!(ids(&found), ["found"]);
    assert_eq!(
        found.entries.len(),
        1,
        "no heading of the one it found nothing in"
    );
    let page = ledger
        .read_history(
            project,
            "chief",
            &ask(HistoryFrom::Newest, None, false, ROOM),
        )
        .expect("read");
    assert_eq!(
        page.entries.first().map(HistoryEntry::key),
        Some(cf_ledger::HistoryKey {
            conversation: now.id,
            row: 0,
            part: 0
        }),
        "a page heads the one with nothing in its place"
    );
}

#[test]
fn words_where_one_part_ends_and_the_next_begins_are_found() {
    let dir = tempfile::tempdir().expect("a folder");
    let (mut ledger, project, conversation) = chief(&dir);
    // "Needle" begins four characters before the first part's end.
    let whole = format!(
        "{}Needle{}",
        "a".repeat(HISTORY_PART - 4),
        "b".repeat(HISTORY_PART)
    );
    ledger
        .copy_transcript(conversation, &[item("long", "assistant", &whole)], 0)
        .expect("copied");
    let found = ledger
        .read_history(
            project,
            "chief",
            &ask(HistoryFrom::Newest, Some("NEEDLE"), false, ROOM),
        )
        .expect("read");
    let HistoryEntry::Item(row) = &found.entries[0] else {
        panic!("found");
    };
    assert!(row.item.text.contains("Needle"));
    assert_eq!(row.cut.expect("a part").from, HISTORY_PART - 4 - 2_000);
}

#[test]
fn a_search_that_goes_on_inside_an_item_reads_the_end_of_the_part_before_as_its_last_copy_wrote_it()
{
    let dir = tempfile::tempdir().expect("a folder");
    let (mut ledger, project, conversation) = chief(&dir);
    // Two parts, then copied again changed: a first part of characters of four
    // bytes, "Needle" beginning three before its end, and a short second.
    let copy = |ledger: &mut Ledger, text: &str| {
        ledger
            .copy_transcript(conversation, &[item("long", "assistant", text)], 0)
            .expect("copied");
    };
    copy(&mut ledger, &"a".repeat(HISTORY_PART * 2));
    copy(
        &mut ledger,
        &format!("{}Needle and on", "🙂".repeat(HISTORY_PART - 3)),
    );
    // Where a search that stopped past the first part goes on from: the
    // second, with the end of the first, its last 2,000 characters, read by
    // itself: a read that loaded the first part whole would fail.
    let place = cf_ledger::HistoryKey {
        conversation,
        row: row_of(&ledger, project, "long"),
        part: 1,
    };
    ledger.limit_value_length(64 * 1024).expect("held");
    let found = ledger
        .read_history(
            project,
            "chief",
            &ask(HistoryFrom::Before(place), Some("needle"), false, ROOM),
        )
        .expect("a search");
    assert_eq!(ids(&found), ["long"], "{:?}", found.scanned);
    let HistoryEntry::Item(row) = &found.entries[0] else {
        panic!("an item");
    };
    assert_eq!(row.cut.expect("a part").from, HISTORY_PART - 2_000);
    assert_eq!(
        row.item.text,
        format!("{}Needle and on", "🙂".repeat(1_997))
    );
}

#[test]
fn a_search_holds_at_most_1_000_characters_and_a_phrase_that_long_is_found_across_two_parts() {
    const MOST: usize = 1_000;
    let dir = tempfile::tempdir().expect("a folder");
    let (mut ledger, project, conversation) = chief(&dir);
    let phrase: String = (0..MOST)
        .map(|at| char::from(b'a' + u8::try_from(at % 26).expect("a letter")))
        .collect();
    // The phrase begins ten characters before the first part's end.
    let whole = format!(
        "{}{phrase}{}",
        "-".repeat(HISTORY_PART - 10),
        "-".repeat(HISTORY_PART)
    );
    ledger
        .copy_transcript(conversation, &[item("long", "assistant", &whole)], 0)
        .expect("copied");
    let search = |find: &str| {
        ledger.read_history(
            project,
            "chief",
            &ask(HistoryFrom::Newest, Some(find), false, ROOM),
        )
    };
    let found = search(&phrase.to_uppercase()).expect("a search");
    assert_eq!(ids(&found), ["long"]);
    // One character more is refused, in the words the viewer shows; so is
    // the reviewers' phrase, which spans three parts.
    for longer in [
        format!("{phrase}z"),
        format!("B{}D", "c".repeat(HISTORY_PART)),
    ] {
        let refused = search(&longer).expect_err("too long a search");
        assert!(
            refused.to_string().contains("at most 1,000 characters"),
            "{refused}"
        );
    }
}

#[test]
fn each_part_says_which_copy_of_its_item_it_is_of_and_a_copy_written_again_changed_is_the_next() {
    let dir = tempfile::tempdir().expect("a folder");
    let (mut ledger, project, conversation) = chief(&dir);
    let copy = |ledger: &mut Ledger, id: &str, text: &str| {
        ledger
            .copy_transcript(conversation, &[item(id, "assistant", text)], 0)
            .expect("copied");
    };
    let revision = |ledger: &Ledger, place: i64, from: usize| {
        ledger
            .read_history_part(project, "chief", conversation, place, from)
            .expect("a part")
            .revision
    };
    // The copy the part a page shows of a long item is of, as the page says it.
    let shown = |ledger: &Ledger| {
        let read = ledger
            .read_history(
                project,
                "chief",
                &ask(HistoryFrom::Newest, None, false, ROOM),
            )
            .expect("a page");
        read.entries
            .iter()
            .find_map(|entry| match entry {
                HistoryEntry::Item(row) if row.item.id == "long" => row.cut,
                _ => None,
            })
            .expect("the long item, cut")
            .revision
    };
    let first = "a".repeat(HISTORY_PART * 2 + 5);
    copy(&mut ledger, "long", &first);
    let long = row_of(&ledger, project, "long");
    let at_first = revision(&ledger, long, 0);
    assert_eq!(shown(&ledger), at_first);
    // Copied again as it was, it is the copy it was; written again changed,
    // the next, at every part, and on the page.
    copy(&mut ledger, "long", &first);
    assert_eq!(revision(&ledger, long, HISTORY_PART), at_first);
    copy(&mut ledger, "long", &"b".repeat(HISTORY_PART * 2 + 5));
    assert_eq!(revision(&ledger, long, 0), at_first + 1);
    assert_eq!(revision(&ledger, long, HISTORY_PART), at_first + 1);
    assert_eq!(shown(&ledger), at_first + 1);
    // An item shorter than a part too: one that grows past a part as it is
    // read is no part of the copy read before.
    copy(&mut ledger, "short", "ok");
    let short = row_of(&ledger, project, "short");
    let short_first = revision(&ledger, short, 0);
    copy(&mut ledger, "short", &"c".repeat(HISTORY_PART + 1));
    assert_eq!(revision(&ledger, short, HISTORY_PART), short_first + 1);
}

#[test]
fn a_long_item_that_shrinks_and_grows_is_read_as_it_is_now() {
    let dir = tempfile::tempdir().expect("a folder");
    let (mut ledger, project, conversation) = chief(&dir);
    // Written over three looks: three parts and more, then less than one, then
    // two and a bit; each time kept in as many parts as it is now, and no more.
    for (length, letter, parts) in [
        (HISTORY_PART * 3 + 5, "a", 4),
        (10, "b", 0),
        (HISTORY_PART * 2 + 1, "c", 3),
    ] {
        let whole = letter.repeat(length);
        ledger
            .copy_transcript(conversation, &[item("w", "assistant", &whole)], 0)
            .expect("copied");
        let kept: (i64, i64) = ledger
            .sqlite()
            .query_row(
                "SELECT (SELECT COUNT(*) FROM transcript_part), (SELECT sum(parts) FROM transcript_head)",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("the parts");
        assert_eq!(
            kept,
            (parts, parts),
            "{length}: the parts kept, and those its head says"
        );
        let place = row_of(&ledger, project, "w");
        let mut read = String::new();
        let mut from = Some(0);
        for _ in 0..10 {
            let Some(at) = from else { break };
            let got = ledger
                .read_history_part(project, "chief", conversation, place, at)
                .expect("a part");
            read.push_str(&got.text);
            from = got.next;
        }
        assert_eq!(read, whole, "{length}");
    }
}

/// The reviewers' ledger: the chief's conversation of an item of two parts,
/// and after it the newest, of one of 24 MiB, about 394 parts. Each with its
/// time, which its row holds past the whole of its text.
///
/// What it writes stays in the write-ahead log, never checkpointed into the
/// database file: a long value's pages that are in the file itself can be read
/// straight from it, past the cache the pages are counted in, and a walk of
/// the item's chain went uncounted so (a read of the whole 24 MiB text counted
/// 9 pages); kept in the log, every page of the walk is counted (6,159).
pub(super) struct Reviewers {
    pub(super) ledger: Ledger,
    pub(super) project: i64,
    /// The conversation of the long item, and the row it stands at.
    pub(super) conversation: i64,
    pub(super) place: i64,
    /// The length of the long item's text, in bytes.
    pub(super) length: usize,
    /// Where the page that shows the item of two parts begins.
    pub(super) two_parts: HistoryFrom,
}

pub(super) fn reviewers(dir: &tempfile::TempDir) -> Reviewers {
    let (mut ledger, project, conversation) = chief(dir);
    ledger
        .sqlite()
        .execute_batch("PRAGMA wal_autocheckpoint = 0")
        .expect("kept in the log");
    let timed = |id: &str, text: &str| {
        let mut timed = item(id, "assistant", text);
        timed["at"] = json!("2026-10-16T09:30:00.000Z");
        timed
    };
    ledger
        .copy_transcript(
            conversation,
            &[timed("two", &"b".repeat(HISTORY_PART + 1))],
            0,
        )
        .expect("copied");
    let chief = ledger
        .read_history(project, "chief", &ask(HistoryFrom::Newest, None, false, 1))
        .expect("read")
        .conversations[0]
        .conversation
        .participant_id;
    let conversation = ledger
        .start_conversation(chief, "claude-code")
        .expect("a conversation")
        .id;
    let whole = "a".repeat(24 * 1024 * 1024);
    ledger
        .copy_transcript(conversation, &[timed("long", &whole)], 0)
        .expect("copied");
    let place = row_of(&ledger, project, "long");
    let long_page = ledger
        .read_history(project, "chief", &ask(HistoryFrom::Newest, None, false, 1))
        .expect("read");
    let two_parts = HistoryFrom::Before(long_page.resume.expect("the one before"));
    Reviewers {
        ledger,
        project,
        conversation,
        place,
        length: whole.len(),
        two_parts,
    }
}

/// The size of the pages of the ledger's file.
fn page_size(ledger: &Ledger) -> usize {
    let size: i64 = ledger
        .sqlite()
        .query_row("PRAGMA page_size", [], |row| row.get(0))
        .expect("a page size");
    usize::try_from(size).expect("a size")
}

/// How many pages a value of `bytes` bytes spreads over where it is longer
/// than its row's own page: each takes the next one's number before its data.
fn chain(page_size: usize, bytes: usize) -> i32 {
    i32::try_from(bytes.div_ceil(page_size - 4)).expect("a count")
}

/// What a read of a part takes besides the chain of its own text: the pages
/// of the indexes and rows it comes to it by (the participant, the item's
/// head, the part's row) and of its row's end kept before its text. It takes
/// 28 for a part of 64,000 bytes, whose chain is 16.
const AROUND: i32 = 14;

/// The most a value may be where a read loads no long item's text: a part and
/// a little more.
pub(super) const LOADED_AT_MOST: usize = 100_000;

#[test]
fn reading_a_24_mib_item_whole_a_part_at_a_time_reads_each_part_alone() {
    // The reviewers' item: 24 MiB, about 394 parts. Read whole for each part,
    // as the second round did, it was read 394 times over (some 6,150 pages a
    // part, tens of seconds), and a part found by going through its item from
    // the first would take more pages the later it is. SQLite's pages are
    // counted, not the seconds the machine takes for them, which another
    // build running beside this one moves.
    let dir = tempfile::tempdir().expect("a folder");
    let Reviewers {
        ledger,
        project,
        conversation,
        place,
        length,
        ..
    } = reviewers(&dir);
    let size = page_size(&ledger);
    // A read that loaded the item's text whole would fail, the value too long.
    ledger.limit_value_length(LOADED_AT_MOST).expect("held");
    let part = |at: usize| {
        ledger
            .read_history_part(project, "chief", conversation, place, at)
            .expect("a part")
    };
    // The first once beforehand: what SQLite reads to make its statements is
    // not counted.
    part(0);
    fetched(&ledger);
    let (mut parts, mut bytes, mut first) = (0, 0, None);
    let mut from = Some(0);
    while let Some(at) = from {
        let got = part(at);
        let pages = fetched(&ledger);
        // The part's own chain and the few pages around it, whichever part it
        // is: a walk of the item's whole text is some 6,150.
        assert!(
            pages <= chain(size, got.text.len()) + AROUND,
            "part {parts} took {pages} pages: a walk of the item's {length} bytes is {}",
            chain(size, length)
        );
        // And none takes more than the first: it did not go through the
        // parts before it.
        let first = *first.get_or_insert(pages);
        assert!(
            pages <= first + SLACK,
            "part {parts} took {pages} pages, the first {first}"
        );
        parts += 1;
        bytes += got.text.len();
        from = got.next;
        assert!(parts < 1_000, "no end");
    }
    assert_eq!((parts, bytes), (394, length));
}

#[test]
fn a_page_that_shows_a_24_mib_item_reads_what_one_that_shows_an_item_of_two_parts_does() {
    // Its fields are read from where they are held apart, never from past the
    // whole of its text: that is a walk of the long text's chain (some 6,150
    // pages), which a count of steps does not show and the seconds a busy
    // machine takes for it do not either, so SQLite's pages are counted.
    let dir = tempfile::tempdir().expect("a folder");
    let Reviewers {
        ledger,
        project,
        length,
        two_parts,
        ..
    } = reviewers(&dir);
    // A read that loaded the item's text whole would fail, the value too long.
    ledger.limit_value_length(LOADED_AT_MOST).expect("held");
    let long = page_cost(&ledger, project, HistoryFrom::Newest);
    let two = page_cost(&ledger, project, two_parts);
    assert!(
        long <= two + SLACK,
        "a page of the 24 MiB item took {long} pages, of the two parts' {two}: a walk of its text is {}",
        chain(page_size(&ledger), length)
    );
}

/// What reading the page of one entry from `from` takes of the ledger's file,
/// in SQLite's pages: the read once first, so that what SQLite reads to make
/// its statements is not counted.
pub(super) fn page_cost(ledger: &Ledger, project: i64, from: HistoryFrom) -> i32 {
    let page = || {
        let read = ledger
            .read_history(project, "chief", &ask(from, None, false, 1))
            .expect("read");
        assert_eq!(read.entries.len(), 1);
    };
    page();
    fetched(ledger);
    page();
    fetched(ledger)
}

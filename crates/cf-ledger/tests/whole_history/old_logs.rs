//! A ledger an older build wrote may hold events whose data is no JSON, JSON
//! that is no object, or what this build never writes where it writes a
//! conversation's id or a participant's handle. Migration 0019's indexes and
//! the lookups of the log a heading takes read such an event as saying nothing
//! of any conversation or participant, so the ledger opens on it and the
//! viewer reads it: a switch whose data says nothing still ended the
//! conversation it came in, to what the log does not say, and the agent the
//! conversation began on is not said either.

use std::path::PathBuf;

use cf_ledger::{
    migrate, open_ledger, HistoryEntry, HistoryFrom, Options, HISTORY_PART, MIGRATIONS,
};
use rusqlite::{params, Connection};

use super::reads::{ask, ids, ROOM};
use super::{everything, lines};

const AT: &str = "2026-10-16T12:00:00.000Z";

/// What an older build could have left as an event's data where this one
/// writes an object: no JSON, empty, JSON of another kind, a conversation's id
/// that is no whole number, a handle that is no text, and JSON cut short.
const UNREADABLE: [&str; 12] = [
    "{",
    "",
    "[1, 2]",
    "5",
    "\"chief\"",
    "null",
    r#"{"conversation": "1", "participant": 7}"#,
    r#"{"conversation": 1.5}"#,
    r#"{"conversation": {"id": 1}, "participant": ["chief"]}"#,
    r#"{"from": "codex", "to": 5}"#,
    r#"{"conversation": 1,"#,
    "\u{0}",
];

/// The kinds of event the viewer reads the log for, each read through an index of 0019.
const READ: [&str; 4] = [
    "conversation.started",
    "conversation.followed",
    "conversation.ended",
    "chief.switched",
];

/// A ledger of version 18 in `dir`, as an older build left it: a project
/// whose chief runs on Codex as diana, its conversations 1 to `conversations`
/// (the last going on now), each with one item, and the log `events` (a kind
/// and its data) in order.
fn version_18(dir: &tempfile::TempDir, conversations: i64, events: &[(&str, &str)]) -> PathBuf {
    let file = dir.path().join("consensflow.db");
    let db = Connection::open(&file).expect("a database");
    migrate(&db, &MIGRATIONS[..18]).expect("version 18");
    db.execute_batch(&format!(
        "INSERT INTO project (id, directory, name, state, gate, created_at, updated_at)
           VALUES (1, '/work/app', 'app', 'open', 0, '{AT}', '{AT}');
         INSERT INTO participant (id, project_id, handle, role, roles, agent, harness, tier, member_id, created_at, designer)
           VALUES (2, 1, 'chief', 'chief', '[]', 'diana', 'codex', NULL, NULL, '{AT}', 0);"
    ))
    .expect("a project");
    for id in 1..=conversations {
        let ended = (id < conversations).then_some(AT);
        db.execute(
            "INSERT INTO conversation (id, participant_id, harness, started_at, ended_at)
               VALUES (?, 2, 'codex', ?, ?)",
            params![id, AT, ended],
        )
        .expect("a conversation");
        db.execute(
            "INSERT INTO transcript (conversation_id, item_id, seq, role, text, complete, at, copied_at)
               VALUES (?, ?, 0, 'user', ?, 1, NULL, ?)",
            params![id, format!("i-{id}"), format!("said in {id}"), AT],
        )
        .expect("an item");
    }
    let mut log = db
        .prepare("INSERT INTO event (project_id, at, kind, data) VALUES (1, ?, ?, ?)")
        .expect("a log");
    for (kind, data) in events {
        log.execute(params![AT, kind, data]).expect("logged");
    }
    file
}

/// An event of the chief's `kind`, of conversation `id`, as the ledger writes it.
fn of(kind: &'static str, id: i64) -> (&'static str, String) {
    (
        kind,
        format!(r#"{{"conversation": {id}, "participant": "chief"}}"#),
    )
}

#[test]
fn a_version_18_ledger_whose_log_holds_unreadable_events_of_every_kind_migrates_opens_and_is_read()
{
    let dir = tempfile::tempdir().expect("a folder");
    let mut events: Vec<(&str, String)> = READ
        .iter()
        .flat_map(|kind| UNREADABLE.iter().map(|data| (*kind, (*data).to_owned())))
        .collect();
    events.push(of("conversation.started", 1));
    events.push(of("conversation.started", 2));
    let events: Vec<(&str, &str)> = events
        .iter()
        .map(|(kind, data)| (*kind, data.as_str()))
        .collect();
    let file = version_18(&dir, 2, &events);

    let mut ledger = open_ledger(&file, Options::default()).expect("migrated and open");
    let history = everything(&ledger, 1, "chief").expect("read");
    assert_eq!(
        lines(&history),
        [
            r#"codex diana Some(Fresh) ["user: said in 1"]"#,
            r#"codex diana None ["user: said in 2"]"#,
        ]
    );
    let found = ledger
        .read_history(
            1,
            "chief",
            &ask(HistoryFrom::Newest, Some("SAID"), false, ROOM),
        )
        .expect("a search");
    assert_eq!(ids(&found), ["i-2", "i-1"]);
    // The log goes on as this build writes it, through the same indexes.
    let third = ledger
        .start_conversation(2, "codex")
        .expect("a conversation");
    ledger
        .copy_transcript(
            third.id,
            &[serde_json::json!({ "id": "i-3", "role": "user", "text": "said in 3" })],
            0,
        )
        .expect("copied");
    let history = everything(&ledger, 1, "chief").expect("read");
    assert_eq!(
        lines(&history),
        [
            r#"codex diana Some(Fresh) ["user: said in 1"]"#,
            r#"codex diana Some(Fresh) ["user: said in 2"]"#,
            r#"codex diana None ["user: said in 3"]"#,
        ]
    );
    drop(ledger);
    open_ledger(&file, Options::default()).expect("and open again");
}

#[test]
fn a_version_18_ledger_holding_a_text_that_is_no_utf_8_migrates_and_is_read_with_replacements() {
    let dir = tempfile::tempdir().expect("a folder");
    let started = of("conversation.started", 1);
    let file = version_18(&dir, 1, &[(started.0, started.1.as_str())]);
    // What no build writes, each writing its language's own strings: the
    // three bytes a lone surrogate would be, in a long text and a short one.
    let long = format!(
        "{}EDA080{}",
        "61".repeat(HISTORY_PART + 10),
        "62".repeat(10)
    );
    let db = Connection::open(&file).expect("the file");
    db.execute_batch(&format!(
        "INSERT INTO transcript (conversation_id, item_id, seq, role, text, complete, at, copied_at) VALUES
           (1, 'long', 1, 'assistant', CAST(X'{long}' AS TEXT), 1, NULL, '{AT}'),
           (1, 'short', 2, 'user', CAST(X'61EDA08062' AS TEXT), 1, NULL, '{AT}');"
    ))
    .expect("copied");
    drop(db);
    let ledger = open_ledger(&file, Options::default()).expect("migrated and open");
    let read = ledger
        .read_history(1, "chief", &ask(HistoryFrom::Newest, None, false, ROOM))
        .expect("read");
    let texts: Vec<(String, i64)> = read
        .entries
        .iter()
        .filter_map(|entry| match entry {
            HistoryEntry::Item(row) => Some((row.item.text.clone(), row.key.row)),
            HistoryEntry::Heading(_) => None,
        })
        .collect();
    assert_eq!(texts[0].0, "a\u{FFFD}\u{FFFD}\u{FFFD}b");
    assert_eq!(texts[1].0, "a".repeat(HISTORY_PART));
    let rest = ledger
        .read_history_part(1, "chief", 1, texts[1].1, HISTORY_PART)
        .expect("its second part");
    assert_eq!(
        (rest.text, rest.next),
        (
            format!(
                "{}\u{FFFD}\u{FFFD}\u{FFFD}{}",
                "a".repeat(10),
                "b".repeat(10)
            ),
            None
        )
    );
    let short = ledger
        .read_history_part(1, "chief", 1, texts[0].1, 0)
        .expect("the short one");
    assert_eq!(short.text, "a\u{FFFD}\u{FFFD}\u{FFFD}b");
}

#[test]
fn a_switch_whose_data_is_longer_than_a_switch_s_is_not_read_and_says_nothing() {
    let dir = tempfile::tempdir().expect("a folder");
    // A switch's data of a mebibyte, as no build writes (a few names).
    let long = format!(
        r#"{{"from": {{"harness": "codex", "agent": "diana"}}, "to": {{"harness": "codex", "agent": "maia"}}, "pad": "{}"}}"#,
        "z".repeat(1 << 20)
    );
    let events = [
        of("conversation.started", 1),
        ("chief.switched", long),
        of("conversation.started", 2),
    ];
    let events: Vec<(&str, &str)> = events
        .iter()
        .map(|(kind, data)| (*kind, data.as_str()))
        .collect();
    let file = version_18(&dir, 2, &events);
    let ledger = open_ledger(&file, Options::default()).expect("migrated and open");
    // A read that loaded the data whole would fail.
    ledger.limit_value_length(64 * 1024).expect("held");
    assert_eq!(
        lines(&everything(&ledger, 1, "chief").expect("read")),
        [
            r#"codex - Some(Switch { harness: "", agent: None }) ["user: said in 1"]"#,
            r#"codex diana None ["user: said in 2"]"#,
        ]
    );
}

#[test]
fn a_heading_reads_an_event_of_the_participant_that_names_no_conversation_or_says_nothing_as_saying_so(
) {
    let dir = tempfile::tempdir().expect("a folder");
    // Each conversation but the last is left for one the log does not name as
    // a whole number; the fourth by a switch whose data is no JSON.
    let events = [
        of("conversation.started", 1),
        (
            "conversation.followed",
            r#"{"conversation": "1", "participant": "chief"}"#.to_owned(),
        ),
        of("conversation.started", 2),
        (
            "conversation.followed",
            r#"{"conversation": 2.5, "participant": "chief"}"#.to_owned(),
        ),
        of("conversation.started", 3),
        (
            "conversation.followed",
            r#"{"conversation": {"id": 3}, "participant": "chief"}"#.to_owned(),
        ),
        of("conversation.started", 4),
        ("chief.switched", "{".to_owned()),
        of("conversation.started", 5),
    ];
    let events: Vec<(&str, &str)> = events
        .iter()
        .map(|(kind, data)| (*kind, data.as_str()))
        .collect();
    let file = version_18(&dir, 5, &events);
    let ledger = open_ledger(&file, Options::default()).expect("migrated and open");
    // A chief's conversation began on what the first switch after it was
    // from: one that says nothing says no agent.
    assert_eq!(
        lines(&everything(&ledger, 1, "chief").expect("read")),
        [
            r#"codex - Some(Window { resumed: false }) ["user: said in 1"]"#,
            r#"codex - Some(Window { resumed: false }) ["user: said in 2"]"#,
            r#"codex - Some(Window { resumed: false }) ["user: said in 3"]"#,
            r#"codex - Some(Switch { harness: "", agent: None }) ["user: said in 4"]"#,
            r#"codex diana None ["user: said in 5"]"#,
        ]
    );
}

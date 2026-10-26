//! A ledger an older build wrote may hold events whose data is no JSON, JSON
//! that is no object, or what this build never writes where it writes a
//! conversation's id or a participant's handle. Migration 0020's indexes and
//! the lookups of the log a heading takes read such an event as saying nothing
//! of any conversation or participant, so the ledger opens on it and the
//! viewer reads it: a switch whose data says nothing still ended the
//! conversation it came in, to what the log does not say, and the agent the
//! conversation began on is not said either.

use std::path::PathBuf;

use cf_ledger::{
    migrate, open_ledger, HistoryEntry, HistoryFrom, Options, HISTORY_PART, MIGRATIONS,
    SCHEMA_VERSION,
};
use rusqlite::{params, Connection};

use super::reads::{ask, ids, ROOM};
use super::{everything, lines};

pub(super) const AT: &str = "2026-10-16T12:00:00.000Z";

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

/// The kinds of event the viewer reads the log for, each read through an index of 0020.
const READ: [&str; 4] = [
    "conversation.started",
    "conversation.followed",
    "conversation.ended",
    "chief.switched",
];

/// A ledger of version 18 in `dir`, as an older build left it (3.0.4): a
/// project whose chief runs on Codex as diana, its conversations 1 to
/// `conversations` (the last going on now), each with one item, and the log
/// `events` (a kind and its data) in order.
pub(super) fn version_18(
    dir: &tempfile::TempDir,
    conversations: i64,
    events: &[(&str, &str)],
) -> PathBuf {
    of_version(dir, 18, conversations, events)
}

/// The same ledger, of the schema `version` (18 or 19: 3.0.5 wrote 19, with
/// B5's migration 0019 and none of the viewer's).
fn of_version(
    dir: &tempfile::TempDir,
    version: usize,
    conversations: i64,
    events: &[(&str, &str)],
) -> PathBuf {
    let file = dir.path().join("consensflow.db");
    let db = Connection::open(&file).expect("a database");
    migrate(&db, &MIGRATIONS[..version]).expect("an older version");
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
pub(super) fn of(kind: &'static str, id: i64) -> (&'static str, String) {
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
fn a_ledger_3_0_5_wrote_migrates_to_the_viewer_s_0020_once_and_is_read_and_its_lets_go_stay_reconciled(
) {
    let dir = tempfile::tempdir().expect("a folder");
    // What 3.0.5 left: version 19, B5's migration 0019 (byte for byte, pinned by
    // the schema's SHA-256s), a carrier's rows it let go of and its start
    // reconciled, and an item longer than a part.
    let started = [of("conversation.started", 1), of("conversation.started", 2)];
    let events: Vec<(&str, &str)> = started
        .iter()
        .map(|(kind, data)| (*kind, data.as_str()))
        .chain([("message.uncarried", r#"{"carrier": 9, "released": []}"#)])
        .collect();
    let file = of_version(&dir, 19, 2, &events);
    let long = "word ".repeat(HISTORY_PART / 4);
    let db = Connection::open(&file).expect("the file");
    db.execute_batch(&format!(
        "INSERT INTO event (project_id, at, kind, data)
           SELECT 1, '{AT}', 'message.reconciled', json_object('uncarried', json_array(id))
             FROM event WHERE kind = 'message.uncarried';
         INSERT INTO transcript (conversation_id, item_id, seq, role, text, complete, at, copied_at)
           VALUES (2, 'long', 1, 'assistant', '{long}', 1, NULL, '{AT}');"
    ))
    .expect("3.0.5's rows");
    let version = |db: &Connection| -> i64 {
        db.query_row("PRAGMA user_version", [], |row| row.get(0))
            .expect("a version")
    };
    let heads = |db: &Connection| -> rusqlite::Result<i64> {
        db.query_row("SELECT COUNT(*) FROM transcript_head", [], |row| row.get(0))
    };
    assert_eq!(version(&db), 19);
    assert!(heads(&db).is_err(), "3.0.5 has no heads' table");
    drop(db);

    let ledger = open_ledger(&file, Options::default()).expect("migrated and open");
    // A page holds a long item's first part.
    let read: Vec<String> = lines(&everything(&ledger, 1, "chief").expect("read"))
        .into_iter()
        .map(|line| line.replace(&long[..HISTORY_PART], "(its first part)"))
        .collect();
    assert_eq!(
        read,
        [
            r#"codex diana Some(Fresh) ["user: said in 1"]"#,
            r#"codex diana None ["user: said in 2", "assistant: (its first part)"]"#,
        ]
    );
    drop(ledger);
    open_ledger(&file, Options::default()).expect("and open again");
    let db = Connection::open(&file).expect("the file");
    assert_eq!(
        version(&db),
        SCHEMA_VERSION as i64,
        "0020 and every one after it"
    );
    assert_eq!(heads(&db).expect("the heads"), 3, "one head an item");
    let parts: i64 = db
        .query_row(
            "SELECT p.parts FROM transcript_head p JOIN transcript t ON t.rowid = p.item
             WHERE t.item_id = 'long'",
            [],
            |row| row.get(0),
        )
        .expect("the long item's head");
    assert_eq!(
        parts,
        long.len().div_ceil(HISTORY_PART) as i64,
        "split in parts"
    );
    let logged: Vec<String> = db
        .prepare("SELECT kind FROM event WHERE kind LIKE 'message.%' ORDER BY id")
        .expect("the log")
        .query_map([], |row| row.get(0))
        .expect("its kinds")
        .collect::<rusqlite::Result<_>>()
        .expect("each");
    assert_eq!(
        logged,
        ["message.uncarried", "message.reconciled"],
        "nothing let go of again, nor reconciled twice"
    );
}

#[test]
fn a_fresh_ledger_is_made_at_0020_after_b5_s_0019_with_the_viewer_s_tables() {
    assert!(
        MIGRATIONS[18].contains("'message.uncarried'"),
        "0019 is B5's"
    );
    assert!(
        MIGRATIONS[19].contains("CREATE TABLE transcript_head"),
        "0020 is the viewer's"
    );
    let dir = tempfile::tempdir().expect("a folder");
    let file = dir.path().join("consensflow.db");
    drop(open_ledger(&file, Options::default()).expect("made and open"));
    let db = Connection::open(&file).expect("the file");
    let version: i64 = db
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("a version");
    assert_eq!(
        version, SCHEMA_VERSION as i64,
        "0020 and every one after it"
    );
    let tables: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master
             WHERE type = 'table' AND name IN ('transcript_head', 'transcript_part')",
            [],
            |row| row.get(0),
        )
        .expect("the tables");
    assert_eq!(tables, 2);
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
fn a_switch_an_older_build_logged_long_is_cut_once_to_what_a_heading_reads_of_it() {
    let dir = tempfile::tempdir().expect("a folder");
    // A switch's data of a mebibyte, as no build writes (a few names); and one
    // of the same size whose names are no text, or longer than a field's.
    let long = |from: &str, to: &str| {
        format!(
            r#"{{"from": {from}, "to": {to}, "cut": true, "pad": "{}"}}"#,
            "z".repeat(1 << 20)
        )
    };
    let named = long(
        r#"{"harness": "codex", "agent": "diana"}"#,
        r#"{"harness": "codex", "agent": "maia"}"#,
    );
    let unnamed = long(
        r#"{"harness": 5, "agent": ["x"]}"#,
        &format!(r#"{{"harness": "codex", "agent": "{}"}}"#, "m".repeat(257)),
    );
    let sizes = (named.len(), unnamed.len());
    let events = [
        of("conversation.started", 1),
        ("chief.switched", named),
        of("conversation.started", 2),
        ("chief.switched", unnamed),
        of("conversation.started", 3),
    ];
    let events: Vec<(&str, &str)> = events
        .iter()
        .map(|(kind, data)| (*kind, data.as_str()))
        .collect();
    let file = version_18(&dir, 3, &events);
    let ledger = open_ledger(&file, Options::default()).expect("migrated and open");
    // A read that loaded the data whole would fail.
    ledger.limit_value_length(64 * 1024).expect("held");
    assert_eq!(
        lines(&everything(&ledger, 1, "chief").expect("read")),
        [
            r#"codex diana Some(Switch { harness: "codex", agent: Some("maia") }) ["user: said in 1"]"#,
            r#"codex - Some(Switch { harness: "codex", agent: None }) ["user: said in 2"]"#,
            r#"codex diana None ["user: said in 3"]"#,
        ]
    );
    drop(ledger);
    let db = Connection::open(&file).expect("the file");
    let data: Vec<String> = db
        .prepare("SELECT data FROM event WHERE kind = 'chief.switched' ORDER BY id")
        .expect("a query")
        .query_map([], |row| row.get(0))
        .expect("the switches")
        .map(|data| data.expect("one"))
        .collect();
    assert_eq!(
        data,
        [
            format!(
                r#"{{"from":{{"harness":"codex","agent":"diana"}},"to":{{"harness":"codex","agent":"maia"}},"cut":true,"cutFromBytes":{}}}"#,
                sizes.0
            ),
            format!(
                r#"{{"from":{{"harness":null,"agent":null}},"to":{{"harness":"codex","agent":null}},"cut":true,"cutFromBytes":{}}}"#,
                sizes.1
            ),
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

/// B5's migration 0019, as 3.0.5 shipped it, and the viewer's 0020: the ledgers
/// below are made of them by name, so that what an older build wrote does not move
/// with the order of [`MIGRATIONS`].
const B5_0019: &str = include_str!("../../migrations/0019.sql");
const VIEWER_0020: &str = include_str!("../../migrations/0020.sql");

/// A ledger an older build left at the schema it migrated to with `migrations`:
/// a project whose task 1 is paused with its brief delivered, as 3.0.5 left one,
/// and none of the Resume's columns.
fn left_at(dir: &tempfile::TempDir, migrations: &[&str]) -> PathBuf {
    let file = dir.path().join("consensflow.db");
    let db = Connection::open(&file).expect("a database");
    migrate(&db, migrations).expect("an older version");
    db.execute_batch(&format!(
        "INSERT INTO project (id, directory, name, state, gate, created_at, updated_at)
           VALUES (1, '/work/app', 'app', 'suspended', 0, '{AT}', '{AT}');
         INSERT INTO participant (id, project_id, handle, role, roles, agent, harness, tier, member_id, created_at, designer) VALUES
           (2, 1, 'chief', 'chief', '[]', 'diana', 'codex', NULL, NULL, '{AT}', 0),
           (3, 1, 'zeus', 'worker', '[\"worker\"]', 'zeus', 'claude-code', 'standard', NULL, '{AT}', 0);
         INSERT INTO task (id, project_id, number, title, body, requester_id, assignee_id, state, created_at, updated_at, paused_at)
           VALUES (1, 1, 1, 'Parser', 'Parser', 2, 3, 'paused', '{AT}', '{AT}', '{AT}');
         INSERT INTO message (id, project_id, recipient_id, sender_id, kind, task_id, body, state, created_at)
           VALUES (1, 1, 3, 2, 'task', 1, 'Parser', 'delivered', '{AT}');"
    ))
    .expect("its rows");
    file
}

/// The ledger's version, and what it holds of the Resume's migrations: 0021's mark
/// on task 1 and 0022's time message 1 was handed, where the columns are there.
fn resume_columns(file: &PathBuf) -> (i64, Option<i64>, Option<Option<String>>) {
    let db = Connection::open(file).expect("the file");
    let version = db
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("a version");
    let mark = db
        .query_row(
            "SELECT paused_with_project FROM task WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .ok();
    let handed = db
        .query_row("SELECT handed_at FROM message WHERE id = 1", [], |row| {
            row.get(0)
        })
        .ok();
    (version, mark, handed)
}

/// The whole schema a ledger holds, by name: what a second open must leave as it is.
fn schema_of(file: &PathBuf) -> Vec<(String, String)> {
    let db = Connection::open(file).expect("the file");
    let schema = db
        .prepare("SELECT name, sql FROM sqlite_master WHERE sql IS NOT NULL ORDER BY name")
        .expect("the schema")
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .expect("its rows")
        .collect::<rusqlite::Result<_>>()
        .expect("each");
    schema
}

/// What 3.0.5 left, version 19 with B5's 0019, opens at the build's version: the
/// viewer's 0020, then the Resume's 0021 (its task reads paused by nothing but the
/// human, so its Resume leaves it paused) and 0022 (its message was handed nothing it
/// marked), and every one after them. A second open migrates nothing again.
#[test]
fn a_ledger_3_0_5_wrote_migrates_to_the_resume_s_0021_and_0022_after_the_viewer_s_0020_once() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut b5 = MIGRATIONS[..18].to_vec();
    b5.push(B5_0019);
    let file = left_at(&dir, &b5);
    assert_eq!(resume_columns(&file), (19, None, None));

    drop(open_ledger(&file, Options::default()).expect("migrated and open"));
    assert_eq!(
        resume_columns(&file),
        (SCHEMA_VERSION as i64, Some(0), Some(None))
    );
    let schema = schema_of(&file);
    assert!(
        schema.iter().any(|(name, _)| name == "transcript_head"),
        "the viewer's 0020 ran too"
    );
    drop(open_ledger(&file, Options::default()).expect("and open again"));
    assert_eq!(
        resume_columns(&file),
        (SCHEMA_VERSION as i64, Some(0), Some(None))
    );
    assert_eq!(schema_of(&file), schema, "nothing migrated again");
}

/// A ledger the next release would write before the Resume lands, version 20 with
/// the viewer's 0020, opens at the build's version with the Resume's columns. A second
/// open migrates nothing again.
#[test]
fn a_ledger_at_the_viewer_s_0020_migrates_to_the_resume_s_0022() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut viewer = MIGRATIONS[..18].to_vec();
    viewer.extend([B5_0019, VIEWER_0020]);
    let file = left_at(&dir, &viewer);
    assert_eq!(resume_columns(&file), (20, None, None));

    drop(open_ledger(&file, Options::default()).expect("migrated and open"));
    assert_eq!(
        resume_columns(&file),
        (SCHEMA_VERSION as i64, Some(0), Some(None))
    );
    let schema = schema_of(&file);
    drop(open_ledger(&file, Options::default()).expect("and open again"));
    assert_eq!(
        resume_columns(&file),
        (SCHEMA_VERSION as i64, Some(0), Some(None))
    );
    assert_eq!(schema_of(&file), schema, "nothing migrated again");
}

/// A fresh ledger is made at the build's version: main's 0019 and 0020 (B5's, the
/// viewer's), then the Resume's 0021 and 0022, then 0023 of the conversation a paste
/// went into, and the exit proof's 0024, in that order.
#[test]
fn a_fresh_ledger_is_made_at_0022_with_main_s_migrations_first_and_the_resume_s_after() {
    assert_eq!(
        (MIGRATIONS.len(), MIGRATIONS[18], MIGRATIONS[19]),
        (SCHEMA_VERSION, B5_0019, VIEWER_0020),
        "main's 0019 and 0020 where main has them"
    );
    assert!(
        MIGRATIONS[20].contains("ADD COLUMN paused_with_project"),
        "0021 is the Resume's mark"
    );
    assert!(
        MIGRATIONS[21].contains("ADD COLUMN handed_at"),
        "0022 is the Resume's hand-over"
    );
    assert!(
        MIGRATIONS[22].contains("ADD COLUMN pasted_into"),
        "0023 is the conversation a paste went into"
    );
    assert!(
        MIGRATIONS[23].contains("ADD COLUMN program_gone"),
        "0024 is the exit proof"
    );
    let dir = tempfile::tempdir().expect("a folder");
    let file = dir.path().join("consensflow.db");
    drop(open_ledger(&file, Options::default()).expect("made and open"));
    let schema = schema_of(&file);
    let table = |name: &str| {
        schema
            .iter()
            .find(|(table, _)| table == name)
            .map(|(_, sql)| sql.clone())
            .unwrap_or_default()
    };
    assert!(table("task").contains("paused_with_project"));
    assert!(table("message").contains("handed_at"));
    assert!(table("message").contains("pasted_into"));
    assert!(table("conversation").contains("program_gone"));
    assert!(!table("transcript_head").is_empty(), "the viewer's table");
    let db = Connection::open(&file).expect("the file");
    let version: i64 = db
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("a version");
    assert_eq!(version, SCHEMA_VERSION as i64);
}

/// What alpha.2 left, version 22 with the Resume's 0021 and 0022, opens at 24: 0023,
/// then the exit proof's 0024, whose column proves no conversation's program gone
/// (what the daemon that wrote it knew went with it). A second open migrates nothing
/// again.
#[test]
fn a_ledger_at_the_resume_s_0022_migrates_to_the_exit_proof_s_0024_proving_nothing_gone() {
    let dir = tempfile::tempdir().expect("a folder");
    let file = left_at(&dir, &MIGRATIONS[..22]);
    Connection::open(&file)
        .expect("the file")
        .execute_batch(&format!(
            "INSERT INTO conversation (id, participant_id, harness, native_session, started_at)
               VALUES (1, 3, 'claude-code', 'zeus-one', '{AT}');"
        ))
        .expect("its conversation");
    let proven = |file: &PathBuf| -> (i64, Option<i64>) {
        let db = Connection::open(file).expect("the file");
        let version = db
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .expect("a version");
        let gone = db
            .query_row(
                "SELECT program_gone FROM conversation WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .ok();
        (version, gone)
    };
    assert_eq!(proven(&file), (22, None));

    drop(open_ledger(&file, Options::default()).expect("migrated and open"));
    assert_eq!(proven(&file), (24, Some(0)));
    let schema = schema_of(&file);
    drop(open_ledger(&file, Options::default()).expect("and open again"));
    assert_eq!(proven(&file), (24, Some(0)));
    assert_eq!(schema_of(&file), schema, "nothing migrated again");
}

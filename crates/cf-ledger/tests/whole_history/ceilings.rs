//! Every field the history reads but an item's text has a ceiling: the copy
//! keeps an item's id and its time within [`FIELD_MAX`] characters, a binding
//! keeps a native session within [`NATIVE_SESSION_MAX`], every event a heading
//! reads is logged within [`HEADING_EVENT_MAX`] bytes, and migration 0019
//! cuts once what an older build wrote past them, whatever it finds. So a read
//! goes through short fields alone, each charged as what it is, and loads no
//! long value: these read under a value limit a long one would fail.

use cf_ledger::{
    open_ledger, ChiefSwitch, HistoryEntry, HistoryFrom, Ledger, NewMember, Options, FIELD_MAX,
    HEADING_EVENT_MAX, HISTORY_PART, NATIVE_SESSION_MAX,
};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::old_logs::{of, version_18, AT};
use super::reads::{ask, chief, ids, ROOM};
use super::{everything, id_of, ledger_in, lines, project};

/// Each value's longest a read may load whole: a part of an item's text and a
/// little more. A read that loaded a longer value whole would fail.
const VALUE_LIMIT: usize = 64 * 1024;

/// An id as the copy keeps one longer than [`FIELD_MAX`] characters: its
/// first characters, "…", and 32 hexadecimal digits of the SHA-256 of `whole`.
fn kept(id: &str, whole: &[u8]) -> String {
    let digits: String = Sha256::digest(whole)
        .iter()
        .take(16)
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let head: String = id.chars().take(FIELD_MAX - 33).collect();
    format!("{head}…{digits}")
}

/// What `sql` gives of the database at `file`, with the ledger closed.
fn column<T: rusqlite::types::FromSql>(file: &std::path::Path, sql: &str) -> Vec<T> {
    let db = Connection::open(file).expect("the file");
    let mut statement = db.prepare(sql).expect("a query");
    statement
        .query_map([], |row| row.get(0))
        .expect("rows")
        .map(|value| value.expect("one"))
        .collect()
}

/// A time a time is read from, `digits` of a second's fraction long.
fn time(digits: usize) -> String {
    format!("2026-10-16T12:00:00.{}Z", "0".repeat(digits))
}

#[test]
fn the_copy_keeps_an_item_s_id_and_time_within_their_ceilings_and_a_long_id_still_its_own() {
    let dir = tempfile::tempdir().expect("a folder");
    let (mut ledger, project, conversation) = chief(&dir);
    // The reviewers' megabyte id, another that begins as it does, and a time
    // of twenty megabytes of a second's zeros, which reads as a time.
    let long = "i".repeat(1 << 20);
    let alike = format!("{long}x");
    assert!(cf_base::time::parse(&time(20 << 20)).is_some(), "a time");
    // A time as long as a field, and one a character longer.
    let edge = time(FIELD_MAX - 21);
    assert_eq!(edge.chars().count(), FIELD_MAX);
    let copied = |text: &str| {
        vec![
            json!({ "id": long, "role": "user", "text": text, "at": time(20 << 20) }),
            json!({ "id": alike, "role": "user", "text": "second", "at": edge }),
            json!({ "id": "short", "role": "user", "text": "third", "at": time(FIELD_MAX - 20) }),
        ]
    };
    ledger
        .copy_transcript(conversation, &copied("first"), 0)
        .expect("copied");
    let page = |ledger: &Ledger| {
        let read = ledger
            .read_history(
                project,
                "chief",
                &ask(HistoryFrom::Newest, None, false, ROOM),
            )
            .expect("a page");
        read.entries
            .iter()
            .filter_map(|entry| match entry {
                HistoryEntry::Item(row) => Some((
                    row.item.id.clone(),
                    row.item.text.clone(),
                    row.item.at.clone(),
                )),
                HistoryEntry::Heading(_) => None,
            })
            .collect::<Vec<_>>()
    };
    let shown = page(&ledger);
    assert_eq!(
        shown,
        [
            ("short".to_owned(), "third".to_owned(), None),
            (
                kept(&alike, alike.as_bytes()),
                "second".to_owned(),
                Some(edge.clone())
            ),
            (kept(&long, long.as_bytes()), "first".to_owned(), None),
        ]
    );
    assert_ne!(
        shown[1].0, shown[2].0,
        "two ids that begin alike are kept apart"
    );
    assert_eq!(shown[2].0.chars().count(), FIELD_MAX);
    // Copied again changed, the long id finds its row, the next copy of it.
    let row = super::reads::row_of(&ledger, project, &shown[2].0);
    let before = ledger
        .read_history_part(project, "chief", conversation, row, 0)
        .expect("a part")
        .revision;
    ledger
        .copy_transcript(conversation, &copied("first, again"), 0)
        .expect("copied");
    assert_eq!(page(&ledger)[2].1, "first, again");
    assert_eq!(page(&ledger).len(), 3);
    let after = ledger
        .read_history_part(project, "chief", conversation, row, 0)
        .expect("a part")
        .revision;
    assert_eq!(after, before + 1);
    ledger.close().expect("closed");
    let file = dir.path().join("consensflow.db");
    let longest: Vec<i64> = column(
        &file,
        "SELECT max(max(length(item_id)), max(ifnull(length(at), 0))) FROM transcript",
    );
    assert_eq!(longest, [i64::try_from(FIELD_MAX).expect("small")]);
}

#[test]
fn a_long_item_s_megabyte_id_an_older_build_copied_is_cut_once_and_a_search_through_its_parts_never_loads_it(
) {
    let dir = tempfile::tempdir().expect("a folder");
    let started = of("conversation.started", 1);
    let file = version_18(&dir, 1, &[(started.0, started.1.as_str())]);
    // The reviewers' item: a megabyte id, its text three parts and more with
    // its words in the last; another as long that begins alike; and one whose
    // id is no UTF-8.
    let long = "i".repeat(1 << 20);
    let alike = format!("{long}x");
    let text = format!("{} the NEEDLE", "a".repeat(HISTORY_PART * 3));
    let db = Connection::open(&file).expect("the file");
    for (seq, id) in [long.as_bytes(), alike.as_bytes()].into_iter().enumerate() {
        db.execute(
            "INSERT INTO transcript (conversation_id, item_id, seq, role, text, complete, at, copied_at)
               VALUES (1, CAST(? AS TEXT), ?, 'assistant', ?, 1, NULL, ?)",
            params![id, i64::try_from(seq + 1).expect("small"), text, AT],
        )
        .expect("copied");
    }
    let odd = [b"j".repeat(300), b"\xED\xA0\x80".to_vec()].concat();
    db.execute(
        "INSERT INTO transcript (conversation_id, item_id, seq, role, text, complete, at, copied_at)
           VALUES (1, CAST(? AS TEXT), 3, 'user', 'odd', 1, NULL, ?)",
        params![odd, AT],
    )
    .expect("copied");
    drop(db);
    let ledger = open_ledger(&file, Options::default()).expect("migrated and open");
    ledger.limit_value_length(VALUE_LIMIT).expect("held");
    let mut found = Vec::new();
    let mut from = HistoryFrom::Newest;
    for _ in 0..20 {
        let read = ledger
            .read_history(1, "chief", &ask(from, Some("needle"), false, ROOM))
            .expect("a search");
        found.extend(ids(&read));
        let Some(place) = read.resume else { break };
        from = HistoryFrom::Before(place);
    }
    assert_eq!(
        found,
        [kept(&alike, alike.as_bytes()), kept(&long, long.as_bytes())]
    );
    let page = ledger
        .read_history(1, "chief", &ask(HistoryFrom::Newest, None, false, ROOM))
        .expect("a page");
    let odd_id = String::from_utf8_lossy(&odd).into_owned();
    assert_eq!(ids(&page)[0], kept(&odd_id, &odd), "its hash of its bytes");
    // Every part of the long one, read by its place.
    let row = page.entries[2].key().row;
    let mut whole = String::new();
    let mut next = Some(0);
    // Four parts: a read that never gets to the end is stopped at ten.
    for _ in 0..10 {
        let Some(at) = next else { break };
        let part = ledger
            .read_history_part(1, "chief", 1, row, at)
            .expect("a part");
        whole.push_str(&part.text);
        next = part.next;
    }
    assert_eq!(whole, text);
    drop(ledger);
    let longest: Vec<i64> = column(
        &file,
        "SELECT max(length(item_id)) FROM transcript UNION ALL SELECT max(length(item_id)) FROM transcript_part",
    );
    assert_eq!(longest, [256, 256]);
}

#[test]
fn an_item_s_long_time_an_older_build_copied_is_none_and_a_page_never_loads_it() {
    let dir = tempfile::tempdir().expect("a folder");
    let started = of("conversation.started", 1);
    let file = version_18(&dir, 1, &[(started.0, started.1.as_str())]);
    // The reviewers' twenty items of two characters, each with a time of a
    // mebibyte; and one with a time as long as a field.
    let db = Connection::open(&file).expect("the file");
    for at in 0..20 {
        db.execute(
            "INSERT INTO transcript (conversation_id, item_id, seq, role, text, complete, at, copied_at)
               VALUES (1, ?, ?, 'user', 'ok', 1, ?, ?)",
            params![format!("t-{at}"), at + 1, time(1 << 20), AT],
        )
        .expect("copied");
    }
    db.execute(
        "INSERT INTO transcript (conversation_id, item_id, seq, role, text, complete, at, copied_at)
           VALUES (1, 'edge', 30, 'user', 'ok', 1, ?, ?)",
        params![time(FIELD_MAX - 21), AT],
    )
    .expect("copied");
    drop(db);
    let ledger = open_ledger(&file, Options::default()).expect("migrated and open");
    ledger.limit_value_length(VALUE_LIMIT).expect("held");
    let read = ledger
        .read_history(
            1,
            "chief",
            &ask(HistoryFrom::Newest, Some("nowhere"), false, ROOM),
        )
        .expect("a search");
    assert_eq!((read.scanned.rows, read.resume), (22, None));
    let page = ledger
        .read_history(1, "chief", &ask(HistoryFrom::Newest, None, false, ROOM))
        .expect("a page");
    let times: Vec<Option<String>> = page
        .entries
        .iter()
        .filter_map(|entry| match entry {
            HistoryEntry::Item(row) => Some(row.item.at.clone()),
            HistoryEntry::Heading(_) => None,
        })
        .collect();
    assert_eq!(times[0], Some(time(FIELD_MAX - 21)));
    assert!(
        times[1..21].iter().all(Option::is_none),
        "{:?}",
        &times[1..]
    );
}

#[test]
fn the_events_an_older_build_logged_long_are_cut_once_to_what_a_heading_reads_of_them() {
    let dir = tempfile::tempdir().expect("a folder");
    // The reviewers' twenty windows gone to the next conversation, each event
    // of a mebibyte of JSON; the last conversation's end, as long; and a start
    // as long whose handle is longer than a field's.
    let pad = "z".repeat(1 << 20);
    let mut events = vec![of("conversation.started", 1)];
    for to in 2..=21 {
        events.push((
            "conversation.followed",
            format!(r#"{{"conversation": {to}, "participant": "chief", "pad": "{pad}"}}"#),
        ));
    }
    events.push((
        "conversation.ended",
        format!(r#"{{"conversation": 21, "pad": "{pad}"}}"#),
    ));
    events.push((
        "conversation.started",
        format!(
            r#"{{"conversation": 22, "participant": "{}", "pad": "{pad}"}}"#,
            "c".repeat(257)
        ),
    ));
    let events: Vec<(&str, &str)> = events
        .iter()
        .map(|(kind, data)| (*kind, data.as_str()))
        .collect();
    let file = version_18(&dir, 22, &events);
    let ledger = open_ledger(&file, Options::default()).expect("migrated and open");
    ledger.limit_value_length(VALUE_LIMIT).expect("held");
    let history = lines(&everything(&ledger, 1, "chief").expect("read"));
    assert_eq!(history.len(), 22);
    assert_eq!(
        history[0],
        r#"codex diana Some(Window { resumed: false }) ["user: said in 1"]"#
    );
    assert_eq!(
        history[19],
        r#"codex diana Some(Window { resumed: false }) ["user: said in 20"]"#
    );
    // The twenty-first ended, which says no more.
    assert_eq!(history[20], r#"codex diana None ["user: said in 21"]"#);
    drop(ledger);
    let cut: Vec<String> = column(
        &file,
        "SELECT data FROM event WHERE kind LIKE 'conversation.%' AND data LIKE '%cutFromBytes%' ORDER BY id",
    );
    assert_eq!(cut.len(), 22);
    assert_eq!(
        cut[0],
        format!(
            r#"{{"conversation":2,"participant":"chief","cutFromBytes":{}}}"#,
            events[1].1.len()
        )
    );
    assert_eq!(
        cut[20],
        format!(
            r#"{{"conversation":21,"participant":null,"cutFromBytes":{}}}"#,
            events[21].1.len()
        )
    );
    assert_eq!(
        cut[21],
        format!(
            r#"{{"conversation":22,"participant":null,"cutFromBytes":{}}}"#,
            events[22].1.len()
        )
    );
    let longest: Vec<i64> = column(&file, "SELECT max(octet_length(data)) FROM event");
    assert!(
        longest[0] <= i64::try_from(HEADING_EVENT_MAX).expect("small"),
        "{longest:?}"
    );
}

#[test]
fn a_newest_conversation_an_older_build_bound_to_a_17_mib_session_is_read_and_older_goes_on_past_it(
) {
    let dir = tempfile::tempdir().expect("a folder");
    let events = [of("conversation.started", 1), of("conversation.started", 2)];
    let events: Vec<(&str, &str)> = events
        .iter()
        .map(|(kind, data)| (*kind, data.as_str()))
        .collect();
    let file = version_18(&dir, 2, &events);
    // The reviewers' 17 MiB native session on the newest conversation; and a
    // mebibyte harness and times on the one before it.
    let db = Connection::open(&file).expect("the file");
    db.execute(
        "UPDATE conversation SET native_session = ? WHERE id = 2",
        ["s".repeat(17 << 20)],
    )
    .expect("a long session");
    db.execute(
        "UPDATE conversation SET harness = ?1 || 'h', native_session = 's-1', started_at = ?1 || 's', ended_at = ?1 || 'e' WHERE id = 1",
        ["é".repeat(1 << 19)],
    )
    .expect("long fields");
    drop(db);
    let ledger = open_ledger(&file, Options::default()).expect("migrated and open");
    ledger.limit_value_length(VALUE_LIMIT).expect("held");
    // An entry a read, back from the newest: both conversations, each headed.
    let history = everything(&ledger, 1, "chief").expect("read");
    assert_eq!(history.len(), 2);
    let (older, newest) = (&history[0].0.conversation, &history[1].0.conversation);
    assert_eq!(newest.native_session, None);
    let cut = format!("{}…", "é".repeat(FIELD_MAX - 1));
    assert_eq!(
        (
            older.harness.clone(),
            older.native_session.clone(),
            older.started_at.clone(),
            older.ended_at.clone()
        ),
        (cut.clone(), None, cut.clone(), Some(cut))
    );
    // A page of the newest goes on to the conversation before it.
    let first = ledger
        .read_history(1, "chief", &ask(HistoryFrom::Newest, None, false, 1))
        .expect("a page");
    assert_eq!(
        (ids(&first), first.resume.map(|key| key.conversation)),
        (vec!["i-2".to_owned()], Some(2))
    );
}

#[test]
fn the_migration_s_cuts_never_fail_on_what_they_would_make_one() {
    let dir = tempfile::tempdir().expect("a folder");
    let started = of("conversation.started", 1);
    let file = version_18(&dir, 1, &[(started.0, started.1.as_str())]);
    let long = "p".repeat(300);
    // An item whose id is the one the cut makes of another's, as only an id
    // written to be it could be.
    let taken = kept(&long, long.as_bytes());
    let db = Connection::open(&file).expect("the file");
    for (seq, id) in [long.as_str(), taken.as_str()].into_iter().enumerate() {
        db.execute(
            "INSERT INTO transcript (conversation_id, item_id, seq, role, text, complete, at, copied_at)
               VALUES (1, ?, ?, 'user', ?, 1, NULL, ?)",
            params![id, i64::try_from(seq + 1).expect("small"), id, AT],
        )
        .expect("copied");
    }
    // Two conversations on one session whose harnesses begin alike past a
    // field's length; a session of 600 characters; one of 512 characters of
    // two bytes each, as long as one may be.
    let harness = "h".repeat(300);
    db.execute_batch(&format!(
        "INSERT INTO conversation (id, participant_id, harness, native_session, started_at, ended_at)
           VALUES (2, 2, '{harness}a', 's-1', '{AT}', '{AT}'), (3, 2, '{harness}b', 's-1', '{AT}', '{AT}'),
                  (4, 2, 'codex', '{}', '{AT}', '{AT}'), (5, 2, 'codex', '{}', '{AT}', '{AT}');",
        "s".repeat(600),
        "é".repeat(NATIVE_SESSION_MAX)
    ))
    .expect("conversations");
    drop(db);
    let ledger = open_ledger(&file, Options::default()).expect("migrated and open");
    let read = ledger
        .read_history(1, "chief", &ask(HistoryFrom::Newest, None, false, ROOM))
        .expect("a page");
    let shown = ids(&read);
    assert!(shown.contains(&taken), "{shown:?}");
    assert_eq!(shown.len(), 3, "{shown:?}");
    assert!(shown.iter().all(|id| id.chars().count() <= FIELD_MAX));
    let texts: Vec<String> = read
        .entries
        .iter()
        .filter_map(|entry| match entry {
            HistoryEntry::Item(row) => Some(row.item.text.clone()),
            HistoryEntry::Heading(_) => None,
        })
        .collect();
    assert!(
        texts.contains(&long),
        "the long one is there, under its own id"
    );
    drop(ledger);
    let sessions: Vec<Option<String>> = column(
        &file,
        "SELECT native_session FROM conversation WHERE id > 1 ORDER BY id",
    );
    assert_eq!(
        sessions,
        [None, None, None, Some("é".repeat(NATIVE_SESSION_MAX))]
    );
}

#[test]
fn every_event_a_heading_reads_is_logged_within_its_ceiling_however_long_what_it_names() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger = ledger_in(&dir);
    let project = project(&mut ledger);
    // The longest agent a member's handle can be, and the longest native
    // session, of characters JSON escapes in six bytes each.
    let agent = "a".repeat(64);
    let member = ledger
        .add_member(
            project.id,
            &NewMember {
                agent: agent.clone(),
                harness: "opencode".into(),
                designer: false,
                roles: vec!["worker".into()],
                tier: "standard".into(),
            },
        )
        .expect("a member");
    let session = "\u{1}".repeat(NATIVE_SESSION_MAX);
    for participant in [member.id, id_of(&project, "chief")] {
        ledger
            .start_conversation(participant, "opencode")
            .expect("a conversation");
        let followed = ledger
            .follow_conversation(participant, "opencode", &session)
            .expect("followed");
        ledger.end_conversation(followed.id).expect("ended");
    }
    ledger
        .switch_chief(
            project.id,
            &ChiefSwitch {
                harness: "claude-code".into(),
                agent: "b".repeat(64),
                cut: true,
            },
        )
        .expect("switched");
    ledger.close().expect("closed");
    let longest: Vec<i64> = column(
        &dir.path().join("consensflow.db"),
        "SELECT max(octet_length(data)) FROM event WHERE kind IN
           ('conversation.started', 'conversation.followed', 'conversation.ended', 'chief.switched')
         GROUP BY kind ORDER BY kind",
    );
    assert_eq!(longest.len(), 4);
    assert!(
        longest
            .iter()
            .all(|bytes| *bytes <= i64::try_from(HEADING_EVENT_MAX).expect("small")),
        "{longest:?}"
    );
    assert!(
        longest[2] > 3_072,
        "the session's escapes are in it: {longest:?}"
    );
}

#[test]
fn every_field_a_read_goes_through_is_charged_as_what_it_is() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger = ledger_in(&dir);
    let project = project(&mut ledger);
    let chief = id_of(&project, "chief");
    let copy = |ledger: &mut Ledger, conversation: i64, at: usize| {
        let items: Vec<Value> = (0..3)
            .map(|n| {
                json!({
                    "id": format!("{conversation}-{n}-{}", "d".repeat(30 + n * 50)),
                    "role": "user",
                    "text": "said",
                    "at": time(at + n),
                })
            })
            .collect();
        ledger
            .copy_transcript(conversation, &items, 0)
            .expect("copied");
    };
    // A conversation bound to its session, ended by a Switch chief; the one
    // after it, going on now.
    let first = ledger
        .start_conversation(chief, "claude-code")
        .expect("a conversation");
    ledger
        .bind_conversation(first.id, &"s".repeat(NATIVE_SESSION_MAX))
        .expect("bound");
    copy(&mut ledger, first.id, 3);
    ledger
        .switch_chief(
            project.id,
            &ChiefSwitch {
                harness: "codex".into(),
                agent: "maia".into(),
                cut: false,
            },
        )
        .expect("switched");
    let now = ledger
        .start_conversation(chief, "codex")
        .expect("a conversation");
    copy(&mut ledger, now.id, 100);
    let read = ledger
        .read_history(
            project.id,
            "chief",
            &ask(HistoryFrom::Newest, None, false, ROOM),
        )
        .expect("a page");
    assert_eq!((read.entries.len(), read.resume), (6, None));
    ledger.close().expect("closed");
    let file = dir.path().join("consensflow.db");
    let sum = |sql: &str| column::<i64>(&file, sql)[0];
    let conversations = sum(
        "SELECT sum(octet_length(harness) + ifnull(octet_length(native_session), 0)
           + octet_length(started_at) + ifnull(octet_length(ended_at), 0)) FROM conversation",
    );
    let rows =
        sum("SELECT sum(octet_length(item_id) + ifnull(octet_length(at), 0)) FROM transcript");
    // The first conversation's heading reads the switch twice (the agent it
    // began on, what ended it) and where its window went after (the next's
    // start); the current one's, nothing of the log's data.
    let switch = sum("SELECT octet_length(data) FROM event WHERE kind = 'chief.switched'");
    let next = sum(
        "SELECT octet_length(data) FROM event WHERE kind = 'conversation.started' ORDER BY id DESC LIMIT 1",
    );
    let charged = usize::try_from(conversations + rows + 2 * switch + next).expect("small");
    assert_eq!(read.scanned.fields, charged, "{:?}", read.scanned);
}

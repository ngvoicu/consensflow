//! An item's id is its identity: the copy keeps it whole, and so does
//! migration 0020, however long; the history reads only its head (the id as a
//! page shows it, and its fields), never the id. Every other field the history
//! reads has a ceiling: a time is kept as a store keeps one, a harness or an
//! agent is cut to [`FIELD_MAX`] bytes, a native session within
//! [`NATIVE_SESSION_MAX`], and every event a heading reads within
//! [`HEADING_EVENT_MAX`] bytes; migration 0020 brings once what an older build
//! wrote past them within them, measured in bytes, never by SQLite's `length()`,
//! which stops at a NUL. So a read goes through short fields alone, each
//! charged as what it is, and loads no long value: these read under a value
//! limit a long one would fail.

use cf_ledger::{
    open_ledger, ChiefSwitch, HistoryEntry, HistoryFrom, Ledger, NewMember, Options, FIELD_MAX,
    HEADING_EVENT_MAX, HISTORY_PART, ID_SHOWN, NATIVE_SESSION_MAX,
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

/// What the reviewers wrote in every capped field: an "x", a NUL, which
/// SQLite's `length()` and `substr()` stop at, and a mebibyte.
fn nul() -> String {
    format!("x\u{0}{}", "y".repeat(1 << 20))
}

/// The id round 5 cut a longer one to where it stored it: its first 223
/// characters, "…", and 32 hexadecimal digits of the SHA-256 of the whole of
/// it. An id a harness could give as it is, which round 5 took for the long one.
fn round_five_cut(id: &str) -> String {
    let digits: String = Sha256::digest(id.as_bytes())
        .iter()
        .take(16)
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let head: String = id.chars().take(223).collect();
    format!("{head}…{digits}")
}

/// An id as a page shows it: its first [`ID_SHOWN`] characters and "…".
fn shown(id: &str) -> String {
    format!("{}…", id.chars().take(ID_SHOWN).collect::<String>())
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

/// A time a time is read from, its fraction `fraction`.
fn time(fraction: &str) -> String {
    format!("2026-10-16T12:00:00.{fraction}Z")
}

/// The items of a page, newest first: each one's id as the page shows it, its text and its time.
fn items(ledger: &Ledger, project: i64) -> Vec<(String, String, Option<String>)> {
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
        .collect()
}

#[test]
fn the_copy_keeps_each_id_whole_so_one_round_5_cut_and_the_id_it_cut_it_to_stay_two_items() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger = ledger_in(&dir);
    let view = project(&mut ledger);
    let (project, chief) = (view.id, id_of(&view, "chief"));
    let conversation = ledger
        .start_conversation(chief, "claude-code")
        .expect("a conversation")
        .id;
    // The reviewers' sequence: a long id, and an id equal to what round 5 cut
    // it to; and the NUL-bearing mebibyte.
    let long = "p".repeat(300);
    let cut = round_five_cut(&long);
    let copied = |text: &str| {
        vec![
            json!({ "id": long, "role": "user", "text": format!("[mark] {text}") }),
            json!({ "id": cut, "role": "user", "text": "the other" }),
            json!({ "id": nul(), "role": "user", "text": "third" }),
        ]
    };
    ledger
        .copy_transcript(conversation, &copied("first"), 0)
        .expect("copied");
    let expect = |text: &str| {
        vec![
            (shown(&nul()), "third".to_owned(), None),
            (cut.clone(), "the other".to_owned(), None),
            (shown(&long), format!("[mark] {text}"), None),
        ]
    };
    assert_eq!(items(&ledger, project), expect("first"));
    // Copied again changed, the long one is written in its own row, and the
    // other stays as it was; a delivery's match names the id the harness gave.
    ledger
        .copy_transcript(conversation, &copied("again"), 0)
        .expect("copied");
    assert_eq!(items(&ledger, project), expect("again"));
    assert_eq!(
        ledger.copied_item_with(chief, "[mark]").expect("found"),
        Some(long.clone())
    );
    ledger.close().expect("closed");
    let stored: Vec<i64> = column(
        &dir.path().join("consensflow.db"),
        "SELECT octet_length(item_id) FROM transcript ORDER BY rowid",
    );
    assert_eq!(stored, [300, 258, (1 << 20) + 2], "each id whole");
}

#[test]
fn the_copy_keeps_a_time_as_a_store_keeps_one_however_it_was_written() {
    let dir = tempfile::tempdir().expect("a folder");
    let (mut ledger, project, conversation) = chief(&dir);
    // Twenty mebibytes of a second's fraction, an offset, no seconds, and no time.
    let times = [
        (
            time(&format!("123456789{}", "1".repeat(20 << 20))),
            Some(time("123456789")),
        ),
        ("2026-10-16T14:00:00.25+02:00".to_owned(), Some(time("250"))),
        ("2026-10-16T12:00Z".to_owned(), Some(time("000"))),
        (nul(), None),
    ];
    let copied: Vec<Value> = times
        .iter()
        .enumerate()
        .map(|(at, (given, _))| json!({ "id": format!("t-{at}"), "role": "user", "text": "ok", "at": given }))
        .collect();
    ledger
        .copy_transcript(conversation, &copied, 0)
        .expect("copied");
    let mut kept: Vec<Option<String>> = items(&ledger, project)
        .into_iter()
        .map(|(_, _, at)| at)
        .collect();
    kept.reverse();
    let expected: Vec<Option<String>> = times.iter().map(|(_, kept)| kept.clone()).collect();
    assert_eq!(kept, expected);
    ledger.close().expect("closed");
    let stored: Vec<Option<String>> = column(
        &dir.path().join("consensflow.db"),
        "SELECT at FROM transcript ORDER BY rowid",
    );
    assert_eq!(stored, expected, "the row's own time too");
}

#[test]
fn a_member_out_until_a_time_is_kept_and_logged_out_until_it_as_a_store_keeps_one() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger = ledger_in(&dir);
    let project = project(&mut ledger);
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
    // The reviewers' reset: twenty mebibytes of a second's zeros.
    let reset = format!("2027-01-01T00:00:00.5{}Z", "0".repeat(20 << 20));
    let out = ledger
        .mark_out(member.id, &reset, "out of quota")
        .expect("marked out");
    assert_eq!(out.out_until.as_deref(), Some("2027-01-01T00:00:00.500Z"));
    ledger.close().expect("closed");
    let file = dir.path().join("consensflow.db");
    let kept: Vec<i64> = column(
        &file,
        "SELECT octet_length(out_until) FROM participant WHERE out_until IS NOT NULL",
    );
    assert_eq!(kept, [24]);
    let logged: Vec<String> = column(
        &file,
        "SELECT json_extract(data, '$.until') FROM event WHERE kind = 'member.out'",
    );
    assert_eq!(logged, ["2027-01-01T00:00:00.500Z"]);
}

#[test]
fn a_version_18_ledger_keeps_its_ids_whole_its_collision_two_items_and_its_parts_read_by_row() {
    let dir = tempfile::tempdir().expect("a folder");
    let started = of("conversation.started", 1);
    let file = version_18(&dir, 1, &[(started.0, started.1.as_str())]);
    // The reviewers' item, a mebibyte id on a text of three parts and more with
    // its words in the last; an id equal to what round 5 cut it to; and an id
    // that is no UTF-8.
    let long = "i".repeat(1 << 20);
    let cut = round_five_cut(&long);
    let text = format!("{} the NEEDLE", "a".repeat(HISTORY_PART * 3));
    let db = Connection::open(&file).expect("the file");
    let mut copy = db
        .prepare(
            "INSERT INTO transcript (conversation_id, item_id, seq, role, text, complete, at, copied_at)
               VALUES (1, CAST(? AS TEXT), ?, ?, ?, 1, NULL, ?)",
        )
        .expect("a copy");
    let odd = [b"j".repeat(300), b"\xED\xA0\x80".to_vec()].concat();
    copy.execute(params![long.as_bytes(), 1, "assistant", text, AT])
        .expect("copied");
    copy.execute(params![cut.as_bytes(), 2, "user", "the other", AT])
        .expect("copied");
    copy.execute(params![odd, 3, "user", "odd", AT])
        .expect("copied");
    drop(copy);
    drop(db);
    let mut ledger = open_ledger(&file, Options::default()).expect("migrated and open");
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
    assert_eq!(found, [shown(&long)]);
    let page = ledger
        .read_history(1, "chief", &ask(HistoryFrom::Newest, None, false, ROOM))
        .expect("a page");
    let odd_id = String::from_utf8_lossy(&odd).into_owned();
    assert_eq!(
        ids(&page),
        [shown(&odd_id), cut.clone(), shown(&long), "i-1".to_owned()]
    );
    // Every part of the long one, read by its row.
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
    // Copied again changed, the long one is written in its own row; the other
    // stays as it was.
    ledger.limit_value_length(usize::MAX).expect("let go");
    ledger
        .copy_transcript(
            1,
            &[json!({ "id": long, "role": "assistant", "text": "now short" })],
            0,
        )
        .expect("copied");
    let texts: Vec<String> = items(&ledger, 1)
        .into_iter()
        .map(|(_, text, _)| text)
        .collect();
    assert_eq!(texts, ["odd", "the other", "now short", "said in 1"]);
    drop(ledger);
    let stored: Vec<i64> = column(
        &file,
        "SELECT octet_length(item_id) FROM transcript ORDER BY rowid",
    );
    assert_eq!(stored, [3, 1 << 20, 258, 303], "each id whole");
}

#[test]
fn a_version_18_ledger_s_capped_fields_holding_a_nul_and_a_mebibyte_are_brought_within_their_ceilings_once(
) {
    let dir = tempfile::tempdir().expect("a folder");
    // A switch whose names hold the NUL and the mebibyte, and four of 256
    // characters JSON writes in six bytes each, as the reviewers' 6 KB one.
    let control = "\u{1}".repeat(256);
    let switches = [
        json!({ "from": { "harness": "codex", "agent": nul() }, "to": { "harness": nul(), "agent": "maia" } }),
        json!({ "from": { "harness": control, "agent": control }, "to": { "harness": control, "agent": control }, "pad": "z".repeat(5_000) }),
    ];
    let events = [
        of("conversation.started", 1),
        ("chief.switched", switches[0].to_string()),
        of("conversation.started", 2),
        ("chief.switched", switches[1].to_string()),
        of("conversation.started", 3),
        // A start whose handle holds the NUL and the mebibyte.
        (
            "conversation.started",
            json!({ "conversation": 9, "participant": nul() }).to_string(),
        ),
    ];
    let events: Vec<(&str, &str)> = events
        .iter()
        .map(|(kind, data)| (*kind, data.as_str()))
        .collect();
    let file = version_18(&dir, 3, &events);
    let db = Connection::open(&file).expect("the file");
    db.execute(
        "UPDATE conversation SET harness = ?1, started_at = ?1, ended_at = ?1 WHERE id = 1",
        [nul()],
    )
    .expect("long conversation fields");
    db.execute(
        "UPDATE conversation SET native_session = ?1, started_at = '2026-10-16T14:00:00+02:00' WHERE id = 2",
        [nul()],
    )
    .expect("a long session");
    db.execute(
        "UPDATE transcript SET at = ?1 WHERE conversation_id = 1",
        [nul()],
    )
    .expect("a long time");
    db.execute(
        "UPDATE transcript SET at = ?1, item_id = ?2 WHERE conversation_id = 2",
        [time(&format!("5{}", "0".repeat(1 << 20))), nul()],
    )
    .expect("a long id and a long time");
    db.execute(
        "UPDATE participant SET agent = ?1, out_until = ?1 WHERE handle = 'chief'",
        [nul()],
    )
    .expect("a long agent");
    drop(db);
    let ledger = open_ledger(&file, Options::default()).expect("migrated and open");
    ledger.limit_value_length(VALUE_LIMIT).expect("held");
    let cut = "x\u{0}".to_owned() + &"y".repeat(FIELD_MAX - 5) + "…";
    let history = lines(&everything(&ledger, 1, "chief").expect("read"));
    assert_eq!(
        history,
        [
            format!(
                r#"{cut} - Some(Switch {{ harness: "", agent: Some("maia") }}) ["user: said in 1"]"#
            ),
            r#"codex - Some(Switch { harness: "", agent: None }) ["user: said in 2"]"#.to_owned(),
            format!(r#"codex {cut} None ["user: said in 3"]"#),
        ]
    );
    let page = ledger
        .read_history(
            1,
            "chief",
            &ask(HistoryFrom::Newest, Some("said"), false, ROOM),
        )
        .expect("a search");
    assert_eq!(page.entries.len(), 3);
    drop(ledger);
    let longest = |sql: &str| column::<i64>(&file, sql)[0];
    for (field, most) in [
        (
            "SELECT max(octet_length(harness)) FROM conversation",
            FIELD_MAX,
        ),
        (
            "SELECT max(octet_length(started_at)) FROM conversation",
            FIELD_MAX,
        ),
        (
            "SELECT max(octet_length(ended_at)) FROM conversation",
            FIELD_MAX,
        ),
        (
            "SELECT max(octet_length(agent)) FROM participant",
            FIELD_MAX,
        ),
        (
            "SELECT max(ifnull(octet_length(at), 0)) FROM transcript_head",
            64,
        ),
        (
            "SELECT max(octet_length(id_shown)) FROM transcript_head",
            4 * ID_SHOWN + 3,
        ),
        (
            "SELECT max(octet_length(data)) FROM event WHERE kind = 'chief.switched'",
            HEADING_EVENT_MAX,
        ),
    ] {
        let found = usize::try_from(longest(field)).expect("a size");
        assert!(found <= most, "{field}: {found}");
    }
    let unbounded: Vec<i64> = column(
        &file,
        "SELECT COUNT(*) FROM conversation WHERE native_session IS NOT NULL",
    );
    assert_eq!(unbounded, [0], "the cut harness's and the long one");
    let times: Vec<Option<String>> = column(
        &file,
        "SELECT at FROM transcript WHERE conversation_id IN (1, 2) ORDER BY rowid",
    );
    assert_eq!(times, [None, Some(time("500"))]);
    let started: Vec<String> = column(&file, "SELECT started_at FROM conversation WHERE id = 2");
    assert_eq!(started, [time("000")]);
    let until: Vec<Option<String>> = column(
        &file,
        "SELECT out_until FROM participant WHERE handle = 'chief'",
    );
    assert_eq!(until, [None]);
    let names: Vec<String> = column(
        &file,
        "SELECT json_extract(data, '$.from') FROM event WHERE kind = 'chief.switched' ORDER BY id",
    );
    assert_eq!(
        names,
        [
            r#"{"harness":"codex","agent":null}"#,
            r#"{"harness":null,"agent":null}"#
        ]
    );
    let handles: Vec<Option<String>> = column(
        &file,
        "SELECT json_extract(data, '$.participant') FROM event
         WHERE kind = 'conversation.started' AND json_extract(data, '$.conversation') = 9",
    );
    assert_eq!(handles, [None]);
    let longest: Vec<i64> = column(&file, "SELECT max(octet_length(data)) FROM event");
    assert!(
        longest[0] <= i64::try_from(HEADING_EVENT_MAX).expect("small"),
        "{longest:?}"
    );
}

#[test]
fn the_events_an_older_build_logged_long_are_cut_once_to_what_a_heading_reads_of_them() {
    let dir = tempfile::tempdir().expect("a folder");
    // The reviewers' twenty windows gone to the next conversation, each event
    // of a mebibyte of JSON; the last conversation's end, as long; and a start
    // as long whose handle JSON writes in more than 258 bytes.
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
fn a_read_of_the_history_reads_of_a_participant_and_its_project_only_what_it_uses() {
    let dir = tempfile::tempdir().expect("a folder");
    let started = of("conversation.started", 1);
    let file = version_18(&dir, 1, &[(started.0, started.1.as_str())]);
    // Fields no read of the history uses, as an older build could have left
    // them: a project's name and a participant's roles of a mebibyte each, and
    // the time it is out until of twenty.
    let db = Connection::open(&file).expect("the file");
    db.execute("UPDATE project SET name = ?", ["n".repeat(1 << 20)])
        .expect("a long name");
    db.execute(
        "UPDATE participant SET roles = ?1, out_until = ?2",
        [
            format!("[\"{}\"]", "r".repeat(1 << 20)),
            format!("2027-01-01T00:00:00.5{}Z", "0".repeat(20 << 20)),
        ],
    )
    .expect("long roles");
    drop(db);
    let ledger = open_ledger(&file, Options::default()).expect("migrated and open");
    ledger.limit_value_length(VALUE_LIMIT).expect("held");
    let read = ledger
        .read_history(1, "chief", &ask(HistoryFrom::Newest, None, false, ROOM))
        .expect("a page");
    let row = read.entries[0].key().row;
    assert_eq!(
        ledger
            .read_history_part(1, "chief", 1, row, 0)
            .expect("a part")
            .text,
        "said in 1"
    );
    // Its refusals are as they were.
    let refused = |handle: &str, project: i64| {
        ledger
            .read_history(
                project,
                handle,
                &ask(HistoryFrom::Newest, None, false, ROOM),
            )
            .unwrap_err()
            .to_string()
    };
    assert_eq!(refused("zeus", 1), "\"zeus\" is not in project 1");
    assert_eq!(refused("chief", 9), "no project 9");
    drop(ledger);
    let until: Vec<String> = column(&file, "SELECT out_until FROM participant");
    assert_eq!(
        until,
        ["2027-01-01T00:00:00.500Z"],
        "kept as a store keeps a time"
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
    // As many characters of two bytes as 256 bytes hold with "…".
    let cut = format!("{}…", "é".repeat((FIELD_MAX - 3) / 2));
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
    // Two conversations on one session whose harnesses begin alike past a
    // field's length; a session of 600 characters; one of 512 characters of
    // two bytes each, as long as one may be.
    let harness = "h".repeat(300);
    let db = Connection::open(&file).expect("the file");
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
    assert_eq!(everything(&ledger, 1, "chief").expect("read").len(), 5);
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
fn every_field_and_part_a_read_goes_through_is_charged_as_what_it_is() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger = ledger_in(&dir);
    let project = project(&mut ledger);
    let chief = id_of(&project, "chief");
    let copy = |ledger: &mut Ledger, conversation: i64, fraction: usize| {
        let items: Vec<Value> = (0..3)
            .map(|n| {
                json!({
                    "id": format!("{conversation}-{n}-{}", "d".repeat(30 + n * 50)),
                    "role": "user",
                    "text": "said",
                    "at": time(&"1".repeat(fraction + n)),
                })
            })
            .collect();
        ledger
            .copy_transcript(conversation, &items, 0)
            .expect("copied");
    };
    // A conversation bound to its session, ended by a Switch chief; the one
    // after it, going on now, its newest item of three parts and a 1,024-byte id.
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
    copy(&mut ledger, now.id, 9);
    let long = json!({
        "id": "🙂".repeat(ID_SHOWN),
        "role": "assistant",
        "text": "a".repeat(HISTORY_PART * 2 + 5),
    });
    ledger.copy_transcript(now.id, &[long], 3).expect("copied");
    let read = ledger
        .read_history(
            project.id,
            "chief",
            &ask(HistoryFrom::Newest, None, false, ROOM),
        )
        .expect("a page");
    assert_eq!((read.entries.len(), read.resume), (7, None));
    // The long one's first part: its size, then its text, a lookup each.
    assert_eq!(read.scanned.parts, 2, "{:?}", read.scanned);
    ledger.close().expect("closed");
    let file = dir.path().join("consensflow.db");
    let sum = |sql: &str| column::<i64>(&file, sql)[0];
    let conversations = sum(
        "SELECT sum(octet_length(harness) + ifnull(octet_length(native_session), 0)
           + octet_length(started_at) + ifnull(octet_length(ended_at), 0)) FROM conversation",
    );
    let rows = sum(
        "SELECT sum(octet_length(id_shown) + ifnull(octet_length(at), 0)) FROM transcript_head",
    );
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

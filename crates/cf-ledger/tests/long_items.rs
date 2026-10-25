//! A long item is kept in parts as well (migration 0020), so that the history
//! viewer reads a part of it and no more: the items a ledger copied before the
//! parts' table are split when it is made, once; one no longer than a part, in
//! characters, is not, however many bytes it takes.

#![allow(clippy::expect_used)]

use cf_ledger::{migrate, HISTORY_PART, MIGRATIONS};
use rusqlite::{params, Connection};

const AT: &str = "2026-10-16T12:00:00.000Z";

/// The parts kept of item `item`, in order, by its row.
fn parts(db: &Connection, item: &str) -> Vec<String> {
    db.prepare(
        "SELECT p.text FROM transcript_part p JOIN transcript t ON t.rowid = p.item
         WHERE t.item_id = ? ORDER BY p.part",
    )
    .expect("a query")
    .query_map([item], |row| row.get(0))
    .expect("the parts")
    .collect::<rusqlite::Result<_>>()
    .expect("each part")
}

#[test]
fn a_populated_version_19_ledger_has_its_long_items_split_into_parts_in_migration_20() {
    let db = Connection::open_in_memory().expect("a database");
    db.execute_batch("PRAGMA foreign_keys = ON").expect("keys");
    migrate(&db, &MIGRATIONS[..19]).expect("version 19");
    db.execute_batch(&format!(
        "INSERT INTO project (id, directory, name, state, gate, created_at, updated_at)
           VALUES (1, '/work/app', 'app', 'open', 0, '{AT}', '{AT}');
         INSERT INTO participant (id, project_id, handle, role, roles, agent, harness, tier, member_id, created_at, designer)
           VALUES (2, 1, 'chief', 'chief', '[]', 'diana', 'codex', NULL, NULL, '{AT}', 0);
         INSERT INTO conversation (id, participant_id, harness, started_at) VALUES (1, 2, 'codex', '{AT}');"
    ))
    .expect("a populated ledger");
    // Long, with a NUL in it, which SQLite's own text functions stop at; long
    // in bytes alone; short; and short of an id too long to read past.
    let long = format!("é\u{0}{}🙂 the end", "word ".repeat(HISTORY_PART / 2));
    let wide = "é".repeat(HISTORY_PART);
    let long_id = "i".repeat(2_048);
    let mut copy = db
        .prepare(
            "INSERT INTO transcript (conversation_id, item_id, seq, role, text, complete, at, copied_at)
             VALUES (1, ?, ?, 'assistant', ?, 1, NULL, ?)",
        )
        .expect("a copy");
    for (seq, (item, text)) in (0_i64..).zip([
        ("long", long.as_str()),
        ("wide", &wide),
        ("short", "ok"),
        (&long_id, "said"),
    ]) {
        copy.execute(params![item, seq, text, AT]).expect("copied");
    }
    drop(copy);

    migrate(&db, &MIGRATIONS[..20]).expect("migrated");
    migrate(&db, &MIGRATIONS[..20]).expect("and again");

    let kept = parts(&db, "long");
    assert_eq!(kept.len(), long.chars().count().div_ceil(HISTORY_PART));
    assert!(kept[..kept.len() - 1]
        .iter()
        .all(|part| part.chars().count() == HISTORY_PART));
    assert_eq!(kept.concat(), long, "the parts are the text, a NUL and all");
    assert!(
        parts(&db, "wide").is_empty(),
        "as many characters as a part"
    );
    assert!(parts(&db, "short").is_empty());
    assert_eq!(parts(&db, &long_id), ["said"], "kept in its one part");
    let counted: Vec<i64> = db
        .prepare("SELECT parts FROM transcript_head ORDER BY item")
        .expect("a query")
        .query_map([], |row| row.get(0))
        .expect("the heads")
        .collect::<rusqlite::Result<_>>()
        .expect("each head");
    assert_eq!(
        counted,
        [i64::try_from(kept.len()).expect("a count"), 0, 0, 1],
        "each head says how many parts its item is kept in"
    );
}

/// What migration 20 takes on a ledger of 200 MB of items of 4 KB, 100 long
/// ones of 1 MB and one of 24 MiB, and a log of 200,000 events of the kinds
/// its indexes read, each JSON checked as it is indexed, every item's head
/// written, with fields past their ceilings to bring within them (times of a
/// mebibyte and times with an offset, events, native sessions, and a harness,
/// times and an agent of a NUL and a mebibyte) and ids of a kilobyte kept
/// whole: what a ledger pays once, at its first open by this build. Run in release:
/// `cargo test --release -p cf-ledger --test long_items -- --ignored --nocapture`.
#[test]
#[ignore = "builds a 330 MB ledger; run by hand, in release"]
fn migration_20_splits_a_long_history_s_long_items_in_what_one_read_of_each_takes() {
    let dir = tempfile::tempdir().expect("a folder");
    let db = Connection::open(dir.path().join("consensflow.db")).expect("a database");
    db.execute_batch("PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL")
        .expect("pragmas");
    migrate(&db, &MIGRATIONS[..19]).expect("version 19");
    db.execute_batch(&format!(
        "INSERT INTO project (id, directory, name, state, gate, created_at, updated_at)
           VALUES (1, '/work/app', 'app', 'open', 0, '{AT}', '{AT}');
         INSERT INTO participant (id, project_id, handle, role, roles, agent, harness, tier, member_id, created_at, designer)
           VALUES (2, 1, 'chief', 'chief', '[]', 'diana', 'codex', NULL, NULL, '{AT}', 0);
         INSERT INTO conversation (id, participant_id, harness, started_at) VALUES (1, 2, 'codex', '{AT}');"
    ))
    .expect("a populated ledger");
    let started = std::time::Instant::now();
    db.execute_batch("BEGIN").expect("begun");
    let mut copy = db
        .prepare(
            "INSERT INTO transcript (conversation_id, item_id, seq, role, text, complete, at, copied_at)
             VALUES (1, ?, ?, 'user', ?, 1, ?, ?)",
        )
        .expect("a copy");
    let short = "w".repeat(4_000);
    let long = "l".repeat(1_000_000);
    let longest = "a".repeat(24 * 1024 * 1024);
    // Past the fields' ceilings, as an older build could have left them: one
    // item in fifty of an id of two kilobytes, too long to read its text past,
    // ten of a time of a mebibyte, one in a hundred of a time with an offset.
    let long_id = "x".repeat(2_000);
    let kilobyte = "x".repeat(1_000);
    let long_time = format!("2026-10-16T12:00:00.{}Z", "0".repeat(1 << 20));
    let offset = "2026-10-16T14:00:00.000+02:00";
    for seq in 0_i64..50_000 {
        let id = if seq % 50 == 0 {
            format!("i-{seq}-{long_id}")
        } else {
            format!("i-{seq}")
        };
        let at = match seq {
            0..10 => long_time.as_str(),
            _ if seq % 100 == 1 => offset,
            _ => AT,
        };
        copy.execute(params![id, seq, short, at, AT])
            .expect("copied");
    }
    for seq in 50_000_i64..50_100 {
        copy.execute(params![format!("i-{seq}"), seq, long, AT, AT])
            .expect("copied");
    }
    copy.execute(params!["longest", 50_100_i64, longest, AT, AT])
        .expect("copied");
    drop(copy);
    let mut log = db
        .prepare("INSERT INTO event (project_id, at, kind, data) VALUES (1, ?, ?, ?)")
        .expect("a log");
    let kinds = [
        "conversation.started",
        "conversation.followed",
        "conversation.ended",
        "chief.switched",
    ];
    // One event in two hundred of 8 KB, and ten of a time of a mebibyte.
    let pad = "z".repeat(8_000);
    for at in 0_i64..200_000 {
        let kind = kinds[usize::try_from(at % 4).expect("a kind")];
        let data = if at % 200 == 0 {
            format!(r#"{{"conversation": {at}, "participant": "chief", "pad": "{pad}"}}"#)
        } else {
            format!(r#"{{"conversation": {at}, "participant": "chief", "harness": "codex"}}"#)
        };
        let when = if at < 10 { long_time.as_str() } else { AT };
        log.execute(params![when, kind, data]).expect("logged");
    }
    drop(log);
    // A hundred ended conversations bound to sessions of a kilobyte.
    for id in 2_i64..102 {
        db.execute(
            "INSERT INTO conversation (id, participant_id, harness, native_session, started_at, ended_at)
               VALUES (?, 2, 'codex', ?, ?, ?)",
            params![id, format!("s-{id}-{kilobyte}"), AT, AT],
        )
        .expect("a conversation");
    }
    // The NUL and the mebibyte, in the first conversation's harness and times
    // and in the chief's agent and the time it is out until.
    let nul = format!("x\u{0}{}", "y".repeat(1 << 20));
    db.execute(
        "UPDATE conversation SET harness = ?1, started_at = ?1, ended_at = ?1 WHERE id = 1",
        [&nul],
    )
    .expect("long fields");
    db.execute(
        "UPDATE participant SET agent = ?1, out_until = ?1 WHERE id = 2",
        [&nul],
    )
    .expect("long fields");
    db.execute_batch("COMMIT").expect("committed");
    println!("built: {:?}", started.elapsed());
    let started = std::time::Instant::now();
    migrate(&db, &MIGRATIONS[..20]).expect("migrated");
    println!("migration 20: {:?}", started.elapsed());
    // The long items' parts, and the one part of each item of a long id; as
    // many as their heads say.
    let kept: (i64, i64) = db
        .query_row(
            "SELECT (SELECT COUNT(*) FROM transcript_part), (SELECT sum(parts) FROM transcript_head)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("the parts");
    assert_eq!(kept, (100 * 16 + 394 + 1_000, 100 * 16 + 394 + 1_000));
    // Each id whole, and each field within its ceiling: the long times and
    // those with an offset written back as a store keeps a time.
    let kept: Vec<i64> = db
        .query_row(
            "SELECT (SELECT max(octet_length(item_id)) FROM transcript),
                    (SELECT max(octet_length(id_shown)) FROM transcript_head),
                    (SELECT COUNT(*) FROM transcript_head),
                    (SELECT max(octet_length(at)) FROM transcript),
                    (SELECT COUNT(*) FROM transcript WHERE at = '2026-10-16T12:00:00.000Z'),
                    (SELECT COUNT(*) FROM conversation WHERE native_session IS NULL),
                    (SELECT max(octet_length(harness) + octet_length(started_at) + octet_length(ended_at)) FROM conversation),
                    (SELECT octet_length(agent) + ifnull(octet_length(out_until), 0) FROM participant),
                    (SELECT max(octet_length(data)) FROM event),
                    (SELECT max(octet_length(at)) FROM event)",
            [],
            |row| (0..10).map(|at| row.get(at)).collect(),
        )
        .expect("the fields");
    assert_eq!(kept[..3], [2_008, 256 + 3, 50_101]);
    assert_eq!(kept[3], 24);
    assert_eq!(kept[4], 50_101, "every time as a store keeps one");
    assert_eq!(kept[5], 101);
    assert!(kept[6] <= 3 * 256 && kept[7] <= 256, "{kept:?}");
    assert!(kept[8] <= 4_096, "{kept:?}");
    assert_eq!(kept[9], 24, "every event's time as a store keeps one");
}

//! A long item is kept in parts as well (migration 0019), so that the history
//! viewer reads a part of it and no more: the items a ledger copied before the
//! parts' table are split when it is made, once; one no longer than a part, in
//! characters, is not, however many bytes it takes.

#![allow(clippy::expect_used)]

use cf_ledger::{migrate, HISTORY_PART, MIGRATIONS};
use rusqlite::{params, Connection};

const AT: &str = "2026-10-16T12:00:00.000Z";

/// The parts kept of item `item`, in order.
fn parts(db: &Connection, item: &str) -> Vec<String> {
    db.prepare("SELECT text FROM transcript_part WHERE item_id = ? ORDER BY part")
        .expect("a query")
        .query_map([item], |row| row.get(0))
        .expect("the parts")
        .collect::<rusqlite::Result<_>>()
        .expect("each part")
}

#[test]
fn a_populated_version_18_ledger_has_its_long_items_split_into_parts_in_migration_19() {
    let db = Connection::open_in_memory().expect("a database");
    db.execute_batch("PRAGMA foreign_keys = ON").expect("keys");
    migrate(&db, &MIGRATIONS[..18]).expect("version 18");
    db.execute_batch(&format!(
        "INSERT INTO project (id, directory, name, state, gate, created_at, updated_at)
           VALUES (1, '/work/app', 'app', 'open', 0, '{AT}', '{AT}');
         INSERT INTO participant (id, project_id, handle, role, roles, agent, harness, tier, member_id, created_at, designer)
           VALUES (2, 1, 'chief', 'chief', '[]', 'diana', 'codex', NULL, NULL, '{AT}', 0);
         INSERT INTO conversation (id, participant_id, harness, started_at) VALUES (1, 2, 'codex', '{AT}');"
    ))
    .expect("a populated ledger");
    // Long, with a NUL in it, which SQLite's own text functions stop at; long
    // in bytes alone; and short.
    let long = format!("é\u{0}{}🙂 the end", "word ".repeat(HISTORY_PART / 2));
    let wide = "é".repeat(HISTORY_PART);
    let mut copy = db
        .prepare(
            "INSERT INTO transcript (conversation_id, item_id, seq, role, text, complete, at, copied_at)
             VALUES (1, ?, ?, 'assistant', ?, 1, NULL, ?)",
        )
        .expect("a copy");
    for (seq, (item, text)) in
        (0_i64..).zip([("long", long.as_str()), ("wide", &wide), ("short", "ok")])
    {
        copy.execute(params![item, seq, text, AT]).expect("copied");
    }
    drop(copy);

    migrate(&db, &MIGRATIONS[..19]).expect("migrated");
    migrate(&db, &MIGRATIONS[..19]).expect("and again");

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
}

/// What migration 19 takes on a ledger of 200 MB of items of 4 KB, 100 long
/// ones of 1 MB and one of 24 MiB: what a ledger pays once, at its first open
/// by this build. Run in release:
/// `cargo test --release -p cf-ledger --test long_items -- --ignored --nocapture`.
#[test]
#[ignore = "builds a 330 MB ledger; run by hand, in release"]
fn migration_19_splits_a_long_history_s_long_items_in_what_one_read_of_each_takes() {
    let dir = tempfile::tempdir().expect("a folder");
    let db = Connection::open(dir.path().join("consensflow.db")).expect("a database");
    db.execute_batch("PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL")
        .expect("pragmas");
    migrate(&db, &MIGRATIONS[..18]).expect("version 18");
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
    for seq in 0_i64..50_000 {
        copy.execute(params![format!("i-{seq}"), seq, short, AT, AT])
            .expect("copied");
    }
    for seq in 50_000_i64..50_100 {
        copy.execute(params![format!("i-{seq}"), seq, long, AT, AT])
            .expect("copied");
    }
    copy.execute(params!["longest", 50_100_i64, longest, AT, AT])
        .expect("copied");
    drop(copy);
    db.execute_batch("COMMIT").expect("committed");
    println!("built: {:?}", started.elapsed());
    let started = std::time::Instant::now();
    migrate(&db, &MIGRATIONS[..19]).expect("migrated");
    println!("migration 19: {:?}", started.elapsed());
    let kept: i64 = db
        .query_row("SELECT COUNT(*) FROM transcript_part", [], |row| row.get(0))
        .expect("the parts");
    assert_eq!(kept, 100 * 16 + 394);
}

//! A database is ConsensFlow's only with a page, a `user_version` of 1 or more and the eight
//! tables every release has (T-2241 finding 9); a first ledger is made whole under another
//! name before it counts (findings 11, 12).

#![allow(clippy::expect_used)]

use std::fs;
use std::path::Path;

use cf_ledger::{hold_ledger, make_ledger, Options};
use rusqlite::Connection;

fn names(dir: &Path) -> Vec<String> {
    let mut found: Vec<String> = fs::read_dir(dir)
        .expect("a folder")
        .map(|entry| {
            entry
                .expect("an entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    found.sort();
    found
}

fn refused(file: &Path) -> String {
    match hold_ledger(file) {
        Ok(_) => panic!("{} was held as a ledger", file.display()),
        Err(failed) => failed.code().unwrap_or("not a refusal").to_owned(),
    }
}

/// An unrelated database: its own tables, a schema version of `version`.
fn unrelated(file: &Path, version: i64, tables: &[&str]) {
    let db = Connection::open(file).expect("a database");
    for table in tables {
        db.execute_batch(&format!("CREATE TABLE {table} (id INTEGER)"))
            .expect("a table");
    }
    db.execute_batch(&format!("PRAGMA user_version = {version}"))
        .expect("a version");
}

#[test]
fn a_ledger_made_under_another_name_is_whole_and_alone() {
    let home = tempfile::tempdir().expect("a home");
    let staging = home.path().join("consensflow.db.new-0123456789abcdef");
    make_ledger(&staging).expect("made");
    assert_eq!(names(home.path()), ["consensflow.db.new-0123456789abcdef"]);
    let db = Connection::open(&staging).expect("opens");
    let version: i64 = db
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("a version");
    assert!(version >= 44, "every migration ran: {version}");
    let tables: i64 = db
        .query_row(
            "SELECT count(*) FROM sqlite_schema WHERE type = 'table' AND name IN \
             ('project','participant','conversation','task','task_need','message','transcript','event')",
            [],
            |row| row.get(0),
        )
        .expect("a count");
    assert_eq!(tables, 8);
    drop(db);
    assert_eq!(
        names(home.path()).len(),
        1,
        "no file beside it, even after a read"
    );
}

#[test]
fn a_name_that_is_there_is_never_made_over() {
    let home = tempfile::tempdir().expect("a home");
    let staging = home.path().join("staging");
    fs::write(&staging, b"someone's").expect("a file");
    assert!(make_ledger(&staging).is_err());
    assert_eq!(fs::read(&staging).expect("kept"), b"someone's");
}

#[test]
fn a_ledger_made_whole_is_held_and_opens_in_wal() {
    let home = tempfile::tempdir().expect("a home");
    let file = home.path().join("consensflow.db");
    make_ledger(&file).expect("made");
    let held = hold_ledger(&file).expect("held");
    let ledger = held.open(Options::default()).expect("opens");
    drop(ledger);
    let db = Connection::open(&file).expect("opens");
    let mode: String = db
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .expect("a mode");
    assert_eq!(mode, "wal");
}

#[test]
fn a_database_that_is_not_a_ledger_is_refused_and_its_bytes_are_the_same_after() {
    let home = tempfile::tempdir().expect("a home");
    let all = [
        "project",
        "participant",
        "conversation",
        "task",
        "task_need",
        "message",
        "transcript",
        "event",
    ];
    let cases: Vec<(&str, i64, Vec<&str>)> = vec![
        ("own tables, no version", 0, vec!["notes", "people"]),
        ("own tables, version 43", 43, vec!["notes", "people"]),
        (
            "a version and seven of the eight tables",
            44,
            all[..7].to_vec(),
        ),
        ("a version, no tables", 44, vec![]),
        ("the eight tables and no version", 0, all.to_vec()),
    ];
    for (row, version, tables) in cases {
        let file = home
            .path()
            .join(format!("{}.db", row.replace([' ', ','], "-")));
        unrelated(&file, version, &tables);
        let before = fs::read(&file).expect("bytes");
        let beside = names(home.path());
        assert_eq!(refused(&file), "ledger-unreadable", "{row}");
        assert_eq!(fs::read(&file).expect("bytes"), before, "{row}: its bytes");
        assert_eq!(
            names(home.path()),
            beside,
            "{row}: nothing was made beside it"
        );
    }
}

#[test]
fn a_file_with_no_page_or_no_database_in_it_is_refused_and_not_written() {
    let home = tempfile::tempdir().expect("a home");
    let empty = home.path().join("empty.db");
    fs::write(&empty, b"").expect("a file");
    assert_eq!(refused(&empty), "ledger-unreadable");
    assert_eq!(
        fs::metadata(&empty).expect("a file").len(),
        0,
        "still no byte"
    );
    let text = home.path().join("text.db");
    fs::write(&text, vec![b'x'; 5000]).expect("a file");
    assert_eq!(refused(&text), "ledger-unreadable");
    assert_eq!(fs::read(&text).expect("bytes"), vec![b'x'; 5000]);
}

#[test]
fn the_look_writes_nothing_to_a_database_that_is_not_in_wal() {
    let home = tempfile::tempdir().expect("a home");
    let file = home.path().join("consensflow.db");
    make_ledger(&file).expect("made");
    let before = fs::read(&file).expect("bytes");
    drop(hold_ledger(&file).expect("held"));
    assert_eq!(fs::read(&file).expect("bytes"), before, "held, not opened");
    let db = Connection::open(&file).expect("opens");
    let mode: String = db
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .expect("a mode");
    assert_ne!(
        mode, "wal",
        "the WAL switch is the first write, and it was not made"
    );
}

#[test]
fn a_ledger_held_is_refused_to_a_second_holder_and_a_newer_one_at_its_open() {
    let home = tempfile::tempdir().expect("a home");
    let file = home.path().join("consensflow.db");
    make_ledger(&file).expect("made");
    let first = hold_ledger(&file).expect("held");
    assert_eq!(refused(&file), "ledger-locked");
    drop(first);
    {
        let db = Connection::open(&file).expect("opens");
        db.execute_batch("PRAGMA user_version = 9999")
            .expect("newer");
    }
    let held = hold_ledger(&file).expect("a newer one is ConsensFlow's");
    let failed = held.open(Options::default()).err().expect("refused");
    assert_eq!(failed.code(), Some("ledger-newer"));
}

#[test]
fn a_file_that_is_not_there_is_not_made() {
    let home = tempfile::tempdir().expect("a home");
    let file = home.path().join("consensflow.db");
    assert!(hold_ledger(&file).is_err());
    assert!(!file.exists());
}

/// The claim-then-rename fallback with SQLite, on a file system with no links: run by hand
/// with `CF_NO_LINKS_DIR` naming a folder on one (an exFAT disk image).
#[test]
#[allow(clippy::disallowed_methods)]
#[ignore = "needs a folder on a file system with no links: CF_NO_LINKS_DIR"]
fn a_first_ledger_is_put_at_its_name_where_the_file_system_makes_no_links() {
    let Some(dir) = std::env::var_os("CF_NO_LINKS_DIR") else {
        panic!("CF_NO_LINKS_DIR names a folder on a file system with no links");
    };
    let home = Path::new(&dir).join(format!("home-{}", std::process::id()));
    fs::create_dir_all(&home).expect("a home");
    fs::write(home.join("a"), "x").expect("a file");
    assert!(
        fs::hard_link(home.join("a"), home.join("b")).is_err(),
        "this folder's file system makes links: the claim and the rename are not run"
    );
    fs::remove_file(home.join("a")).expect("removed");
    let (staging, name) = (
        home.join("consensflow.db.new-aa"),
        home.join("consensflow.db"),
    );
    make_ledger(&staging).expect("made");
    cf_base::file::flush_path(&staging).expect("flushed");
    cf_base::file::put_new(&staging, &name).expect("put");
    assert_eq!(names(&home), ["consensflow.db"], "the staging name is gone");
    let ledger = hold_ledger(&name)
        .expect("held")
        .open(Options::default())
        .expect("opens");
    drop(ledger);
    let db = Connection::open(&name).expect("opens");
    let ok: String = db
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .expect("a check");
    assert_eq!(ok, "ok");
    drop(db);
    // A name that is taken is not replaced.
    let again = home.join("consensflow.db.new-bb");
    make_ledger(&again).expect("made");
    let taken = cf_base::file::put_new(&again, &name).expect_err("taken");
    assert_eq!(taken.kind(), std::io::ErrorKind::AlreadyExists);
    assert!(again.exists());
    fs::remove_dir_all(&home).expect("removed");
}

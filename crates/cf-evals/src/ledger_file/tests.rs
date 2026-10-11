//! A ledger file made from the native ledger's migrations: the migrations run in
//! order, frozen rows loaded into it, and what is refused.

use rusqlite::Connection;

use super::*;

/// A ledger file of the test's own, `n` in `folder`.
fn file(folder: &tempfile::TempDir, n: usize) -> PathBuf {
    folder.path().join(format!("ledger-{n}.db"))
}

fn pragma(db: &Connection, name: &str) -> Vec<String> {
    let mut statement = db.prepare(&format!("PRAGMA {name}")).unwrap();
    let rows = statement
        .query_map([], |row| row.get::<_, rusqlite::types::Value>(0))
        .unwrap();
    rows.map(|value| match value.unwrap() {
        rusqlite::types::Value::Text(text) => text,
        other => format!("{other:?}"),
    })
    .collect()
}

fn count(db: &Connection, table: &str) -> i64 {
    db.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
        row.get(0)
    })
    .unwrap()
}

#[test]
fn a_ledger_file_is_at_the_schema_the_build_knows_which_is_the_number_of_migrations_and_sound() {
    let folder = tempfile::tempdir().unwrap();
    let on_disk = fs::read_dir(migrations_folder())
        .unwrap()
        .filter(|entry| {
            entry
                .as_ref()
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".sql")
        })
        .count();
    assert!(on_disk > 0);
    assert_eq!(schema_version().unwrap(), on_disk);
    assert_eq!(migrations().unwrap().len(), on_disk);
    let db = open(&file(&folder, 1), false).unwrap();
    assert_eq!(user_version(&db).unwrap(), on_disk);
    assert_eq!(pragma(&db, "integrity_check"), ["ok"]);
    assert!(pragma(&db, "foreign_key_check").is_empty());
    let tables: Vec<String> = db
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .map(|name| name.unwrap())
        .collect();
    for table in [
        "project",
        "participant",
        "task",
        "message",
        "transcript",
        "event",
    ] {
        assert!(
            tables.iter().any(|found| found == table),
            "{table}: {tables:?}"
        );
    }
}

#[test]
fn a_ledger_of_an_earlier_version_is_taken_on_from_where_it_was_and_one_at_the_version_is_left() {
    let folder = tempfile::tempdir().unwrap();
    let path = file(&folder, 1);
    // The ledger as an older build left it: the first migrations only.
    let old = Connection::open(&path).unwrap();
    for (at, migration) in migrations().unwrap().iter().take(5).enumerate() {
        old.execute_batch(migration).unwrap();
        old.execute_batch(&format!("PRAGMA user_version = {}", at + 1))
            .unwrap();
    }
    drop(old);
    let db = open(&path, false).unwrap();
    assert_eq!(user_version(&db).unwrap(), schema_version().unwrap());
    // The column migration 9 adds is there.
    let columns: Vec<String> = db
        .prepare("PRAGMA table_info(participant)")
        .unwrap()
        .query_map([], |row| row.get(1))
        .unwrap()
        .map(|name| name.unwrap())
        .collect();
    assert!(columns.iter().any(|name| name == "designer"), "{columns:?}");
    drop(db);
    let again = open(&path, false).unwrap();
    assert_eq!(user_version(&again).unwrap(), schema_version().unwrap());
}

#[test]
fn a_ledger_of_a_newer_version_is_refused_in_the_words_the_native_ledger_says_it_in() {
    let folder = tempfile::tempdir().unwrap();
    let path = file(&folder, 1);
    let version = schema_version().unwrap();
    let future = Connection::open(&path).unwrap();
    future
        .execute_batch(&format!("PRAGMA user_version = {}", version + 1))
        .unwrap();
    drop(future);
    let refused = open(&path, false).unwrap_err().to_string();
    assert_eq!(
        refused,
        format!(
            "this home was written by a newer ConsensFlow (schema {}; this build knows {version})",
            version + 1
        )
    );
}

#[test]
fn a_ledger_in_use_is_in_write_ahead_mode() {
    let folder = tempfile::tempdir().unwrap();
    let db = open(&file(&folder, 1), true).unwrap();
    assert_eq!(pragma(&db, "journal_mode"), ["wal"]);
    let plain = open(&file(&folder, 2), false).unwrap();
    assert_eq!(pragma(&plain, "journal_mode"), ["delete"]);
}

#[test]
fn the_rows_of_a_recording_are_loaded_and_leave_every_reference_holding() {
    let folder = tempfile::tempdir().unwrap();
    let db = load_ledger(&file(&folder, 1), "controls", false, &[]).unwrap();
    assert!(pragma(&db, "foreign_key_check").is_empty());
    assert_eq!(count(&db, "project"), 1);
    assert!(count(&db, "message") > 5);
    assert!(fixture_sql("controls")
        .unwrap()
        .starts_with("INSERT INTO project "));
    // What a recording was made to answer is there beside it.
    assert!(fixture_json("controls").unwrap().is_object());
    assert!(fixture_sql("no-such-recording").is_err());
}

#[test]
fn a_name_in_a_recording_that_stands_for_a_folder_is_put_where_the_caller_says() {
    let folder = tempfile::tempdir().unwrap();
    let directories = |db: &Connection| -> Vec<String> {
        db.prepare("SELECT directory FROM project ORDER BY id")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .map(|directory| directory.unwrap())
            .collect()
    };
    let home = load_ledger(
        &file(&folder, 1),
        "candidate-home",
        false,
        &[("@WORK@", "/the/work")],
    )
    .unwrap();
    assert_eq!(
        directories(&home),
        ["site", "docs", "billing", "legacy"].map(|name| format!("/the/work/{name}"))
    );
    // A folder with a quote in its name (a user called O'Brien) is a string in the rows all the same.
    let quoted = load_ledger(
        &file(&folder, 2),
        "candidate-home",
        false,
        &[("@WORK@", "/o'brien")],
    )
    .unwrap();
    assert_eq!(directories(&quoted)[0], "/o'brien/site");
}

#[test]
fn rows_that_leave_a_reference_that_does_not_hold_are_refused() {
    let folder = tempfile::tempdir().unwrap();
    let refused = load_rows(
        &file(&folder, 1),
        "INSERT INTO participant (project_id, handle, role, roles, created_at) VALUES (9, 'ghost', 'worker', '[]', 'now');",
        false,
        &[],
    )
    .unwrap_err()
    .to_string();
    assert_eq!(refused, "the rows leave 1 references that do not hold");
}

#[test]
fn migrations_that_are_not_numbered_one_after_another_from_the_first_are_refused_by_name() {
    let folder = tempfile::tempdir().unwrap();
    for name in [
        "0001.sql",
        "0002.sql",
        "0004.sql",
        "notes.txt",
        "0005.sqlite",
        "12.sql",
    ] {
        fs::write(folder.path().join(name), format!("-- {name}")).unwrap();
    }
    let refused = migrations_in(folder.path()).unwrap_err().to_string();
    assert_eq!(
        refused,
        "the ledger's migrations are numbered in order: 0004.sql is the 3th"
    );
    // The first that is not where it should be is the one said; a folder in order is read.
    fs::write(folder.path().join("0003.sql"), "-- 0003.sql").unwrap();
    fs::remove_file(folder.path().join("0004.sql")).unwrap();
    assert_eq!(
        migrations_in(folder.path()).unwrap(),
        ["-- 0001.sql", "-- 0002.sql", "-- 0003.sql"]
    );
    assert!(migrations_in(&folder.path().join("missing")).is_err());
}

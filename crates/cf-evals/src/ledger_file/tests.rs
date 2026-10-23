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
fn a_ledger_file_made_to_be_read_waits_for_no_disk_at_a_commit_whether_or_not_it_is_in_wal_mode() {
    let folder = tempfile::tempdir().unwrap();
    for (n, wal) in [(1, false), (2, true)] {
        let db = open(&file(&folder, n), wal).unwrap();
        // SQLite's own numbers: 0 is OFF, 2 is FULL.
        assert_eq!(pragma(&db, "synchronous"), ["Integer(0)"], "wal {wal}");
    }
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

/// The project of the tests of [`add_project`].
fn probe<'a>(staff: &'a [Member<'a>]) -> NewProject<'a> {
    NewProject {
        directory: "/work/probe",
        name: "round trip probe",
        chief: Chief {
            harness: "claude-code",
            agent: Some("probe-chief"),
        },
        staff,
    }
}

const PROBE: Member = Member {
    agent: "probe",
    harness: "claude-code",
    roles: &["worker"],
    tier: "standard",
};

#[test]
fn a_project_written_by_hand_has_a_human_the_chief_on_its_agent_and_each_member_and_the_log_of_each_members_joining(
) {
    let folder = tempfile::tempdir().unwrap();
    let db = open(&file(&folder, 1), false).unwrap();
    let id = add_project(&db, &probe(&[PROBE])).unwrap();
    // Each row as SQLite quotes its columns: a text in quotes, a number as it is, NULL.
    let lines = |sql: &str| -> Vec<String> {
        db.prepare(sql)
            .unwrap()
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .map(|line| line.unwrap())
            .collect()
    };
    assert_eq!(
        lines(
            "SELECT quote(handle) || ' ' || quote(role) || ' ' || quote(roles) || ' ' ||
                    quote(agent) || ' ' || quote(harness) || ' ' || quote(designer) || ' ' ||
                    quote(tier) || ' ' || quote(left_at)
             FROM participant ORDER BY id"
        ),
        [
            "'human' 'human' '[]' NULL NULL 0 NULL NULL",
            "'chief' 'chief' '[]' 'probe-chief' 'claude-code' 0 NULL NULL",
            "'probe' 'worker' '[\"worker\"]' 'probe' 'claude-code' 0 'standard' NULL",
        ]
    );
    assert_eq!(
        lines("SELECT quote(id) || ' ' || quote(name) || ' ' || quote(directory) || ' ' || quote(state) || ' ' || quote(gate) FROM project"),
        [format!("{id} 'round trip probe' '/work/probe' 'open' 0")]
    );
    // The member's joining is logged before the project is, as the ledger does.
    let events: Vec<(i64, String, serde_json::Value)> = db
        .prepare("SELECT project_id, kind, data FROM event ORDER BY id")
        .unwrap()
        .query_map([], |row| {
            let data: String = row.get(2)?;
            Ok((
                row.get(0)?,
                row.get(1)?,
                serde_json::from_str(&data).unwrap(),
            ))
        })
        .unwrap()
        .map(|row| row.unwrap())
        .collect();
    assert_eq!(
        events,
        [
            (
                id,
                "member.added".to_owned(),
                serde_json::json!({
                    "handle": "probe", "role": "worker", "roles": ["worker"], "harness": "claude-code"
                })
            ),
            (
                id,
                "project.created".to_owned(),
                serde_json::json!({ "name": "round trip probe", "directory": "/work/probe" })
            ),
        ]
    );
}

#[test]
fn a_project_is_all_or_nothing_a_member_that_cannot_join_leaves_no_project_behind() {
    let folder = tempfile::tempdir().unwrap();
    let db = open(&file(&folder, 1), false).unwrap();
    let twice = add_project(&db, &probe(&[PROBE, PROBE]))
        .unwrap_err()
        .to_string();
    assert!(
        twice.contains("UNIQUE") || twice.to_lowercase().contains("constraint"),
        "{twice}"
    );
    assert_eq!(count(&db, "project"), 0);
    assert_eq!(count(&db, "event"), 0);
    assert_eq!(count(&db, "participant"), 0);
    // The ledger is as it was: a project can be added after the refusal.
    add_project(&db, &probe(&[PROBE])).unwrap();
    assert_eq!(count(&db, "project"), 1);
}

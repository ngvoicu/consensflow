//! Migration 0014, which gives a project its wake-up calls: every row and
//! sequence of a populated version 13 ledger stays, and the new columns hold
//! what a project that never set them has.

use cf_ledger::{migrate, open_ledger, Options, DEFAULT_WAKE_UP_TEXT, MIGRATIONS};
use rusqlite::Connection;

const AT: &str = "2026-10-13T00:00:00.000Z";

/// A version 13 ledger: two projects, one of them with work and a gap in its ids.
fn populate(db: &Connection) {
    db.execute_batch("PRAGMA foreign_keys = ON").unwrap();
    migrate(db, &MIGRATIONS[..13]).unwrap();
    db.execute_batch(&format!(
        "INSERT INTO project (id, directory, name, state, gate, created_at, updated_at) VALUES
           (1, '/work/app', 'app', 'suspended', 1, '{AT}', '{AT}'),
           (2, '/work/docs', 'docs', 'open', 0, '{AT}', '{AT}'),
           (3, '/work/gone', 'gone', 'suspended', 0, '{AT}', '{AT}');
         INSERT INTO participant (id, project_id, handle, role, roles, agent, harness, tier, member_id, created_at, designer) VALUES
           (1, 1, 'human', 'human', '[]', NULL, NULL, NULL, NULL, '{AT}', 0),
           (2, 1, 'chief', 'chief', '[]', 'apollo', 'claude-code', NULL, NULL, '{AT}', 0),
           (3, 1, 'zeus', 'worker', '[\"worker\"]', 'zeus', 'claude-code', 'standard', NULL, '{AT}', 0),
           (4, 2, 'human', 'human', '[]', NULL, NULL, NULL, NULL, '{AT}', 0),
           (5, 2, 'chief', 'chief', '[]', 'apollo', 'claude-code', NULL, NULL, '{AT}', 0);
         INSERT INTO task (id, project_id, number, title, body, requester_id, assignee_id, state, created_at, updated_at)
           VALUES (1, 1, 1, 'Parser', 'Parser', 2, 3, 'working', '{AT}', '{AT}');
         INSERT INTO message (id, project_id, recipient_id, sender_id, kind, task_id, body, state, created_at)
           VALUES (1, 1, 3, 2, 'task', 1, 'Parser', 'delivered', '{AT}');
         INSERT INTO event (id, project_id, at, kind, data) VALUES
           (1, 1, '{AT}', 'project.state', '{{\"from\":\"open\",\"to\":\"suspended\"}}');
         DELETE FROM project WHERE id = 3;"
    ))
    .unwrap();
}

/// The rows `sql` reads, one line each, every cell as SQLite holds it.
fn rows(db: &Connection, sql: &str) -> Vec<String> {
    let mut read = db.prepare(sql).unwrap();
    let columns = read.column_count();
    read.query_map([], move |row| {
        (0..columns)
            .map(|at| {
                row.get::<_, rusqlite::types::Value>(at)
                    .map(|value| format!("{value:?}"))
            })
            .collect::<rusqlite::Result<Vec<_>>>()
            .map(|cells| cells.join("|"))
    })
    .unwrap()
    .map(Result::unwrap)
    .collect()
}

#[test]
fn a_populated_version_13_ledger_gains_the_wake_up_columns_in_migration_14_and_every_row_and_sequence_stays(
) {
    const TABLES: [&str; 4] = ["participant", "event", "task", "message"];
    let db = Connection::open_in_memory().unwrap();
    populate(&db);
    let before_of = |db: &Connection| -> Vec<Vec<String>> {
        TABLES
            .iter()
            .map(|table| rows(db, &format!("SELECT * FROM {table} ORDER BY rowid")))
            .collect()
    };
    let project = "SELECT id, directory, name, state, gate, created_at, updated_at FROM project ORDER BY rowid";
    let sequences = "SELECT name, seq FROM sqlite_sequence ORDER BY rowid";
    let (project_before, others_before, sequences_before) =
        (rows(&db, project), before_of(&db), rows(&db, sequences));

    // Through 0014 and no further: what a later migration does is its own test's.
    migrate(&db, &MIGRATIONS[..14]).unwrap();
    migrate(&db, &MIGRATIONS[..14]).unwrap();

    let version: i64 = db
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, 14);
    assert_eq!(
        rows(&db, project),
        project_before,
        "the project's own columns are as they were"
    );
    assert_eq!(
        before_of(&db),
        others_before,
        "the rows of the other tables are as they were"
    );
    assert_eq!(
        rows(&db, sequences),
        sequences_before,
        "each sequence in its place"
    );
    assert_eq!(
        rows(
            &db,
            "SELECT wake_up_on, wake_up_every_ms, wake_up_text FROM project ORDER BY rowid"
        ),
        [
            "Integer(0)|Integer(3600000)|Null",
            "Integer(0)|Integer(3600000)|Null"
        ],
        "every project has its calls off, an hour apart, with the default words"
    );
    let integrity: String = db
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .unwrap();
    assert_eq!(integrity, "ok");
    assert!(
        !db.prepare("PRAGMA foreign_key_check")
            .unwrap()
            .exists([])
            .unwrap(),
        "every reference holds"
    );
    // The id of the project that was deleted does not come back.
    db.execute_batch(&format!(
        "INSERT INTO project (directory, name, state, created_at, updated_at)
           VALUES ('/work/new', 'new', 'open', '{AT}', '{AT}')"
    ))
    .unwrap();
    let new: i64 = db
        .query_row("SELECT id FROM project WHERE name = 'new'", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(new, 4);
    // The columns keep their checks.
    for broken in [
        "UPDATE project SET wake_up_on = 2 WHERE id = 1",
        "UPDATE project SET wake_up_every_ms = 0 WHERE id = 1",
        "UPDATE project SET wake_up_every_ms = -60000 WHERE id = 1",
    ] {
        assert!(db.execute_batch(broken).is_err(), "{broken}");
    }
}

#[test]
fn a_version_13_ledger_opens_with_its_projects_tasks_and_messages_and_the_calls_off() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("consensflow.db");
    {
        let db = Connection::open(&file).unwrap();
        populate(&db);
        db.close().unwrap();
    }
    let ledger = open_ledger(&file, Options::default()).unwrap();
    let projects = ledger.projects().unwrap();
    assert_eq!(
        projects
            .iter()
            .map(|project| (project.id, project.name.as_str(), project.state.as_str()))
            .collect::<Vec<_>>(),
        [(1, "app", "suspended"), (2, "docs", "open")]
    );
    assert_eq!(ledger.task(1, 1).unwrap().unwrap().task.state, "working");
    assert_eq!(ledger.message(1).unwrap().unwrap().state, "delivered");
    for project in &projects {
        let setting = ledger.wake_up(project.id).unwrap();
        assert_eq!(
            (setting.on, setting.every_ms, setting.words()),
            (false, 3_600_000, DEFAULT_WAKE_UP_TEXT),
            "{}",
            project.name
        );
    }
    ledger.close().unwrap();
}

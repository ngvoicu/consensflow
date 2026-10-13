//! The migration that gives a project its description: every row and sequence of
//! a ledger before it stays (whichever version that was, 0015's or another
//! landing's 0016 after it), the new column is none in every project, and a
//! ledger file at 0015 opens as it was and takes a first line.

use cf_ledger::{migrate, open_ledger, Options, MIGRATIONS};
use rusqlite::Connection;

const AT: &str = "2026-10-13T10:00:00.000Z";

/// Where the migration that gave a project its description is in the list.
fn at() -> usize {
    MIGRATIONS
        .iter()
        .position(|sql| sql.contains("ADD COLUMN description"))
        .expect("a migration that adds the description")
}

/// A ledger at the version `migrations` make: two projects (one of them with
/// work and a gap in its ids), and what the others of its tables hold.
fn populate(db: &Connection, migrations: &[&str]) {
    db.execute_batch("PRAGMA foreign_keys = ON").expect("keys");
    migrate(db, migrations).expect("the version before");
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
    .expect("a populated ledger");
}

/// The rows `sql` reads, one line each, every cell as SQLite holds it.
fn rows(db: &Connection, sql: &str) -> Vec<String> {
    let mut statement = db.prepare(sql).expect("a query");
    let columns = statement.column_count();
    statement
        .query_map([], |row| {
            (0..columns)
                .map(|at| {
                    row.get::<_, rusqlite::types::Value>(at)
                        .map(|value| format!("{value:?}"))
                })
                .collect::<rusqlite::Result<Vec<_>>>()
                .map(|cells| cells.join("|"))
        })
        .expect("the rows")
        .map(|row| row.expect("a row"))
        .collect()
}

const PROJECT_OWN: &str =
    "SELECT id, directory, name, state, gate, created_at, updated_at FROM project ORDER BY rowid";

/// The migrations `after` over a populated ledger at the version `before`
/// makes: every row and sequence stays, the description comes last and is none
/// in every project.
fn migrates_and_keeps_everything(before: &[&str], after: &[&str]) {
    const OTHERS: [&str; 4] = ["participant", "event", "task", "message"];
    let db = Connection::open_in_memory().expect("a database");
    populate(&db, before);
    let others = |db: &Connection| {
        OTHERS.map(|table| rows(db, &format!("SELECT * FROM {table} ORDER BY rowid")))
    };
    let columns_of = |db: &Connection| -> Vec<String> {
        db.prepare("SELECT name FROM pragma_table_info('project') ORDER BY cid")
            .expect("the columns")
            .query_map([], |row| row.get(0))
            .expect("the columns")
            .map(|name| name.expect("a name"))
            .collect()
    };
    let sequences = "SELECT name, seq FROM sqlite_sequence ORDER BY rowid";
    let kept = (
        rows(&db, PROJECT_OWN),
        others(&db),
        rows(&db, sequences),
        columns_of(&db),
    );

    migrate(&db, after).expect("migrated");
    migrate(&db, after).expect("and again");

    let version: i64 = db
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("the version");
    assert_eq!(version as usize, after.len());
    assert_eq!(rows(&db, PROJECT_OWN), kept.0, "the project's own columns");
    assert_eq!(others(&db), kept.1, "the rows of the other tables");
    assert_eq!(rows(&db, sequences), kept.2, "each sequence in its place");
    let columns = columns_of(&db);
    assert_eq!(
        columns[..kept.3.len()],
        kept.3,
        "every column is where it was"
    );
    assert!(columns.contains(&"description".to_owned()));
    assert_eq!(
        rows(&db, "SELECT description FROM project ORDER BY rowid"),
        ["Null", "Null"],
        "no project had a line to show"
    );
    let integrity: String = db
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .expect("the check");
    assert_eq!(integrity, "ok");
    assert!(
        !db.prepare("PRAGMA foreign_key_check")
            .expect("the check")
            .exists([])
            .expect("the check"),
        "every reference holds"
    );
    // The id of the project that was deleted does not come back.
    db.execute_batch(&format!(
        "INSERT INTO project (directory, name, state, created_at, updated_at)
           VALUES ('/work/new', 'new', 'open', '{AT}', '{AT}')"
    ))
    .expect("a new project");
    let new: i64 = db
        .query_row("SELECT id FROM project WHERE name = 'new'", [], |row| {
            row.get(0)
        })
        .expect("its id");
    assert_eq!(new, 4);
}

/// How many migrations the two landings share: the ones up to 0015. The next
/// number, 0016, is another landing's, and its place holds what stands for it.
const SHARED: usize = 15;

#[test]
fn a_ledger_at_0015_gains_the_description_empty_and_every_row_and_sequence_stays() {
    migrates_and_keeps_everything(&MIGRATIONS[..SHARED], &MIGRATIONS[..=at()]);
}

#[test]
fn a_ledger_at_0016_gains_the_description_empty_and_every_row_and_sequence_stays() {
    // 0016 is whatever comes before the description's: what holds its place here,
    // and the other landing's migration once it is merged.
    migrates_and_keeps_everything(&MIGRATIONS[..at()], &MIGRATIONS[..=at()]);
}

#[test]
fn a_ledger_that_a_0016_of_another_landing_that_touches_the_same_table_took_on_keeps_everything_too(
) {
    // What holds 0016's place changes nothing: a migration that adds to the
    // project's table stands for the other landing's.
    const ANOTHER: &str = "ALTER TABLE project ADD COLUMN pinned INTEGER NOT NULL DEFAULT 0;";
    let before = [&MIGRATIONS[..SHARED], &[ANOTHER]].concat();
    let after = [&before[..], &MIGRATIONS[at()..=at()]].concat();
    migrates_and_keeps_everything(&before, &after);
}

#[test]
fn a_ledger_file_at_0015_opens_with_its_projects_tasks_and_messages_and_no_project_has_a_line() {
    let dir = tempfile::tempdir().expect("a home");
    let file = dir.path().join("consensflow.db");
    {
        let db = Connection::open(&file).expect("a file");
        populate(&db, &MIGRATIONS[..SHARED]);
        db.close().expect("closed");
    }
    let mut ledger = open_ledger(&file, Options::default()).expect("a ledger");
    let projects = ledger.projects().expect("the projects");
    assert_eq!(
        projects
            .iter()
            .map(|project| (project.id, project.name.as_str(), project.state.as_str()))
            .collect::<Vec<_>>(),
        [(1, "app", "suspended"), (2, "docs", "open")]
    );
    assert!(projects.iter().all(|project| project.description.is_none()));
    assert_eq!(
        ledger.task(1, 1).expect("read").expect("task").task.state,
        "working"
    );
    assert_eq!(
        ledger.message(1).expect("read").expect("message").state,
        "delivered"
    );
    // And the first line is written, and read back, on a ledger that was one.
    ledger
        .describe_project(2, Some("The docs"))
        .expect("described");
    ledger.rename_project(1, "application").expect("renamed");
    let read = ledger.projects().expect("the projects");
    assert_eq!(
        read.iter()
            .map(|project| (project.name.as_str(), project.description.as_deref()))
            .collect::<Vec<_>>(),
        [("application", None), ("docs", Some("The docs"))]
    );
    ledger.close().expect("closed");
}

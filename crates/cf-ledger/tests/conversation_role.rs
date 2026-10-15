//! The role text a conversation was last given: migration 0016 gives the
//! conversation table a column that no row needs a value in, the ledger writes
//! the text and reads it back (and logs nothing of it: it is written at the start
//! of every conversation of a harness that keeps its first instructions), a
//! conversation's view never holds it, and a conversation the human switched
//! their window to has none, which is how the engine knows it is not known.

#![allow(clippy::expect_used)]

use cf_ledger::{
    migrate, open_ledger, Ledger, NewChief, NewProject, Options, ProjectView, MIGRATIONS,
};
use rusqlite::Connection;

const AT: &str = "2026-10-13T12:00:00.000Z";

fn ledger_in(dir: &tempfile::TempDir) -> Ledger {
    open_ledger(&dir.path().join("consensflow.db"), Options::default()).expect("a ledger")
}

fn project(ledger: &mut Ledger, name: &str) -> ProjectView {
    ledger
        .create_project(&NewProject {
            directory: format!("/work/{name}"),
            name: name.into(),
            chief: NewChief {
                harness: "codex".into(),
                agent: Some("diana".into()),
            },
            staff: Vec::new(),
            gate: false,
            wake_up: None,
            specs: None,
        })
        .expect("a project")
}

/// The rows `sql` reads, one line each, every cell as SQLite holds it.
fn rows(db: &Connection, sql: &str) -> Vec<String> {
    let mut read = db.prepare(sql).expect("a query");
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
    .expect("the rows")
    .map(|row| row.expect("a row"))
    .collect()
}

#[test]
fn a_populated_version_15_ledger_gains_the_role_text_column_in_migration_16_and_every_row_stays() {
    let db = Connection::open_in_memory().expect("a database");
    db.execute_batch("PRAGMA foreign_keys = ON").expect("keys");
    migrate(&db, &MIGRATIONS[..15]).expect("version 15");
    db.execute_batch(&format!(
        "INSERT INTO project (id, directory, name, state, gate, created_at, updated_at)
           VALUES (1, '/work/app', 'app', 'suspended', 0, '{AT}', '{AT}');
         INSERT INTO participant (id, project_id, handle, role, roles, agent, harness, tier, member_id, created_at, designer) VALUES
           (1, 1, 'human', 'human', '[]', NULL, NULL, NULL, NULL, '{AT}', 0),
           (2, 1, 'chief', 'chief', '[]', 'diana', 'codex', NULL, NULL, '{AT}', 0);
         INSERT INTO conversation (id, participant_id, harness, native_session, started_at, ended_at) VALUES
           (1, 2, 'codex', 'thread-1', '{AT}', '{AT}'),
           (2, 2, 'codex', 'thread-2', '{AT}', NULL);"
    ))
    .expect("a populated ledger");
    let conversations = "SELECT id, participant_id, harness, native_session, started_at, ended_at FROM conversation ORDER BY rowid";
    let columns_of = |db: &Connection| -> Vec<String> {
        db.prepare("SELECT name FROM pragma_table_info('conversation') ORDER BY cid")
            .expect("the columns")
            .query_map([], |row| row.get(0))
            .expect("the columns")
            .map(|name| name.expect("a name"))
            .collect()
    };
    let (conversations_before, columns_before) = (rows(&db, conversations), columns_of(&db));

    // Through 0016 and no further: what a later migration does is its own test's.
    migrate(&db, &MIGRATIONS[..16]).expect("migrated");
    migrate(&db, &MIGRATIONS[..16]).expect("and again");

    let version: i64 = db
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("the version");
    assert_eq!(version, 16);
    let mut columns = columns_before;
    columns.push("role_text".to_owned());
    assert_eq!(
        columns_of(&db),
        columns,
        "the new one comes last, and every column is where it was"
    );
    assert_eq!(
        rows(&db, conversations),
        conversations_before,
        "the conversation's own columns are as they were"
    );
    assert_eq!(
        rows(&db, "SELECT role_text FROM conversation ORDER BY rowid"),
        ["Null", "Null"],
        "no conversation has been given a role text"
    );
    // The table keeps its own rules: one current conversation for a participant,
    // and a native session that belongs to one conversation.
    db.execute_batch(&format!(
        "INSERT INTO conversation (participant_id, harness, started_at) VALUES (2, 'codex', '{AT}')"
    ))
    .expect_err("a second current conversation");
    db.execute_batch(&format!(
        "INSERT INTO conversation (participant_id, harness, native_session, started_at, ended_at)
           VALUES (2, 'codex', 'thread-2', '{AT}', '{AT}')"
    ))
    .expect_err("a native session that is another conversation's");
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
}

#[test]
fn a_conversation_reads_back_the_role_text_it_was_given_the_latest_one_and_none_before() {
    let dir = tempfile::tempdir().expect("a home");
    let mut ledger = ledger_in(&dir);
    let app = project(&mut ledger, "app");
    let chief = app
        .participants
        .iter()
        .find(|participant| participant.role == "chief")
        .expect("a chief");
    let first = ledger
        .start_conversation(chief.id, "codex")
        .expect("a conversation");
    assert_eq!(ledger.conversation_role_text(first.id).expect("read"), None);

    ledger
        .set_conversation_role_text(first.id, "the role\nit began with, \u{2014} whole")
        .expect("written");
    assert_eq!(
        ledger.conversation_role_text(first.id).expect("read"),
        Some("the role\nit began with, \u{2014} whole".to_owned())
    );
    ledger
        .set_conversation_role_text(first.id, "the role it was told since")
        .expect("written again");
    assert_eq!(
        ledger.conversation_role_text(first.id).expect("read"),
        Some("the role it was told since".to_owned()),
        "the latest text is the one kept"
    );

    // Another conversation of the same participant has its own.
    let second = ledger
        .start_conversation(chief.id, "codex")
        .expect("another conversation");
    assert_eq!(
        ledger.conversation_role_text(second.id).expect("read"),
        None
    );
    assert_eq!(
        ledger.conversation_role_text(first.id).expect("read"),
        Some("the role it was told since".to_owned()),
        "the one before it keeps its own"
    );

    // It is the file's: a ledger opened again reads it.
    drop(ledger);
    let ledger = ledger_in(&dir);
    assert_eq!(
        ledger.conversation_role_text(first.id).expect("read"),
        Some("the role it was told since".to_owned())
    );
}

#[test]
fn a_conversation_that_is_not_there_is_refused_for_the_text_it_is_read_and_given() {
    let dir = tempfile::tempdir().expect("a home");
    let mut ledger = ledger_in(&dir);
    for refused in [
        ledger
            .conversation_role_text(77)
            .expect_err("no such conversation"),
        ledger
            .set_conversation_role_text(77, "text")
            .expect_err("no such conversation"),
    ] {
        assert_eq!(refused.code(), Some("unknown-conversation"));
        assert_eq!(refused.to_string(), "no conversation 77");
    }
}

#[test]
fn giving_a_conversation_its_role_text_logs_no_event_and_its_view_never_holds_it() {
    let dir = tempfile::tempdir().expect("a home");
    let mut ledger = ledger_in(&dir);
    let app = project(&mut ledger, "app");
    let chief = app
        .participants
        .iter()
        .find(|participant| participant.role == "chief")
        .expect("a chief");
    let started = ledger
        .start_conversation(chief.id, "codex")
        .expect("a conversation");
    let events_before = ledger.events(app.id, 0, 500).expect("the events").len();
    ledger
        .set_conversation_role_text(started.id, "the role text")
        .expect("written");
    assert_eq!(
        ledger.events(app.id, 0, 500).expect("the events").len(),
        events_before,
        "no event says it"
    );
    let view = ledger
        .current_conversation(chief.id)
        .expect("a read")
        .expect("the current conversation");
    assert!(
        !serde_json::to_string(&view)
            .expect("the view as the API says it")
            .contains("the role text"),
        "the API's view of a conversation never carries it"
    );
}

#[test]
fn a_conversation_the_human_switched_their_window_to_has_no_role_text_whatever_the_one_before_had()
{
    let dir = tempfile::tempdir().expect("a home");
    let mut ledger = ledger_in(&dir);
    let app = project(&mut ledger, "app");
    let chief = app
        .participants
        .iter()
        .find(|participant| participant.role == "chief")
        .expect("a chief");
    let began = ledger
        .start_conversation(chief.id, "codex")
        .expect("a conversation");
    ledger
        .bind_conversation(began.id, "thread-a")
        .expect("bound");
    ledger
        .set_conversation_role_text(began.id, "the role it began with")
        .expect("written");

    let followed = ledger
        .follow_conversation(chief.id, "codex", "thread-b")
        .expect("followed");
    assert_ne!(followed.id, began.id);
    assert_eq!(
        ledger.conversation_role_text(followed.id).expect("read"),
        None,
        "what the window shows now was begun with instructions nobody here knows"
    );

    // Back to its own earlier conversation, which still has what it was given.
    let back = ledger
        .follow_conversation(chief.id, "codex", "thread-a")
        .expect("followed back");
    assert_eq!(back.id, began.id);
    assert_eq!(
        ledger.conversation_role_text(back.id).expect("read"),
        Some("the role it began with".to_owned())
    );
}

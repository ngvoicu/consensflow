//! What a replacement of a role is to the ledger: migration 0018 gives the
//! message table a column that no row needs a value in, a note written to say a
//! conversation its role holds the text and no other message does, and a ledger
//! opened again still knows which messages they are.

#![allow(clippy::expect_used)]

use cf_ledger::{
    migrate, open_ledger, Ledger, NewChief, NewNote, NewProject, Options, ProjectView, MIGRATIONS,
};
use rusqlite::Connection;

const AT: &str = "2026-10-14T12:00:00.000Z";

fn ledger_in(dir: &tempfile::TempDir) -> Ledger {
    open_ledger(&dir.path().join("consensflow.db"), Options::default()).expect("a ledger")
}

fn project(ledger: &mut Ledger) -> ProjectView {
    ledger
        .create_project(&NewProject {
            directory: "/work/app".into(),
            name: "app".into(),
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

/// The cells of the one row `sql` reads, as SQLite holds them.
fn cells(db: &Connection, sql: &str) -> Vec<String> {
    let mut read = db.prepare(sql).expect("a query");
    let columns = read.column_count();
    read.query_row([], move |row| {
        (0..columns)
            .map(|at| {
                row.get::<_, rusqlite::types::Value>(at)
                    .map(|value| format!("{value:?}"))
            })
            .collect::<rusqlite::Result<Vec<_>>>()
    })
    .expect("a row")
}

#[test]
fn a_populated_version_17_ledger_gains_the_role_text_column_of_message_in_migration_18_and_every_row_stays(
) {
    let db = Connection::open_in_memory().expect("a database");
    db.execute_batch("PRAGMA foreign_keys = ON").expect("keys");
    migrate(&db, &MIGRATIONS[..17]).expect("version 17");
    db.execute_batch(&format!(
        "INSERT INTO project (id, directory, name, state, gate, created_at, updated_at)
           VALUES (1, '/work/app', 'app', 'open', 0, '{AT}', '{AT}');
         INSERT INTO participant (id, project_id, handle, role, roles, agent, harness, tier, member_id, created_at, designer) VALUES
           (1, 1, 'human', 'human', '[]', NULL, NULL, NULL, NULL, '{AT}', 0),
           (2, 1, 'chief', 'chief', '[]', 'diana', 'codex', NULL, NULL, '{AT}', 0);
         INSERT INTO message (id, project_id, recipient_id, sender_id, kind, body, state, attempts, created_at)
           VALUES (1, 1, 2, NULL, 'note', 'Your ConsensFlow role instructions have changed', 'queued', 0, '{AT}');"
    ))
    .expect("a populated ledger");
    let message = "SELECT id, project_id, recipient_id, sender_id, kind, task_id, reply_to, body, questions, choices, state, attempts, reason, receipt, created_at, delivered_at, carried_by, claimed_at, door_closed_at FROM message";
    let before = cells(&db, message);

    // Through 0018 and no further: what a later migration does is its own test's.
    migrate(&db, &MIGRATIONS[..18]).expect("migrated");
    migrate(&db, &MIGRATIONS[..18]).expect("and again");

    let version: i64 = db
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("the version");
    assert_eq!(version, 18);
    assert_eq!(
        cells(&db, message),
        before,
        "the message's own columns are as they were"
    );
    assert_eq!(
        cells(&db, "SELECT role_text FROM message"),
        ["Null"],
        "no message says a role: its words are none of the ledger's knowledge of it"
    );
    let last: String = db
        .query_row(
            "SELECT name FROM pragma_table_info('message') ORDER BY cid DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .expect("the last column");
    assert_eq!(last, "role_text", "the new one comes last");
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
fn a_ledger_opened_again_still_knows_which_messages_say_a_role_and_which_only_say_its_words() {
    let dir = tempfile::tempdir().expect("a home");
    let mut ledger = ledger_in(&dir);
    let app = project(&mut ledger);
    let chief = app
        .participants
        .iter()
        .find(|participant| participant.role == "chief")
        .expect("a chief")
        .id;
    let words = "Your ConsensFlow role instructions have changed since this conversation began.";
    let said = ledger
        .note_role(app.id, "chief", words, "the role")
        .expect("a note that says a role");
    let alike = ledger
        .note(
            app.id,
            &NewNote {
                from: None,
                to: "chief".into(),
                body: words.into(),
                task: None,
            },
        )
        .expect("a note with the same words");
    assert_eq!(ledger.role_notes_to(chief).expect("read"), [said.id]);
    assert_eq!(said.body, alike.body);
    assert_eq!(
        (said.kind.as_str(), said.sender.clone(), said.task_number),
        ("note", None, None),
        "from ConsensFlow, about no task, as a note of ConsensFlow's is"
    );

    drop(ledger);
    let ledger = ledger_in(&dir);
    assert_eq!(
        ledger.role_notes_to(chief).expect("read"),
        [said.id],
        "it is the file's"
    );
}

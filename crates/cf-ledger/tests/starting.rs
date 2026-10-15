//! The ledger at a daemon's start: every project that was open is suspended, as
//! the human's Close leaves it, and none is marked to come back by itself; what
//! was in them (tasks, messages) is as it was. And the schema that has no such
//! mark to keep (migration 0013), every row and sequence as it was.

// The tests' own scaffolding: a failure in it is the test's.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use cf_ledger::{
    migrate, open_ledger, Ledger, NewChief, NewMember, NewNote, NewProject, NewTask, Options,
    ProjectView, Resume, MIGRATIONS,
};
use rusqlite::Connection;
use serde_json::json;

const AT: &str = "2026-10-12T00:00:00.000Z";

/// A project of its own name, with a chief and one worker.
fn project(name: &str) -> NewProject {
    NewProject {
        directory: format!("/work/{name}"),
        name: name.to_owned(),
        chief: NewChief {
            harness: "claude-code".to_owned(),
            agent: Some("apollo".to_owned()),
        },
        staff: vec![NewMember {
            agent: "zeus".to_owned(),
            harness: "claude-code".to_owned(),
            designer: false,
            roles: vec!["worker".to_owned()],
            tier: "standard".to_owned(),
        }],
        gate: false,
        wake_up: None,
        specs: None,
    }
}

fn ledger_in(dir: &tempfile::TempDir) -> Ledger {
    open_ledger(&dir.path().join("consensflow.db"), Options::default()).unwrap()
}

fn states(projects: &[ProjectView]) -> Vec<(&str, &str)> {
    projects
        .iter()
        .map(|project| (project.name.as_str(), project.state.as_str()))
        .collect()
}

#[test]
fn a_start_suspends_every_open_project_and_marks_none_and_leaves_what_was_in_them_as_it_was() {
    let dir = tempfile::tempdir().unwrap();
    let mut ledger = ledger_in(&dir);
    let site = ledger.create_project(&project("site")).unwrap();
    ledger.create_project(&project("docs")).unwrap();
    let closed = ledger.create_project(&project("legacy")).unwrap();
    ledger.set_project_state(closed.id, "suspended").unwrap();
    // Work in the first: a task given to the worker, its brief waiting for it.
    let task = ledger
        .create_task(
            site.id,
            &NewTask {
                from: "chief".to_owned(),
                to: Some("zeus".to_owned()),
                body: "Write the parser".to_owned(),
                ..NewTask::default()
            },
        )
        .unwrap();
    let zeus = ledger
        .project(site.id)
        .unwrap()
        .unwrap()
        .participants
        .into_iter()
        .find(|participant| participant.handle == "zeus")
        .unwrap();
    let waiting = ledger.next_delivery(zeus.id).unwrap().unwrap();
    let before = ledger.board(site.id).unwrap().lanes;

    let suspended = ledger.suspend_for_restart().unwrap();

    assert_eq!(
        states(&suspended),
        [("site", "suspended"), ("docs", "suspended")],
        "the ones that were open, in order, and not the one already closed"
    );
    assert_eq!(
        states(&ledger.projects().unwrap()),
        [
            ("site", "suspended"),
            ("docs", "suspended"),
            ("legacy", "suspended")
        ]
    );
    // A project says nothing of coming back by itself, and neither does the event.
    let shown = serde_json::to_value(&suspended[0]).unwrap();
    assert_eq!(shown.get("resumeOnStart"), None, "{shown}");
    let events = ledger.events(site.id, 0, 500).unwrap();
    let last = events.last().unwrap();
    assert_eq!(
        (last.kind.as_str(), &last.data),
        (
            "project.state",
            &json!({ "from": "open", "to": "suspended" })
        )
    );
    let suspensions = ledger
        .events(closed.id, 0, 500)
        .unwrap()
        .iter()
        .filter(|event| event.data == json!({ "from": "open", "to": "suspended" }))
        .count();
    assert_eq!(
        suspensions, 1,
        "the closed one has its own, from when it was closed, and none from the start"
    );
    // The board shows its tasks as they were, and the brief still waits for its window.
    assert_eq!(ledger.board(site.id).unwrap().lanes, before);
    assert_eq!(
        ledger
            .task(site.id, task.task.number)
            .unwrap()
            .unwrap()
            .task
            .state,
        "queued"
    );
    assert_eq!(
        ledger.next_delivery(zeus.id).unwrap().unwrap().id,
        waiting.id
    );

    // Nothing is left to suspend: a start again changes nothing.
    assert!(ledger.suspend_for_restart().unwrap().is_empty());

    // What the human resumed is open until the next start, which suspends it again.
    ledger.set_project_state(site.id, "open").unwrap();
    let again = ledger.suspend_for_restart().unwrap();
    assert_eq!(states(&again), [("site", "suspended")]);
}

#[test]
fn the_board_of_a_suspended_project_says_what_its_resume_will_start_and_an_open_one_says_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mut ledger = ledger_in(&dir);
    let site = ledger.create_project(&project("site")).unwrap();
    let give = |ledger: &mut Ledger, body: &str| {
        ledger
            .create_task(
                site.id,
                &NewTask {
                    from: "chief".to_owned(),
                    to: Some("zeus".to_owned()),
                    body: body.to_owned(),
                    ..NewTask::default()
                },
            )
            .unwrap()
    };
    give(&mut ledger, "Write the parser");
    let dropped = give(&mut ledger, "Write the lexer");
    ledger
        .cancel_task(site.id, dropped.task.number, "chief")
        .unwrap();
    // A word for the chief waits, and one for the human, which Resume starts nothing for.
    ledger
        .note(
            site.id,
            &NewNote {
                from: None,
                to: "chief".to_owned(),
                task: None,
                body: "Waiting for you".to_owned(),
            },
        )
        .unwrap();
    ledger
        .note(
            site.id,
            &NewNote {
                from: None,
                to: "human".to_owned(),
                task: None,
                body: "Waiting for you too".to_owned(),
            },
        )
        .unwrap();
    let open = ledger.board(site.id).unwrap();
    assert_eq!(open.resume, None);
    assert_eq!(serde_json::to_value(&open).unwrap().get("resume"), None);

    ledger.suspend_for_restart().unwrap();

    let board = ledger.board(site.id).unwrap();
    assert_eq!(
        board.resume,
        Some(Resume {
            tasks: 1,
            messages: 2
        }),
        "the task not finished, and the brief and the word that wait for windows"
    );
    assert_eq!(
        serde_json::to_value(&board).unwrap()["resume"],
        json!({ "tasks": 1, "messages": 2 })
    );
    // Resumed, it says nothing again.
    ledger.set_project_state(site.id, "open").unwrap();
    assert_eq!(ledger.board(site.id).unwrap().resume, None);
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
fn a_populated_version_12_ledger_loses_the_mark_in_migration_13_and_every_row_and_sequence_stays() {
    const THE_REST: [&str; 4] = ["participant", "event", "task", "message"];
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch("PRAGMA foreign_keys = ON").unwrap();
    migrate(&db, &MIGRATIONS[..12]).unwrap();
    db.execute_batch(&format!(
        "INSERT INTO project (id, directory, name, state, resume_on_start, gate, created_at, updated_at) VALUES
           (1, '/work/app', 'app', 'suspended', 1, 0, '{AT}', '{AT}'),
           (2, '/work/docs', 'docs', 'open', 0, 1, '{AT}', '{AT}'),
           (3, '/work/gone', 'gone', 'suspended', 0, 0, '{AT}', '{AT}');
         INSERT INTO participant (id, project_id, handle, role, roles, agent, harness, tier, member_id, created_at, designer) VALUES
           (1, 1, 'human', 'human', '[]', NULL, NULL, NULL, NULL, '{AT}', 0),
           (2, 1, 'chief', 'chief', '[]', 'apollo', 'claude-code', NULL, NULL, '{AT}', 0),
           (3, 1, 'zeus', 'worker', '[\"worker\"]', 'zeus', 'claude-code', 'standard', NULL, '{AT}', 0),
           (4, 2, 'human', 'human', '[]', NULL, NULL, NULL, NULL, '{AT}', 0);
         INSERT INTO task (id, project_id, number, title, body, requester_id, assignee_id, state, created_at, updated_at)
           VALUES (1, 1, 1, 'Parser', 'Parser', 2, 3, 'working', '{AT}', '{AT}');
         INSERT INTO message (id, project_id, recipient_id, sender_id, kind, task_id, body, state, created_at)
           VALUES (1, 1, 3, 2, 'task', 1, 'Parser', 'delivered', '{AT}');
         INSERT INTO event (id, project_id, at, kind, data) VALUES
           (1, 1, '{AT}', 'project.state', '{{\"from\":\"open\",\"to\":\"suspended\",\"resumeOnStart\":true}}');
         DELETE FROM project WHERE id = 3;"
    ))
    .unwrap();
    let project = "SELECT id, directory, name, state, gate, created_at, updated_at FROM project ORDER BY rowid";
    let others = |db: &Connection| -> Vec<Vec<String>> {
        THE_REST
            .iter()
            .map(|table| rows(db, &format!("SELECT * FROM {table} ORDER BY rowid")))
            .collect()
    };
    let sequences = "SELECT name, seq FROM sqlite_sequence ORDER BY rowid";
    let (project_before, others_before, sequences_before) =
        (rows(&db, project), others(&db), rows(&db, sequences));

    // Up to migration 13, which is what this test is of: later ones are theirs.
    migrate(&db, &MIGRATIONS[..13]).unwrap();
    migrate(&db, &MIGRATIONS[..13]).unwrap();

    let version: i64 = db
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, 13);
    let columns: Vec<String> = db
        .prepare("SELECT name FROM pragma_table_info('project') ORDER BY cid")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(
        columns,
        [
            "id",
            "directory",
            "name",
            "state",
            "gate",
            "created_at",
            "updated_at"
        ],
        "the mark is gone, and every other column is where it was"
    );
    assert_eq!(
        rows(&db, "SELECT * FROM project ORDER BY rowid"),
        project_before
    );
    assert_eq!(
        others(&db),
        others_before,
        "the rows of the other tables are as they were"
    );
    assert_eq!(
        rows(&db, sequences),
        sequences_before,
        "each sequence in its place"
    );
    let integrity: String = db
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .unwrap();
    assert_eq!(integrity, "ok");
    let dangling = db
        .prepare("PRAGMA foreign_key_check")
        .unwrap()
        .exists([])
        .unwrap();
    assert!(!dangling, "every reference holds");

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
    // The table kept its checks, and what refers to it still goes with it.
    let refused = |state: &str, gate: i64| {
        db.execute_batch(&format!(
            "INSERT INTO project (directory, name, state, gate, created_at, updated_at)
               VALUES ('/work/x', 'x', '{state}', {gate}, '{AT}', '{AT}')"
        ))
        .unwrap_err()
        .to_string()
    };
    assert!(refused("closed", 0).contains("project_state_check"));
    assert!(refused("open", 2).contains("project_gate_check"));
    db.execute_batch("DELETE FROM project WHERE id = 1")
        .unwrap();
    let left: i64 = db
        .query_row(
            "SELECT count(*) FROM participant WHERE project_id = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(left, 0, "a deleted project takes its participants with it");
}

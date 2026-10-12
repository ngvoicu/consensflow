//! What the tests of the evals share: a ledger file made without the ledger, in a
//! folder of its own, and what the measure says of it.

#![allow(dead_code)]

use std::path::{Path, PathBuf};

use cf_evals::ledger_file::{self, add_project, Chief, Member, NewProject};
use cf_evals::measure::{measure, Metrics, Options};
use rusqlite::Connection;
use serde_json::Value;

/// A folder for a ledger file of a test's own.
pub struct Folder(tempfile::TempDir);

impl Folder {
    pub fn new() -> Self {
        Self(tempfile::tempdir().unwrap())
    }

    /// The ledger file of the folder.
    pub fn ledger(&self) -> PathBuf {
        self.0.path().join("consensflow.db")
    }

    pub fn path(&self) -> &Path {
        self.0.path()
    }
}

/// The recording `name` loaded into a ledger file of the folder, closed as a
/// finished run leaves it: its path.
pub fn recorded(folder: &Folder, name: &str, replacing: &[(&str, &str)]) -> PathBuf {
    let file = folder.ledger();
    drop(ledger_file::load_ledger(&file, name, false, replacing).unwrap());
    file
}

/// What `measure` says of the recording `name`, read from a file closed as a
/// finished run leaves it.
pub fn measured(name: &str) -> Metrics {
    measured_replacing(name, &[])
}

/// [`measured`], with names in the recording put where the caller says.
pub fn measured_replacing(name: &str, replacing: &[(&str, &str)]) -> Metrics {
    let folder = Folder::new();
    measure(&recorded(&folder, name, replacing), &Options::default()).unwrap()
}

/// What the recording `name` says the measure answers, from its `.json`.
pub fn recording(name: &str) -> Value {
    ledger_file::fixture_json(name).unwrap()
}

/// A ledger of a project called `site` in the work folder `/work/site`, whose
/// chief is on Claude Code, with the members `staff` (each on Claude Code, a
/// worker of standard tier): the open ledger, and the project's id. The
/// participants are the human (1), the chief (2) and then the members, in order.
pub fn project_with(folder: &Folder, staff: &[&str]) -> (Connection, i64) {
    let db = ledger_file::open(&folder.ledger(), false).unwrap();
    let members: Vec<Member> = staff
        .iter()
        .map(|agent| Member {
            agent,
            harness: "claude-code",
            roles: &["worker"],
            tier: "standard",
        })
        .collect();
    let project = add_project(
        &db,
        &NewProject {
            directory: "/work/site",
            name: "site",
            chief: Chief {
                harness: "claude-code",
                agent: None,
            },
            staff: &members,
        },
    )
    .unwrap();
    (db, project)
}

/// A task of the chief (2) for the member `assignee` of a ledger made by
/// [`project_with`], with the state `state`: its id.
pub fn task(db: &Connection, project: i64, number: i64, assignee: i64, state: &str) -> i64 {
    db.execute(
        "INSERT INTO task (project_id, number, title, body, requester_id, assignee_id, state, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?3, 2, ?4, ?5, '2026-10-08T10:00:00.000Z', '2026-10-08T10:20:00.000Z')",
        rusqlite::params![project, number, format!("Task {number}"), assignee, state],
    )
    .unwrap();
    db.last_insert_rowid()
}

/// A message of a task, from `sender` to `recipient`, made at `created` and given
/// to its window at `delivered`.
#[allow(clippy::too_many_arguments)]
pub fn message(
    db: &Connection,
    project: i64,
    (recipient, sender): (i64, i64),
    kind: &str,
    task: i64,
    state: &str,
    created: &str,
    delivered: Option<&str>,
) {
    db.execute(
        "INSERT INTO message (project_id, recipient_id, sender_id, kind, task_id, body, state, created_at, delivered_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 'x', ?6, ?7, ?8)",
        rusqlite::params![project, recipient, sender, kind, task, state, created, delivered],
    )
    .unwrap();
}

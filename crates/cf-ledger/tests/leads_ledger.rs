//! The ledger learns the role `lead` (leads step 2, contracts section 1): the migration that
//! widens `participant`'s role check and makes the `lead` table and `task.added_by_id`, with
//! its acceptance on a populated ledger at its predecessor; the reads of `lead` and `added_by`,
//! which a view says only where there is something to say; and `add_lead`, which makes the
//! handle and keeps the limit, and which no route, command or page operation reaches.

#![allow(clippy::expect_used)]

use std::fs;
use std::path::Path;

use cf_ledger::testing::undo_migration_0049;
use cf_ledger::{
    migrate, open_ledger, Ledger, NewChief, NewLead, NewProject, NewTask, Options, ProjectView,
    MIGRATIONS, MOST_LEADS, SCHEMA_VERSION,
};
use rusqlite::Connection;
use serde_json::{json, Value};

/// The migration the leads' step adds is the 49th: the table of its place.
const LEADS: usize = 49;

fn version(db: &Connection) -> i64 {
    db.query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("the SQL of a test reads the ledger")
}

/// A ledger made by the build before the leads' migration, with what a populated one holds:
/// two projects, a chief and a human each, members (one gone), a backup of a member (one gone),
/// tasks and events, and ids that were taken and deleted so the sequences run ahead of the rows.
fn ledger_at_the_predecessor() -> Connection {
    let db = Connection::open_in_memory().expect("a database");
    migrate(&db, &MIGRATIONS[..LEADS - 1]).expect("the predecessor's schema");
    assert_eq!(version(&db), (LEADS - 1) as i64);
    db.execute_batch(
        "INSERT INTO project (id, directory, name, state, created_at, updated_at)
           VALUES (1, '/work/app', 'app', 'open', '2026-11-01T10:00:00.000Z', '2026-11-01T10:00:00.000Z'),
                  (2, '/work/site', 'site', 'open', '2026-11-02T10:00:00.000Z', '2026-11-02T10:00:00.000Z');
         INSERT INTO participant (id, project_id, handle, role, roles, harness, model, tier, created_at, left_at, former_handles)
           VALUES (1, 1, 'human', 'human', '[]', NULL, NULL, NULL, '2026-11-01T10:00:00.000Z', NULL, '[]'),
                  (2, 1, 'chief', 'chief', '[]', 'claude-code', 'apollo', NULL, '2026-11-01T10:00:00.000Z', NULL, '[]'),
                  (3, 1, 'opus-high', 'worker', '[\"worker\",\"reviewer\"]', 'claude-code', 'opus', 'complex', '2026-11-01T10:01:00.000Z', NULL, '[\"opus\"]'),
                  (4, 1, 'sonnet-low', 'advisor', '[\"advisor\"]', 'claude-code', 'sonnet', 'standard', '2026-11-01T10:02:00.000Z', '2026-11-01T11:00:00.000Z', '[]'),
                  (5, 1, 'opus-high-b', 'worker', '[\"worker\"]', 'codex', 'gpt', 'complex', '2026-11-01T10:03:00.000Z', NULL, '[]'),
                  (6, 1, 'gone-backup', 'worker', '[\"worker\"]', 'codex', 'gpt', 'complex', '2026-11-01T10:04:00.000Z', '2026-11-01T12:00:00.000Z', '[]'),
                  (7, 2, 'human', 'human', '[]', NULL, NULL, NULL, '2026-11-02T10:00:00.000Z', NULL, '[]'),
                  (8, 2, 'chief', 'chief', '[]', 'pi', NULL, NULL, '2026-11-02T10:00:00.000Z', NULL, '[]');
         INSERT INTO participant (id, project_id, handle, role, roles, member_id, created_at)
           VALUES (10, 1, 'opus-high-amber-pine', 'worker', '[\"worker\"]', 3, '2026-11-01T10:06:00.000Z');
         UPDATE participant SET backup_of = 3 WHERE id = 5;
         UPDATE participant SET backup_of = 3 WHERE id = 6;
         UPDATE participant SET
             agent = 'agent-of-3', switched_from_harness = 'switched-from-harness-of-3',
             switched_from_agent = 'switched-from-agent-of-3', switched_from_cut = 7, designer = 1,
             switched_from_model = 'switched-from-model-of-3', launched_model = 'launched-model-of-3',
             gone_model = 'gone-model-of-3', effort = 'effort-of-3', model_name = 'model-name-of-3',
             needs_edit = 1, out_until = '2026-11-01T13:00:00.000Z', out_since = '2026-11-01T12:30:00.000Z',
             vacation_since = '2026-11-01T11:30:00.000Z', out_kind = 'busy', out_form = '{\"kind\":\"billing\"}'
           WHERE id = 3;
         INSERT INTO participant (id, project_id, handle, role, roles, created_at) VALUES (9, 2, 'temporary', 'worker', '[\"worker\"]', '2026-11-02T10:05:00.000Z');
         DELETE FROM participant WHERE id = 9;
         INSERT INTO task (id, project_id, number, title, body, requester_id, assignee_id, state, pool, tier, created_at, updated_at)
           VALUES (1, 1, 1, 'Build', 'Build the page', 2, 3, 'working', 'worker', 'complex', '2026-11-01T10:10:00.000Z', '2026-11-01T10:11:00.000Z'),
                  (2, 1, 2, 'Review', 'Review T-1', 2, NULL, 'open', 'reviewer', 'complex', '2026-11-01T10:12:00.000Z', '2026-11-01T10:12:00.000Z'),
                  (3, 2, 1, 'Footer', 'Fix the footer', 8, NULL, 'open', 'worker', 'standard', '2026-11-02T10:12:00.000Z', '2026-11-02T10:12:00.000Z');
         INSERT INTO message (id, project_id, recipient_id, sender_id, kind, task_id, body, state, created_at)
           VALUES (1, 1, 3, 2, 'task', 1, 'Build the page', 'delivered', '2026-11-01T10:10:00.000Z');
         INSERT INTO event (project_id, at, kind, data) VALUES (1, '2026-11-01T10:00:00.000Z', 'project.created', '{}');",
    )
    .expect("a populated ledger");
    db
}

/// Every table's rows by rowid, each value with its type as SQLite quotes it, by the columns given
/// for it (read from the schema of the predecessor: `task` after the migration has one more).
fn every_row(db: &Connection, tables: &[(String, Vec<String>)]) -> Vec<(String, Vec<Vec<String>>)> {
    tables
        .iter()
        .map(|(table, columns)| {
            let quoted = columns
                .iter()
                .map(|column| format!("typeof(\"{column}\") || ':' || quote(\"{column}\")"))
                .collect::<Vec<_>>()
                .join(", ");
            // By rowid, and each value with its type: a value moved to another column or another
            // type is a difference.
            let rows = db
                .prepare(&format!(
                    "SELECT quote(rowid), {quoted} FROM \"{table}\" ORDER BY rowid"
                ))
                .expect("the SQL of a test reads the ledger")
                .query_map([], |row| {
                    (0..=columns.len())
                        .map(|at| row.get::<_, String>(at))
                        .collect::<rusqlite::Result<Vec<_>>>()
                })
                .expect("the SQL of a test reads the ledger")
                .map(Result::unwrap)
                .collect();
            (table.clone(), rows)
        })
        .collect()
}

/// Each column of `participant`, read from the schema, holds in some row a value that is not its
/// default: a column added to the table without a value here fails this, so a copy that forgets
/// it cannot pass by comparing nulls with nulls. The values of the rich row differ from one
/// another's, so two columns exchanged by a copy are a difference.
fn every_column_of_participant_is_seeded(db: &Connection) {
    let columns: Vec<(String, Option<String>)> = db
        .prepare("SELECT name, dflt_value FROM pragma_table_info('participant')")
        .expect("the SQL of a test reads the ledger")
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .expect("the SQL of a test reads the ledger")
        .map(Result::unwrap)
        .collect();
    assert_eq!(
        columns.len(),
        29,
        "participant's columns at the predecessor"
    );
    for (column, default) in columns {
        let default = default.unwrap_or_else(|| "NULL".into());
        let seeded: i64 = db
            .query_row(
                &format!(
                    "SELECT COUNT(*) FROM participant WHERE quote(\"{column}\") != ?1 AND \"{column}\" IS NOT NULL"
                ),
                [&default],
                |row| row.get(0),
            )
            .expect("the SQL of a test reads the ledger");
        assert!(
            seeded > 0,
            "no row of `participant` gives `{column}` a value of its own"
        );
        let distinct: i64 = db
            .query_row(
                &format!("SELECT COUNT(DISTINCT quote(\"{column}\")) FROM participant"),
                [],
                |row| row.get(0),
            )
            .expect("the SQL of a test reads the ledger");
        assert!(distinct > 1, "`{column}` holds one value in every row");
    }
}

/// The tables the predecessor has, each with its columns.
fn tables_of(db: &Connection) -> Vec<(String, Vec<String>)> {
    let names: Vec<String> = db
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
        .expect("the SQL of a test reads the ledger")
        .query_map([], |row| row.get(0))
        .expect("the SQL of a test reads the ledger")
        .map(Result::unwrap)
        .collect();
    names
        .into_iter()
        .map(|table| {
            let columns = db
                .prepare(&format!("PRAGMA table_info(\"{table}\")"))
                .expect("the SQL of a test reads the ledger")
                .query_map([], |row| row.get(1))
                .expect("the SQL of a test reads the ledger")
                .map(Result::unwrap)
                .collect();
            (table, columns)
        })
        .collect()
}

fn schema(db: &Connection, kinds: &str) -> Vec<(String, String, String)> {
    db.prepare(&format!(
        "SELECT type, name, sql FROM sqlite_master WHERE type IN ({kinds}) AND sql IS NOT NULL ORDER BY name"
    ))
    .expect("the SQL of a test reads the ledger")
    .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
    .expect("the SQL of a test reads the ledger")
    .map(Result::unwrap)
    .collect()
}

fn sequences(db: &Connection) -> Vec<(String, i64)> {
    db.prepare("SELECT name, seq FROM sqlite_sequence ORDER BY rowid")
        .expect("the SQL of a test reads the ledger")
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .expect("the SQL of a test reads the ledger")
        .map(Result::unwrap)
        .collect()
}

#[test]
fn the_migration_keeps_every_row_every_id_and_every_sequence_of_a_populated_ledger() {
    let db = ledger_at_the_predecessor();
    let tables = tables_of(&db);
    every_column_of_participant_is_seeded(&db);
    let before = every_row(&db, &tables);
    let ahead = sequences(&db);
    assert!(
        ahead
            .iter()
            .any(|(name, seq)| name == "participant" && *seq == 10),
        "the sequence runs ahead of the rows: {ahead:?}"
    );
    migrate(&db, &MIGRATIONS).expect("migrated");
    assert_eq!(version(&db), SCHEMA_VERSION as i64);
    assert_eq!(
        every_row(&db, &tables),
        before,
        "every row of every table the predecessor has, by its own columns"
    );
    assert_eq!(
        sequences(&db),
        ahead,
        "each sequence and the order they were in"
    );
    let broken: Option<String> = db
        .query_row("PRAGMA foreign_key_check", [], |row| row.get(0))
        .ok();
    assert_eq!(broken, None, "every reference still holds");
    // Departed seats and backup links, named.
    let links: Vec<(i64, Option<i64>, bool)> = db
        .prepare("SELECT id, backup_of, left_at IS NOT NULL FROM participant WHERE backup_of IS NOT NULL ORDER BY id")
        .expect("the SQL of a test reads the ledger")
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .expect("the SQL of a test reads the ledger")
        .map(Result::unwrap)
        .collect();
    assert_eq!(links, [(5, Some(3), false), (6, Some(3), true)]);
    // A new participant takes the next id, not a deleted one's.
    db.execute(
        "INSERT INTO participant (project_id, handle, role, created_at) VALUES (2, 'next', 'worker', 'x')",
        [],
    )
    .expect("a new participant");
    assert_eq!(db.last_insert_rowid(), 11);
}

#[test]
fn the_migration_keeps_each_index_and_each_trigger_as_the_predecessor_held_it() {
    let db = ledger_at_the_predecessor();
    let before = schema(&db, "'index', 'trigger'");
    assert!(
        before
            .iter()
            .filter(|(kind, _, _)| kind == "trigger")
            .count()
            >= 3,
        "the predecessor has the three of 0039: {before:?}"
    );
    migrate(&db, &MIGRATIONS).expect("migrated");
    let after = schema(&db, "'index', 'trigger'");
    // The triggers are the predecessor's with one word more, `lead`, in the two lists of what
    // is no backup and has none; every index and every other word is as it was.
    let widened: Vec<_> = before
        .into_iter()
        .map(|(kind, name, sql)| {
            let sql = if kind == "trigger" {
                sql.replace("('human', 'chief')", "('human', 'chief', 'lead')")
            } else {
                sql
            };
            (kind, name, sql)
        })
        .collect();
    assert_eq!(after, widened, "each index and trigger, by its text");
}

/// The foreign-key check cannot see a trigger that the rebuild dropped: an invalid backup
/// update that is still refused shows it is there.
#[test]
fn each_of_the_three_backup_triggers_still_refuses_what_it_refused() {
    let db = ledger_at_the_predecessor();
    migrate(&db, &MIGRATIONS).expect("migrated");
    let refused = |sql: &str| -> String { db.execute_batch(sql).expect_err(sql).to_string() };
    // The insert trigger: a backup of a participant that is not in its project.
    assert!(
        refused("INSERT INTO participant (project_id, handle, role, created_at, backup_of) VALUES (1, 'bad-insert', 'worker', 'x', 8)")
            .contains("invalid backup relationship")
    );
    // The update trigger: a backup that is its own owner, and one that is a chief.
    assert!(
        refused("UPDATE participant SET backup_of = id WHERE id = 4")
            .contains("invalid backup relationship")
    );
    assert!(
        refused("UPDATE participant SET backup_of = 3, role = 'chief' WHERE id = 4")
            .contains("invalid backup relationship")
    );
    // The owner trigger: an owner with a live backup cannot leave.
    assert!(refused("UPDATE participant SET left_at = 'x' WHERE id = 3")
        .contains("member still has a backup"));
}

/// A lead has no backup, as the chief has none, and is nobody's backup: the three triggers refuse
/// the word `lead` where they refuse `human` and `chief`.
#[test]
fn at_the_table_a_lead_has_no_backup_and_is_no_backup() {
    let db = ledger_at_the_predecessor();
    migrate(&db, &MIGRATIONS).expect("migrated");
    db.execute(
        "INSERT INTO participant (id, project_id, handle, role, created_at) VALUES (20, 1, 'lead-1', 'lead', 'x')",
        [],
    )
    .expect("a lead's row");
    let refused = |sql: &str| -> String { db.execute_batch(sql).expect_err(sql).to_string() };
    // The insert trigger: a backup of a lead, and a lead that is a backup.
    assert!(refused("INSERT INTO participant (project_id, handle, role, created_at, backup_of) VALUES (1, 'bad-1', 'worker', 'x', 20)")
        .contains("invalid backup relationship"));
    assert!(refused("INSERT INTO participant (project_id, handle, role, created_at, backup_of) VALUES (1, 'bad-2', 'lead', 'x', 3)")
        .contains("invalid backup relationship"));
    // The update trigger: a member made the backup of a lead, and a member made a lead that backs up.
    assert!(
        refused("UPDATE participant SET backup_of = 20 WHERE id = 4")
            .contains("invalid backup relationship")
    );
    assert!(
        refused("UPDATE participant SET backup_of = 3, role = 'lead' WHERE id = 4")
            .contains("invalid backup relationship")
    );
    // The owner trigger: a member with a backup cannot become a lead.
    assert!(refused("UPDATE participant SET role = 'lead' WHERE id = 3")
        .contains("member still has a backup"));
}

#[test]
fn the_migration_writes_no_lead_and_no_added_by() {
    let db = ledger_at_the_predecessor();
    migrate(&db, &MIGRATIONS).expect("migrated");
    let leads: i64 = db
        .query_row("SELECT COUNT(*) FROM lead", [], |row| row.get(0))
        .expect("lead is a table");
    let in_role: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM participant WHERE role = 'lead'",
            [],
            |row| row.get(0),
        )
        .expect("the SQL of a test reads the ledger");
    let added: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM task WHERE added_by_id IS NOT NULL",
            [],
            |row| row.get(0),
        )
        .expect("the column is there");
    let tasks: i64 = db
        .query_row("SELECT COUNT(*) FROM task", [], |row| row.get(0))
        .expect("the SQL of a test reads the ledger");
    assert_eq!((leads, in_role, added, tasks), (0, 0, 0, 3));
}

#[test]
fn the_role_check_takes_lead_and_no_other_new_word_and_the_lead_table_holds_its_own_checks() {
    let db = ledger_at_the_predecessor();
    migrate(&db, &MIGRATIONS).expect("migrated");
    db.execute(
        "INSERT INTO participant (id, project_id, handle, role, created_at) VALUES (20, 1, 'lead-1', 'lead', 'x')",
        [],
    )
    .expect("the role lead");
    assert!(db
        .execute(
            "INSERT INTO participant (project_id, handle, role, created_at) VALUES (1, 'boss', 'boss', 'x')",
            [],
        )
        .is_err());
    for (scope, hands) in [("  ", 0), ("x", 2)] {
        assert!(
            db.execute(
                "INSERT INTO lead (participant_id, scope, own_hands) VALUES (20, ?, ?)",
                rusqlite::params![scope, hands],
            )
            .is_err(),
            "{scope:?} {hands}"
        );
    }
    db.execute(
        "INSERT INTO lead (participant_id, scope) VALUES (20, 'the site')",
        [],
    )
    .expect("a lead's row");
    let own_hands: i64 = db
        .query_row("SELECT own_hands FROM lead", [], |row| row.get(0))
        .expect("the SQL of a test reads the ledger");
    assert_eq!(own_hands, 0, "own hands is off by default");
    db.execute_batch("PRAGMA foreign_keys = ON; DELETE FROM participant WHERE id = 20")
        .expect("a lead goes");
    let left: i64 = db
        .query_row("SELECT COUNT(*) FROM lead", [], |row| row.get(0))
        .expect("the SQL of a test reads the ledger");
    assert_eq!(left, 0, "a lead's row goes with its participant");
}

#[test]
fn a_second_open_of_a_migrated_home_writes_nothing() {
    let home = tempfile::tempdir().expect("a home");
    let file = home.path().join("consensflow.db");
    {
        let db = ledger_at_the_predecessor();
        db.execute("VACUUM INTO ?", [file.to_str().expect("a path")])
            .expect("a copy at the predecessor");
    }
    let first = open_ledger(&file, Options::default()).expect("migrated");
    first.close().expect("closed");
    let bytes = fs::read(&file).expect("the file");
    let second = open_ledger(&file, Options::default()).expect("opened again");
    second.close().expect("closed");
    assert_eq!(
        fs::read(&file).expect("the file"),
        bytes,
        "the second open wrote no byte"
    );
    assert!(
        !home.path().join("consensflow.db-wal").exists(),
        "no log left"
    );
    let raw = Connection::open(&file).expect("opens");
    assert_eq!(version(&raw), SCHEMA_VERSION as i64);
}

#[test]
fn an_older_build_refuses_the_whole_home_with_or_without_a_lead() {
    let older = &MIGRATIONS[..LEADS - 1];
    // Without a lead: the schema alone is newer.
    let db = Connection::open_in_memory().expect("a database");
    migrate(&db, &MIGRATIONS).expect("this build's ledger");
    let refused = migrate(&db, older).expect_err("refused");
    assert_eq!(
        (refused.code(), refused.to_string()),
        (
            Some("ledger-newer"),
            format!(
                "this home was written by a newer ConsensFlow (schema {LEADS}; this build knows {})",
                LEADS - 1
            )
        )
    );
    assert_eq!(version(&db), LEADS as i64, "and leaves it as it is");
    // With a lead.
    let home = tempfile::tempdir().expect("a home");
    let file = home.path().join("consensflow.db");
    let mut ledger = open_ledger(&file, Options::default()).expect("a ledger");
    let project = project(&mut ledger);
    ledger
        .add_lead(project.id, &lead("the site"))
        .expect("a lead");
    ledger.close().expect("closed");
    let raw = Connection::open(&file).expect("opens");
    let refused = migrate(&raw, older).expect_err("refused with a lead");
    assert_eq!(refused.code(), Some("ledger-newer"));
    let leads: i64 = raw
        .query_row("SELECT COUNT(*) FROM lead", [], |row| row.get(0))
        .expect("the lead stays");
    assert_eq!(leads, 1);
}

#[test]
fn the_undo_of_the_last_migration_is_the_predecessors_schema_and_the_migration_runs_again() {
    let predecessor = ledger_at_the_predecessor();
    let wanted = schema(&predecessor, "'table', 'index', 'trigger'");
    let tables = tables_of(&predecessor);
    let rows = every_row(&predecessor, &tables);
    migrate(&predecessor, &MIGRATIONS).expect("migrated");
    undo_migration_0049(&predecessor).expect("undone");
    predecessor
        .execute_batch(&format!("PRAGMA user_version = {}", LEADS - 1))
        .expect("the version of the predecessor");
    assert_eq!(
        schema(&predecessor, "'table', 'index', 'trigger'"),
        wanted,
        "the schema the predecessor held, object by object"
    );
    assert_eq!(every_row(&predecessor, &tables), rows);
    migrate(&predecessor, &MIGRATIONS).expect("migrated again");
    assert_eq!(version(&predecessor), SCHEMA_VERSION as i64);
}

fn project(ledger: &mut Ledger) -> ProjectView {
    ledger
        .create_project_overflowing(&NewProject {
            directory: "/work/app".into(),
            name: "app".into(),
            chief: NewChief {
                harness: "claude-code".into(),
                model: Some("apollo".into()),
                effort: None,
                model_name: None,
            },
            staff: vec![],
            gate: false,
            wake_up: None,
            specs: None,
            staff_rules: None,
            compaction: None,
            advisor_tool: false,
            prompt_suggestion: false,
            own_hands: false,
        })
        .expect("a project")
}

fn lead(scope: &str) -> NewLead {
    NewLead {
        harness: "claude-code".into(),
        model: Some("opus".into()),
        effort: Some("high".into()),
        model_name: Some("claude-opus-5-5".into()),
        scope: scope.into(),
        own_hands: false,
    }
}

fn ledger_with_a_project() -> (tempfile::TempDir, Ledger, ProjectView) {
    let home = tempfile::tempdir().expect("a home");
    let mut ledger =
        open_ledger(&home.path().join("consensflow.db"), Options::default()).expect("a ledger");
    let project = project(&mut ledger);
    (home, ledger, project)
}

/// Writes what only a later step's operation writes: through the ledger's own connection, as a person
/// would repair a file.
fn set(ledger: &Ledger, sql: &str) {
    ledger.sqlite().execute_batch(sql).expect("written");
}

#[test]
fn a_lead_is_made_with_its_handle_what_it_leads_and_its_own_hands() {
    let (_home, mut ledger, project) = ledger_with_a_project();
    let made = ledger
        .add_lead(
            project.id,
            &NewLead {
                own_hands: true,
                ..lead("the site's pages")
            },
        )
        .expect("a lead");
    assert_eq!(made.handle, "lead-1");
    assert_eq!(made.role, "lead");
    assert_eq!(
        (
            made.roles.clone(),
            made.tier.clone(),
            made.member_id,
            made.backup_of
        ),
        (vec![], None, None, None)
    );
    assert_eq!(
        (
            made.harness.as_deref(),
            made.model.as_deref(),
            made.effort.as_deref(),
            made.model_name.as_deref()
        ),
        (
            Some("claude-code"),
            Some("opus"),
            Some("high"),
            Some("claude-opus-5-5")
        )
    );
    let view = made.lead.clone().expect("a lead's row");
    assert_eq!(
        (view.scope.as_str(), view.own_hands),
        ("the site's pages", true)
    );
    assert_eq!(
        serde_json::to_value(&made).expect("json")["lead"],
        json!({ "scope": "the site's pages", "ownHands": true })
    );
    // The project reads it, beside the chief, who says nothing of a lead.
    let read = ledger
        .project(project.id)
        .expect("read")
        .expect("a project");
    let handles: Vec<_> = read
        .participants
        .iter()
        .map(|p| p.handle.as_str())
        .collect();
    assert_eq!(handles, ["human", "chief", "lead-1"]);
    let chief = serde_json::to_value(&read.participants[1]).expect("json");
    assert!(chief.get("lead").is_none(), "{chief}");
    let events = ledger.events(project.id, 0, 500).expect("events");
    let added: Vec<_> = events
        .iter()
        .filter(|event| event.kind == "lead.added")
        .collect();
    assert_eq!(added.len(), 1);
    assert_eq!(added[0].data["handle"], "lead-1");
    assert_eq!(added[0].data["scope"], "the site's pages");
    assert_eq!(added[0].data["ownHands"], true);
}

#[test]
fn a_models_name_goes_with_a_model_only() {
    let (_home, mut ledger, project) = ledger_with_a_project();
    let made = ledger
        .add_lead(
            project.id,
            &NewLead {
                model: None,
                effort: None,
                ..lead("pages")
            },
        )
        .expect("a lead on its harness's default");
    assert_eq!(
        (made.model, made.model_name, made.effort),
        (None, None, None)
    );
}

#[test]
fn the_handle_is_the_lowest_lead_n_that_was_never_any_participants() {
    let (_home, mut ledger, project) = ledger_with_a_project();
    let first = ledger.add_lead(project.id, &lead("a")).expect("lead-1");
    let second = ledger.add_lead(project.id, &lead("b")).expect("lead-2");
    assert_eq!(
        (first.handle.as_str(), second.handle.as_str()),
        ("lead-1", "lead-2")
    );
    // lead-1 leaves: its handle is still the project's, present or departed, so the next is lead-3.
    set(
        &ledger,
        &format!(
            "UPDATE participant SET left_at = 'x' WHERE id = {}",
            first.id
        ),
    );
    let third = ledger.add_lead(project.id, &lead("c")).expect("lead-3");
    assert_eq!(third.handle, "lead-3");
    // A participant that once went by `lead-4`, and one that goes by `lead-5` now, are taken too.
    set(
        &ledger,
        &format!(
            "UPDATE participant SET former_handles = '[\"opus\",\"lead-4\"]' WHERE id = {}",
            second.id
        ),
    );
    set(
        &ledger,
        &format!(
            "UPDATE participant SET left_at = 'x' WHERE id = {}",
            second.id
        ),
    );
    set(
        &ledger,
        "INSERT INTO participant (project_id, handle, role, created_at, left_at) SELECT project_id, 'lead-5', 'worker', 'x', 'x' FROM participant WHERE handle = 'chief'",
    );
    let next = ledger
        .add_lead(project.id, &lead("d"))
        .expect("the lowest never used");
    assert_eq!(next.handle, "lead-6");
}

#[test]
fn another_project_has_its_own_lead_1() {
    let (_home, mut ledger, project) = ledger_with_a_project();
    ledger.add_lead(project.id, &lead("a")).expect("lead-1");
    let other = ledger
        .create_project_overflowing(&NewProject {
            directory: "/work/other".into(),
            name: "other".into(),
            chief: NewChief {
                harness: "claude-code".into(),
                model: None,
                effort: None,
                model_name: None,
            },
            staff: vec![],
            gate: false,
            wake_up: None,
            specs: None,
            staff_rules: None,
            compaction: None,
            advisor_tool: false,
            prompt_suggestion: false,
            own_hands: false,
        })
        .expect("a second project");
    let theirs = ledger
        .add_lead(other.id, &lead("b"))
        .expect("its first lead");
    assert_eq!(theirs.handle, "lead-1");
}

#[test]
fn a_fourth_lead_is_refused_in_the_contracts_words_and_nothing_is_written() {
    let (_home, mut ledger, project) = ledger_with_a_project();
    assert_eq!(MOST_LEADS, 3);
    let mut made = Vec::new();
    for scope in ["a", "b", "c"] {
        made.push(ledger.add_lead(project.id, &lead(scope)).expect("a lead"));
    }
    let before = ledger.events(project.id, 0, 500).expect("events").len();
    let refused = ledger
        .add_lead(project.id, &lead("d"))
        .expect_err("a fourth");
    assert_eq!(
        (refused.code(), refused.to_string()),
        (
            Some("too-many-leads"),
            "a project has at most 3 leads: remove one first".into()
        )
    );
    assert_eq!(
        ledger.events(project.id, 0, 500).expect("events").len(),
        before,
        "no event"
    );
    let read = ledger
        .project(project.id)
        .expect("read")
        .expect("a project");
    assert_eq!(read.participants.len(), 5, "no participant");
    // One that left makes room: the limit counts the leads with no `left_at`.
    set(
        &ledger,
        &format!(
            "UPDATE participant SET left_at = 'x' WHERE id = {}",
            made[0].id
        ),
    );
    let again = ledger.add_lead(project.id, &lead("d")).expect("room again");
    assert_eq!(again.handle, "lead-4");
    let refused = ledger
        .add_lead(project.id, &lead("e"))
        .expect_err("three present again");
    assert_eq!(refused.code(), Some("too-many-leads"));
}

#[test]
fn a_leads_model_and_its_name_are_held_to_what_a_chiefs_are() {
    let (_home, mut ledger, project) = ledger_with_a_project();
    let bad_id = ledger
        .add_lead(
            project.id,
            &NewLead {
                model: Some("-flag".into()),
                ..lead("pages")
            },
        )
        .expect_err("a model that reads as a flag");
    assert_eq!(bad_id.code(), Some("invalid-model"));
    let bad_name = ledger
        .add_lead(
            project.id,
            &NewLead {
                model_name: Some("two\nlines".into()),
                ..lead("pages")
            },
        )
        .expect_err("a name with a control character");
    assert_eq!(bad_name.code(), Some("invalid-model-name"));
    let blank_name = ledger
        .add_lead(
            project.id,
            &NewLead {
                model_name: Some("  ".into()),
                ..lead("pages")
            },
        )
        .expect_err("a blank name");
    assert_eq!(blank_name.code(), Some("invalid-model-name"));
    let read = ledger
        .project(project.id)
        .expect("read")
        .expect("a project");
    assert_eq!(read.participants.len(), 2, "nothing was written");
}

#[test]
fn a_blank_scope_or_an_unknown_harness_is_refused_and_nothing_is_written() {
    let (_home, mut ledger, project) = ledger_with_a_project();
    let blank = ledger
        .add_lead(project.id, &lead("  \n "))
        .expect_err("blank");
    assert_eq!(blank.code(), Some("blank-scope"));
    let harness = ledger
        .add_lead(
            project.id,
            &NewLead {
                harness: "notepad".into(),
                ..lead("pages")
            },
        )
        .expect_err("unknown harness");
    assert_eq!(harness.code(), Some("invalid-harness"));
    let unknown = ledger
        .add_lead(project.id + 99, &lead("pages"))
        .expect_err("no project");
    assert_eq!(unknown.code(), Some("unknown-project"));
    let read = ledger
        .project(project.id)
        .expect("read")
        .expect("a project");
    assert_eq!(read.participants.len(), 2);
}

/// Every key of a value, by its path: what a view says.
fn keys(value: &Value, path: &str, into: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (key, inner) in map {
                let at = format!("{path}.{key}");
                into.push(at.clone());
                keys(inner, &at, into);
            }
        }
        Value::Array(items) => items
            .iter()
            .for_each(|item| keys(item, &format!("{path}[]"), into)),
        _ => {}
    }
}

#[test]
fn a_project_with_no_lead_says_nothing_of_one_in_any_view() {
    let (_home, mut ledger, project) = ledger_with_a_project();
    let made = ledger
        .create_task(
            project.id,
            &NewTask {
                from: "chief".into(),
                pool: Some("worker".into()),
                tier: Some("standard".into()),
                body: "Build the page".into(),
                ..NewTask::default()
            },
        )
        .expect("a task");
    let thread = ledger
        .task(project.id, made.task.number)
        .expect("read")
        .expect("a task");
    let mut said = Vec::new();
    keys(
        &serde_json::to_value(&thread).expect("json"),
        "task",
        &mut said,
    );
    keys(
        &serde_json::to_value(
            ledger
                .project(project.id)
                .expect("read")
                .expect("a project"),
        )
        .expect("json"),
        "project",
        &mut said,
    );
    keys(
        &serde_json::to_value(ledger.open_tasks(project.id).expect("open")).expect("json"),
        "open",
        &mut said,
    );
    assert!(!said.is_empty());
    for key in &said {
        assert!(
            !key.ends_with(".lead") && !key.ends_with(".addedBy"),
            "a view of a project with no lead says {key}"
        );
    }
}

#[test]
fn a_task_says_who_added_it_only_where_its_requester_has_changed() {
    let (_home, mut ledger, project) = ledger_with_a_project();
    let made = ledger.add_lead(project.id, &lead("pages")).expect("a lead");
    let task = ledger
        .create_task(
            project.id,
            &NewTask {
                from: "chief".into(),
                pool: Some("worker".into()),
                tier: Some("standard".into()),
                body: "Build the page".into(),
                ..NewTask::default()
            },
        )
        .expect("a task");
    let number = task.task.number;
    let read = |ledger: &Ledger| {
        ledger
            .task(project.id, number)
            .expect("read")
            .expect("a task")
    };
    let thread = read(&ledger);
    assert_eq!(thread.task.added_by, None);
    assert!(serde_json::to_value(&thread)
        .expect("json")
        .get("addedBy")
        .is_none());
    set(
        &ledger,
        &format!(
            "UPDATE task SET added_by_id = {} WHERE number = {number}",
            made.id
        ),
    );
    let thread = read(&ledger);
    assert_eq!(thread.task.added_by.as_deref(), Some("lead-1"));
    assert_eq!(
        serde_json::to_value(&thread).expect("json")["addedBy"],
        "lead-1"
    );
    let board = ledger.open_tasks(project.id).expect("open");
    assert!(board
        .iter()
        .all(|task| task.number != number || task.added_by.as_deref() == Some("lead-1")));
}

/// `add_lead` is the ledger's alone until the page's step: it is compiled only for tests and the
/// `test-support` feature, and this source test is the second guard: no file of the product but
/// the three below names `add_lead` or `NewLead`, by whatever path or wrapper (a bare word, not a
/// call), in any crate, the app and the xtask included. Test folders are the callers to come.
#[test]
fn nothing_but_the_ledger_names_add_lead_or_its_request() {
    fn walk(dir: &Path, found: &mut Vec<String>) {
        for entry in fs::read_dir(dir).expect("a folder") {
            let path = entry.expect("an entry").path();
            let name = path
                .file_name()
                .expect("a name")
                .to_string_lossy()
                .to_string();
            if path.is_dir() {
                if !["target", "node_modules", "tests", ".git"].contains(&name.as_str()) {
                    walk(&path, found);
                }
            } else if name.ends_with(".rs") && name != "tests.rs" && !name.ends_with("_tests.rs") {
                let text = fs::read_to_string(&path).expect("a source");
                if text.contains("add_lead") || text.contains("NewLead") {
                    found.push(path.to_string_lossy().replace('\\', "/"));
                }
            }
        }
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut found = Vec::new();
    for source in ["crates", "app/src-tauri/src", "tools/xtask"] {
        let dir = root.join(source);
        assert!(
            dir.is_dir(),
            "{source} is a folder of the product's sources"
        );
        walk(&dir, &mut found);
    }
    found.sort();
    let found: Vec<String> = found
        .into_iter()
        .map(|path| path.split("/crates/").last().unwrap_or(&path).to_string())
        .collect();
    // And the feature gate is on both: the shipped build holds no such symbol.
    let source = |path: &str| {
        fs::read_to_string(root.join("crates/cf-ledger/src").join(path)).expect("a source")
    };
    assert!(
        source("lib.rs").contains("#[cfg(feature = \"test-support\")]\nmod leads;"),
        "the module is for tests and the test-support feature only"
    );
    assert!(
        source("ledger/staff.rs")
            .contains("#[cfg(feature = \"test-support\")]\n    pub fn add_lead("),
        "the method is for tests and the test-support feature only"
    );
    assert_eq!(
        found,
        [
            "cf-ledger/src/leads.rs",
            "cf-ledger/src/ledger/staff.rs",
            "cf-ledger/src/lib.rs"
        ],
        "who names add_lead or NewLead"
    );
}

//! The four roles of spec new-roles in the ledger (FD-04): their migration (0037)
//! widens the role check of `participant` and the pool check of `task` to the nine roles of the
//! table, every row of a ledger an older build left kept; a member of each new role is held and
//! a task of each new pool is taken by it alone; a build of the migration before it that meets
//! the migrated ledger refuses it as one a newer build wrote; and a follow-up that asks for a
//! role goes only to a window of that role (T-1453).

#![allow(clippy::expect_used)]

use cf_ledger::testing::{new_roles_version, undo_new_roles_migration};
use cf_ledger::{
    migrate, open_ledger, Ledger, NewChief, NewMember, NewProject, NewTask, Options, ProjectView,
    MIGRATIONS, SCHEMA_VERSION,
};
use rusqlite::Connection;
use serde_json::{json, Value};

const NEW: [&str; 4] = ["frontend", "architect", "researcher", "integrator"];

fn member(agent: &str, roles: &[&str], tier: &str) -> NewMember {
    NewMember {
        model: None,
        effort: None,
        model_name: Some(agent.into()),
        harness: "claude-code".into(),
        designer: false,
        roles: roles.iter().map(|role| (*role).to_owned()).collect(),
        tier: tier.into(),
    }
}

fn image_designer() -> NewMember {
    NewMember {
        harness: "codex".into(),
        designer: true,
        ..member("codex-default", &["designer"], "light")
    }
}

fn project(ledger: &mut Ledger, staff: Vec<NewMember>) -> ProjectView {
    project_in(ledger, "app", staff, false)
}

fn project_in(
    ledger: &mut Ledger,
    name: &str,
    staff: Vec<NewMember>,
    prompt_suggestion: bool,
) -> ProjectView {
    ledger
        .create_project(&NewProject {
            directory: format!("/work/{name}"),
            name: name.into(),
            chief: NewChief {
                harness: "claude-code".into(),
                model: Some("apollo".into()),
                effort: None,
                model_name: None,
            },
            staff,
            gate: false,
            wake_up: None,
            specs: None,
            staff_rules: None,
            compaction: None,
            advisor_tool: false,
            prompt_suggestion,
        })
        .expect("a project")
}

fn task(pool: &str, tier: Option<&str>, body: &str) -> NewTask {
    NewTask {
        from: "chief".into(),
        pool: Some(pool.into()),
        tier: tier.map(Into::into),
        body: body.into(),
        ..NewTask::default()
    }
}

/// Every row of `table` in rowid order, each value as SQLite quotes it.
fn rows(db: &Connection, table: &str) -> Vec<Vec<String>> {
    let columns: Vec<String> = db
        .prepare(&format!("PRAGMA table_info(\"{table}\")"))
        .expect("the SQL of a test reads the ledger")
        .query_map([], |row| row.get(1))
        .expect("the SQL of a test reads the ledger")
        .map(Result::unwrap)
        .collect();
    let quoted = columns
        .iter()
        .map(|column| format!("quote(\"{column}\")"))
        .collect::<Vec<_>>()
        .join(", ");
    db.prepare(&format!("SELECT {quoted} FROM \"{table}\" ORDER BY rowid"))
        .expect("the SQL of a test reads the ledger")
        .query_map([], |row| {
            (0..columns.len())
                .map(|at| row.get::<_, String>(at))
                .collect::<rusqlite::Result<Vec<_>>>()
        })
        .expect("the SQL of a test reads the ledger")
        .map(Result::unwrap)
        .collect()
}

/// What a migration of the two tables must keep: their rows and the sequences, and the
/// projects as the migration before it left them (0036's prompt suggestion among their columns).
fn kept(db: &Connection) -> Value {
    json!({
        "projects": rows(db, "project"),
        "participant": rows(db, "participant"),
        "task": rows(db, "task"),
        "sequences": rows(db, "sqlite_sequence"),
        "needs": rows(db, "task_need"),
        "messages": rows(db, "message"),
        "conversations": rows(db, "conversation"),
    })
}

fn schema(db: &Connection) -> Vec<(String, String)> {
    db.prepare("SELECT name, sql FROM sqlite_master WHERE sql IS NOT NULL ORDER BY name")
        .expect("the SQL of a test reads the ledger")
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .expect("the SQL of a test reads the ledger")
        .map(Result::unwrap)
        .collect()
}

fn version(db: &Connection) -> i64 {
    db.query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("the SQL of a test reads the ledger")
}

#[test]
fn a_ledger_a_build_before_the_new_roles_left_with_members_and_tasks_of_the_five_roles_keeps_every_row_through_their_migration(
) {
    let dir = tempfile::tempdir().expect("a home");
    let file = dir.path().join("consensflow.db");
    let mut ledger = open_ledger(&file, Options::default()).expect("a ledger");
    let project = project(
        &mut ledger,
        vec![
            member("zeus", &["worker", "reviewer"], "complex"),
            member("hera", &["advisor"], "complex"),
            member("ares", &["tester"], "standard"),
            member("hebe", &["worker"], "light"),
            image_designer(),
        ],
    );
    for (pool, tier, body) in [
        ("worker", Some("complex"), "Build the page"),
        ("reviewer", Some("complex"), "Review T-1"),
        ("advisor", Some("complex"), "Which store?"),
        ("tester", Some("standard"), "Run T-1"),
        ("designer", None, "Draw the logo"),
        ("worker", Some("light"), "Fix a typo"),
    ] {
        ledger
            .create_task(project.id, &task(pool, tier, body))
            .expect("a task");
    }
    // A second project with the prompt suggestion left on, and one task, so both of 0036's
    // values are in the ledger the migration meets.
    let other = project_in(
        &mut ledger,
        "site",
        vec![member("zeus", &["worker"], "standard")],
        true,
    );
    ledger
        .create_task(
            other.id,
            &task("worker", Some("standard"), "Fix the footer"),
        )
        .expect("a task");
    ledger
        .cancel_task(project.id, 6, "chief")
        .expect("T-6 cancelled");
    ledger
        .remove_member(project.id, "hebe")
        .expect("hebe leaves the staff");
    ledger.close().expect("closed");

    // As a build of the migration before it left it: its checks name the five roles, its
    // version is that migration's.
    let raw = Connection::open(&file).unwrap();
    undo_new_roles_migration(&raw).expect("the new roles' migration undone");
    raw.execute_batch(&format!(
        "PRAGMA user_version = {}",
        new_roles_version() - 1
    ))
    .unwrap();
    let before = kept(&raw);
    let suggestions: Vec<(String, i64)> = raw
        .prepare("SELECT name, prompt_suggestion FROM project ORDER BY id")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(suggestions, [("app".into(), 0), ("site".into(), 1)]);
    let narrow = schema(&raw);
    assert!(
        narrow
            .iter()
            .all(|(_, sql)| !NEW.iter().any(|role| sql.contains(&format!("'{role}'")))),
        "{narrow:?}"
    );
    drop(raw);

    let ledger = open_ledger(&file, Options::default()).expect("migrated");
    ledger.close().expect("closed");
    let raw = Connection::open(&file).unwrap();
    assert_eq!(version(&raw), SCHEMA_VERSION as i64);
    assert_eq!(
        kept(&raw),
        before,
        "every row of the two tables and the projects kept"
    );
    let wide = schema(&raw);
    // The test helper reconstructs the old tables with today's columns and
    // index, but drops the new triggers; migration 0038 puts them back.
    // Compare every other schema object exactly as before.
    let backup_triggers = [
        "participant_backup_insert",
        "participant_backup_update",
        "participant_backup_owner_update",
    ];
    for name in backup_triggers {
        assert!(wide.iter().any(|(found, _)| found == name));
    }
    let narrowed: Vec<(String, String)> = wide
        .iter()
        .filter(|(name, _)| !backup_triggers.contains(&name.as_str()))
        .map(|(name, sql)| {
            (
                name.clone(),
                sql.replace(", 'frontend', 'architect', 'researcher', 'integrator'", ""),
            )
        })
        .collect();
    assert_eq!(narrowed, narrow, "only the two lists changed");
    // Its references still hold, as the migration checked before it committed.
    let broken: Option<String> = raw
        .query_row("PRAGMA foreign_key_check", [], |row| row.get(0))
        .ok();
    assert_eq!(broken, None);
}

#[test]
fn a_build_before_the_new_roles_refuses_the_ledger_once_their_migration_has_run_as_one_a_newer_build_wrote(
) {
    let db = Connection::open_in_memory().unwrap();
    let theirs = new_roles_version();
    migrate(&db, &MIGRATIONS[..theirs]).expect("a ledger with the new roles");
    let older = &MIGRATIONS[..theirs - 1];
    let refused = migrate(&db, older).expect_err("an older build refuses it");
    assert_eq!(
        (refused.code(), refused.to_string()),
        (
            Some("ledger-newer"),
            format!(
                "this home was written by a newer ConsensFlow (schema {theirs}; this build knows {})",
                theirs - 1
            )
        )
    );
    assert_eq!(version(&db), theirs as i64, "and leaves it as it is");
}

#[test]
fn a_member_of_each_new_role_is_held_and_takes_its_pools_tasks_alone() {
    let dir = tempfile::tempdir().expect("a home");
    let mut ledger =
        open_ledger(&dir.path().join("consensflow.db"), Options::default()).expect("a ledger");
    let project = project(
        &mut ledger,
        vec![
            member("zeus", &["worker"], "standard"),
            member("iris", &["frontend"], "light"),
            member("daedalus", &["architect"], "light"),
            member("argus", &["researcher"], "light"),
            member("janus", &["integrator"], "light"),
        ],
    );
    let held: Vec<(String, Vec<String>, Option<String>)> = project
        .participants
        .iter()
        .filter(|participant| participant.member_id.is_none() && participant.role != "chief")
        .filter(|participant| participant.role != "human")
        .map(|participant| {
            (
                participant.role.clone(),
                participant.roles.clone(),
                participant.tier.clone(),
            )
        })
        .collect();
    for role in NEW {
        assert!(
            held.iter().any(|(led, roles, tier)| led == role
                && roles == &[role.to_owned()]
                && tier.as_deref() == Some("light")),
            "{role}: {held:?}"
        );
    }
    for (number, role) in (1..).zip(NEW) {
        let created = ledger
            .create_task(
                project.id,
                &task(role, None, &format!("Work for the {role}")),
            )
            .expect("a task of the pool");
        assert_eq!(
            (created.task.pool.as_deref(), created.task.tier.as_deref()),
            (Some(role), None),
            "a task of the {role}'s pool has no tier"
        );
        let candidates = ledger
            .candidates(project.id, number)
            .expect("who may take it");
        let roles: Vec<&str> = candidates
            .iter()
            .map(|candidate| candidate.member.handle.as_str())
            .map(|handle| {
                [
                    ("iris", "frontend"),
                    ("daedalus", "architect"),
                    ("argus", "researcher"),
                    ("janus", "integrator"),
                    ("zeus", "worker"),
                ]
                .into_iter()
                .find(|(named, _)| *named == handle)
                .map_or(handle, |(_, role)| role)
            })
            .collect();
        assert_eq!(
            roles,
            [role],
            "only a member of the {role}'s role may take it"
        );
    }
    // A tier the ledger is handed with a task of a role with none is not kept, as the image
    // designer's is not (Node's traces hand it one: ledger-tiered-004). `POST /api/tasks`
    // refuses one asked there, in the role's name (T-1453).
    let given = ledger
        .create_task(project.id, &task("integrator", Some("standard"), "Land it"))
        .expect("an integrator's task");
    assert_eq!(given.task.tier, None);
}

#[test]
fn a_build_before_the_new_roles_could_hold_no_member_of_one() {
    // The narrow checks are what an older build has: a row of a new role is no row there.
    let db = Connection::open_in_memory().unwrap();
    migrate(&db, &MIGRATIONS[..new_roles_version() - 1]).unwrap();
    db.execute_batch(
        "INSERT INTO project (id, name, directory, created_at, updated_at) VALUES (1, 'app', '/work/app', 'now', 'now')",
    )
    .ok();
    for role in NEW {
        let refused = db
            .execute(
                "INSERT INTO participant (project_id, handle, role, created_at) VALUES (1, ?, ?, 'now')",
                [role, role],
            )
            .expect_err("the narrow check refuses it");
        assert!(
            refused.to_string().contains("participant_role_check")
                || refused.to_string().contains("CHECK constraint failed"),
            "{refused}"
        );
    }
}

fn tier_of(project: &ProjectView, handle: &str) -> Option<String> {
    project
        .participants
        .iter()
        .find(|participant| participant.handle == handle)
        .and_then(|participant| participant.tier.clone())
}

#[test]
fn a_member_with_no_chosen_tier_gains_a_role_with_tiers_only_with_a_tier_and_one_that_drops_them_is_kept_in_the_lowest(
) {
    let dir = tempfile::tempdir().expect("a home");
    let mut ledger =
        open_ledger(&dir.path().join("consensflow.db"), Options::default()).expect("a ledger");
    let project = project(
        &mut ledger,
        vec![
            member("janus", &["integrator"], "light"),
            member("zeus", &["worker", "integrator"], "complex"),
        ],
    );
    // An integrator alone was given no tier: a worker's work has one, which it must be given.
    let refused = ledger
        .set_roles(project.id, "janus", &["integrator", "worker"], None)
        .expect_err("a worker's tier is the human's choice");
    assert_eq!(
        (refused.code(), refused.to_string()),
        (
            Some("tier-required"),
            "janus has no tier: a worker's tasks have one, so its tier comes with the role"
                .to_owned()
        )
    );
    let unchanged = ledger.project(project.id).unwrap().unwrap();
    assert_eq!(tier_of(&unchanged, "janus").as_deref(), Some("light"));

    // A member that keeps a role with tiers keeps its tier; one left with none has the lowest.
    ledger
        .set_roles(project.id, "zeus", &["integrator"], None)
        .expect("zeus only integrates");
    let now = ledger.project(project.id).unwrap().unwrap();
    assert_eq!(tier_of(&now, "zeus").as_deref(), Some("light"));

    // With its tier the worker comes, and a later change of roles keeps it.
    let given = ledger
        .set_roles(
            project.id,
            "janus",
            &["integrator", "worker"],
            Some("standard"),
        )
        .expect("a worker at standard");
    assert_eq!(
        (given.roles.clone(), given.tier.as_deref()),
        (
            vec!["integrator".to_owned(), "worker".to_owned()],
            Some("standard")
        )
    );
    let kept = ledger
        .set_roles(
            project.id,
            "janus",
            &["worker", "integrator", "reviewer"],
            None,
        )
        .expect("a reviewer too");
    assert_eq!(kept.tier.as_deref(), Some("standard"));
    let moved = ledger
        .set_roles(project.id, "janus", &["worker"], Some("complex"))
        .expect("a worker at complex");
    assert_eq!(moved.tier.as_deref(), Some("complex"));
    // A word that is no tier is refused before anything changes.
    let refused = ledger
        .set_roles(project.id, "janus", &["worker"], Some("huge"))
        .expect_err("no such tier");
    assert_eq!(refused.code(), Some("invalid-tier"));
    let after = ledger.project(project.id).unwrap().unwrap();
    assert_eq!(tier_of(&after, "janus").as_deref(), Some("complex"));
}

fn member_id(project: &ProjectView, handle: &str) -> i64 {
    project
        .participants
        .iter()
        .find(|participant| participant.handle == handle)
        .map(|participant| participant.id)
        .expect("the member")
}

/// A follow-up that asks for a role (the review of T-1441, T-1453): the ledger gave it to the
/// window of the task it follows whatever that window's role was, so that `--research --after`
/// a task of an architect's went to the architect, with nobody on the staff a researcher. It is
/// taken when the window is of the role asked and its member still holds it, and refused
/// otherwise in words that name both roles; a follow-up that asks for none continues the window
/// as before. The window's role is the one it began for (`participant.role` of the session),
/// which a member's change of roles leaves as it was.
#[test]
fn a_follow_up_for_a_role_goes_only_to_a_window_of_that_role_whose_member_still_holds_it() {
    let dir = tempfile::tempdir().expect("a home");
    let mut ledger =
        open_ledger(&dir.path().join("consensflow.db"), Options::default()).expect("a ledger");
    let project = project(
        &mut ledger,
        vec![
            member("zeus", &["worker"], "standard"),
            member("daedalus", &["architect"], "light"),
        ],
    );
    ledger
        .create_task(project.id, &task("architect", None, "The frame"))
        .expect("an architect's task");
    ledger
        .assign_task(project.id, 1, member_id(&project, "daedalus"))
        .expect("given to a window of the architect");
    let follow = |pool: &str| NewTask {
        from: "chief".into(),
        after: Some(1),
        pool: Some(pool.into()),
        body: format!("Next, for the {pool}"),
        ..NewTask::default()
    };
    for (pool, asked) in [
        ("researcher", "a researcher's"),
        ("frontend", "a frontend designer's"),
        ("worker", "a worker's"),
        ("advisor", "an advisor's"),
    ] {
        let refused = ledger
            .create_task(project.id, &follow(pool))
            .expect_err("a window of another role");
        assert_eq!(
            (refused.code(), refused.to_string()),
            (
                Some("follow-up-of-another-role"),
                format!(
                    "T-1 is an architect's task; {asked} task starts in a new window: leave out --after"
                )
            ),
            "{pool}"
        );
    }
    let taken = ledger
        .create_task(project.id, &follow("architect"))
        .expect("the architect's window takes the architect's next step");
    let window = taken.task.assignee.clone().expect("the window of T-1");
    assert!(window.starts_with("daedalus-"), "{window}");
    assert_eq!(taken.task.pool, None, "a follow-up is stored as before");
    // The member gives the role up: its window, begun an architect's, stays one, and is no
    // architect's to continue for the architect's work.
    ledger
        .set_roles(project.id, "daedalus", &["researcher"], None)
        .expect("the member's roles changed");
    let session_role = |ledger: &Ledger| {
        ledger
            .project(project.id)
            .expect("the project")
            .expect("the project")
            .participants
            .into_iter()
            .find(|participant| participant.handle == window)
            .map(|participant| participant.role)
    };
    assert_eq!(session_role(&ledger).as_deref(), Some("architect"));
    let refused = ledger
        .create_task(project.id, &follow("architect"))
        .expect_err("its member holds the role no more");
    assert_eq!(
        (refused.code(), refused.to_string()),
        (
            Some("follow-up-of-another-role"),
            "T-1 is an architect's task, and @daedalus is no architect now: an architect's task starts in a new window: leave out --after".to_owned()
        )
    );
    // And a follow-up that asks for no role continues the window, as it did.
    ledger
        .create_task(
            project.id,
            &NewTask {
                pool: None,
                ..follow("architect")
            },
        )
        .expect("the window's own next step");
    assert_eq!(session_role(&ledger).as_deref(), Some("architect"));
}

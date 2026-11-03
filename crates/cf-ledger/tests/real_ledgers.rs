//! The migration of the models (harness-models, 0029 on this branch, 0031 once landed)
//! checked on copies of real ledgers (T-1202). Not run by default: it reads the folders
//! `CF_LEDGER_COPIES` names (`:`-separated), each a home as the daemon keeps it, a
//! `consensflow.db` (with its `-wal` and `-shm` where they are) and the `agents.json` beside
//! it. Never point it at `~/.consensflow`: it refuses to, and it works on a copy of each
//! folder made in a folder of its own, leaving the folder given as it was.
//!
//!     CF_LEDGER_COPIES=/path/a:/path/b CF_LEDGER_REPORT=/path/report.txt \
//!       cargo test -p cf-ledger --test real_ledgers -- --ignored --nocapture
//!
//! For each copy, as it is and again with agents of the human's own put in (the first member
//! of each project on an agent its `agents.json` saves, the second on one no file or catalog
//! knows), it holds that:
//! - the ledger migrates and opens, and every project reads, its models' lists too;
//! - every participant keeps its id, its harness, its tier and its roles;
//! - every participant of an agent runs on a model, or is the image designer on Codex's
//!   default, or needs an edit, keeping the handle it had; none is turned into a default;
//! - an agent the file saved is run on its model and effort, one it does not have needs an
//!   edit;
//! - every task's and message's participants are there, as many as before;
//! - the handles are unique in each project and the same when the ledger opens again.
//!
//! It writes each project's staff before and after to `CF_LEDGER_REPORT` where it is set.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use cf_ledger::{open_ledger, Options};
use rusqlite::Connection;
use serde_json::{json, Value};

/// A participant as the checks read it, before or after.
#[derive(Debug, Clone, PartialEq)]
struct Seen {
    project: i64,
    handle: String,
    role: String,
    roles: String,
    agent: Option<String>,
    harness: Option<String>,
    model: Option<String>,
    effort: Option<String>,
    tier: Option<String>,
    designer: bool,
    window: bool,
    left: bool,
    needs_edit: bool,
}

fn has(db: &Connection, column: &str) -> bool {
    db.prepare("SELECT name FROM pragma_table_info('participant')")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .any(|name| name.unwrap() == column)
}

fn column(db: &Connection, name: &str) -> String {
    if has(db, name) {
        format!("p.{name}")
    } else {
        "NULL".to_owned()
    }
}

/// Every participant of `db`, by id, read with whatever columns its version has.
fn participants(db: &Connection) -> BTreeMap<i64, Seen> {
    let sql = format!(
        "SELECT p.id, p.project_id, p.handle, p.role, {roles}, {agent}, {harness}, {model},
                {effort}, {tier}, {designer}, {member_id}, p.left_at, {needs_edit}
           FROM participant p",
        roles = column(db, "roles"),
        agent = column(db, "agent"),
        harness = column(db, "harness"),
        model = column(db, "model"),
        effort = column(db, "effort"),
        tier = column(db, "tier"),
        designer = column(db, "designer"),
        member_id = column(db, "member_id"),
        needs_edit = column(db, "needs_edit"),
    );
    db.prepare(&sql)
        .unwrap()
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                Seen {
                    project: row.get(1)?,
                    handle: row.get(2)?,
                    role: row.get(3)?,
                    roles: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
                    agent: row.get(5)?,
                    harness: row.get(6)?,
                    model: row.get(7)?,
                    effort: row.get(8)?,
                    tier: row.get(9)?,
                    designer: row.get::<_, Option<i64>>(10)?.unwrap_or(0) == 1,
                    window: row.get::<_, Option<i64>>(11)?.is_some(),
                    left: row.get::<_, Option<String>>(12)?.is_some(),
                    needs_edit: row.get::<_, Option<i64>>(13)?.unwrap_or(0) == 1,
                },
            ))
        })
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

/// How many tasks and messages `db` holds, and how many point at a participant it has not.
fn history(db: &Connection) -> (i64, i64, i64) {
    let count = |sql: &str| db.query_row(sql, [], |row| row.get::<_, i64>(0)).unwrap();
    (
        count("SELECT COUNT(*) FROM task"),
        count("SELECT COUNT(*) FROM message"),
        count(
            "SELECT
               (SELECT COUNT(*) FROM task t WHERE t.assignee_id IS NOT NULL
                  AND NOT EXISTS (SELECT 1 FROM participant p WHERE p.id = t.assignee_id))
             + (SELECT COUNT(*) FROM task t WHERE t.requester_id IS NOT NULL
                  AND NOT EXISTS (SELECT 1 FROM participant p WHERE p.id = t.requester_id))
             + (SELECT COUNT(*) FROM message m WHERE m.sender_id IS NOT NULL
                  AND NOT EXISTS (SELECT 1 FROM participant p WHERE p.id = m.sender_id))
             + (SELECT COUNT(*) FROM message m WHERE m.recipient_id IS NOT NULL
                  AND NOT EXISTS (SELECT 1 FROM participant p WHERE p.id = m.recipient_id))",
        ),
    )
}

/// The home `from` copied into a folder of the test's own: the ledger and its WAL, and the
/// agents file.
fn copy_home(from: &Path, into: &Path) {
    for name in [
        "consensflow.db",
        "consensflow.db-wal",
        "consensflow.db-shm",
        "agents.json",
    ] {
        let file = from.join(name);
        if file.is_file() {
            std::fs::copy(&file, into.join(name)).unwrap();
        }
    }
}

/// Puts agents of the human's own into the copy at `home`, which must not be migrated yet:
/// in each project, the first member of an agent runs on `mine-<project>`, which the agents
/// file saves (Codex, GPT-6.1 Sol at ultra), and the second on `ghost-<project>`, which no
/// file or catalog knows. Their windows follow them. What was put in, by participant id:
/// the agent's name, and the model and effort it saved where it saved one.
fn put_in_custom_agents(home: &Path) -> BTreeMap<i64, (String, Option<(String, String)>)> {
    let db = Connection::open(home.join("consensflow.db")).unwrap();
    let rows = participants(&db);
    let mut by_project: BTreeMap<i64, Vec<i64>> = BTreeMap::new();
    for (id, seen) in &rows {
        if !seen.window
            && seen.agent.is_some()
            && !seen.designer
            && !matches!(seen.role.as_str(), "human" | "chief" | "designer")
        {
            by_project.entry(seen.project).or_default().push(*id);
        }
    }
    let mut put = BTreeMap::new();
    let mut saved = Vec::new();
    for (project, members) in by_project {
        let mut put_on = |id: i64, agent: String, kind: &str, runs: Option<(String, String)>| {
            let windows = if has(&db, "member_id") {
                format!(" OR member_id = {id}")
            } else {
                String::new()
            };
            db.execute_batch(&format!(
                "UPDATE participant SET agent = '{agent}', harness = '{kind}', designer = 0
                   WHERE id = {id}{windows};"
            ))
            .unwrap();
            put.insert(id, (agent, runs));
        };
        if let Some(&first) = members.first() {
            let agent = format!("mine-{project}");
            saved.push(json!({ "id": agent, "kind": "codex", "model": "gpt-6.1-sol", "effort": "ultra", "workTier": "standard" }));
            put_on(
                first,
                agent,
                "codex",
                Some(("gpt-6.1-sol".to_owned(), "ultra".to_owned())),
            );
        }
        if let Some(&second) = members.get(1) {
            put_on(second, format!("ghost-{project}"), "codex", None);
        }
    }
    let file = home.join("agents.json");
    let mut agents: Value = std::fs::read(&file)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_else(|| json!({ "schemaVersion": 1, "agents": [] }));
    if let Some(list) = agents["agents"].as_array_mut() {
        list.extend(saved);
    }
    std::fs::write(&file, agents.to_string()).unwrap();
    put
}

/// The checks of one copy, as it is or with agents of the human's own put in; what they
/// came to said into `report`.
fn check_copy(
    name: &str,
    from: &Path,
    custom: bool,
    report: &mut String,
    problems: &mut Vec<String>,
) {
    let dir = tempfile::tempdir().unwrap();
    copy_home(from, dir.path());
    let put = if custom {
        put_in_custom_agents(dir.path())
    } else {
        BTreeMap::new()
    };
    let file = dir.path().join("consensflow.db");
    let (before, history_before, version_before) = {
        let db = Connection::open(&file).unwrap();
        let version: i64 = db
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        (participants(&db), history(&db), version)
    };
    let ledger = open_ledger(&file, Options::default())
        .unwrap_or_else(|failed| panic!("{name}: the ledger does not open: {failed}"));
    let projects = ledger.projects().expect("the projects");
    for project in &projects {
        ledger
            .project(project.id)
            .unwrap_or_else(|failed| {
                panic!("{name}: project {} does not read: {failed}", project.id)
            })
            .unwrap_or_else(|| panic!("{name}: project {} is gone", project.id));
    }
    // Said, not stopped at: on a chain whose earlier numbers are not the released ledger's
    // (this branch before its migration is renumbered), a ledger from a release may lack a
    // table one of them made, and the rest is still worth checking.
    for harness in ["claude", "codex", "pi", "devin", "muse"] {
        if let Err(failed) = ledger.model_listing(harness) {
            problems.push(format!(
                "{name}: {harness}'s list of models does not read: {failed}"
            ));
            break;
        }
    }
    drop(ledger);
    let db = Connection::open(&file).unwrap();
    let version_after: i64 = db
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    let after = participants(&db);
    let history_after = history(&db);
    let label = if custom {
        "with agents of the human's own"
    } else {
        "as it is"
    };
    writeln!(
        report,
        "#### {name}, {label}: version {version_before} -> {version_after}; {} participants; tasks {} and messages {} before, {} and {} after; {} pointing at nobody",
        after.len(), history_before.0, history_before.1, history_after.0, history_after.1, history_after.2
    )
    .unwrap();
    assert_eq!(
        before.keys().collect::<Vec<_>>(),
        after.keys().collect::<Vec<_>>(),
        "{name}: every participant is there, by its id"
    );
    assert_eq!(
        (history_after.0, history_after.1, history_after.2),
        (history_before.0, history_before.1, 0),
        "{name}: history keeps its tasks, its messages and their participants"
    );
    let mut handles: BTreeMap<(i64, String), i64> = BTreeMap::new();
    for (id, then) in &before {
        let now = &after[id];
        assert_eq!(
            (&now.harness, &now.tier, &now.roles, &now.role),
            (&then.harness, &then.tier, &then.roles, &then.role),
            "{name}: #{id} {} keeps its harness, tier and roles",
            then.handle
        );
        assert!(
            handles
                .insert((now.project, now.handle.clone()), *id)
                .is_none(),
            "{name}: @{} is two participants of project {}",
            now.handle,
            now.project
        );
        if then.agent.is_some() && now.role != "human" {
            let resolved =
                now.model.is_some() || (now.designer && now.harness.as_deref() == Some("codex"));
            assert!(
                resolved || now.needs_edit || now.role == "chief",
                "{name}: #{id} @{} ({:?} on {:?}) runs on a default it was never given",
                now.handle,
                then.agent,
                now.harness
            );
            if now.needs_edit && !now.window && now.role != "chief" {
                assert_eq!(
                    now.handle, then.handle,
                    "{name}: #{id} keeps its name while it needs an edit"
                );
            }
            assert!(
                !(now.handle.ends_with("-default") && !now.designer),
                "{name}: #{id} @{} is named for a default",
                now.handle
            );
        }
        if let Some((agent, runs)) = put.get(id) {
            match runs {
                Some((model, effort)) => assert_eq!(
                    (now.model.as_deref(), now.effort.as_deref(), now.needs_edit),
                    (Some(model.as_str()), Some(effort.as_str()), false),
                    "{name}: #{id} runs on what {agent} saved"
                ),
                None => assert!(
                    now.needs_edit && now.model.is_none(),
                    "{name}: #{id} of {agent}, which nothing saved, needs an edit"
                ),
            }
        }
    }
    // The same handles when the ledger opens again.
    drop(db);
    let ledger = open_ledger(&file, Options::default()).unwrap();
    drop(ledger);
    let again = participants(&Connection::open(&file).unwrap());
    for (id, now) in &after {
        assert_eq!(
            again[id].handle, now.handle,
            "{name}: #{id} keeps its handle"
        );
    }
    for project in projects {
        writeln!(report, "\n== project {} {}", project.id, project.name).unwrap();
        for (when, rows) in [("before", &before), ("after", &after)] {
            writeln!(report, "  {when}:").unwrap();
            for (id, seen) in rows
                .iter()
                .filter(|(_, seen)| seen.project == project.id && !seen.window)
            {
                writeln!(
                    report,
                    "    #{id} @{} {} {} agent={} {} model={} effort={} tier={}{}{}",
                    seen.handle,
                    seen.role,
                    seen.roles,
                    seen.agent.as_deref().unwrap_or("-"),
                    seen.harness.as_deref().unwrap_or("-"),
                    seen.model.as_deref().unwrap_or("-"),
                    seen.effort.as_deref().unwrap_or("-"),
                    seen.tier.as_deref().unwrap_or("-"),
                    if seen.needs_edit {
                        " NEEDS AN EDIT"
                    } else {
                        ""
                    },
                    if seen.left { " (left)" } else { "" },
                )
                .unwrap();
            }
        }
    }
    writeln!(report).unwrap();
}

#[test]
#[ignore = "reads copies of real ledgers that CF_LEDGER_COPIES names"]
fn every_real_ledger_copy_migrates_with_each_participant_on_what_it_ran_on_or_marked() {
    let env = cf_base::env::Env::from_process();
    let copies = env
        .text("CF_LEDGER_COPIES")
        .expect("CF_LEDGER_COPIES names the copies")
        .to_owned();
    let mut report = String::new();
    let mut problems = Vec::new();
    for folder in copies.split(':').filter(|folder| !folder.is_empty()) {
        let from = PathBuf::from(folder);
        let real = env.path("HOME").map(|home| home.join(".consensflow"));
        assert!(
            real.as_deref() != Some(from.as_path()) && !from.ends_with(".consensflow"),
            "{folder} is the real home: copy it first"
        );
        let name = from
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| folder.to_owned());
        check_copy(&name, &from, false, &mut report, &mut problems);
        check_copy(&name, &from, true, &mut report, &mut problems);
    }
    if let Some(path) = env.path("CF_LEDGER_REPORT") {
        std::fs::write(path, &report).unwrap();
    }
    println!(
        "{}",
        report
            .lines()
            .filter(|line| line.starts_with("####"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

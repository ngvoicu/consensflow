//! The migrations of the models (harness-models: 0031, and 0033 which names every member by its
//! model's id) checked on copies of real ledgers (T-1202, T-1263). Not run by default: it reads the folders
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
//! - the handles are unique in each project and the same when the ledger opens again;
//! - every member with a model is named by its id and effort (its name holds them), each of its
//!   windows by that name and two words, and a name a participant left is kept: in its row's
//!   `former_handles`, or as its `agent` where that was the name it had;
//! - every member's `model_name` is its model's id, a window's its member's, a chief's its model's
//!   (its handle, which is its role, is as it was);
//! - every ownership link is as it was, exactly: each foreign key the schema declares (tasks'
//!   requesters and assignees, messages' senders, recipients, tasks and carriers, windows'
//!   members, conversations' participants, History's items' conversations, task needs, and
//!   what a message was pasted into), row by row against a copy of the ledger taken before the
//!   migration, and every row of tasks, messages, conversations and task needs whole, and
//!   each participant's project, member, role, roles, harness, tier, designer, times and quota
//!   marks.
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
    model_name: Option<String>,
    member: Option<i64>,
    former: Vec<String>,
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
    let sql = sql.replace(
        "FROM participant p",
        &format!(
            ", {}, {} FROM participant p",
            column(db, "model_name"),
            column(db, "former_handles")
        ),
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
                    member: row.get(11)?,
                    model_name: row.get(14)?,
                    former: row
                        .get::<_, Option<String>>(15)?
                        .and_then(|former| serde_json::from_str(&former).ok())
                        .unwrap_or_default(),
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

/// The tables of `db`.
fn tables(db: &Connection) -> Vec<String> {
    db.prepare(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
    )
    .unwrap()
    .query_map([], |row| row.get::<_, String>(0))
    .unwrap()
    .map(Result::unwrap)
    .collect()
}

/// The columns of `table`, as the schema says them.
fn columns_of(db: &Connection, table: &str) -> Vec<String> {
    db.prepare(&format!("SELECT name FROM pragma_table_info('{table}')"))
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

/// The columns of `table` that are a foreign key, whatever they refer to.
fn foreign_keys_of(db: &Connection, table: &str) -> Vec<String> {
    db.prepare(&format!(
        "SELECT DISTINCT \"from\" FROM pragma_foreign_key_list('{table}') ORDER BY 1"
    ))
    .unwrap()
    .query_map([], |row| row.get::<_, String>(0))
    .unwrap()
    .map(Result::unwrap)
    .collect()
}

/// `table`'s rows read at `columns`, each as the text of its values: by its `id` where it has
/// one, else the rows themselves, sorted (a table of pairs has no row to name).
fn rows_at(db: &Connection, table: &str, columns: &[String]) -> BTreeMap<String, String> {
    let keyed = columns_of(db, table).iter().any(|name| name == "id");
    let list = columns
        .iter()
        .map(|name| format!("\"{name}\""))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = if keyed {
        format!("SELECT id, {list} FROM \"{table}\"")
    } else {
        format!("SELECT {list} FROM \"{table}\"")
    };
    let mut read = db.prepare(&sql).unwrap();
    let skip = usize::from(keyed);
    let mut found = BTreeMap::new();
    let mut rows = read.query([]).unwrap();
    let mut number = 0_u64;
    while let Some(row) = rows.next().unwrap() {
        let text = (0..columns.len())
            .map(|at| {
                let value: rusqlite::types::Value = row.get(at + skip).unwrap();
                format!("{}={value:?}", columns[at])
            })
            .collect::<Vec<_>>()
            .join(";");
        let key = if keyed {
            format!("{:020}", row.get::<_, i64>(0).unwrap())
        } else {
            // Rows with no id are a set: the text names the row, and a repeat counts again.
            number += 1;
            format!("{text}#{number}")
        };
        found.insert(key, text);
    }
    if !keyed {
        // Order does not matter for a set of rows: key them by their text, in order of it.
        let mut texts: Vec<String> = found.into_values().collect();
        texts.sort();
        return texts
            .into_iter()
            .enumerate()
            .map(|(at, text)| (format!("{at:012}"), text))
            .collect();
    }
    found
}

/// What `before` and `after` hold of every ownership link, exactly, and of the rows that name
/// who did what whole: how many tables and rows were compared, and every difference, said.
fn compare_links(
    name: &str,
    before: &Connection,
    after: &Connection,
    problems: &mut Vec<String>,
) -> (usize, usize) {
    // The tables that say who did what and what came of it: their rows are compared whole, not
    // only the columns that are a foreign key (a task's number, a message's words, a
    // conversation's session, a follow-up's carrier).
    const WHOLE: [&str; 4] = ["task", "message", "conversation", "task_need"];
    let in_before = tables(before);
    let (mut compared_tables, mut compared_rows) = (0, 0);
    for table in tables(after) {
        if !in_before.contains(&table) {
            continue;
        }
        let had = columns_of(before, &table);
        let mut wanted: Vec<String> = foreign_keys_of(after, &table)
            .into_iter()
            .filter(|column| had.contains(column))
            .collect();
        if WHOLE.contains(&table.as_str()) {
            wanted = columns_of(after, &table)
                .into_iter()
                .filter(|column| had.contains(column))
                .collect();
        }
        if table == "participant" {
            // Who each participant is and belongs to: all but what 0031 and 0033 write (its
            // handle, its model, its effort, its model's name, the names it left, the mark of
            // one that needs an edit).
            wanted = [
                "project_id",
                "role",
                "roles",
                "harness",
                "tier",
                "member_id",
                "designer",
                "created_at",
                "left_at",
                "out_until",
                "out_since",
                "agent",
            ]
            .iter()
            .filter(|column| had.iter().any(|name| name == *column))
            .map(|column| (*column).to_owned())
            .collect();
        }
        if wanted.is_empty() {
            continue;
        }
        let (was, is) = (
            rows_at(before, &table, &wanted),
            rows_at(after, &table, &wanted),
        );
        compared_tables += 1;
        compared_rows += was.len();
        if was != is {
            let moved: Vec<String> = was
                .iter()
                .filter(|(key, text)| is.get(*key) != Some(text))
                .take(3)
                .map(|(key, text)| {
                    format!(
                        "{key}: was {text}; is {}",
                        is.get(key).map_or("gone", String::as_str)
                    )
                })
                .collect();
            problems.push(format!(
                "{name}: {table} ({}) differs after the migration: {} rows before, {} after; {}",
                wanted.join(", "),
                was.len(),
                is.len(),
                moved.join(" | ")
            ));
        }
    }
    (compared_tables, compared_rows)
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
    // The ledger as it is before anything opens it, for every link to be compared with.
    let witness = tempfile::tempdir().unwrap();
    copy_home(dir.path(), witness.path());
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
    drop(ledger);
    let db = Connection::open(&file).unwrap();
    let version_after: i64 = db
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    let after = participants(&db);
    let history_after = history(&db);
    let witnessed = Connection::open(witness.path().join("consensflow.db")).unwrap();
    let problems_before = problems.len();
    let (link_tables, link_rows) = compare_links(name, &witnessed, &db, problems);
    let link_differences = problems.len() - problems_before;
    let label = if custom {
        "with agents of the human's own"
    } else {
        "as it is"
    };
    let renamed = |windows: bool| {
        after
            .iter()
            .filter(|(id, now)| {
                now.window == windows
                    && now.model.is_some()
                    && !matches!(now.role.as_str(), "human" | "chief")
                    && before.get(id).is_some_and(|then| then.handle != now.handle)
            })
            .count()
    };
    let (members_with_a_model, windows_of_them) = (
        after
            .values()
            .filter(|now| {
                !now.window
                    && now.model.is_some()
                    && !matches!(now.role.as_str(), "human" | "chief")
            })
            .count(),
        after
            .values()
            .filter(|now| now.window && now.model.is_some())
            .count(),
    );
    // How each Devin member's listed id was found (0033): in the list this copy keeps, else with
    // the effort's word taken out of its launch id (where what is left reads as its label), else it
    // stays the launch id.
    let kept: Vec<(String, Vec<String>)> = db
        .query_row(
            "SELECT models FROM model_list WHERE harness = 'devin'",
            [],
            |row| row.get::<_, String>(0),
        )
        .ok()
        .and_then(|models| serde_json::from_str::<Value>(&models).ok())
        .and_then(|models| models.as_array().cloned())
        .unwrap_or_default()
        .iter()
        .filter_map(|model| {
            let launches = model["launchIds"].as_object().map_or_else(Vec::new, |ids| {
                ids.values()
                    .filter_map(|id| id.as_str().map(str::to_owned))
                    .collect()
            });
            Some((model["id"].as_str()?.to_owned(), launches))
        })
        .collect();
    let (mut by_list, mut by_hyphen, mut by_launch_id) = (0, 0, 0);
    for now in after.values().filter(|now| {
        !now.window
            && now.harness.as_deref() == Some("devin")
            && now.model.is_some()
            && !matches!(now.role.as_str(), "human" | "chief")
    }) {
        let model = now.model.as_deref().unwrap_or_default();
        if kept
            .iter()
            .any(|(id, launches)| id == model || launches.iter().any(|l| l == model))
        {
            by_list += 1;
        } else if now.model_name.as_deref() != Some(model) {
            by_hyphen += 1;
        } else {
            by_launch_id += 1;
        }
    }
    let chiefs: Vec<String> = after
        .values()
        .filter(|now| now.role == "chief" && now.model.is_some() && !now.needs_edit)
        .map(|now| {
            format!(
                "{} {}",
                now.harness.as_deref().unwrap_or("-"),
                now.model_name.as_deref().unwrap_or("-")
            )
        })
        .collect();
    writeln!(
        report,
        "#### {name}, {label}: {} chiefs with a model, each shown by its id: {}",
        chiefs.len(),
        chiefs.join(", ")
    )
    .unwrap();
    writeln!(
        report,
        "#### {name}, {label}: ownership links: {link_rows} rows of {link_tables} tables compared exactly with the ledger before, {link_differences} differ"
    )
    .unwrap();
    writeln!(
        report,
        "#### {name}, {label}: Devin members' listed ids: {by_list} from the list, {by_hyphen} by taking the effort's word out of the launch id, {by_launch_id} the launch id itself"
    )
    .unwrap();
    writeln!(
        report,
        "#### {name}, {label}: version {version_before} -> {version_after}; {} participants; tasks {} and messages {} before, {} and {} after; {} pointing at nobody; {} of {members_with_a_model} members with a model renamed, {} of their {windows_of_them} windows",
        after.len(), history_before.0, history_before.1, history_after.0, history_after.1, history_after.2,
        renamed(false), renamed(true)
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
        // A chief keeps its handle (its role), and its model is shown by its id, as a member's.
        if matches!(now.role.as_str(), "human" | "chief") {
            assert_eq!(
                now.handle, then.handle,
                "{name}: #{id} {} keeps its handle",
                then.handle
            );
        }
        if now.role == "chief" && now.model.is_some() && !now.needs_edit {
            let model = now.model.as_deref().unwrap();
            assert!(
                now.model_name.as_deref().is_some_and(|shown| {
                    shown == model || now.harness.as_deref() == Some("devin")
                }),
                "{name}: the chief #{id} shows {:?} for {:?}, not its id",
                now.model_name,
                now.model
            );
        }
        // 0033: a member with a model is named by its id and effort; its windows follow; what it
        // left is kept; its `model_name` is its id.
        if now.model.is_some() && !now.needs_edit && !matches!(now.role.as_str(), "human" | "chief")
        {
            if now.window {
                let member = &after[&now.member.unwrap()];
                assert!(
                    now.handle.starts_with(&format!("{}-", member.handle)),
                    "{name}: window #{id} @{} is not named after its member @{}",
                    now.handle,
                    member.handle
                );
                assert_eq!(
                    now.model_name, member.model_name,
                    "{name}: window #{id} shows what its member shows"
                );
            } else {
                assert!(
                    cf_ledger::handles::names_its_model(
                        &now.handle,
                        now.model_name.as_deref(),
                        now.model.as_deref(),
                        now.effort.as_deref()
                    ),
                    "{name}: #{id} @{} ({:?}, {:?}) is not named by its model",
                    now.handle,
                    now.model_name,
                    now.effort
                );
                assert!(
                    now.model_name.is_some()
                        && now.model.as_ref().is_some_and(|model| {
                            now.model_name.as_deref() == Some(model.as_str())
                                || now.harness.as_deref() == Some("devin")
                        }),
                    "{name}: #{id} shows {:?} for {:?}, not its id",
                    now.model_name,
                    now.model
                );
            }
            // (A row the test gave an agent of the human's own under another name has no
            // record of the handle it had: the test made it so; a real one's agent is its name.)
            let was_named_by_its_agent = then
                .agent
                .as_deref()
                .is_none_or(|agent| agent == then.handle);
            if now.handle != then.handle && !now.window && was_named_by_its_agent {
                assert!(
                    now.former.contains(&then.handle) || now.agent.as_deref() == Some(&then.handle),
                    "{name}: #{id} was @{}, which it keeps no record of ({:?}, agent {:?})",
                    then.handle,
                    now.former,
                    now.agent
                );
            }
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

/// A small ledger at this build's schema with every kind of link in it: a project of a human,
/// a chief and two workers (`a`, `b`), a window of `a`, tasks that need each other, a task taken
/// from `a`, messages (a question and the answer to it, carried by another), and a conversation
/// of the window.
fn linked() -> Connection {
    let db = Connection::open_in_memory().unwrap();
    cf_ledger::migrate(&db, &cf_ledger::MIGRATIONS).unwrap();
    db.execute_batch(
        "INSERT INTO project (id, directory, name, state, created_at, updated_at)
           VALUES (1, '/w', 'w', 'open', 't', 't'), (2, '/v', 'v', 'open', 't', 't');
         INSERT INTO event (id, project_id, at, kind, data) VALUES (1, 1, 't', 'member.added', '{}');
         INSERT INTO participant (id, project_id, handle, role, roles, harness, model, tier, member_id, created_at) VALUES
           (1, 1, 'human', 'human', '[]', NULL, NULL, NULL, NULL, 't'),
           (2, 1, 'chief', 'chief', '[]', 'claude-code', 'claude-opus-5-5', NULL, NULL, 't'),
           (3, 1, 'a', 'worker', '[\"worker\"]', 'codex', 'gpt-6-astra', 'light', NULL, 't'),
           (4, 1, 'b', 'worker', '[\"worker\"]', 'codex', 'gpt-6-luna', 'light', NULL, 't'),
           (5, 1, 'a-amber-pine', 'worker', '[\"worker\"]', 'codex', 'gpt-6-astra', 'light', 3, 't');
         INSERT INTO task (id, project_id, number, title, body, requester_id, assignee_id, taken_from_id, state, created_at, updated_at) VALUES
           (1, 1, 1, 'One', 'First', 2, 5, NULL, 'working', 't', 't'),
           (2, 1, 2, 'Two', 'Second', 2, 4, 3, 'queued', 't', 't');
         INSERT INTO task_need (task_id, needs_id) VALUES (2, 1);
         INSERT INTO message (id, project_id, recipient_id, sender_id, kind, task_id, reply_to, body, state, created_at) VALUES
           (1, 1, 2, 5, 'question', 1, NULL, 'Which?', 'delivered', 't'),
           (2, 1, 5, 2, 'answer', 1, 1, 'This.', 'queued', 't'),
           (3, 1, 2, 5, 'result', 1, NULL, 'Done', 'queued', 't');
         INSERT INTO conversation (id, participant_id, harness, native_session, started_at)
           VALUES (1, 5, 'codex', 'session-1', 't');",
    )
    .unwrap();
    db
}

/// The comparison that stands behind "nothing points at nobody" is not one that passes
/// whatever happens: each link moved to another participant that exists, a row lost, a word
/// changed in a task, a message or a conversation, and a participant who was somebody else's
/// each make it say which table changed; the same ledger twice makes it say nothing.
#[test]
fn the_link_comparison_finds_a_link_moved_to_another_participant_that_exists() {
    let mut problems = Vec::new();
    let (tables, rows) = compare_links("same", &linked(), &linked(), &mut problems);
    assert!(problems.is_empty(), "{problems:?}");
    // 5 participants, 2 tasks, 3 messages, 1 conversation, 1 need and 1 event: every row is read.
    assert!(tables >= 6 && rows == 13, "{tables} tables, {rows} rows");
    for (change, table) in [
        ("UPDATE task SET assignee_id = 3 WHERE id = 1", "task"),
        ("UPDATE task SET requester_id = 4 WHERE id = 2", "task"),
        ("UPDATE task SET taken_from_id = 4 WHERE id = 2", "task"),
        ("UPDATE task SET title = 'Other' WHERE id = 1", "task"),
        ("UPDATE message SET sender_id = 3 WHERE id = 1", "message"),
        (
            "UPDATE message SET recipient_id = 4 WHERE id = 2",
            "message",
        ),
        ("UPDATE message SET task_id = 2 WHERE id = 3", "message"),
        ("UPDATE message SET reply_to = 3 WHERE id = 2", "message"),
        ("UPDATE message SET body = 'That.' WHERE id = 2", "message"),
        ("UPDATE conversation SET participant_id = 4", "conversation"),
        (
            "UPDATE conversation SET native_session = 'session-2'",
            "conversation",
        ),
        // A table that is read for its links alone: the log's project.
        ("UPDATE event SET project_id = 2", "event"),
        ("DELETE FROM task_need", "task_need"),
        (
            "UPDATE task_need SET needs_id = 2, task_id = 1",
            "task_need",
        ),
        (
            "UPDATE participant SET member_id = 4 WHERE id = 5",
            "participant",
        ),
        (
            "UPDATE participant SET tier = 'critical' WHERE id = 4",
            "participant",
        ),
        (
            "UPDATE participant SET left_at = 't' WHERE id = 3",
            "participant",
        ),
    ] {
        let changed = linked();
        changed.execute_batch(change).unwrap();
        let mut found = Vec::new();
        compare_links("changed", &linked(), &changed, &mut found);
        assert!(
            found
                .iter()
                .any(|said| said.contains(&format!("{table} ("))),
            "{change} is not found: {found:?}"
        );
    }
    // What the migration does write is no difference: a handle, a model, a name left.
    let renamed = linked();
    renamed
        .execute_batch(
            "UPDATE participant SET handle = 'claude-opus-5-5-high', model_name = 'x',
                    former_handles = '[\"opus-high\"]' WHERE id = 3",
        )
        .unwrap();
    let mut found = Vec::new();
    compare_links("renamed", &linked(), &renamed, &mut found);
    assert!(found.is_empty(), "{found:?}");
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
        // Agents of the human's own are read by migration 0031: a copy already past it (an
        // alpha.2 ledger, schema 32) has none left to read, so it is checked as it is.
        let version: i64 = Connection::open(from.join("consensflow.db"))
            .and_then(|db| db.query_row("PRAGMA user_version", [], |row| row.get(0)))
            .unwrap_or(0);
        if version < 31 {
            check_copy(&name, &from, true, &mut report, &mut problems);
        }
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

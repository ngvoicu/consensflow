//! A ledger file made without the ledger, for the tests and tools that need one
//! to read: the schema is the native ledger's own migrations
//! (`crates/cf-ledger/migrations`, the files its `schema.rs` includes), run in
//! order as its `migrate` runs them, and the rows are a recording
//! (`tests/fixtures/ledgers`) or a few written by hand. The files are read as
//! files, at the time a ledger is made, and nothing of the ledger's code is
//! linked: the measure is held to the schema the product has, and to no reading
//! of it that the product makes.
//!
//! A recording is `<name>.sql`, one `INSERT` for each row of a ledger, and
//! `<name>.json`, what the measure must answer of that ledger.

use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde_json::Value;

use crate::{Error, Result};

#[cfg(test)]
mod tests;

/// Where the ledger's migrations are, from the checkout.
fn migrations_folder() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("cf-ledger")
        .join("migrations")
}

/// Where the recordings of ledgers are, from the checkout.
fn fixtures_folder() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tests")
        .join("fixtures")
        .join("ledgers")
}

/// The ledger's migrations, in order: the text of each `NNNN.sql` of the folder,
/// numbered from 0001 with none missing.
pub fn migrations() -> Result<Vec<String>> {
    migrations_in(&migrations_folder())
}

/// The migrations in `folder`: the files `NNNN.sql`, in order, which are
/// numbered one after another from 0001, or the error that says which is not.
fn migrations_in(folder: &Path) -> Result<Vec<String>> {
    let mut names: Vec<String> = fs::read_dir(folder)
        .map_err(Error::file("list", folder))?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| {
            name.strip_suffix(".sql").is_some_and(|stem| {
                stem.len() == 4 && stem.bytes().all(|byte| byte.is_ascii_digit())
            })
        })
        .collect();
    names.sort();
    for (at, name) in names.iter().enumerate() {
        if *name != format!("{:04}.sql", at + 1) {
            return Err(Error::Refused(format!(
                "the ledger's migrations are numbered in order: {name} is the {}th",
                at + 1
            )));
        }
    }
    names
        .iter()
        .map(|name| {
            fs::read_to_string(folder.join(name)).map_err(Error::file("read", &folder.join(name)))
        })
        .collect()
}

/// The schema version this build's ledger knows: `PRAGMA user_version` after a
/// start, the number of migrations.
pub fn schema_version() -> Result<usize> {
    Ok(migrations()?.len())
}

/// The text of the recording `name` (`tests/fixtures/ledgers/<name>.sql`).
pub fn fixture_sql(name: &str) -> Result<String> {
    let file = fixtures_folder().join(format!("{name}.sql"));
    fs::read_to_string(&file).map_err(Error::file("read", &file))
}

/// What the measure must answer of the recording `name`
/// (`tests/fixtures/ledgers/<name>.json`).
pub fn fixture_json(name: &str) -> Result<Value> {
    let file = fixtures_folder().join(format!("{name}.json"));
    let text = fs::read_to_string(&file).map_err(Error::file("read", &file))?;
    serde_json::from_str(&text)
        .map_err(|cause| Error::Refused(format!("{} is no JSON: {cause}", file.display())))
}

/// The ledger file `file`, made if it is not there, open at the schema this build
/// knows: the migrations after its version run in order, each in a transaction
/// that sets the version, with references unchecked while they run, and a ledger
/// of a newer version refused, in the words the native ledger says it in.
/// `wal` has it in write-ahead mode, as a ledger in use is.
pub fn open(file: &Path, wal: bool) -> Result<Connection> {
    open_at(file, wal, &migrations()?)
}

/// [`open`] at the schema `migrations` make.
fn open_at(file: &Path, wal: bool, migrations: &[String]) -> Result<Connection> {
    let db = Connection::open(file)?;
    // A file made to be read, in a folder that goes: no commit of it waits for the
    // disk (each of a test's inserts was a flush on Windows' runner).
    db.execute_batch("PRAGMA synchronous = OFF")?;
    if wal {
        db.query_row("PRAGMA journal_mode = WAL", [], |_| Ok(()))?;
    }
    let version = user_version(&db)?;
    if version > migrations.len() {
        return Err(Error::Refused(format!(
            "this home was written by a newer ConsensFlow (schema {version}; this build knows {})",
            migrations.len()
        )));
    }
    db.execute_batch("PRAGMA foreign_keys = OFF")?;
    for (at, migration) in migrations.iter().enumerate().skip(version) {
        db.execute_batch("BEGIN IMMEDIATE")?;
        db.execute_batch(migration)?;
        db.execute_batch(&format!("PRAGMA user_version = {}", at + 1))?;
        db.execute_batch("COMMIT")?;
    }
    Ok(db)
}

/// The version of the schema the ledger `db` is at: its `PRAGMA user_version`.
pub fn user_version(db: &Connection) -> Result<usize> {
    let version: i64 = db.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    Ok(usize::try_from(version).unwrap_or_default())
}

/// The rows `sql` inserts, loaded into the new ledger file `file`, open: they
/// must leave every reference holding. `replacing` names text of `sql`, which is
/// in a string, and what it stands for here (a folder's placeholder, whose
/// quotes are doubled, so that a user called O'Brien is a string all the same).
pub fn load_rows(
    file: &Path,
    sql: &str,
    wal: bool,
    replacing: &[(&str, &str)],
) -> Result<Connection> {
    let db = open(file, wal)?;
    let mut rows = sql.to_owned();
    for (token, text) in replacing {
        rows = rows.replace(token, &text.replace('\'', "''"));
    }
    db.execute_batch(&rows)?;
    let broken = db
        .prepare("PRAGMA foreign_key_check")?
        .query_map([], |_| Ok(()))?
        .count();
    if broken > 0 {
        return Err(Error::Refused(format!(
            "the rows leave {broken} references that do not hold"
        )));
    }
    db.execute_batch("PRAGMA foreign_keys = ON")?;
    Ok(db)
}

/// The recording `name` loaded into the new file `file`, open (see [`load_rows`]).
pub fn load_ledger(
    file: &Path,
    name: &str,
    wal: bool,
    replacing: &[(&str, &str)],
) -> Result<Connection> {
    load_rows(file, &fixture_sql(name)?, wal, replacing)
}

/// The chief of a project written by hand: the harness it runs on, and the
/// agent of the roster it is, if it is one.
#[derive(Debug, Clone, Copy)]
pub struct Chief<'a> {
    pub harness: &'a str,
    pub agent: Option<&'a str>,
}

/// A member of the staff of a project written by hand.
#[derive(Debug, Clone, Copy)]
pub struct Member<'a> {
    pub agent: &'a str,
    pub harness: &'a str,
    pub roles: &'a [&'a str],
    pub tier: &'a str,
}

/// A project as the ledger's `createProject` leaves it, which [`add_project`]
/// writes by hand.
#[derive(Debug, Clone, Copy)]
pub struct NewProject<'a> {
    pub directory: &'a str,
    pub name: &'a str,
    pub chief: Chief<'a>,
    pub staff: &'a [Member<'a>],
}

/// A project with its human, its chief and its staff, as the ledger's
/// `createProject` leaves it, written by hand into the open ledger `db`: the rows
/// of the project, of each participant and the log of each member's joining,
/// which comes before the log of the project, as the ledger has it. All or
/// nothing: a member that cannot join (one agent twice) leaves no project
/// behind. The id of the project.
pub fn add_project(db: &Connection, project: &NewProject) -> Result<i64> {
    db.execute_batch("BEGIN IMMEDIATE")?;
    match write_project(db, project) {
        Ok(id) => {
            db.execute_batch("COMMIT")?;
            Ok(id)
        }
        Err(failure) => {
            // The failure is the answer; a rollback that fails too adds nothing to it.
            let _ = db.execute_batch("ROLLBACK");
            Err(failure)
        }
    }
}

/// The rows of [`add_project`], in the transaction the caller began.
fn write_project(db: &Connection, project: &NewProject) -> Result<i64> {
    let at = crate::clock::now();
    db.execute(
        "INSERT INTO project (directory, name, state, gate, created_at, updated_at)
         VALUES (?1, ?2, 'open', 0, ?3, ?3)",
        rusqlite::params![project.directory, project.name, at],
    )?;
    let id = db.last_insert_rowid();
    let join = |handle: &str,
                role: &str,
                roles: &str,
                agent: Option<&str>,
                harness: Option<&str>,
                tier: Option<&str>| {
        db.execute(
            "INSERT INTO participant (project_id, handle, role, roles, agent, harness, designer, tier, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, ?7, ?8)",
            rusqlite::params![id, handle, role, roles, agent, harness, tier, at],
        )
    };
    let log = |kind: &str, data: &serde_json::Value| {
        db.execute(
            "INSERT INTO event (project_id, at, kind, data) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![id, at, kind, data.to_string()],
        )
    };
    join("human", "human", "[]", None, None, None)?;
    join(
        "chief",
        "chief",
        "[]",
        project.chief.agent,
        Some(project.chief.harness),
        None,
    )?;
    for member in project.staff {
        let roles = serde_json::json!(member.roles);
        join(
            member.agent,
            member.roles.first().copied().unwrap_or_default(),
            &roles.to_string(),
            Some(member.agent),
            Some(member.harness),
            Some(member.tier),
        )?;
        log(
            "member.added",
            &serde_json::json!({
                "handle": member.agent,
                "role": member.roles.first().copied().unwrap_or_default(),
                "roles": member.roles,
                "harness": member.harness,
            }),
        )?;
    }
    log(
        "project.created",
        &serde_json::json!({ "name": project.name, "directory": project.directory }),
    )?;
    Ok(id)
}

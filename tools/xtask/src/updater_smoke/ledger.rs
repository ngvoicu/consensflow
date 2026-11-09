//! The ledger of a home, read from outside: `consensflow.db` as it is once no
//! daemon holds it, which the ledger's exclusive lock does not allow before. That
//! lock is the one-writer rule of the flip release (a copied home goes Node, then
//! Rust, then Node again), so the smoke reads the file where no daemon runs and
//! asks what it needs of a running one by other means: the lock's refusal
//! (evidence.rs), and the events the daemon logs as the ledger takes them
//! (`events.jsonl`, appended as they happen, readable at any time).
//!
//! "Whole" is: SQLite finds the file sound, every reference holds, the schema is
//! where the older daemon left it or past it, and what the older daemon wrote is
//! still there as it wrote it. A column that a daemon's restart rewrites by
//! design is not held to its old value: the state a window or a project is in,
//! and when it was last touched.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags};
use serde::Deserialize;
use serde_json::{json, Map, Value};

use super::{files, Error, Result};

/// Where, in a checkout, the list of what its migrations take out on purpose is: it sits with
/// them, and the ledger's own tests hold it to what they drop (cf-ledger's `schema.rs`).
pub const DROPPED: &str = "crates/cf-ledger/migrations/dropped.json";

/// A column (or a whole table) a migration takes out of the ledger on purpose: the state of a
/// ledger the daemon of the update opens is its migrations', and a column that is gone because
/// one of them says so is no loss.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Dropped {
    /// The number of the migration, as `PRAGMA user_version` counts them.
    pub migration: i64,
    pub table: String,
    /// None where the whole table goes.
    #[serde(default)]
    pub column: Option<String>,
    /// The column this one was renamed to, where its values are kept under another name.
    #[serde(default)]
    pub renamed_to: Option<String>,
    pub why: String,
}

/// The list as `text` says it.
pub fn read_dropped(text: &str) -> Result<Vec<Dropped>> {
    serde_json::from_str(text).map_err(|cause| {
        Error::new(format!(
            "the list of what the migrations drop is not what it should be ({DROPPED}): {cause}"
        ))
    })
}

/// The list of the checkout `root`.
pub fn dropped_in(root: &Path) -> Result<Vec<Dropped>> {
    let file = root.join(DROPPED);
    read_dropped(&fs::read_to_string(&file).map_err(files("read", &file))?)
}

/// The columns a daemon rewrites when it starts, or a window when it ends.
const REWRITTEN: [&str; 11] = [
    "state",
    "updated_at",
    "left_at",
    "ended_at",
    "out_until",
    "out_since",
    "attempts",
    "reason",
    "receipt",
    "delivered_at",
    "held_until",
];

/// What closing a window writes on the rows of the ledger besides the columns a restart rewrites
/// by design ([`REWRITTEN`]), as (table, column): the live update case closes the two windows it
/// opened before it installs, between the look at the ledger and the end of it, which the cases of
/// an app replaced by hand don't (the app is quit before the ledger is read). Named one by one: a
/// column that is not here is held to its value.
pub const WINDOWS_CLOSED: [(&str, &str); 1] = [("conversation", "program_gone")];

/// What a column of a row holds.
#[derive(Debug, Clone, PartialEq)]
pub enum Cell {
    Null,
    Integer(i64),
    Real(f64),
    Text(String),
    Blob(Vec<u8>),
}

impl Cell {
    /// The cell as JSON says it, as the smoke tells it of a row.
    fn json(&self) -> Value {
        match self {
            Self::Null => Value::Null,
            Self::Integer(number) => json!(number),
            Self::Real(number) => json!(number),
            Self::Text(text) => json!(text),
            Self::Blob(bytes) => json!(bytes),
        }
    }

    /// The cell as text, where it is some.
    fn text(&self) -> Option<&str> {
        match self {
            Self::Text(text) => Some(text),
            _ => None,
        }
    }
}

/// A row of a table: its columns, in the order the table has them.
#[derive(Debug, Clone, PartialEq)]
pub struct Record(Vec<(String, Cell)>);

impl Record {
    /// What the column `name` holds, where the table has it.
    pub fn get(&self, name: &str) -> Option<&Cell> {
        self.0
            .iter()
            .find(|(column, _)| column == name)
            .map(|(_, cell)| cell)
    }

    /// The row as JSON says it.
    fn json(&self) -> Value {
        Value::Object(
            self.0
                .iter()
                .map(|(column, cell)| (column.clone(), cell.json()))
                .collect::<Map<_, _>>(),
        )
    }

    /// What names the row: its `id`, said as it is.
    fn id(&self) -> String {
        self.get("id")
            .map_or_else(|| "undefined".to_string(), |cell| cell.json().to_string())
    }
}

/// The ledger as read: every table with its rows, the schema's version and what SQLite says of the file.
#[derive(Debug, Clone, PartialEq)]
pub struct Ledger {
    pub version: i64,
    /// What `PRAGMA integrity_check` answered: `ok`, or what is wrong.
    pub integrity: Vec<String>,
    /// What `PRAGMA foreign_key_check` found: a line for each reference that does not hold.
    pub references: Vec<String>,
    pub tables: BTreeMap<String, Vec<Record>>,
}

/// How a failure of SQLite is told.
fn sqlite(file: &Path) -> impl Fn(rusqlite::Error) -> Error + '_ {
    move |cause| {
        Error::new(format!(
            "could not read the ledger {}: {cause}",
            file.display()
        ))
    }
}

/// Every table of the ledger with its rows, the schema's version and what SQLite says of the file.
pub fn read_ledger(file: &Path) -> Result<Ledger> {
    let db = Connection::open_with_flags(file, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(sqlite(file))?;
    read_from(&db, file)
}

/// The ledger of a home whose daemon runs, as it is at this moment: the daemon holds the file in
/// SQLite's exclusive locking mode, so no other connection can open it, and its commits sit in
/// the write-ahead log. The file and its log are copied into `scratch` (a clone is what the
/// board's own backup takes, `backup-live-ledger.sh`), checked to have held still while they
/// were, and the copy is opened, which recovers the log. Written to nothing but `scratch`. The
/// smoke asks it where the daemon is idle: the page has had its answers.
pub fn read_live_ledger(file: &Path, scratch: &Path) -> Result<Ledger> {
    let clone = copy_when_still(file, scratch, |_| {})?;
    // Read and write, for the copy's log is recovered into a shared-memory file of its own, which
    // only a connection that may write can make.
    let db = Connection::open_with_flags(&clone, OpenFlags::SQLITE_OPEN_READ_WRITE)
        .map_err(sqlite(file))?;
    read_from(&db, file)
}

/// How many times the copy of a live ledger is made before it is said to have never held still.
const COPIES: usize = 5;

/// The write-ahead log of the ledger `file`.
fn log_of(file: &Path) -> PathBuf {
    let mut name = file.as_os_str().to_owned();
    name.push("-wal");
    PathBuf::from(name)
}

/// What a ledger file looks like from outside: its size and when it was written; none where it
/// is not there.
fn stamp(file: &Path) -> Option<(u64, Option<std::time::SystemTime>)> {
    let seen = fs::metadata(file).ok()?;
    Some((seen.len(), seen.modified().ok()))
}

/// Copies the ledger `file` and its log into `scratch` as one whose stamps are the same before and
/// after the copy, and answers where the copy is; `between` is told the number of each try once
/// its copy is made (a test writes to the ledger there). A ledger written to all the while is
/// said so.
fn copy_when_still(file: &Path, scratch: &Path, mut between: impl FnMut(usize)) -> Result<PathBuf> {
    let log = log_of(file);
    let clone = scratch.join("live.db");
    let clone_log = log_of(&clone);
    for attempt in 1..=COPIES {
        let before = (stamp(file), stamp(&log));
        let _ = fs::remove_file(&clone);
        let _ = fs::remove_file(&clone_log);
        fs::copy(file, &clone).map_err(files("copy", file))?;
        if before.1.is_some() {
            // A log that is gone by now was checkpointed into the file: the stamps say so.
            match fs::copy(&log, &clone_log) {
                Ok(_) => {}
                Err(cause) if cause.kind() == io::ErrorKind::NotFound => {}
                Err(cause) => {
                    return Err(Error::new(format!(
                        "could not copy {}: {cause}",
                        log.display()
                    )))
                }
            }
        }
        between(attempt);
        if (stamp(file), stamp(&log)) == before {
            return Ok(clone);
        }
        thread::sleep(Duration::from_millis(200));
    }
    Err(Error::new(format!(
        "the ledger {} never held still for a copy ({COPIES} tries)",
        file.display()
    )))
}

/// Every table of the open ledger `db`, which is the file `file`.
fn read_from(db: &Connection, file: &Path) -> Result<Ledger> {
    let told = sqlite(file);
    let names: Vec<String> = db
        .prepare(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
        )
        .and_then(|mut names| names.query_map([], |row| row.get(0))?.collect())
        .map_err(&told)?;
    let mut tables = BTreeMap::new();
    for name in names {
        let select = format!(
            "SELECT * FROM \"{}\" ORDER BY rowid",
            name.replace('"', "\"\"")
        );
        let rows = db
            .prepare(&select)
            .and_then(|mut select| {
                let columns: Vec<String> = select
                    .column_names()
                    .iter()
                    .map(ToString::to_string)
                    .collect();
                select
                    .query_map([], |row| {
                        columns
                            .iter()
                            .enumerate()
                            .map(|(at, column)| Ok((column.clone(), cell_of(row.get_ref(at)?))))
                            .collect::<rusqlite::Result<Vec<_>>>()
                            .map(Record)
                    })?
                    .collect()
            })
            .map_err(&told)?;
        tables.insert(name, rows);
    }
    let version = db
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(&told)?;
    let integrity = db
        .prepare("PRAGMA integrity_check")
        .and_then(|mut check| check.query_map([], |row| row.get(0))?.collect())
        .map_err(&told)?;
    let references = db
        .prepare("PRAGMA foreign_key_check")
        .and_then(|mut check| {
            check
                .query_map([], |row| {
                    Ok(format!(
                        "{} {:?} {} {}",
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<i64>>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?
                    ))
                })?
                .collect()
        })
        .map_err(&told)?;
    Ok(Ledger {
        version,
        integrity,
        references,
        tables,
    })
}

fn cell_of(value: ValueRef<'_>) -> Cell {
    match value {
        ValueRef::Null => Cell::Null,
        ValueRef::Integer(number) => Cell::Integer(number),
        ValueRef::Real(number) => Cell::Real(number),
        ValueRef::Text(text) => Cell::Text(String::from_utf8_lossy(text).into_owned()),
        ValueRef::Blob(bytes) => Cell::Blob(bytes.to_vec()),
    }
}

/// The file is a sound ledger: SQLite's own check, every reference, and a schema this build knows.
pub fn assert_sound(ledger: &Ledger, at_least: i64) -> Result {
    ensure!(
        ledger.integrity == ["ok"],
        "SQLite's integrity check: {}",
        ledger.integrity.join(",")
    );
    ensure!(
        ledger.references.is_empty(),
        "a reference of the ledger does not hold"
    );
    ensure!(
        ledger.version >= at_least,
        "the schema is at {}, below {at_least}",
        ledger.version
    );
    ensure!(!ledger.tables.is_empty(), "the ledger holds no table");
    Ok(())
}

/// The ledger holds a project for each of `directories`.
pub fn assert_projects(ledger: &Ledger, directories: &[PathBuf]) -> Result {
    let held: Vec<&str> = ledger
        .tables
        .get("project")
        .into_iter()
        .flatten()
        .filter_map(|row| row.get("directory").and_then(Cell::text))
        .collect();
    for directory in directories {
        let directory = directory.to_string_lossy();
        ensure!(
            held.contains(&directory.as_ref()),
            "the ledger has no project in {directory}: {}",
            held.join(",")
        );
    }
    Ok(())
}

/// Nothing the ledger held is gone or changed: each row `before` had is there
/// after, by its `id`, with each column it had, bar the ones a restart rewrites; and
/// the schema has not gone back. A table with no `id` (a link between two others)
/// has its rows by what they hold. A column is not looked for where a migration that
/// ran between the two versions takes it out (`dropped`); one renamed has its values
/// under the new name. Whatever else is gone is lost.
pub fn assert_kept(
    before: &Ledger,
    after: &Ledger,
    dropped: &[Dropped],
    rewritten: &[(&str, &str)],
) -> Result {
    assert_sound(after, before.version)?;
    // What the migrations that ran between the two ledgers took out.
    let taken_out = |table: &str, column: Option<&str>| {
        dropped.iter().find(|each| {
            each.table == table
                && each.column.as_deref() == column
                && before.version < each.migration
                && each.migration <= after.version
        })
    };
    for (table, rows) in &before.tables {
        let Some(now) = after.tables.get(table) else {
            if taken_out(table, None).is_some() {
                continue;
            }
            return Err(Error::new(format!("the ledger lost its {table} table")));
        };
        for row in rows {
            let kept = now.iter().rev().find(|each| match row.get("id") {
                Some(id) => each.get("id") == Some(id),
                None => each.0 == row.0,
            });
            let Some(kept) = kept else {
                return Err(Error::new(format!(
                    "{table} {} is gone: {}",
                    row.id(),
                    row.json()
                )));
            };
            for (column, value) in &row.0 {
                if REWRITTEN.contains(&column.as_str())
                    || rewritten.contains(&(table.as_str(), column.as_str()))
                {
                    continue;
                }
                let held = match (kept.get(column), taken_out(table, Some(column))) {
                    (Some(held), _) => Some(held),
                    // A column a migration took out is not there, and is no loss: its values
                    // are under another name where it was renamed.
                    (None, Some(dropped)) => match &dropped.renamed_to {
                        None => continue,
                        Some(new) => kept.get(new),
                    },
                    (None, None) => None,
                };
                ensure!(
                    held == Some(value),
                    "{table} {}: {column} was {} and is {}",
                    row.id(),
                    value.json(),
                    held.map_or_else(|| "undefined".to_string(), |cell| cell.json().to_string())
                );
            }
        }
    }
    Ok(())
}

/// The ledger events a daemon's trace file holds, in order: its lines of the
/// four keys a ledger event is written with (`at`, `project`, `kind`, `data`),
/// which no other line has.
pub fn traced_events(text: &str) -> Vec<Value> {
    text.split('\n')
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|parsed| {
            parsed
                .as_object()
                .is_some_and(|keys| keys.keys().eq(["at", "project", "kind", "data"]))
        })
        .collect()
}

/// Each event the older daemon traced is in the ledger the newer one holds.
pub fn assert_traced(events: &[Value], ledger: &Ledger) -> Result {
    let mut held = Vec::new();
    for row in ledger.tables.get("event").into_iter().flatten() {
        let cell = |name: &str| row.get(name).map_or(Value::Null, Cell::json);
        let data = match row.get("data") {
            Some(Cell::Text(text)) => serde_json::from_str::<Value>(text).map_err(|cause| {
                Error::new(format!("an event of the ledger holds no JSON: {cause}"))
            })?,
            _ => Value::Null,
        };
        held.push(json!([cell("at"), cell("project_id"), cell("kind"), data]).to_string());
    }
    for event in events {
        let key = json!([event["at"], event["project"], event["kind"], event["data"]]).to_string();
        ensure!(
            held.contains(&key),
            "the ledger lost an event the daemon traced: {key}"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests;

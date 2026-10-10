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
use std::path::{Path, PathBuf};

use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags};
use serde_json::{json, Map, Value};

use super::{Error, Result};

/// The columns a daemon rewrites when it starts, or a window when it ends.
const REWRITTEN: [&str; 12] = [
    "state",
    "updated_at",
    "left_at",
    "ended_at",
    "out_until",
    "out_since",
    "resume_on_start",
    "attempts",
    "reason",
    "receipt",
    "delivered_at",
    "held_until",
];

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
    let told = sqlite(file);
    let db = Connection::open_with_flags(file, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(&told)?;
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
/// has its rows by what they hold.
pub fn assert_kept(before: &Ledger, after: &Ledger) -> Result {
    assert_sound(after, before.version)?;
    for (table, rows) in &before.tables {
        let Some(now) = after.tables.get(table) else {
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
                if REWRITTEN.contains(&column.as_str()) {
                    continue;
                }
                ensure!(
                    kept.get(column) == Some(value),
                    "{table} {}: {column} was {} and is {}",
                    row.id(),
                    value.json(),
                    kept.get(column)
                        .map_or_else(|| "undefined".to_string(), |cell| cell.json().to_string())
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

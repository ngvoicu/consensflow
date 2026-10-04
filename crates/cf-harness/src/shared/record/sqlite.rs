//! A harness's SQLite store, read as `node:sqlite`'s `DatabaseSync(file, {
//! readOnly: true })` read it (OpenCode's store, Devin's sessions):
//! - read-only, and never waiting on a writer: a busy timeout of none, as
//!   `DatabaseSync`'s default is;
//! - never `immutable` or `nolock`, so a writer's changes are seen as SQLite
//!   promises them. Like Node, a reader cannot promise that no `-wal` or
//!   `-shm` file appears beside the store;
//! - each look one connection, its reads in one deferred transaction;
//! - a row as JavaScript held it: an object of its columns, an integer or a
//!   real a number, text as text.

use std::path::Path;
use std::time::Duration;

use rusqlite::types::{Value as Bound, ValueRef};
use rusqlite::{Connection, OpenFlags, Params, Row, Statement};
use serde_json::{Map, Number, Value};

/// The largest integer a JavaScript number holds exactly, either way
/// (`Number.MAX_SAFE_INTEGER`).
const SAFE: u64 = (1 << 53) - 1;

/// A store opened for a look.
pub(crate) struct Store {
    connection: Connection,
}

impl Store {
    /// The store in `file`, opened read-only.
    pub(crate) fn open(file: &Path) -> Result<Self, String> {
        let connection = Connection::open_with_flags(
            file,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|error| error.to_string())?;
        connection
            .busy_timeout(Duration::ZERO)
            .map_err(|error| error.to_string())?;
        Ok(Self { connection })
    }

    /// `read`, under one deferred transaction: every query of it reads the
    /// same state of the store. The transaction ends with the read, its
    /// failure too.
    pub(crate) fn read<T>(
        &self,
        read: impl FnOnce(&Reads<'_>) -> Result<T, String>,
    ) -> Result<T, String> {
        self.connection
            .execute_batch("BEGIN")
            .map_err(|error| error.to_string())?;
        let read = read(&Reads {
            connection: &self.connection,
        });
        // A read-only transaction holds nothing to keep, and one SQLite
        // already ended is no failure of the read.
        let _ = self.connection.execute_batch("ROLLBACK");
        read
    }
}

/// The queries of one read.
pub(crate) struct Reads<'a> {
    connection: &'a Connection,
}

impl Reads<'_> {
    /// Every row `sql` selects with `params` (`.all()`), each an object of its
    /// columns.
    pub(crate) fn all(
        &self,
        sql: &str,
        params: impl Params,
    ) -> Result<Vec<Map<String, Value>>, String> {
        let mut statement = self.prepare(sql)?;
        let names = names(&statement);
        let mut rows = statement.query(params).map_err(|error| error.to_string())?;
        let mut all = Vec::new();
        while let Some(row) = rows.next().map_err(|error| error.to_string())? {
            all.push(object(row, &names)?);
        }
        Ok(all)
    }

    /// The first row `sql` selects with `params` (`.get()`), none when it
    /// selects none.
    pub(crate) fn get(
        &self,
        sql: &str,
        params: impl Params,
    ) -> Result<Option<Map<String, Value>>, String> {
        let mut statement = self.prepare(sql)?;
        let names = names(&statement);
        let mut rows = statement.query(params).map_err(|error| error.to_string())?;
        rows.next()
            .map_err(|error| error.to_string())?
            .map(|row| object(row, &names))
            .transpose()
    }

    fn prepare(&self, sql: &str) -> Result<Statement<'_>, String> {
        self.connection
            .prepare(sql)
            .map_err(|error| error.to_string())
    }
}

fn names(statement: &Statement<'_>) -> Vec<String> {
    statement
        .column_names()
        .into_iter()
        .map(str::to_owned)
        .collect()
}

/// A row as `node:sqlite` hands it to JavaScript: its columns by name, a
/// later column of a name the earlier's value.
fn object(row: &Row<'_>, names: &[String]) -> Result<Map<String, Value>, String> {
    let mut fields = Map::new();
    for (index, name) in names.iter().enumerate() {
        let value = row.get_ref(index).map_err(|error| error.to_string())?;
        fields.insert(name.clone(), column(value)?);
    }
    Ok(fields)
}

/// A column's value as JavaScript held it: null, a number, or text, its bytes
/// that are no UTF-8 as U+FFFD. An integer past what a number holds exactly
/// fails, as `node:sqlite` throws on one when it reads no `BigInt`. A blob is
/// a `Uint8Array` there, which no reader reads: it fails here.
fn column(value: ValueRef<'_>) -> Result<Value, String> {
    match value {
        ValueRef::Null => Ok(Value::Null),
        ValueRef::Integer(whole) if whole.unsigned_abs() <= SAFE => Ok(Value::from(whole)),
        ValueRef::Integer(whole) => Err(format!(
            "the integer {whole} is past what a JavaScript number holds"
        )),
        ValueRef::Real(real) => Number::from_f64(real)
            .map(Value::Number)
            .ok_or_else(|| format!("the real {real} is no JSON number")),
        ValueRef::Text(bytes) => Ok(Value::String(String::from_utf8_lossy(bytes).into_owned())),
        ValueRef::Blob(_) => Err("a blob, where no reader reads one".to_owned()),
    }
}

/// What a read gave, bound back as `node:sqlite` binds the JavaScript value:
/// a number as a double, an integer too; text as text; null as NULL. A read
/// gives no other kind.
pub(crate) fn bound(value: &Value) -> Bound {
    match value {
        Value::Number(number) => number.as_f64().map_or(Bound::Null, Bound::Real),
        Value::String(text) => Bound::Text(text.clone()),
        _ => Bound::Null,
    }
}

#[cfg(test)]
mod tests;

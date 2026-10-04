//! A harness's SQLite store, read as `node:sqlite`'s `DatabaseSync(file, {
//! readOnly: true })` read it (OpenCode's store, Devin's sessions):
//! - read-only, and never waiting on a writer: a busy timeout of none, as
//!   `DatabaseSync`'s default is;
//! - with `DatabaseSync`'s other defaults: foreign keys enforced, and no
//!   double-quoted string literal, where SQLite itself takes `"x"` for one
//!   when no column has that name;
//! - never `immutable` or `nolock`, so a writer's changes are seen as SQLite
//!   promises them. Like Node, a reader cannot promise that no `-wal` or
//!   `-shm` file appears beside the store;
//! - each look one connection, its reads in one deferred transaction;
//! - a row as JavaScript held it: an object of its columns, each a [`Cell`].

mod cell;

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use rusqlite::config::DbConfig;
use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags, Params, Statement};

pub(crate) use cell::Cell;

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
        let failed = |error: rusqlite::Error| error.to_string();
        let connection = Connection::open_with_flags(
            file,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(failed)?;
        for (config, on) in [
            (DbConfig::SQLITE_DBCONFIG_DQS_DML, false),
            (DbConfig::SQLITE_DBCONFIG_DQS_DDL, false),
            (DbConfig::SQLITE_DBCONFIG_ENABLE_FKEY, true),
        ] {
            connection.set_db_config(config, on).map_err(failed)?;
        }
        connection.busy_timeout(Duration::ZERO).map_err(failed)?;
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
    /// Every row `sql` selects with `params` (`.all()`).
    pub(crate) fn all(&self, sql: &str, params: impl Params) -> Result<Vec<Row>, String> {
        let mut statement = self.prepare(sql)?;
        let names = names(&statement);
        let mut rows = statement.query(params).map_err(|error| error.to_string())?;
        let mut all = Vec::new();
        while let Some(row) = rows.next().map_err(|error| error.to_string())? {
            all.push(Row::read(row, &names)?);
        }
        Ok(all)
    }

    /// The first row `sql` selects with `params` (`.get()`), none when it
    /// selects none.
    pub(crate) fn get(&self, sql: &str, params: impl Params) -> Result<Option<Row>, String> {
        let mut statement = self.prepare(sql)?;
        let names = names(&statement);
        let mut rows = statement.query(params).map_err(|error| error.to_string())?;
        rows.next()
            .map_err(|error| error.to_string())?
            .map(|row| Row::read(row, &names))
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

/// A row as `node:sqlite` hands it to JavaScript: its columns by name, in
/// their order, a later column of a name holding the earlier's place with
/// its own value.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Row {
    columns: Vec<(String, Cell)>,
}

impl Row {
    fn read(row: &rusqlite::Row<'_>, names: &[String]) -> Result<Self, String> {
        let mut columns: Vec<(String, Cell)> = Vec::with_capacity(names.len());
        for (index, name) in names.iter().enumerate() {
            let cell = cell(row.get_ref(index).map_err(|error| error.to_string())?)?;
            match columns.iter_mut().find(|(held, _)| held == name) {
                Some((_, held)) => *held = cell,
                None => columns.push((name.clone(), cell)),
            }
        }
        Ok(Self { columns })
    }

    /// The column `name`, none where the row has no such column.
    pub(crate) fn get(&self, name: &str) -> Option<&Cell> {
        self.columns
            .iter()
            .find(|(held, _)| held == name)
            .map(|(_, cell)| cell)
    }

    /// The column `name`, taken out of the row.
    pub(crate) fn take(&mut self, name: &str) -> Option<Cell> {
        let at = self.columns.iter().position(|(held, _)| held == name)?;
        Some(self.columns.remove(at).1)
    }
}

/// A column's value as JavaScript held it. An integer past what a number
/// holds exactly fails, as `node:sqlite` throws on one when it reads no
/// `BigInt`; text that is no UTF-8 has U+FFFD for what is not.
fn cell(value: ValueRef<'_>) -> Result<Cell, String> {
    match value {
        ValueRef::Null => Ok(Cell::Null),
        // Within 2^53, so a double holds it exactly.
        #[allow(clippy::cast_precision_loss)]
        ValueRef::Integer(whole) if whole.unsigned_abs() <= SAFE => Ok(Cell::Number(whole as f64)),
        ValueRef::Integer(whole) => Err(format!(
            "the integer {whole} is past what a JavaScript number holds"
        )),
        ValueRef::Real(real) => Ok(Cell::Number(real)),
        ValueRef::Text(bytes) => Ok(Cell::Text(String::from_utf8_lossy(bytes).into_owned())),
        ValueRef::Blob(bytes) => Ok(Cell::Bytes(Arc::from(bytes))),
    }
}

#[cfg(test)]
mod tests;

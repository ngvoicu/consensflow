//! The database a ledger left, as the recorder dumps it (and as step 3.1's
//! replay does): its version, its schema, and each table's rows in rowid
//! order, each value as SQLite quotes it.

use std::path::Path;

use rusqlite::{Connection, OpenFlags};
use serde_json::{json, Value};

pub fn dump(file: &Path) -> Value {
    let db = Connection::open_with_flags(file, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .expect("the ledger's file");
    let schema: Vec<Value> = db
        .prepare("SELECT name, sql FROM sqlite_master WHERE type IN ('table', 'index') AND sql IS NOT NULL ORDER BY name")
        .expect("the schema")
        .query_map([], |row| Ok(json!({ "name": row.get::<_, String>(0)?, "sql": row.get::<_, String>(1)? })))
        .expect("the schema")
        .map(|entry| entry.expect("a schema entry"))
        .collect();
    let names: Vec<String> = db
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
        .expect("the tables")
        .query_map([], |row| row.get(0))
        .expect("the tables")
        .map(|name| name.expect("a table's name"))
        .collect();
    let mut tables = serde_json::Map::new();
    for name in names {
        let columns: Vec<String> = db
            .prepare(&format!("PRAGMA table_info(\"{name}\")"))
            .expect("a table's columns")
            .query_map([], |row| row.get(1))
            .expect("a table's columns")
            .map(|column| column.expect("a column"))
            .collect();
        let quoted = columns
            .iter()
            .map(|column| format!("quote(\"{column}\")"))
            .collect::<Vec<_>>()
            .join(", ");
        let rows: Vec<Value> = db
            .prepare(&format!("SELECT {quoted} FROM \"{name}\" ORDER BY rowid"))
            .expect("a table's rows")
            .query_map([], |row| {
                (0..columns.len())
                    .map(|at| row.get::<_, String>(at))
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .expect("a table's rows")
            .map(|row| json!(row.expect("a row")))
            .collect();
        tables.insert(name, json!({ "columns": columns, "rows": rows }));
    }
    let version: i64 = db
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("the version");
    json!({ "userVersion": version, "schema": schema, "tables": tables })
}

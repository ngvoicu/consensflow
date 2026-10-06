//! What a player made, held to what Node recorded: texts as bytes (key order,
//! spacing and absent against null count), and the database a ledger left.

use std::path::Path;

use rusqlite::{Connection, OpenFlags};
use serde_json::{json, Value};

/// Where two texts first differ, and what is near it: why they are not the
/// same bytes. None when they are.
pub fn differs(what: &str, ours: impl AsRef<[u8]>, theirs: impl AsRef<[u8]>) -> Option<String> {
    let (ours, theirs) = (ours.as_ref(), theirs.as_ref());
    if ours == theirs {
        return None;
    }
    let at = ours.iter().zip(theirs).take_while(|(a, b)| a == b).count();
    let from = at.saturating_sub(80);
    let near =
        |text: &[u8]| String::from_utf8_lossy(&text[from..(at + 160).min(text.len())]).into_owned();
    Some(format!(
        "{what} differs at byte {at}:\n    here: …{}…\n    node: …{}…",
        near(ours),
        near(theirs)
    ))
}

/// `actual` against what Node answered: exactly, key order and all; for an
/// error SQLite raised, its message.
pub fn compare(what: &str, actual: &Value, expected: &Value) -> Option<String> {
    let (actual, expected) = match (actual.get("$error"), expected.get("$error")) {
        (Some(ours), Some(theirs)) if theirs["name"] != "LedgerError" => (
            json!({ "error": ours["message"] }),
            json!({ "error": theirs["message"] }),
        ),
        _ => (actual.clone(), expected.clone()),
    };
    differs(what, actual.to_string(), expected.to_string())
}

/// The database of the closed ledger at `file` against the one Node left
/// (`ledger.final` of the trace).
pub fn left(file: &Path, expected: &Value) -> Option<String> {
    compare("the database it left", &dump(file), expected)
}

/// The database a ledger left, as the recorder dumps it (`tests/goldens/ledger`,
/// and step 3.1's replay): its version, its schema, and each table's rows in
/// rowid order, each value as SQLite quotes it.
fn dump(file: &Path) -> Value {
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

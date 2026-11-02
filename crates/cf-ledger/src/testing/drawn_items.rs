//! What migration 0028 added, which Node's ledger never had (chief-harness, CHF-16): the
//! columns of the `transcript` table that hold what the history draws of an item and
//! whether only the history shows it, and the text SQLite wrote for them into the table's
//! schema; the table of an edit's change. (Its columns of `transcript_head` are of a table
//! migration 0020 made, which the players take out whole.) The players of Node's
//! recordings take them out of what this ledger left
//! ([`super::hold_apart_what_node_never_writes`] for the rows and
//! [`super::hold_apart_what_node_never_had_in_its_schema`] for the schema), so that what the
//! two wrote in everything else is compared.

/// The columns migration 0028 added to `transcript`, which Node's copies never had.
pub(super) const COLUMNS: [(&str, &str); 2] =
    [("transcript", "detail"), ("transcript", "drawn_only")];

/// What SQLite wrote into the text of the `transcript` table for those columns, with the
/// text migration 0028 added them with.
pub(super) const COLUMN_LINES: &str = ", detail TEXT, drawn_only INTEGER NOT NULL DEFAULT 0";

/// The table migration 0028 made.
pub(super) const TABLE: &str = "transcript_diff";

/// The columns migration 0028 added to `transcript_head`, in the order it added them.
const HEAD_COLUMNS: [&str; 7] = [
    "kind",
    "name",
    "header",
    "status",
    "took",
    "tokens",
    "diff_bytes",
];

/// What migration 0028 made taken out of the ledger `db` holds: its table, and its columns
/// of `transcript_head` and `transcript`, so that a test can take a ledger this build wrote
/// for one an older build left, whose version it sets, and which migrates to this build's
/// again.
pub fn undo_migration_0028(db: &rusqlite::Connection) -> rusqlite::Result<()> {
    db.execute_batch(&format!("DROP TABLE {TABLE}"))?;
    for column in HEAD_COLUMNS.iter().rev() {
        db.execute_batch(&format!("ALTER TABLE transcript_head DROP COLUMN {column}"))?;
    }
    for (table, column) in COLUMNS.iter().rev() {
        db.execute_batch(&format!("ALTER TABLE {table} DROP COLUMN {column}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::super::{
        hold_apart_what_node_never_had_in_its_schema, hold_apart_what_node_never_writes,
    };

    #[test]
    fn takes_the_drawn_columns_and_the_diff_table_out_of_a_dump_and_nothing_else() {
        let mut tables = json!({
            "transcript": {
                "columns": ["item_id", "text", "compaction", "detail", "drawn_only"],
                "rows": [["'u1'", "'hi'", "0", "NULL", "0"], ["'t1'", "'out'", "0", "'{\"kind\":\"tool\"}'", "1"]],
            },
            "transcript_diff": { "columns": ["item", "text"], "rows": [["2", "'-a'"]] },
            "task": { "columns": ["id", "detail"], "rows": [["1", "'kept'"]] },
        });
        hold_apart_what_node_never_writes(tables.as_object_mut().unwrap());
        assert_eq!(
            tables,
            json!({
                "transcript": {
                    "columns": ["item_id", "text"],
                    "rows": [["'u1'", "'hi'"], ["'t1'", "'out'"]],
                },
                "task": { "columns": ["id", "detail"], "rows": [["1", "'kept'"]] },
            })
        );
    }

    #[test]
    fn takes_the_drawn_columns_out_of_the_transcript_tables_text() {
        let db = rusqlite::Connection::open_in_memory().unwrap();
        crate::schema::migrate(&db, &crate::schema::MIGRATIONS).unwrap();
        let sql: String = db
            .query_row(
                "SELECT sql FROM sqlite_master WHERE name = 'transcript'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let mut dump = json!({ "userVersion": 11, "schema": [
            { "name": "transcript", "sql": sql },
            { "name": "transcript_diff", "sql": "CREATE TABLE transcript_diff (item INTEGER)" },
        ] });
        hold_apart_what_node_never_had_in_its_schema(&mut dump);
        let held = dump["schema"][0]["sql"].as_str().unwrap();
        assert!(
            !held.contains("detail") && !held.contains("drawn_only"),
            "{held}"
        );
        assert!(held.contains("copied_at TEXT NOT NULL"), "{held}");
        assert_eq!(dump["schema"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn undoing_migration_0028_leaves_the_schema_0027_left_and_the_ledger_migrates_again() {
        use crate::schema::{migrate, MIGRATIONS};
        let schema = |db: &rusqlite::Connection| -> Vec<(String, String)> {
            db.prepare("SELECT name, sql FROM sqlite_master WHERE sql IS NOT NULL ORDER BY name")
                .unwrap()
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                .unwrap()
                .map(Result::unwrap)
                .collect()
        };
        let before = rusqlite::Connection::open_in_memory().unwrap();
        migrate(&before, &MIGRATIONS[..27]).unwrap();
        let db = rusqlite::Connection::open_in_memory().unwrap();
        migrate(&db, &MIGRATIONS[..28]).unwrap();
        let migrated = schema(&db);
        super::undo_migration_0028(&db).unwrap();
        assert_eq!(schema(&db), schema(&before));
        db.execute_batch("PRAGMA user_version = 27").unwrap();
        migrate(&db, &MIGRATIONS[..28]).unwrap();
        assert_eq!(schema(&db), migrated);
    }
}

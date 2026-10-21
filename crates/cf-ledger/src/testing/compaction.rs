//! What migration 0020 added, which Node's ledger never had: the column of
//! the `transcript` table that marks a compaction's summary, and the text
//! SQLite wrote for it into the table's schema; and the tables that hold what
//! the history viewer reads of each item and keep a long item in parts, and the
//! indexes it reads through, each a table or an index of its own. The players of
//! Node's recordings take them out of what this ledger left
//! ([`super::hold_apart_what_node_never_writes`] for the rows and
//! [`super::hold_apart_what_node_never_had_in_its_schema`] for the schema), so
//! that what the two wrote in everything else is compared.

/// The column migration 0020 added to `transcript`, which Node's copies never
/// had: whether an item is the summary a compaction wrote.
pub(super) const COLUMNS: [(&str, &str); 1] = [("transcript", "compaction")];

/// What SQLite wrote into the text of the `transcript` table for that column,
/// with the text migration 0020 added it with.
pub(super) const COLUMN_LINES: &str = ", compaction INTEGER NOT NULL DEFAULT 0 CONSTRAINT transcript_compaction_check CHECK (compaction IN (0, 1))";

/// The tables (`"TABLE"`) or indexes (`"INDEX"`) migration 0020 made, by the
/// names it gave them, as its lines say them (`CREATE INDEX name ON …`).
pub(super) fn made(kind: &str) -> impl Iterator<Item = &'static str> + '_ {
    include_str!("../../migrations/0020.sql")
        .lines()
        .filter_map(move |line| {
            line.trim()
                .strip_prefix("CREATE ")?
                .strip_prefix(kind)?
                .split_whitespace()
                .next()
        })
}

/// What migration 0020 made taken out of the ledger `db` holds: its indexes, its
/// tables and its column of `transcript`, so that a test can take a ledger this
/// build wrote for one an older build left, whose version it sets, and which
/// migrates to this build's again (what 0020 wrote in the rows it kept stays).
pub fn undo_migration_0020(db: &rusqlite::Connection) -> rusqlite::Result<()> {
    for index in made("INDEX") {
        db.execute_batch(&format!("DROP INDEX {index}"))?;
    }
    let tables: Vec<&str> = made("TABLE").collect();
    // The parts' table refers to the heads'.
    for table in tables.into_iter().rev() {
        db.execute_batch(&format!("DROP TABLE {table}"))?;
    }
    for (table, column) in COLUMNS {
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
    fn undoing_migration_0020_leaves_the_schema_0019_left_and_the_ledger_migrates_again() {
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
        migrate(&before, &MIGRATIONS[..19]).unwrap();
        let db = rusqlite::Connection::open_in_memory().unwrap();
        migrate(&db, &MIGRATIONS[..20]).unwrap();
        let migrated = schema(&db);
        super::undo_migration_0020(&db).unwrap();
        assert_eq!(schema(&db), schema(&before));
        db.execute_batch("PRAGMA user_version = 19").unwrap();
        migrate(&db, &MIGRATIONS[..20]).unwrap();
        assert_eq!(schema(&db), migrated);
    }

    #[test]
    fn takes_the_compaction_column_and_its_values_out_of_the_transcript_table_and_nothing_else() {
        let mut tables = json!({
            "transcript": {
                "columns": ["item_id", "role", "compaction"],
                "rows": [["'u1'", "'user'", "0"], ["'s1'", "'user'", "1"]],
            },
            "message": { "columns": ["id", "compaction"], "rows": [["1", "'kept'"]] },
        });
        hold_apart_what_node_never_writes(tables.as_object_mut().unwrap());
        assert_eq!(
            tables,
            json!({
                "transcript": {
                    "columns": ["item_id", "role"],
                    "rows": [["'u1'", "'user'"], ["'s1'", "'user'"]],
                },
                "message": { "columns": ["id", "compaction"], "rows": [["1", "'kept'"]] },
            })
        );
    }

    #[test]
    fn takes_the_column_out_of_the_text_sqlite_wrote_for_the_transcript_table_and_of_no_other() {
        use crate::schema::{migrate, MIGRATIONS};
        let table_text = |migrations: &[&str]| {
            let db = rusqlite::Connection::open_in_memory().unwrap();
            migrate(&db, migrations).unwrap();
            db.query_row(
                "SELECT sql FROM sqlite_master WHERE name = 'transcript'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap()
        };
        // The table as migration 0019 left it, and as 0020 left it.
        let before = table_text(&MIGRATIONS[..19]);
        let after = table_text(&MIGRATIONS[..20]);
        assert_ne!(before, after, "the migration changed the table's text");
        let mut dump = json!({
            "userVersion": 20,
            "schema": [
                { "name": "transcript", "sql": after },
                { "name": "message", "sql": after },
            ],
            "tables": {},
        });
        hold_apart_what_node_never_had_in_its_schema(&mut dump);
        assert_eq!(dump["schema"][0]["sql"], before);
        assert_eq!(
            dump["schema"][1]["sql"], after,
            "another table's text is its own"
        );
        let held = dump.clone();
        hold_apart_what_node_never_had_in_its_schema(&mut dump);
        assert_eq!(dump, held, "a schema of Node's is left as it is");
    }

    #[test]
    fn takes_the_tables_and_the_indexes_migration_0020_made_out_of_a_dump_and_nothing_else() {
        use crate::schema::{migrate, MIGRATIONS};
        let names = |migrations: &[&str]| -> Vec<String> {
            let db = rusqlite::Connection::open_in_memory().unwrap();
            migrate(&db, migrations).unwrap();
            let names = db
                .prepare("SELECT name FROM sqlite_master WHERE type IN ('table', 'index') AND sql IS NOT NULL ORDER BY name")
                .unwrap()
                .query_map([], |row| row.get(0))
                .unwrap()
                .map(Result::unwrap)
                .collect();
            names
        };
        let before = names(&MIGRATIONS[..19]);
        let after = names(&MIGRATIONS[..20]);
        assert_eq!(after.len(), before.len() + 11, "{after:?}");
        let schema: Vec<_> = after
            .iter()
            .map(|name| json!({ "name": name, "sql": "CREATE …" }))
            .collect();
        let mut dump = json!({ "userVersion": 20, "schema": schema, "tables": {} });
        hold_apart_what_node_never_had_in_its_schema(&mut dump);
        let left: Vec<&str> = dump["schema"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["name"].as_str().unwrap())
            .collect();
        assert_eq!(left, before);
        let mut tables = json!({
            "transcript_part": { "columns": ["text"], "rows": [["'a part'"]] },
            "transcript_head": { "columns": ["id_shown"], "rows": [["'u1'"]] },
            "transcript": { "columns": ["text"], "rows": [["'kept'"]] },
        });
        hold_apart_what_node_never_writes(tables.as_object_mut().unwrap());
        assert_eq!(
            tables,
            json!({ "transcript": { "columns": ["text"], "rows": [["'kept'"]] } })
        );
    }
}

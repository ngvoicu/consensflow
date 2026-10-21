//! What migration 0022 added to the `message` table, which Node's messages never
//! had (when the transport let a message's hand-over go to its window): its
//! column, and the text SQLite wrote for it into the table's schema. The players of
//! Node's recordings take them out of what this ledger left
//! ([`super::hold_apart_what_node_never_writes`] for the rows and
//! [`super::hold_apart_what_node_never_had_in_its_schema`] for the text of the
//! table), so that what the two wrote in everything else is compared.

/// The column migration 0022 added to `message`, which Node's messages never
/// had: when the transport let its hand-over go.
pub(super) const COLUMNS: [(&str, &str); 1] = [("message", "handed_at")];

/// What SQLite wrote into the text of the `message` table for that column, as
/// migration 0022 added it: after the last column and before the table's own
/// checks, with the text it was added with.
pub(super) const COLUMN_LINES: &str = ", handed_at TEXT";

/// What migration 0022 made taken out of the ledger `db` holds, its column of
/// `message`, so that a test can take a ledger this build wrote for one an older
/// build left, whose version it sets, and which migrates to this build's again.
pub fn undo_migration_0022(db: &rusqlite::Connection) -> rusqlite::Result<()> {
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
    fn takes_the_handed_column_and_its_values_out_of_the_message_table_and_nothing_else() {
        let mut tables = json!({
            "message": {
                "columns": ["id", "body", "handed_at"],
                "rows": [["1", "'hello'", "NULL"], ["2", "'again'", "'2026-10-17T10:00:00.000Z'"]],
            },
            "task": { "columns": ["id", "handed_at"], "rows": [["1", "'kept'"]] },
        });
        hold_apart_what_node_never_writes(tables.as_object_mut().unwrap());
        assert_eq!(
            tables,
            json!({
                "message": {
                    "columns": ["id", "body"],
                    "rows": [["1", "'hello'"], ["2", "'again'"]],
                },
                "task": { "columns": ["id", "handed_at"], "rows": [["1", "'kept'"]] },
            })
        );
    }

    #[test]
    fn undoing_migration_0022_leaves_the_schema_0021_left_and_the_ledger_migrates_again() {
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
        migrate(&before, &MIGRATIONS[..21]).unwrap();
        let db = rusqlite::Connection::open_in_memory().unwrap();
        migrate(&db, &MIGRATIONS[..22]).unwrap();
        let migrated = schema(&db);
        super::undo_migration_0022(&db).unwrap();
        assert_eq!(schema(&db), schema(&before));
        db.execute_batch("PRAGMA user_version = 21").unwrap();
        migrate(&db, &MIGRATIONS[..22]).unwrap();
        assert_eq!(schema(&db), migrated);
    }

    #[test]
    fn takes_the_column_out_of_the_text_sqlite_wrote_for_the_message_table_and_of_no_other() {
        use crate::schema::{migrate, MIGRATIONS};
        let table_text = |migrations: &[&str]| {
            let db = rusqlite::Connection::open_in_memory().unwrap();
            migrate(&db, migrations).unwrap();
            db.query_row(
                "SELECT sql FROM sqlite_master WHERE name = 'message'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap()
        };
        // The table as migration 17 left it, and as 0022 left it: the players take
        // 0018's column out too.
        let before = table_text(&MIGRATIONS[..17]);
        let after = table_text(&MIGRATIONS[..22]);
        assert_ne!(
            table_text(&MIGRATIONS[..21]),
            after,
            "the migration changed the table's text"
        );
        let mut dump = json!({
            "userVersion": 22,
            "schema": [
                { "name": "message", "sql": after },
                { "name": "participant", "sql": after },
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
}

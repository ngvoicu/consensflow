//! What migration 0021 added to the `task` table, which Node's tasks never had
//! (the mark that a project's suspension paused a task, for its Resume to resume):
//! its column, and the text SQLite wrote for it into the table's schema. The
//! players of Node's recordings take them out of what this ledger left
//! ([`super::hold_apart_what_node_never_writes`] for the rows and
//! [`super::hold_apart_what_node_never_had_in_its_schema`] for the text of the
//! table), so that what the two wrote in everything else is compared.

/// The column migration 0021 added to `task`, which Node's tasks never had.
pub(super) const COLUMNS: [(&str, &str); 1] = [("task", "paused_with_project")];

/// What SQLite wrote into the text of the `task` table for that column, as
/// migration 0021 added it: after the last column and before the table's own
/// checks, with the text it was added with.
pub(super) const COLUMN_LINES: &str = ", paused_with_project INTEGER NOT NULL DEFAULT 0 CONSTRAINT task_paused_with_project_check CHECK (paused_with_project IN (0, 1))";

/// What migration 0021 made taken out of the ledger `db` holds, its column of
/// `task`, so that a test can take a ledger this build wrote for one an older
/// build left, whose version it sets, and which migrates to this build's again.
pub fn undo_migration_0021(db: &rusqlite::Connection) -> rusqlite::Result<()> {
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
    fn takes_the_mark_column_and_its_values_out_of_the_task_table_and_nothing_else() {
        let mut tables = json!({
            "task": {
                "columns": ["id", "state", "paused_with_project"],
                "rows": [["1", "'paused'", "1"], ["2", "'working'", "0"]],
            },
            "project": { "columns": ["id", "paused_with_project"], "rows": [["1", "1"]] },
        });
        hold_apart_what_node_never_writes(tables.as_object_mut().unwrap());
        assert_eq!(
            tables,
            json!({
                "task": {
                    "columns": ["id", "state"],
                    "rows": [["1", "'paused'"], ["2", "'working'"]],
                },
                "project": { "columns": ["id", "paused_with_project"], "rows": [["1", "1"]] },
            })
        );
    }

    #[test]
    fn undoing_migration_0021_leaves_the_schema_0020_left_and_the_ledger_migrates_again() {
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
        migrate(&before, &MIGRATIONS[..20]).unwrap();
        let db = rusqlite::Connection::open_in_memory().unwrap();
        migrate(&db, &MIGRATIONS[..21]).unwrap();
        let migrated = schema(&db);
        super::undo_migration_0021(&db).unwrap();
        assert_eq!(schema(&db), schema(&before));
        db.execute_batch("PRAGMA user_version = 20").unwrap();
        migrate(&db, &MIGRATIONS[..21]).unwrap();
        assert_eq!(schema(&db), migrated);
    }

    #[test]
    fn takes_the_column_out_of_the_text_sqlite_wrote_for_the_task_table_and_of_no_other() {
        use crate::schema::{migrate, MIGRATIONS};
        let table_text = |migrations: &[&str]| {
            let db = rusqlite::Connection::open_in_memory().unwrap();
            migrate(&db, migrations).unwrap();
            db.query_row(
                "SELECT sql FROM sqlite_master WHERE name = 'task'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap()
        };
        // The table as migration 20 left it, and as 0021 left it; and the tester
        // the players take out of its checks, as they do of every table of tasks.
        let before =
            table_text(&MIGRATIONS[..20]).replace("'reviewer', 'tester', ", "'reviewer', ");
        let after = table_text(&MIGRATIONS[..21]);
        assert_ne!(
            table_text(&MIGRATIONS[..20]),
            after,
            "the migration changed the table's text"
        );
        let mut dump = json!({
            "userVersion": 21,
            "schema": [
                { "name": "task", "sql": after },
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
}

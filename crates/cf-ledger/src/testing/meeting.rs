//! What migration 0025 added to the `project` table, which Node's projects never
//! had (when a meeting with the chief began, 3.0.7 chief-meeting): its one column, which
//! [`super::hold_apart_what_node_never_writes`] takes out of the rows and
//! [`super::hold_apart_what_node_never_had_in_its_schema`] out of the text SQLite
//! wrote for the table, as it does the wake-up calls' columns, so that what the
//! two ledgers wrote in everything else is compared.

/// What migration 0025 made taken out of the ledger `db` holds, its column of
/// `project`, so that a test can take a ledger this build wrote for one an older
/// build left, whose version it sets, and which migrates to this build's again.
pub fn undo_migration_0025(db: &rusqlite::Connection) -> rusqlite::Result<()> {
    db.execute_batch("ALTER TABLE project DROP COLUMN meeting_at")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::super::{
        hold_apart_what_node_never_had_in_its_schema, hold_apart_what_node_never_writes,
    };
    use crate::schema::{migrate, MIGRATIONS};

    /// Where the migration that added the meeting is in the list.
    fn at() -> usize {
        MIGRATIONS
            .iter()
            .position(|sql| sql.contains("ADD COLUMN meeting_at"))
            .expect("a migration that adds the meeting")
    }

    #[test]
    fn takes_the_meeting_and_its_value_out_of_the_project_table_and_nothing_else() {
        let mut tables = json!({
            "project": {
                "columns": ["id", "gate", "meeting_at", "wake_up_on"],
                "rows": [["1", "0", "NULL", "0"], ["2", "1", "'2026-10-29T02:00:00.000Z'", "1"]],
            },
            "task": { "columns": ["id", "meeting_at"], "rows": [["1", "'a task'"]] },
        });
        hold_apart_what_node_never_writes(tables.as_object_mut().unwrap());
        assert_eq!(
            tables,
            json!({
                "project": { "columns": ["id", "gate"], "rows": [["1", "0"], ["2", "1"]] },
                "task": { "columns": ["id", "meeting_at"], "rows": [["1", "'a task'"]] },
            })
        );
    }

    #[test]
    fn takes_the_meeting_out_of_the_text_sqlite_wrote_for_the_project_table_and_of_no_other() {
        let project_text = |migrations: &[&str]| {
            let db = rusqlite::Connection::open_in_memory().unwrap();
            migrate(&db, migrations).unwrap();
            db.query_row(
                "SELECT sql FROM sqlite_master WHERE name = 'project'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap()
        };
        // The table as the migration before it left it, and as it left it. Both
        // are held apart from every column Node's schema never had, so that
        // what is left of the two texts is the same, and the meeting is what
        // the one has that the other has not.
        let before = project_text(&MIGRATIONS[..at()]);
        let after = project_text(&MIGRATIONS[..=at()]);
        assert_ne!(before, after, "the migration changed the table's text");
        assert!(after.contains(", meeting_at TEXT"));
        let held = |text: &str, table: &str| {
            let mut dump = json!({
                "userVersion": 11,
                "schema": [{ "name": table, "sql": text }],
                "tables": {},
            });
            hold_apart_what_node_never_had_in_its_schema(&mut dump);
            dump["schema"][0]["sql"].as_str().unwrap().to_owned()
        };
        assert_eq!(held(&after, "project"), held(&before, "project"));
        assert!(!held(&after, "project").contains("meeting"));
        assert_eq!(
            held(&after, "task"),
            after,
            "another table's text is its own"
        );
    }

    #[test]
    fn undoing_migration_0025_leaves_the_schema_0024_left_and_the_ledger_migrates_again() {
        let schema = |db: &rusqlite::Connection| -> Vec<(String, String)> {
            db.prepare("SELECT name, sql FROM sqlite_master WHERE sql IS NOT NULL ORDER BY name")
                .unwrap()
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                .unwrap()
                .map(Result::unwrap)
                .collect()
        };
        let before = rusqlite::Connection::open_in_memory().unwrap();
        migrate(&before, &MIGRATIONS[..24]).unwrap();
        let db = rusqlite::Connection::open_in_memory().unwrap();
        migrate(&db, &MIGRATIONS[..25]).unwrap();
        let migrated = schema(&db);
        super::undo_migration_0025(&db).unwrap();
        assert_eq!(schema(&db), schema(&before));
        db.execute_batch("PRAGMA user_version = 24").unwrap();
        migrate(&db, &MIGRATIONS[..25]).unwrap();
        assert_eq!(schema(&db), migrated);
    }
}

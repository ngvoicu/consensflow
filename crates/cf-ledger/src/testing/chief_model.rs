//! What migration 0028 added to the `participant` table, which Node's participants never
//! had (a chief's model, the model it was switched from, the model its last launch asked
//! for, CHF-04, and the model a launch found gone, CHF-06): its columns, and the text SQLite wrote for them into the table's
//! schema. The players of Node's recordings take them out of what this ledger left
//! ([`super::hold_apart_what_node_never_writes`] for the rows and
//! [`super::hold_apart_what_node_never_had_in_its_schema`] for the text of the table), so
//! that what the two wrote in everything else is compared.

/// The columns migration 0028 added to `participant`, which Node's participants never had.
pub(super) const COLUMNS: [(&str, &str); 4] = [
    ("participant", "model"),
    ("participant", "switched_from_model"),
    ("participant", "launched_model"),
    ("participant", "gone_model"),
];

/// What SQLite wrote into the text of the `participant` table for those columns, as
/// migration 0028 added them: after the last column and before the table's own checks.
pub(super) const COLUMN_LINES: &str =
    ", model TEXT, switched_from_model TEXT, launched_model TEXT, gone_model TEXT";

/// What migration 0028 made taken out of the ledger `db` holds, its columns of
/// `participant`, so that a test can take a ledger this build wrote for one an older
/// build left, whose version it sets, and which migrates to this build's again.
pub fn undo_migration_0028(db: &rusqlite::Connection) -> rusqlite::Result<()> {
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
    fn takes_the_chiefs_model_columns_and_their_values_out_of_the_participant_table_and_nothing_else(
    ) {
        let mut tables = json!({
            "participant": {
                "columns": ["id", "handle", "model", "switched_from_model", "launched_model", "gone_model"],
                "rows": [["1", "'chief'", "NULL", "NULL", "NULL", "NULL"], ["2", "'chief'", "'gpt-6-luna'", "'swe-2'", "'gpt-6-luna'", "'gpt-gone-9'"]],
            },
            "task": { "columns": ["id", "model"], "rows": [["1", "'kept'"]] },
        });
        hold_apart_what_node_never_writes(tables.as_object_mut().unwrap());
        assert_eq!(
            tables,
            json!({
                "participant": {
                    "columns": ["id", "handle"],
                    "rows": [["1", "'chief'"], ["2", "'chief'"]],
                },
                "task": { "columns": ["id", "model"], "rows": [["1", "'kept'"]] },
            })
        );
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

    #[test]
    fn takes_the_chiefs_model_columns_out_of_the_text_sqlite_wrote_for_the_participant_table_and_of_no_other(
    ) {
        use crate::schema::{migrate, MIGRATIONS};
        let table_text = |migrations: &[&str]| {
            let db = rusqlite::Connection::open_in_memory().unwrap();
            migrate(&db, migrations).unwrap();
            db.query_row(
                "SELECT sql FROM sqlite_master WHERE name = 'participant'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap()
        };
        let before = table_text(&MIGRATIONS[..27]);
        let after = table_text(&MIGRATIONS[..28]);
        assert_ne!(before, after, "the migration changed the table's text");
        let held = |text: &str, table: &str| {
            let mut dump = json!({
                "userVersion": 26,
                "schema": [{ "name": table, "sql": text }],
                "tables": {},
            });
            hold_apart_what_node_never_had_in_its_schema(&mut dump);
            dump["schema"][0]["sql"].as_str().unwrap().to_owned()
        };
        assert_eq!(held(&after, "participant"), held(&before, "participant"));
        assert!(!held(&after, "participant").contains("launched_model"));
        assert_eq!(
            held(&after, "event"),
            after,
            "another table's text is its own"
        );
    }
}

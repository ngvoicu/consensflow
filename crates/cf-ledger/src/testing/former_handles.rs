//! What migration 0033 added, which Node's ledger never had (spec `harness-models`, HMD-12):
//! the column of `participant` that keeps the names a participant left, and the text SQLite
//! wrote for it into the table's schema. The players of Node's recordings take it out of
//! what this ledger left ([`super::hold_apart_what_node_never_writes`] for the rows and
//! [`super::hold_apart_what_node_never_had_in_its_schema`] for the schema), so that what the
//! two wrote in everything else is compared.

/// The column migration 0033 added to `participant`, which Node's copies never had.
pub(super) const COLUMNS: [(&str, &str); 1] = [("participant", "former_handles")];

/// What SQLite wrote into the text of the `participant` table for that column, with the text
/// migration 0033 added it with.
pub(super) const COLUMN_LINES: &str = ", former_handles TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(former_handles) AND json_type(former_handles) = 'array')";

/// For tests alone, and of the table's shape alone: what migration 0033 made taken out of
/// the ledger `db` holds, so that a test can take a ledger this build wrote for one an older
/// build left, whose version it sets, and which migrates to this build's again. No build
/// undoes 0033, and this keeps no data: the names a participant left go with the column, the
/// handles 0033 made stay.
pub fn undo_migration_0033(db: &rusqlite::Connection) -> rusqlite::Result<()> {
    db.execute_batch("ALTER TABLE participant DROP COLUMN former_handles")
}

#[cfg(test)]
mod tests {
    use crate::schema::{migrate, MIGRATIONS};

    fn participant_sql(db: &rusqlite::Connection) -> String {
        db.query_row(
            "SELECT sql FROM sqlite_master WHERE name = 'participant'",
            [],
            |row| row.get(0),
        )
        .unwrap()
    }

    #[test]
    fn undoing_migration_0033_leaves_the_table_0032_left_and_the_ledger_migrates_again() {
        let before = rusqlite::Connection::open_in_memory().unwrap();
        migrate(&before, &MIGRATIONS[..32]).unwrap();
        let db = rusqlite::Connection::open_in_memory().unwrap();
        migrate(&db, &MIGRATIONS[..33]).unwrap();
        let after = participant_sql(&db);
        assert!(after.contains(super::COLUMN_LINES), "{after}");
        super::undo_migration_0033(&db).unwrap();
        assert_eq!(participant_sql(&db), participant_sql(&before));
        db.execute_batch("PRAGMA user_version = 32").unwrap();
        migrate(&db, &MIGRATIONS[..33]).unwrap();
        assert_eq!(participant_sql(&db), after);
    }

    #[test]
    fn takes_the_column_out_of_the_text_sqlite_wrote_for_the_participant_table_and_of_no_other() {
        let db = rusqlite::Connection::open_in_memory().unwrap();
        migrate(&db, &MIGRATIONS).unwrap();
        let mut dump = serde_json::json!({ "userVersion": 11, "schema": [
            { "name": "participant", "sql": participant_sql(&db) },
        ] });
        super::super::hold_apart_what_node_never_had_in_its_schema(&mut dump);
        let held = dump["schema"][0]["sql"].as_str().unwrap();
        assert!(!held.contains("former_handles"), "{held}");
        assert!(held.contains("handle TEXT NOT NULL"), "{held}");
    }
}

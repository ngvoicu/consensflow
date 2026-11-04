//! What migration 0030 added to the `participant` table (whether a member's tier is the one
//! the human chose, T-1143), which 0031 took out again: its column, and the text SQLite wrote
//! for it into the table's schema.

/// The column migration 0030 added to `participant`.
const COLUMNS: [(&str, &str); 1] = [("participant", "tier_chosen")];

/// What SQLite wrote into the text of the `participant` table for it, as migration 0030 added
/// it: after the last column, before the table's own checks.
#[cfg(test)]
const COLUMN_LINES: &str = ", tier_chosen INTEGER NOT NULL DEFAULT 0 CHECK (tier_chosen IN (0, 1))";

/// What migration 0030 made taken out of the ledger `db` holds, so that a test can take a
/// ledger this build wrote for one an older build left, whose version it sets, and which
/// migrates to this build's again.
pub fn undo_migration_0030(db: &rusqlite::Connection) -> rusqlite::Result<()> {
    for (table, column) in COLUMNS {
        db.execute_batch(&format!("ALTER TABLE {table} DROP COLUMN {column}"))?;
    }
    Ok(())
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
    fn undoing_migration_0030_leaves_the_table_0029_left_and_the_ledger_migrates_again() {
        let before = rusqlite::Connection::open_in_memory().unwrap();
        migrate(&before, &MIGRATIONS[..29]).unwrap();
        let db = rusqlite::Connection::open_in_memory().unwrap();
        migrate(&db, &MIGRATIONS[..30]).unwrap();
        let after = participant_sql(&db);
        assert!(after.contains(super::COLUMN_LINES), "{after}");
        super::undo_migration_0030(&db).unwrap();
        assert_eq!(participant_sql(&db), participant_sql(&before));
        db.execute_batch("PRAGMA user_version = 29").unwrap();
        migrate(&db, &MIGRATIONS[..30]).unwrap();
        assert_eq!(participant_sql(&db), after);
    }
}

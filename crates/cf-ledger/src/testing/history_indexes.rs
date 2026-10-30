//! What migration 0027 took out of the ledger: the indexes migration 0020 made for the
//! history's headings (where the log says a conversation began, was gone to or ended, and
//! the chief's switches) and for counting a conversation's tools' outputs apart, which
//! nothing reads since the history is one timeline of items (chief-harness, CHF-12). Node's
//! ledger never had them, so the players of its recordings have nothing to hold apart.

/// The indexes migration 0027 dropped, by the names its lines give them (`DROP INDEX name;`).
fn dropped() -> impl Iterator<Item = &'static str> {
    include_str!("../../migrations/0027.sql")
        .lines()
        .filter_map(|line| line.trim().strip_prefix("DROP INDEX ")?.strip_suffix(';'))
}

/// What migration 0027 took out put back into the ledger `db` holds, each index as
/// migration 0020's line made it, so that a test can take a ledger this build wrote for
/// one an older build left, whose version it sets, and which migrates to this build's
/// again.
pub fn undo_migration_0027(db: &rusqlite::Connection) -> rusqlite::Result<()> {
    for index in dropped() {
        let made = include_str!("../../migrations/0020.sql")
            .lines()
            .map(str::trim)
            .find(|line| line.starts_with(&format!("CREATE INDEX {index} ")))
            .ok_or_else(|| rusqlite::Error::InvalidParameterName(index.to_owned()))?;
        db.execute_batch(made)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn undoing_migration_0027_leaves_the_schema_0026_left_and_the_ledger_migrates_again() {
        use crate::schema::{migrate, MIGRATIONS};
        let schema = |db: &rusqlite::Connection| -> Vec<(String, String)> {
            db.prepare("SELECT name, sql FROM sqlite_master WHERE sql IS NOT NULL ORDER BY name")
                .unwrap()
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                .unwrap()
                .map(Result::unwrap)
                .collect()
        };
        assert_eq!(super::dropped().count(), 5);
        let before = rusqlite::Connection::open_in_memory().unwrap();
        migrate(&before, &MIGRATIONS[..26]).unwrap();
        let db = rusqlite::Connection::open_in_memory().unwrap();
        migrate(&db, &MIGRATIONS[..27]).unwrap();
        let migrated = schema(&db);
        assert_ne!(
            migrated,
            schema(&before),
            "the migration changed the schema"
        );
        super::undo_migration_0027(&db).unwrap();
        assert_eq!(schema(&db), schema(&before));
        db.execute_batch("PRAGMA user_version = 26").unwrap();
        migrate(&db, &MIGRATIONS[..27]).unwrap();
        assert_eq!(schema(&db), migrated);
    }
}

//! Make a current test ledger look as it did before migration 0038.

pub fn undo_migration_0038(db: &rusqlite::Connection) -> rusqlite::Result<()> {
    db.execute_batch("ALTER TABLE project DROP COLUMN own_hands")
}

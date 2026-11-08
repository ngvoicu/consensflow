//! What migration 0034 changed of the `project` table (smaller-contexts, Phase 7): 0026's
//! `claude_compaction`, renamed `compaction` now that every member is given it whatever its
//! harness. Node's projects never had the column under either name: the players hold it
//! apart by its new name too ([`COLUMN`], read by `added_columns`).

/// The column as migration 0034 left it, as SQLite writes it into the table's text after its
/// rename: what Node's schema never had.
pub(super) const COLUMN: &str = "compaction INTEGER";

/// What migration 0034 made undone in the ledger `db` holds: the column under 0026's name
/// again, its values kept, so that a test can take a ledger this build wrote for one an older
/// build left, whose version it sets, and which migrates to this build's again.
pub fn undo_migration_0034(db: &rusqlite::Connection) -> rusqlite::Result<()> {
    db.execute_batch("ALTER TABLE project RENAME COLUMN compaction TO claude_compaction")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::super::hold_apart_what_node_never_had_in_its_schema;
    use crate::schema::{migrate, MIGRATIONS};

    /// Where migration 0034 is in the list.
    fn at() -> usize {
        MIGRATIONS
            .iter()
            .position(|sql| sql.contains("RENAME COLUMN claude_compaction TO compaction"))
            .expect("the migration that renames the compaction column")
    }

    fn schema(db: &rusqlite::Connection) -> Vec<(String, String)> {
        db.prepare("SELECT name, sql FROM sqlite_master WHERE sql IS NOT NULL ORDER BY name")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }

    #[test]
    fn undoing_migration_0034_leaves_the_schema_0033_left_and_the_ledger_migrates_again() {
        assert_eq!(at(), 33, "migration 0034");
        let before = rusqlite::Connection::open_in_memory().unwrap();
        migrate(&before, &MIGRATIONS[..at()]).unwrap();
        let db = rusqlite::Connection::open_in_memory().unwrap();
        migrate(&db, &MIGRATIONS[..=at()]).unwrap();
        let migrated = schema(&db);
        super::undo_migration_0034(&db).unwrap();
        assert_eq!(schema(&db), schema(&before));
        db.execute_batch("PRAGMA user_version = 33").unwrap();
        migrate(&db, &MIGRATIONS[..=at()]).unwrap();
        assert_eq!(schema(&db), migrated);
    }

    #[test]
    fn a_size_a_project_had_for_its_claude_members_is_its_members_size_after_the_migration() {
        let db = rusqlite::Connection::open_in_memory().unwrap();
        migrate(&db, &MIGRATIONS[..at()]).unwrap();
        let text = |db: &rusqlite::Connection| -> String {
            db.query_row(
                "SELECT sql FROM sqlite_master WHERE name = 'project'",
                [],
                |row| row.get(0),
            )
            .unwrap()
        };
        assert!(text(&db).contains(", claude_compaction INTEGER"));
        let columns = |db: &rusqlite::Connection| -> Vec<String> {
            db.prepare("SELECT name FROM pragma_table_info('project') ORDER BY cid")
                .unwrap()
                .query_map([], |row| row.get(0))
                .unwrap()
                .map(Result::unwrap)
                .collect()
        };
        let before = columns(&db);
        migrate(&db, &MIGRATIONS[..=at()]).unwrap();
        let after = columns(&db);
        // The same columns in the same places, the one renamed.
        assert_eq!(
            after,
            before
                .iter()
                .map(|name| if name == "claude_compaction" {
                    "compaction".to_owned()
                } else {
                    name.clone()
                })
                .collect::<Vec<_>>()
        );
        assert!(text(&db).contains(&format!(", {}", super::COLUMN)));
        assert!(!text(&db).contains("claude_compaction"));
    }

    #[test]
    fn the_players_hold_the_renamed_column_apart_from_what_node_s_schema_had() {
        let project_text = |migrations: &[&str]| -> String {
            let db = rusqlite::Connection::open_in_memory().unwrap();
            migrate(&db, migrations).unwrap();
            db.query_row(
                "SELECT sql FROM sqlite_master WHERE name = 'project'",
                [],
                |row| row.get(0),
            )
            .unwrap()
        };
        let held = |text: &str| {
            let mut dump = json!({
                "userVersion": 11,
                "schema": [{ "name": "project", "sql": text }],
                "tables": {},
            });
            hold_apart_what_node_never_had_in_its_schema(&mut dump);
            dump["schema"][0]["sql"].as_str().unwrap().to_owned()
        };
        // Held apart, the table's text before the rename and after it are the same: neither
        // holds the column, under either name.
        let before = held(&project_text(&MIGRATIONS[..at()]));
        let after = held(&project_text(&MIGRATIONS[..=at()]));
        assert_eq!(after, before);
        assert!(!after.contains("compaction"), "{after}");
    }
}

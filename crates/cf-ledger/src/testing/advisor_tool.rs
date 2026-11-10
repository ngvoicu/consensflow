//! What migration 0035 added to the `project` table (alpha-5 A5-20): whether the human has
//! the members run with Claude Code's own advisor tool, which Node's projects never had. The
//! players hold the column apart through `added_columns`, which reads its line in the
//! migration.

/// What migration 0035 made undone in the ledger `db` holds: the column gone, so that a test
/// can take a ledger this build wrote for one an older build left, whose version it sets, and
/// which migrates to this build's again.
pub fn undo_migration_0035(db: &rusqlite::Connection) -> rusqlite::Result<()> {
    db.execute_batch("ALTER TABLE project DROP COLUMN advisor_tool")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::super::{
        hold_apart_what_node_never_had_in_its_schema, hold_apart_what_node_never_writes,
    };
    use crate::schema::{migrate, MIGRATIONS};

    /// Where migration 0035 is in the list.
    fn at() -> usize {
        MIGRATIONS
            .iter()
            .position(|sql| sql.contains("ADD COLUMN advisor_tool"))
            .expect("the migration that adds the advisor tool")
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
    fn undoing_migration_0035_leaves_the_schema_0034_left_and_the_ledger_migrates_again() {
        assert_eq!(at(), 34, "migration 0035");
        let before = rusqlite::Connection::open_in_memory().unwrap();
        migrate(&before, &MIGRATIONS[..at()]).unwrap();
        let db = rusqlite::Connection::open_in_memory().unwrap();
        migrate(&db, &MIGRATIONS[..=at()]).unwrap();
        let migrated = schema(&db);
        super::undo_migration_0035(&db).unwrap();
        assert_eq!(schema(&db), schema(&before));
        db.execute_batch("PRAGMA user_version = 34").unwrap();
        migrate(&db, &MIGRATIONS[..=at()]).unwrap();
        assert_eq!(schema(&db), migrated);
    }

    #[test]
    fn the_players_hold_the_column_and_its_value_apart_from_what_node_s_ledger_wrote() {
        let db = rusqlite::Connection::open_in_memory().unwrap();
        migrate(&db, &MIGRATIONS[..=at()]).unwrap();
        let text: String = db
            .query_row(
                "SELECT sql FROM sqlite_master WHERE name = 'project'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(
            text.contains(", advisor_tool INTEGER NOT NULL DEFAULT 0"),
            "{text}"
        );
        let mut dump = json!({
            "userVersion": 11,
            "schema": [{ "name": "project", "sql": text }],
            "tables": {},
        });
        hold_apart_what_node_never_had_in_its_schema(&mut dump);
        assert!(!dump["schema"][0]["sql"]
            .as_str()
            .unwrap()
            .contains("advisor_tool"));
        let mut tables = json!({
            "project": { "columns": ["id", "advisor_tool"], "rows": [[1, 0], [2, 1]] },
        });
        hold_apart_what_node_never_writes(tables.as_object_mut().unwrap());
        assert_eq!(
            tables["project"],
            json!({ "columns": ["id"], "rows": [[1], [2]] })
        );
    }
}

//! What migration 0036 added to the `project` table (T-1444): whether the human leaves
//! Claude Code's prompt suggestion on in the project's windows, which Node's projects never had. The
//! players hold the column apart through `added_columns`, which reads its line in the
//! migration.

/// What migration 0036 made undone in the ledger `db` holds: the column gone, so that a test
/// can take a ledger this build wrote for one an older build left, whose version it sets, and
/// which migrates to this build's again.
pub fn undo_migration_0036(db: &rusqlite::Connection) -> rusqlite::Result<()> {
    db.execute_batch("ALTER TABLE project DROP COLUMN prompt_suggestion")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::super::{
        hold_apart_what_node_never_had_in_its_schema, hold_apart_what_node_never_writes,
    };
    use crate::schema::{migrate, MIGRATIONS};

    /// Where migration 0036 is in the list.
    fn at() -> usize {
        MIGRATIONS
            .iter()
            .position(|sql| sql.contains("ADD COLUMN prompt_suggestion"))
            .expect("the migration that adds the prompt suggestion")
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
    fn undoing_migration_0036_leaves_the_schema_0035_left_and_the_ledger_migrates_again() {
        assert_eq!(at(), 35, "migration 0036");
        let before = rusqlite::Connection::open_in_memory().unwrap();
        migrate(&before, &MIGRATIONS[..at()]).unwrap();
        let db = rusqlite::Connection::open_in_memory().unwrap();
        migrate(&db, &MIGRATIONS[..=at()]).unwrap();
        let migrated = schema(&db);
        super::undo_migration_0036(&db).unwrap();
        assert_eq!(schema(&db), schema(&before));
        db.execute_batch("PRAGMA user_version = 35").unwrap();
        migrate(&db, &MIGRATIONS[..=at()]).unwrap();
        assert_eq!(schema(&db), migrated);
    }

    #[test]
    fn the_players_hold_the_suggestion_column_and_its_value_apart_from_what_node_s_ledger_wrote() {
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
            text.contains(", prompt_suggestion INTEGER NOT NULL DEFAULT 0"),
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
            .contains("prompt_suggestion"));
        let mut tables = json!({
            "project": { "columns": ["id", "prompt_suggestion"], "rows": [[1, 0], [2, 1]] },
        });
        hold_apart_what_node_never_writes(tables.as_object_mut().unwrap());
        assert_eq!(
            tables["project"],
            json!({ "columns": ["id"], "rows": [[1], [2]] })
        );
    }
}

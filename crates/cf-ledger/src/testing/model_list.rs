//! What migration 0025 made, which Node's ledger never had: the table of each harness's
//! models (CHF-03). The players of Node's recordings take it out of what this ledger left
//! ([`super::hold_apart_what_node_never_writes`] for its rows and
//! [`super::hold_apart_what_node_never_had_in_its_schema`] for its text), so that what the
//! two wrote in everything else is compared.

/// The table migration 0025 made.
pub(super) const TABLE: &str = "model_list";

/// What migration 0025 made taken out of the ledger `db` holds, so that a test can take a
/// ledger this build wrote for one an older build left, whose version it sets, and which
/// migrates to this build's again.
pub fn undo_migration_0025(db: &rusqlite::Connection) -> rusqlite::Result<()> {
    db.execute_batch(&format!("DROP TABLE {TABLE}"))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::super::{
        hold_apart_what_node_never_had_in_its_schema, hold_apart_what_node_never_writes,
    };

    #[test]
    fn takes_the_model_list_table_and_its_schema_out_and_nothing_else() {
        let mut tables = json!({
            "model_list": { "columns": ["harness"], "rows": [["'claude'"]] },
            "task": { "columns": ["id"], "rows": [["1"]] },
        });
        hold_apart_what_node_never_writes(tables.as_object_mut().unwrap());
        assert_eq!(
            tables,
            json!({ "task": { "columns": ["id"], "rows": [["1"]] } })
        );

        let mut dump = json!({ "userVersion": 11, "schema": [
            { "name": "model_list", "sql": "CREATE TABLE model_list (harness TEXT)" },
            { "name": "task_need", "sql": "CREATE TABLE task_need (id INTEGER)" },
        ] });
        hold_apart_what_node_never_had_in_its_schema(&mut dump);
        assert_eq!(
            dump["schema"],
            json!([{ "name": "task_need", "sql": "CREATE TABLE task_need (id INTEGER)" }])
        );
    }

    #[test]
    fn undoing_migration_0025_leaves_the_schema_0024_left_and_the_ledger_migrates_again() {
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

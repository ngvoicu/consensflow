//! What migration 0019 added to the `task` table, which Node's tasks never had
//! (the mark that a project's suspension paused a task, for its Resume to resume):
//! its column, and the text SQLite wrote for it into the table's schema. The
//! players of Node's recordings take them out of what this ledger left
//! ([`super::hold_apart_what_node_never_writes`] for the rows and
//! [`super::hold_apart_what_node_never_had_in_its_schema`] for the text of the
//! table), so that what the two wrote in everything else is compared.

/// The column migration 0019 added to `task`, which Node's tasks never had.
pub(super) const COLUMNS: [(&str, &str); 1] = [("task", "paused_with_project")];

/// What SQLite wrote into the text of the `task` table for that column, as
/// migration 0019 added it: after the last column and before the table's own
/// checks, with the text it was added with.
pub(super) const COLUMN_LINES: &str = ", paused_with_project INTEGER NOT NULL DEFAULT 0 CONSTRAINT task_paused_with_project_check CHECK (paused_with_project IN (0, 1))";

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::super::{
        hold_apart_what_node_never_had_in_its_schema, hold_apart_what_node_never_writes,
    };

    #[test]
    fn takes_the_mark_column_and_its_values_out_of_the_task_table_and_nothing_else() {
        let mut tables = json!({
            "task": {
                "columns": ["id", "state", "paused_with_project"],
                "rows": [["1", "'paused'", "1"], ["2", "'working'", "0"]],
            },
            "project": { "columns": ["id", "paused_with_project"], "rows": [["1", "1"]] },
        });
        hold_apart_what_node_never_writes(tables.as_object_mut().unwrap());
        assert_eq!(
            tables,
            json!({
                "task": {
                    "columns": ["id", "state"],
                    "rows": [["1", "'paused'"], ["2", "'working'"]],
                },
                "project": { "columns": ["id", "paused_with_project"], "rows": [["1", "1"]] },
            })
        );
    }

    #[test]
    fn takes_the_column_out_of_the_text_sqlite_wrote_for_the_task_table_and_of_no_other() {
        use crate::schema::{migrate, MIGRATIONS};
        let table_text = |migrations: &[&str]| {
            let db = rusqlite::Connection::open_in_memory().unwrap();
            migrate(&db, migrations).unwrap();
            db.query_row(
                "SELECT sql FROM sqlite_master WHERE name = 'task'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap()
        };
        // The table as migration 18 left it, and as 0019 left it; and the tester
        // the players take out of its checks, as they do of every table of tasks.
        let before =
            table_text(&MIGRATIONS[..18]).replace("'reviewer', 'tester', ", "'reviewer', ");
        let after = table_text(&MIGRATIONS[..19]);
        assert_ne!(
            table_text(&MIGRATIONS[..18]),
            after,
            "the migration changed the table's text"
        );
        let mut dump = json!({
            "userVersion": 19,
            "schema": [
                { "name": "task", "sql": after },
                { "name": "message", "sql": after },
            ],
            "tables": {},
        });
        hold_apart_what_node_never_had_in_its_schema(&mut dump);
        assert_eq!(dump["schema"][0]["sql"], before);
        assert_eq!(
            dump["schema"][1]["sql"], after,
            "another table's text is its own"
        );
        let held = dump.clone();
        hold_apart_what_node_never_had_in_its_schema(&mut dump);
        assert_eq!(dump, held, "a schema of Node's is left as it is");
    }
}

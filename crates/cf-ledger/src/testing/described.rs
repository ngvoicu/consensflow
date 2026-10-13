//! What migration 0017 added to the `project` table, which Node's projects never
//! had (the line a human writes under a project's name): its one column, which
//! [`super::hold_apart_what_node_never_writes`] takes out of the rows and
//! [`super::hold_apart_what_node_never_had_in_its_schema`] out of the text SQLite
//! wrote for the table, as it does the wake-up calls' columns, so that what the
//! two ledgers wrote in everything else is compared.

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::super::{
        hold_apart_what_node_never_had_in_its_schema, hold_apart_what_node_never_writes,
    };
    use crate::schema::{migrate, MIGRATIONS};

    /// Where the migration that added the description is in the list.
    fn at() -> usize {
        MIGRATIONS
            .iter()
            .position(|sql| sql.contains("ADD COLUMN description"))
            .expect("a migration that adds the description")
    }

    #[test]
    fn takes_the_description_and_its_value_out_of_the_project_table_and_nothing_else() {
        let mut tables = json!({
            "project": {
                "columns": ["id", "gate", "description", "wake_up_on"],
                "rows": [["1", "0", "NULL", "0"], ["2", "1", "'Moves the cargo'", "1"]],
            },
            "task": { "columns": ["id", "description"], "rows": [["1", "'a task'"]] },
        });
        hold_apart_what_node_never_writes(tables.as_object_mut().unwrap());
        assert_eq!(
            tables,
            json!({
                "project": { "columns": ["id", "gate"], "rows": [["1", "0"], ["2", "1"]] },
                "task": { "columns": ["id", "description"], "rows": [["1", "'a task'"]] },
            })
        );
    }

    #[test]
    fn takes_the_description_out_of_the_text_sqlite_wrote_for_the_project_table_and_of_no_other() {
        let project_text = |migrations: &[&str]| {
            let db = rusqlite::Connection::open_in_memory().unwrap();
            migrate(&db, migrations).unwrap();
            db.query_row(
                "SELECT sql FROM sqlite_master WHERE name = 'project'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap()
        };
        // The table as the migration before it left it, and as it left it. Both
        // are held apart from every column Node's schema never had, so that
        // what is left of the two texts is the same, and the description is
        // what the one has that the other has not.
        let before = project_text(&MIGRATIONS[..at()]);
        let after = project_text(&MIGRATIONS[..=at()]);
        assert_ne!(before, after, "the migration changed the table's text");
        assert!(after.contains(", description TEXT"));
        let held = |text: &str, table: &str| {
            let mut dump = json!({
                "userVersion": 11,
                "schema": [{ "name": table, "sql": text }],
                "tables": {},
            });
            hold_apart_what_node_never_had_in_its_schema(&mut dump);
            dump["schema"][0]["sql"].as_str().unwrap().to_owned()
        };
        assert_eq!(held(&after, "project"), held(&before, "project"));
        assert!(!held(&after, "project").contains("description"));
        assert_eq!(
            held(&after, "task"),
            after,
            "another table's text is its own"
        );
    }
}

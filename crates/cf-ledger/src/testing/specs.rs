//! What migration 0015 added to the `project` table, which Node's projects never
//! had (what a project says of specs): its three columns, and the text SQLite
//! wrote for them into the table's schema. The players of Node's recordings take
//! them out of what this ledger left ([`super::hold_apart_what_node_never_writes`]
//! for the rows and [`super::hold_apart_what_node_never_had_in_its_schema`] for the
//! text of the table), so that what the two wrote in everything else is compared.

/// The columns migration 0015 added to `project`, which Node's projects never
/// had: what a project says of specs.
pub(super) const COLUMNS: [(&str, &str); 3] = [
    ("project", "specs"),
    ("project", "specs_folder"),
    ("project", "specs_variant"),
];

/// What SQLite wrote into the text of the `project` table for those three
/// columns, as migration 0015 added them: each after the last column and before
/// the table's own checks, with the text it was added with.
pub(super) const COLUMN_LINES: &str = ", specs INTEGER\n    CONSTRAINT project_specs_check CHECK (specs IS NULL OR specs IN (0, 1)), specs_folder TEXT, specs_variant TEXT\n    CONSTRAINT project_specs_variant_check\n    CHECK (specs_variant IS NULL OR specs_variant IN ('core', 'core-html', 'tdd', 'tdd-html'))";

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::super::{
        hold_apart_what_node_never_had_in_its_schema, hold_apart_what_node_never_writes,
    };

    #[test]
    fn takes_the_three_columns_of_specs_and_their_values_out_of_the_project_table_and_nothing_else()
    {
        let mut tables = json!({
            "project": {
                "columns": ["id", "gate", "specs", "specs_folder", "specs_variant"],
                "rows": [["1", "0", "NULL", "NULL", "NULL"], ["2", "1", "1", "'plans'", "'tdd'"]],
            },
            "task": { "columns": ["id", "specs"], "rows": [["1", "7"]] },
        });
        hold_apart_what_node_never_writes(tables.as_object_mut().unwrap());
        assert_eq!(
            tables,
            json!({
                "project": { "columns": ["id", "gate"], "rows": [["1", "0"], ["2", "1"]] },
                "task": { "columns": ["id", "specs"], "rows": [["1", "7"]] },
            })
        );
    }

    #[test]
    fn takes_the_columns_of_specs_out_of_the_text_sqlite_wrote_for_the_project_table_and_of_no_other(
    ) {
        use crate::schema::{migrate, MIGRATIONS};
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
        // The table as every migration but 0015 left it, and as all of them did.
        let without: Vec<&str> = MIGRATIONS
            .iter()
            .copied()
            .filter(|sql| !sql.contains("project_specs_check"))
            .collect();
        assert_eq!(without.len(), MIGRATIONS.len() - 1);
        let before = project_text(&without);
        let after = project_text(&MIGRATIONS);
        assert_ne!(before, after, "the migration changed the table's text");
        let mut dump = json!({
            "userVersion": 15,
            "schema": [
                { "name": "project", "sql": after },
                { "name": "task", "sql": after },
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

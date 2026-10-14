//! What migration 0017 added to the `message` table, which Node's messages never
//! had (the role text a replacement of a role says): its column, and the text
//! SQLite wrote for it into the table's schema. The players of Node's recordings
//! take them out of what this ledger left
//! ([`super::hold_apart_what_node_never_writes`] for the rows and
//! [`super::hold_apart_what_node_never_had_in_its_schema`] for the text of the
//! table), so that what the two wrote in everything else is compared.

/// The column migration 0017 added to `message`, which Node's messages never
/// had: the role text a replacement of a role says.
pub(super) const COLUMNS: [(&str, &str); 1] = [("message", "role_text")];

/// What SQLite wrote into the text of the `message` table for that column, as
/// migration 0017 added it: after the last column and before the table's own
/// checks, with the text it was added with.
pub(super) const COLUMN_LINES: &str = ", role_text TEXT";

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::super::{
        hold_apart_what_node_never_had_in_its_schema, hold_apart_what_node_never_writes,
    };

    #[test]
    fn takes_the_role_text_column_and_its_values_out_of_the_message_table_and_nothing_else() {
        let mut tables = json!({
            "message": {
                "columns": ["id", "body", "role_text"],
                "rows": [["1", "'hello'", "NULL"], ["2", "'new role'", "'the role'"]],
            },
            "project": { "columns": ["id", "role_text"], "rows": [["1", "'kept'"]] },
        });
        hold_apart_what_node_never_writes(tables.as_object_mut().unwrap());
        assert_eq!(
            tables,
            json!({
                "message": {
                    "columns": ["id", "body"],
                    "rows": [["1", "'hello'"], ["2", "'new role'"]],
                },
                "project": { "columns": ["id", "role_text"], "rows": [["1", "'kept'"]] },
            })
        );
    }

    #[test]
    fn takes_the_column_out_of_the_text_sqlite_wrote_for_the_message_table_and_of_no_other() {
        use crate::schema::{migrate, MIGRATIONS};
        let table_text = |migrations: &[&str]| {
            let db = rusqlite::Connection::open_in_memory().unwrap();
            migrate(&db, migrations).unwrap();
            db.query_row(
                "SELECT sql FROM sqlite_master WHERE name = 'message'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap()
        };
        // The table as migration 16 left it, and as 0017 left it.
        let before = table_text(&MIGRATIONS[..16]);
        let after = table_text(&MIGRATIONS[..17]);
        assert_ne!(before, after, "the migration changed the table's text");
        let mut dump = json!({
            "userVersion": 17,
            "schema": [
                { "name": "message", "sql": after },
                { "name": "participant", "sql": after },
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

//! What migration 0020 added to the `message` table, which Node's messages never
//! had (when the transport let a message's hand-over go to its window): its
//! column, and the text SQLite wrote for it into the table's schema. The players of
//! Node's recordings take them out of what this ledger left
//! ([`super::hold_apart_what_node_never_writes`] for the rows and
//! [`super::hold_apart_what_node_never_had_in_its_schema`] for the text of the
//! table), so that what the two wrote in everything else is compared.

/// The column migration 0020 added to `message`, which Node's messages never
/// had: when the transport let its hand-over go.
pub(super) const COLUMNS: [(&str, &str); 1] = [("message", "handed_at")];

/// What SQLite wrote into the text of the `message` table for that column, as
/// migration 0020 added it: after the last column and before the table's own
/// checks, with the text it was added with.
pub(super) const COLUMN_LINES: &str = ", handed_at TEXT";

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::super::{
        hold_apart_what_node_never_had_in_its_schema, hold_apart_what_node_never_writes,
    };

    #[test]
    fn takes_the_handed_column_and_its_values_out_of_the_message_table_and_nothing_else() {
        let mut tables = json!({
            "message": {
                "columns": ["id", "body", "handed_at"],
                "rows": [["1", "'hello'", "NULL"], ["2", "'again'", "'2026-10-17T10:00:00.000Z'"]],
            },
            "task": { "columns": ["id", "handed_at"], "rows": [["1", "'kept'"]] },
        });
        hold_apart_what_node_never_writes(tables.as_object_mut().unwrap());
        assert_eq!(
            tables,
            json!({
                "message": {
                    "columns": ["id", "body"],
                    "rows": [["1", "'hello'"], ["2", "'again'"]],
                },
                "task": { "columns": ["id", "handed_at"], "rows": [["1", "'kept'"]] },
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
        // The table as migration 17 left it, and as 0020 left it: the players take
        // 0018's column out too.
        let before = table_text(&MIGRATIONS[..17]);
        let after = table_text(&MIGRATIONS[..20]);
        assert_ne!(
            table_text(&MIGRATIONS[..19]),
            after,
            "the migration changed the table's text"
        );
        let mut dump = json!({
            "userVersion": 20,
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

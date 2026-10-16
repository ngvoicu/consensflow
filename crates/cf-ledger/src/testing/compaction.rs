//! What migration 0019 added to the `transcript` table, which Node's copies
//! never had (the mark of a compaction's summary): its column, and the text
//! SQLite wrote for it into the table's schema. The players of Node's
//! recordings take them out of what this ledger left
//! ([`super::hold_apart_what_node_never_writes`] for the rows and
//! [`super::hold_apart_what_node_never_had_in_its_schema`] for the text of the
//! table), so that what the two wrote in everything else is compared.

/// The column migration 0019 added to `transcript`, which Node's copies never
/// had: whether an item is the summary a compaction wrote.
pub(super) const COLUMNS: [(&str, &str); 1] = [("transcript", "compaction")];

/// What SQLite wrote into the text of the `transcript` table for that column,
/// as migration 0019 added it, with the text it was added with.
pub(super) const COLUMN_LINES: &str = ", compaction INTEGER NOT NULL DEFAULT 0 CONSTRAINT transcript_compaction_check CHECK (compaction IN (0, 1))";

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::super::{
        hold_apart_what_node_never_had_in_its_schema, hold_apart_what_node_never_writes,
    };

    #[test]
    fn takes_the_compaction_column_and_its_values_out_of_the_transcript_table_and_nothing_else() {
        let mut tables = json!({
            "transcript": {
                "columns": ["item_id", "role", "compaction"],
                "rows": [["'u1'", "'user'", "0"], ["'s1'", "'user'", "1"]],
            },
            "message": { "columns": ["id", "compaction"], "rows": [["1", "'kept'"]] },
        });
        hold_apart_what_node_never_writes(tables.as_object_mut().unwrap());
        assert_eq!(
            tables,
            json!({
                "transcript": {
                    "columns": ["item_id", "role"],
                    "rows": [["'u1'", "'user'"], ["'s1'", "'user'"]],
                },
                "message": { "columns": ["id", "compaction"], "rows": [["1", "'kept'"]] },
            })
        );
    }

    #[test]
    fn takes_the_column_out_of_the_text_sqlite_wrote_for_the_transcript_table_and_of_no_other() {
        use crate::schema::{migrate, MIGRATIONS};
        let table_text = |migrations: &[&str]| {
            let db = rusqlite::Connection::open_in_memory().unwrap();
            migrate(&db, migrations).unwrap();
            db.query_row(
                "SELECT sql FROM sqlite_master WHERE name = 'transcript'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap()
        };
        // The table as migration 18 left it, and as 0019 left it.
        let before = table_text(&MIGRATIONS[..18]);
        let after = table_text(&MIGRATIONS[..19]);
        assert_ne!(before, after, "the migration changed the table's text");
        let mut dump = json!({
            "userVersion": 19,
            "schema": [
                { "name": "transcript", "sql": after },
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

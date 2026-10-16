//! What migration 0019 added, which Node's ledger never had: the column of
//! the `transcript` table that marks a compaction's summary, and the text
//! SQLite wrote for it into the table's schema; and the tables that hold what
//! the history viewer reads of each item and keep a long item in parts, and the
//! indexes it reads through, each a table or an index of its own. The players of
//! Node's recordings take them out of what this ledger left
//! ([`super::hold_apart_what_node_never_writes`] for the rows and
//! [`super::hold_apart_what_node_never_had_in_its_schema`] for the schema), so
//! that what the two wrote in everything else is compared.

/// The column migration 0019 added to `transcript`, which Node's copies never
/// had: whether an item is the summary a compaction wrote.
pub(super) const COLUMNS: [(&str, &str); 1] = [("transcript", "compaction")];

/// What SQLite wrote into the text of the `transcript` table for that column,
/// with the text migration 0019 added it with.
pub(super) const COLUMN_LINES: &str = ", compaction INTEGER NOT NULL DEFAULT 0 CONSTRAINT transcript_compaction_check CHECK (compaction IN (0, 1))";

/// The tables (`"TABLE"`) or indexes (`"INDEX"`) migration 0019 made, by the
/// names it gave them, as its lines say them (`CREATE INDEX name ON …`).
pub(super) fn made(kind: &str) -> impl Iterator<Item = &'static str> + '_ {
    include_str!("../../migrations/0019.sql")
        .lines()
        .filter_map(move |line| {
            line.trim()
                .strip_prefix("CREATE ")?
                .strip_prefix(kind)?
                .split_whitespace()
                .next()
        })
}

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

    #[test]
    fn takes_the_tables_and_the_indexes_migration_0019_made_out_of_a_dump_and_nothing_else() {
        use crate::schema::{migrate, MIGRATIONS};
        let names = |migrations: &[&str]| -> Vec<String> {
            let db = rusqlite::Connection::open_in_memory().unwrap();
            migrate(&db, migrations).unwrap();
            let names = db
                .prepare("SELECT name FROM sqlite_master WHERE type IN ('table', 'index') AND sql IS NOT NULL ORDER BY name")
                .unwrap()
                .query_map([], |row| row.get(0))
                .unwrap()
                .map(Result::unwrap)
                .collect();
            names
        };
        let before = names(&MIGRATIONS[..18]);
        let after = names(&MIGRATIONS[..19]);
        assert_eq!(after.len(), before.len() + 10, "{after:?}");
        let schema: Vec<_> = after
            .iter()
            .map(|name| json!({ "name": name, "sql": "CREATE …" }))
            .collect();
        let mut dump = json!({ "userVersion": 19, "schema": schema, "tables": {} });
        hold_apart_what_node_never_had_in_its_schema(&mut dump);
        let left: Vec<&str> = dump["schema"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["name"].as_str().unwrap())
            .collect();
        assert_eq!(left, before);
        let mut tables = json!({
            "transcript_part": { "columns": ["text"], "rows": [["'a part'"]] },
            "transcript_head": { "columns": ["id_shown"], "rows": [["'u1'"]] },
            "transcript": { "columns": ["text"], "rows": [["'kept'"]] },
        });
        hold_apart_what_node_never_writes(tables.as_object_mut().unwrap());
        assert_eq!(
            tables,
            json!({ "transcript": { "columns": ["text"], "rows": [["'kept'"]] } })
        );
    }
}

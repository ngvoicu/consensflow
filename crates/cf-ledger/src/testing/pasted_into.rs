//! What migration 0023 added, which Node's ledger never had: the column of the
//! `message` table that names the conversation a delivery was pasted into, the text
//! SQLite wrote for it into the table's schema, and the trigger that clears it. The
//! players of Node's recordings take them out of what this ledger left
//! ([`super::hold_apart_what_node_never_writes`] for the rows and
//! [`super::hold_apart_what_node_never_had_in_its_schema`] for the schema), so that
//! what the two wrote in everything else is compared.

/// The column migration 0023 added to `message`, which Node's messages never had.
pub(super) const COLUMNS: [(&str, &str); 1] = [("message", "pasted_into")];

/// What SQLite wrote into the text of the `message` table for that column, with the
/// text migration 0023 added it with.
pub(super) const COLUMN_LINES: &str = ", pasted_into INTEGER CONSTRAINT message_pasted_into_fk REFERENCES conversation (id) ON DELETE SET NULL";

/// The trigger migration 0023 made, as the schema names it.
pub(super) const TRIGGER: &str = "message_pasted_into_settled";

/// What migration 0023 made taken out of the ledger `db` holds: its trigger and its
/// column of `message`, so that a test can take a ledger this build wrote for one an
/// older build left, whose version it sets, and which migrates to this build's again.
pub fn undo_migration_0023(db: &rusqlite::Connection) -> rusqlite::Result<()> {
    db.execute_batch(&format!("DROP TRIGGER {TRIGGER}"))?;
    for (table, column) in COLUMNS {
        db.execute_batch(&format!("ALTER TABLE {table} DROP COLUMN {column}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::super::{
        hold_apart_what_node_never_had_in_its_schema, hold_apart_what_node_never_writes,
    };
    use super::*;

    #[test]
    fn takes_the_pasted_into_column_its_text_and_its_trigger_out_and_nothing_else() {
        let mut tables = json!({
            "message": {
                "columns": ["id", "body", "pasted_into"],
                "rows": [["1", "'hi'", "NULL"], ["2", "'there'", "3"]],
            },
        });
        hold_apart_what_node_never_writes(tables.as_object_mut().unwrap());
        assert_eq!(
            tables,
            json!({
                "message": { "columns": ["id", "body"], "rows": [["1", "'hi'"], ["2", "'there'"]] },
            })
        );
        let mut dump = json!({
            "userVersion": 11,
            "schema": [
                { "name": "message", "sql": format!("CREATE TABLE message (id INTEGER, body TEXT{COLUMN_LINES})") },
                { "name": TRIGGER, "sql": "CREATE TRIGGER …" },
            ],
        });
        hold_apart_what_node_never_had_in_its_schema(&mut dump);
        assert_eq!(
            dump,
            json!({
                "userVersion": 11,
                "schema": [{ "name": "message", "sql": "CREATE TABLE message (id INTEGER, body TEXT)" }],
            })
        );
    }

    #[test]
    fn the_column_lines_are_the_migrations_own() {
        let migration = include_str!("../../migrations/0023.sql");
        let added = COLUMN_LINES.trim_start_matches(", ");
        assert!(migration.contains(&format!("ADD COLUMN {added};")));
        assert!(migration.contains(&format!("CREATE TRIGGER {TRIGGER} ")));
    }
}

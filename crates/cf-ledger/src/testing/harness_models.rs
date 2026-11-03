//! What migration 0029 changed of the `participant` table, which Node's participants never
//! had (spec `harness-models`: a member's effort and the name its harness gives its model,
//! its tier always the human's): its columns, and the text SQLite wrote for them into the
//! table's schema. The players of Node's recordings take them out of what this ledger left
//! ([`super::hold_apart_what_node_never_writes`] for the rows and
//! [`super::hold_apart_what_node_never_had_in_its_schema`] for the text of the table), so
//! that what the two wrote in everything else is compared. A member no longer names a
//! ready-made agent, so the column that named it, which Node's members always filled, is
//! held apart too.

use serde_json::Value;

/// A member as Node's ledger was given it, as this one takes it: Node's member named the
/// ready-made agent it ran on (`agent`), which was its handle too; this ledger's is named
/// by its model, and the agent's name stands for the model's name, so its handle is the
/// one Node's had.
pub fn node_member(member: &Value) -> Value {
    let mut member = member.clone();
    if let Value::Object(fields) = &mut member {
        if let Some(agent) = fields.shift_remove("agent") {
            fields.insert("modelName".to_owned(), agent);
        }
    }
    member
}

/// A project's request as Node's ledger was given it, each member of its staff as
/// [`node_member`] takes it.
pub fn node_staff(request: &Value) -> Value {
    let mut request = request.clone();
    if let Some(Value::Array(staff)) = request.get_mut("staff") {
        for member in staff.iter_mut() {
            *member = node_member(member);
        }
    }
    request
}

/// What neither side of an answer is compared by: what a participant runs on (Node's named
/// its ready-made agent, this one's its model, its effort, its model's name, its harness's
/// name and the two as a lane says them, each held apart, a chief's model aside, which the
/// recordings were moved to), and of a staff a new
/// project starts from, all but its harness and roles.
pub fn hold_apart_what_members_run_on(value: &mut Value) {
    match value {
        Value::Object(fields) => {
            let participant = fields.contains_key("handle");
            if participant || fields.contains_key("roles") {
                for key in ["agent", "effort", "modelName", "harnessName", "runsOn"] {
                    fields.shift_remove(key);
                }
                let chief = fields.get("role").and_then(Value::as_str) == Some("chief");
                if !chief {
                    fields.shift_remove("model");
                }
                if !participant {
                    fields.shift_remove("designer");
                    fields.shift_remove("tier");
                }
            }
            fields.values_mut().for_each(hold_apart_what_members_run_on);
        }
        Value::Array(items) => items.iter_mut().for_each(hold_apart_what_members_run_on),
        _ => {}
    }
}

/// The columns migration 0029 added to `participant`, and the one it no longer writes.
pub(super) const COLUMNS: [(&str, &str); 3] = [
    ("participant", "effort"),
    ("participant", "model_name"),
    ("participant", "agent"),
];

/// What SQLite wrote into the text of the `participant` table for the columns migration
/// 0029 added: after the last column and before the table's own checks.
pub(super) const COLUMN_LINES: &str = ", effort TEXT, model_name TEXT";

/// What migration 0029 made taken out of the ledger `db` holds: its columns of
/// `participant` dropped, and 0028's mark of a tier the human chose back, as 0028 left it,
/// so that a test can take a ledger this build wrote for one an older build left, whose
/// version it sets, and which migrates to this build's again. The handles 0029 made stay.
pub fn undo_migration_0029(db: &rusqlite::Connection) -> rusqlite::Result<()> {
    db.execute_batch(
        "ALTER TABLE participant DROP COLUMN model_name;
         ALTER TABLE participant DROP COLUMN effort;
         ALTER TABLE participant ADD COLUMN tier_chosen INTEGER NOT NULL DEFAULT 0 CHECK (tier_chosen IN (0, 1));",
    )
}

#[cfg(test)]
mod tests {
    use crate::schema::{migrate, MIGRATIONS};

    fn participant_sql(db: &rusqlite::Connection) -> String {
        db.query_row(
            "SELECT sql FROM sqlite_master WHERE name = 'participant'",
            [],
            |row| row.get(0),
        )
        .unwrap()
    }

    #[test]
    fn undoing_migration_0029_leaves_the_table_0028_left_and_the_ledger_migrates_again() {
        let before = rusqlite::Connection::open_in_memory().unwrap();
        migrate(&before, &MIGRATIONS[..28]).unwrap();
        let db = rusqlite::Connection::open_in_memory().unwrap();
        migrate(&db, &MIGRATIONS).unwrap();
        let after = participant_sql(&db);
        assert!(after.contains(super::COLUMN_LINES), "{after}");
        assert!(!after.contains("tier_chosen"), "{after}");
        super::undo_migration_0029(&db).unwrap();
        assert_eq!(participant_sql(&db), participant_sql(&before));
        db.execute_batch("PRAGMA user_version = 28").unwrap();
        migrate(&db, &MIGRATIONS).unwrap();
        assert_eq!(participant_sql(&db), after);
    }
}

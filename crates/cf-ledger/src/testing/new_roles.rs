//! What migration 0036 widened (spec new-roles, FD-04): the four roles it added to the role
//! check of `participant` and the pool check of `task`, which Node's checks never named. The
//! players take them out of the schema's text before 0012's tester, so what is left is the
//! text Node's ledger had.

/// The four roles as migration 0036 wrote them into both checks, after the image designer,
/// and the nothing Node's checks had in their place.
pub(super) const IN_THE_CHECKS: (&str, &str) =
    (", 'frontend', 'architect', 'researcher', 'integrator'", "");

/// The tables migration 0036 rebuilt.
const REBUILT: [&str; 2] = ["participant", "task"];

/// What migration 0036 made undone in the ledger `db` holds: the two tables rebuilt with the
/// checks of five roles, every row, index and sequence as it was, so that a test can take a
/// ledger this build wrote for one a build of migration 0035 left, whose version it sets, and
/// which migrates to this build's again. A row of one of the four roles makes it fail, as the
/// older build's check would.
pub fn undo_migration_0036(db: &rusqlite::Connection) -> rusqlite::Result<()> {
    // Dropping a table others refer to would cascade through them with foreign keys on, as
    // the ledger's migrations turn them off for the same reason.
    let keys: i64 = db.query_row("PRAGMA foreign_keys", [], |row| row.get(0))?;
    db.execute_batch("PRAGMA foreign_keys = OFF")?;
    let undone = rebuild_with_five(db);
    db.execute_batch(&format!("PRAGMA foreign_keys = {keys}"))?;
    undone
}

/// The two tables rebuilt with the checks of five roles, and the sequences put back.
fn rebuild_with_five(db: &rusqlite::Connection) -> rusqlite::Result<()> {
    let sequences =
        "CREATE TEMP TABLE sequence_rows AS SELECT rowid AS at, name, seq FROM sqlite_sequence";
    db.execute_batch(sequences)?;
    for table in REBUILT {
        let sql: String = db.query_row(
            "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = ?",
            [table],
            |row| row.get(0),
        )?;
        let indexes: Vec<String> = db
            .prepare(
                "SELECT sql FROM sqlite_master WHERE type = 'index' AND tbl_name = ? AND sql IS NOT NULL ORDER BY rowid",
            )?
            .query_map([table], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        let narrowed = sql.replace(IN_THE_CHECKS.0, IN_THE_CHECKS.1);
        db.execute_batch(&format!(
            "CREATE TEMP TABLE {table}_rows AS SELECT * FROM {table};
             DROP TABLE {table};
             {narrowed};
             INSERT INTO {table} SELECT * FROM {table}_rows;
             DROP TABLE {table}_rows;"
        ))?;
        for index in indexes {
            db.execute_batch(&index)?;
        }
    }
    db.execute_batch(
        "DELETE FROM sqlite_sequence;
         INSERT INTO sqlite_sequence (name, seq) SELECT name, seq FROM sequence_rows ORDER BY at;
         DROP TABLE sequence_rows;",
    )
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::super::hold_apart_what_node_never_had_in_its_schema;
    use crate::schema::{migrate, MIGRATIONS};

    /// Where migration 0036 is in the list.
    fn at() -> usize {
        MIGRATIONS
            .iter()
            .position(|sql| sql.contains(super::IN_THE_CHECKS.0))
            .expect("the migration that adds the four roles")
    }

    fn schema(db: &rusqlite::Connection) -> Vec<(String, String)> {
        db.prepare("SELECT name, sql FROM sqlite_master WHERE sql IS NOT NULL ORDER BY name")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }

    #[test]
    fn migration_0036_widens_the_two_checks_and_nothing_else_of_the_schema() {
        assert_eq!(at(), 35, "migration 0036");
        assert_eq!(
            MIGRATIONS[at()].matches(super::IN_THE_CHECKS.0).count(),
            2,
            "one list in each check"
        );
        let before = rusqlite::Connection::open_in_memory().unwrap();
        migrate(&before, &MIGRATIONS[..at()]).unwrap();
        let db = rusqlite::Connection::open_in_memory().unwrap();
        migrate(&db, &MIGRATIONS[..=at()]).unwrap();
        let narrowed: Vec<(String, String)> = schema(&db)
            .into_iter()
            .map(|(name, sql)| (name, sql.replace(super::IN_THE_CHECKS.0, "")))
            .collect();
        assert_eq!(narrowed, schema(&before));
        assert_ne!(schema(&db), schema(&before));
    }

    #[test]
    fn undoing_migration_0036_leaves_the_schema_0035_left_and_the_ledger_migrates_again() {
        let before = rusqlite::Connection::open_in_memory().unwrap();
        migrate(&before, &MIGRATIONS[..at()]).unwrap();
        let db = rusqlite::Connection::open_in_memory().unwrap();
        migrate(&db, &MIGRATIONS[..=at()]).unwrap();
        let migrated = schema(&db);
        super::undo_migration_0036(&db).unwrap();
        assert_eq!(schema(&db), schema(&before));
        db.execute_batch("PRAGMA user_version = 35").unwrap();
        migrate(&db, &MIGRATIONS[..=at()]).unwrap();
        assert_eq!(schema(&db), migrated);
    }

    #[test]
    fn the_players_hold_the_four_roles_apart_from_the_checks_node_s_ledger_had() {
        let db = rusqlite::Connection::open_in_memory().unwrap();
        migrate(&db, &MIGRATIONS[..=at()]).unwrap();
        let node = rusqlite::Connection::open_in_memory().unwrap();
        migrate(&node, &MIGRATIONS[..11]).unwrap();
        for table in super::REBUILT {
            let text = |db: &rusqlite::Connection| -> String {
                db.query_row(
                    "SELECT sql FROM sqlite_master WHERE name = ?",
                    [table],
                    |row| row.get(0),
                )
                .unwrap()
            };
            let ours = text(&db);
            assert!(
                ours.contains("'designer', 'frontend', 'architect', 'researcher', 'integrator')"),
                "{ours}"
            );
            let mut dump = json!({
                "userVersion": 36,
                "schema": [{ "name": table, "sql": ours }],
                "tables": {},
            });
            hold_apart_what_node_never_had_in_its_schema(&mut dump);
            let held = dump["schema"][0]["sql"].as_str().unwrap().to_owned();
            for role in [
                "frontend",
                "architect",
                "researcher",
                "integrator",
                "tester",
            ] {
                assert!(!held.contains(&format!("'{role}'")), "{table}: {held}");
            }
            // Node's check of the same table, word for word.
            let node_check = |sql: &str| {
                let at = sql
                    .find(if table == "task" {
                        "task_pool_check"
                    } else {
                        "participant_role_check"
                    })
                    .unwrap();
                let check = &sql[at..];
                check[..check.find(")),").unwrap()].to_owned()
            };
            assert_eq!(node_check(&held), node_check(&text(&node)), "{table}");
        }
    }
}

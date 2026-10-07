//! The ledger's schema, as an ordered list of migrations (`src/ledger/schema.js`).
//! Migration `n` takes the database from `PRAGMA user_version = n` to
//! `n + 1`; the version is the list's length. A migration is SQL run inside
//! one transaction, and it is never edited once it has shipped: a change is a
//! new file at the end. The files are the JavaScript's strings byte for byte
//! (a test there holds them equal while both exist), and each is pinned here
//! by its SHA-256.

use rusqlite::Connection;
use serde_json::json;

use crate::model::LedgerError;

pub const MIGRATIONS: [&str; 11] = [
    include_str!("../migrations/0001.sql"),
    include_str!("../migrations/0002.sql"),
    include_str!("../migrations/0003.sql"),
    include_str!("../migrations/0004.sql"),
    include_str!("../migrations/0005.sql"),
    include_str!("../migrations/0006.sql"),
    include_str!("../migrations/0007.sql"),
    include_str!("../migrations/0008.sql"),
    include_str!("../migrations/0009.sql"),
    include_str!("../migrations/0010.sql"),
    include_str!("../migrations/0011.sql"),
];

pub const SCHEMA_VERSION: usize = MIGRATIONS.len();

/// Takes a ledger from the version it was written at to the last of
/// `migrations` (this build's, or the first few for an earlier one); a
/// newer ledger is refused.
pub fn migrate(db: &Connection, migrations: &[&str]) -> Result<(), LedgerError> {
    let known = migrations.len();
    let version: i64 = db.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if version > known as i64 {
        return Err(LedgerError::refused_with(
            "ledger-newer",
            format!(
                "this home was written by a newer ConsensFlow (schema {version}; this build knows {known})"
            ),
            409,
        ));
    }
    if version == known as i64 {
        return Ok(());
    }
    // A migration may rebuild a table others refer to; with foreign keys on,
    // dropping it would cascade through them. Off for the migrations, every
    // reference checked before each one commits, then on again: a start
    // refused here leaves the version as it was, so the next start checks too.
    db.execute_batch("PRAGMA foreign_keys = OFF")?;
    let migrated = (|| {
        let start = usize::try_from(version).unwrap_or(0);
        for (from, migration) in migrations.iter().enumerate().skip(start) {
            db.execute_batch("BEGIN IMMEDIATE")?;
            let step = (|| {
                db.execute_batch(migration)?;
                db.execute_batch(&format!("PRAGMA user_version = {}", from + 1))?;
                let broken = db
                    .prepare("PRAGMA foreign_key_check")?
                    .query_map([], |row| {
                        Ok(json!({
                            "table": row.get::<_, String>(0)?,
                            "rowid": row.get::<_, Option<i64>>(1)?,
                            "parent": row.get::<_, String>(2)?,
                            "fkid": row.get::<_, i64>(3)?,
                        }))
                    })?
                    .next()
                    .transpose()?;
                if let Some(first) = broken {
                    return Err(LedgerError::refused_with(
                        "ledger-broken",
                        format!("the ledger's references do not hold after migration: {first}"),
                        500,
                    ));
                }
                db.execute_batch("COMMIT")?;
                Ok(())
            })();
            if let Err(cause) = step {
                let _ = db.execute_batch("ROLLBACK");
                return Err(cause);
            }
        }
        Ok(())
    })();
    db.execute_batch("PRAGMA foreign_keys = ON")?;
    migrated
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(db: &Connection) -> i64 {
        db.query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap()
    }

    #[test]
    fn keeps_each_migration_byte_for_byte_as_it_shipped() {
        use sha2::{Digest, Sha256};
        const SHIPPED: [&str; 11] = [
            "51006450d3168c24b6a42ce41afdcf525994957ea22161b4a82f12bf26b65c01",
            "2c6ed236d1829599fbb378164b0c830893fac89eb9780f634688f9ca40f7f0f0",
            "14165bc20e6cb9d4c3c46a81bee45a95b4962ab759f4622aa1cebb8f8edc1ff2",
            "bcf20988ca6813437fb9e83155f6dabc2fcd36be8fed3e9680a420a5b4f9b974",
            "460d600e2a91f2b6b91855e02fb8cedc5db9825a1b922c26f96c44004760448c",
            "fd2a23c5ecc49ba4e397fd1decd7c6a7abe48bb57fb06f32b343540784b493b2",
            "b24997aa7d26552e08b5c4c66b836887d5ed9a2d0cc258130682c0840fcf36bf",
            "fe0ea090c22f838f2f77721fc54e3afcef9be9e47ff8fe7ba65b9eaf20663cd2",
            "402f0727daed5d35470d24d44169cb0187ad0afa50d51aa46e1df6d379f9c6a5",
            "ed27219b667184f2552284e38242ac8ea8060f61746edc876a63781e13f74a28",
            "76dbc67a043e337b4ffc5fe46fa66c0e236bcc9ef303156e2d1135c8bd481922",
        ];
        for (at, (sql, shipped)) in MIGRATIONS.iter().zip(SHIPPED).enumerate() {
            let hash: String = Sha256::digest(sql.as_bytes())
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect();
            assert_eq!(
                hash,
                shipped,
                "migration {} changed after it shipped: add a new one instead",
                at + 1
            );
        }
    }

    #[test]
    fn makes_a_new_ledger_at_the_current_version_and_a_current_one_is_left_alone() {
        let db = Connection::open_in_memory().unwrap();
        migrate(&db, &MIGRATIONS).unwrap();
        assert_eq!(version(&db), SCHEMA_VERSION as i64);
        migrate(&db, &MIGRATIONS).unwrap();
        assert_eq!(version(&db), SCHEMA_VERSION as i64);
    }

    #[test]
    fn refuses_a_ledger_a_newer_build_wrote() {
        let db = Connection::open_in_memory().unwrap();
        migrate(&db, &MIGRATIONS[..6]).unwrap();
        let refused = migrate(&db, &MIGRATIONS[..5]).unwrap_err();
        assert_eq!(
            (refused.code(), refused.to_string()),
            (
                Some("ledger-newer"),
                "this home was written by a newer ConsensFlow (schema 6; this build knows 5)"
                    .into()
            )
        );
    }

    #[test]
    fn keeps_a_migration_that_leaves_a_reference_dangling_from_counting() {
        let db = Connection::open_in_memory().unwrap();
        let first = "CREATE TABLE parent (id INTEGER PRIMARY KEY);
                     CREATE TABLE child (id INTEGER PRIMARY KEY, parent_id INTEGER REFERENCES parent (id));";
        let dangling = "INSERT INTO child (id, parent_id) VALUES (1, 7);";
        let refused = migrate(&db, &[first, dangling]).unwrap_err();
        assert_eq!(
            (refused.code(), refused.to_string()),
            (
                Some("ledger-broken"),
                r#"the ledger's references do not hold after migration: {"table":"child","rowid":1,"parent":"parent","fkid":0}"#.into()
            )
        );
        assert_eq!(version(&db), 1, "the next start checks again");
        let rows: i64 = db
            .query_row("SELECT COUNT(*) FROM child", [], |row| row.get(0))
            .unwrap();
        assert_eq!(rows, 0, "the broken migration wrote nothing");
        let on: i64 = db
            .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
            .unwrap();
        assert_eq!(on, 1, "foreign keys are on again");
    }
}

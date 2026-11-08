//! What older builds kept of each harness's models (CHF-03; the table is migration 0027's): a
//! list is read when it is asked for now (A4-28), so nothing writes or reads these rows, and the
//! daemon forgets what an older build left as it starts.

#![allow(clippy::unwrap_used)]

use cf_ledger::{open_ledger, Options};
use rusqlite::Connection;

fn rows(file: &std::path::Path) -> i64 {
    Connection::open(file)
        .unwrap()
        .query_row("SELECT count(*) FROM model_list", [], |row| row.get(0))
        .unwrap()
}

#[test]
fn forgetting_the_kept_listings_takes_every_row_an_older_build_left() {
    let folder = tempfile::tempdir().unwrap();
    let file = folder.path().join("consensflow.db");
    let ledger = open_ledger(&file, Options::default()).unwrap();
    ledger.close().unwrap();
    let db = Connection::open(&file).unwrap();
    for harness in ["claude", "devin", "pi"] {
        db.execute(
            "INSERT INTO model_list (harness, models, read_at, how, stale_why) \
             VALUES (?, '[]', 10, 'an older build', 'Not logged in')",
            [harness],
        )
        .unwrap();
    }
    drop(db);
    assert_eq!(rows(&file), 3);

    let mut ledger = open_ledger(&file, Options::default()).unwrap();
    assert_eq!(
        rows(&file),
        3,
        "opening the ledger forgets nothing: migrations read these rows"
    );
    ledger.forget_model_listings().unwrap();
    assert_eq!(rows(&file), 0, "every row of every harness");
}

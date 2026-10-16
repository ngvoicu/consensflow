//! The field's shapes of 2026-10-15, as recorded ledgers
//! (`tests/fixtures/ledgers/field-*.sql`), and the checks that must catch them.
//!
//! - `field-t14-mutation-run`: batch A's T-14 (another Mac, a Claude Code worker). The
//!   worker started a 33-step mutation run with `run_in_background`, said it would
//!   write up the result once it finished, and ended its turn: ConsensFlow took that
//!   for the result (m-131, A2), ended the run mid-edit, and its notice m-132 came
//!   after the result (A3).
//! - `field-b5-delivered-twice`: batch B's m-246 and m-248 (Windows, a Devin chief):
//!   m-246 was pasted again when Devin's record was late, the copy stayed in Devin's
//!   input, and m-248's paste joined it (B5).

use cf_evals::ledger_file;
use cf_evals::long_run::invariants::{check, Break};
use cf_evals::long_run::ledger::Ledger;
use cf_evals::long_run::look::Look;
use rusqlite::Connection;

use crate::support::Folder;

/// The recording `name`, loaded, and what the watch finds of it an hour after.
fn watched(name: &str) -> Vec<Break> {
    let folder = Folder::new();
    let file = folder.ledger();
    drop(ledger_file::load_ledger(&file, name, false, &[]).unwrap());
    let db = Connection::open(&file).unwrap();
    let ledger = Ledger::read(&db).unwrap();
    let last = ledger
        .messages
        .iter()
        .filter_map(|message| cf_evals::clock::millis(&message.created_at))
        .max()
        .unwrap();
    let look = Look {
        at: last + 3_600_000,
        ledger,
        ..Look::default()
    };
    check(&look, &Default::default())
}

fn of(breaks: &[Break], invariant: u8) -> Vec<&Break> {
    breaks
        .iter()
        .filter(|found| found.invariant == invariant)
        .collect()
}

#[test]
fn a2_the_task_done_while_its_mutation_run_still_ran_is_caught() {
    let breaks = watched("field-t14-mutation-run");
    let held = of(&breaks, 2);
    assert_eq!(held.len(), 1, "{breaks:?}");
    assert_eq!(
        held[0].said,
        "T-14 was done by m-131 while @zeus's background job bk2f9x still ran"
    );
    assert!(held[0].evidence.record[0].contains("Command running in background with ID: bk2f9x"));
}

#[test]
fn a3_the_ended_notice_after_its_result_is_caught() {
    let breaks = watched("field-t14-mutation-run");
    let order = of(&breaks, 3);
    assert_eq!(order.len(), 1, "{breaks:?}");
    assert_eq!(
        order[0].said,
        "T-14's ended notice m-132 came after its result m-131"
    );
}

#[test]
fn b5_the_result_delivered_twice_and_glued_to_the_next_is_caught() {
    let breaks = watched("field-b5-delivered-twice");
    let said: Vec<&str> = of(&breaks, 7)
        .iter()
        .map(|found| found.said.as_str())
        .collect();
    assert_eq!(
        said,
        [
            "@chief was given m-246 and m-248 as one prompt",
            "@chief was given m-246 2 times"
        ]
    );
    let twice = of(&breaks, 7)[1];
    assert!(twice
        .evidence
        .rows
        .iter()
        .any(|row| row.contains("delivery.retried") && row.contains("never showed it")));
}

#[test]
fn the_field_ledgers_break_nothing_else_the_watch_holds() {
    for name in ["field-t14-mutation-run", "field-b5-delivered-twice"] {
        let others: Vec<String> = watched(name)
            .into_iter()
            .filter(|found| ![2, 3, 7].contains(&found.invariant))
            .map(|found| found.key)
            .collect();
        assert!(others.is_empty(), "{name}: {others:?}");
    }
}

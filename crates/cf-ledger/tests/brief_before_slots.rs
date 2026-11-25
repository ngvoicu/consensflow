//! A Critical brief that an earlier version saved for a role that changes files still leads with
//! "It changes no project file", and main knows a task's own brief by its text. The slots change
//! that text (`delivery_body`), so the move and the edit also know the earlier form
//! (`brief_before_slots`, `architecture/staff-slots.md` section 9.3, difference 2). On alpha.13's
//! own ledger (`fixtures/t2013-alpha13.db.gz`: T-6's Critical worker brief queued for its window,
//! and the gated project's T-1, whose brief waits at the human's gate).

#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::io::Read;

use cf_ledger::{open_ledger, Ledger, Options};

const BEFORE: &str =
    "It changes no project file: return analysis, evidence and recommendations to the chief.";
const QUEUED: &str = "Critical work: hard-problem. It changes no project file: return analysis, evidence and recommendations to the chief.\n\nModify a project file";

fn alpha13() -> (tempfile::TempDir, Ledger) {
    let mut raw = Vec::new();
    flate2::read::GzDecoder::new(&include_bytes!("fixtures/t2013-alpha13.db.gz")[..])
        .read_to_end(&mut raw)
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("consensflow.db"), raw).unwrap();
    let ledger = open_ledger(&dir.path().join("consensflow.db"), Options::default())
        .expect("the update opens");
    (dir, ledger)
}

fn body(ledger: &Ledger, id: i64) -> String {
    ledger.message(id).unwrap().unwrap().body
}

/// The brief a window is given after the task moved: one paste with the work once, today's first
/// line, and the earlier instruction nowhere.
fn assert_the_work_once(brief: &str) {
    assert_eq!(
        brief.matches("Modify a project file").count(),
        1,
        "the work once: {brief}"
    );
    assert!(
        brief.starts_with("Critical work: hard-problem.\n\nModify a project file"),
        "today's first line: {brief}"
    );
    assert!(!brief.contains(BEFORE), "{brief}");
    assert!(!brief.contains("Kept from before"), "{brief}");
}

#[test]
fn a_reassign_before_the_saved_brief_arrived_gives_the_next_window_the_work_once() {
    let (_dir, mut ledger) = alpha13();
    assert_eq!(body(&ledger, 8), QUEUED, "the fixture's own brief");
    ledger
        .release_task(1, 6, "the person reassigned it")
        .expect("released");
    assert_eq!(body(&ledger, 8).len(), QUEUED.len());
    let moved = ledger.assign_task(1, 6, 4).expect("assigned again");
    assert_the_work_once(&moved.message.expect("its brief").body);
}

#[test]
fn the_backups_hand_off_before_the_saved_brief_arrived_gives_the_next_window_the_work_once() {
    let (_dir, mut ledger) = alpha13();
    ledger
        .hand_task_on(1, 6, "its member is out of quota")
        .expect("handed on");
    let moved = ledger.assign_task(1, 6, 4).expect("assigned again");
    assert_the_work_once(&moved.message.expect("its brief").body);
}

#[test]
fn an_edit_of_the_queued_saved_brief_is_what_the_window_is_given() {
    let (_dir, mut ledger) = alpha13();
    ledger
        .edit_task(1, 6, "chief", "Modify another file")
        .expect("edited");
    assert_eq!(
        body(&ledger, 8),
        "Critical work: hard-problem.\n\nModify another file"
    );
    ledger.begin_delivery(8).expect("its delivery begins");
    ledger
        .confirm_delivery(8, Some(&serde_json::json!({ "item": "i-1" })))
        .expect("delivered");
    let given = ledger.message(8).unwrap().unwrap();
    assert_eq!(given.state, "delivered");
    assert_eq!(
        given.body,
        "Critical work: hard-problem.\n\nModify another file"
    );
}

#[test]
fn an_edit_of_the_gated_saved_brief_is_what_the_window_is_given_after_its_approval() {
    let (_dir, mut ledger) = alpha13();
    // The gated project's worker task (its number is 1).
    assert_eq!(ledger.message(9).unwrap().unwrap().state, "gated");
    ledger
        .edit_task(2, 1, "chief", "Modify another file")
        .expect("edited");
    ledger.approve_message(9, "human").expect("passed on");
    ledger.begin_delivery(9).expect("its delivery begins");
    ledger
        .confirm_delivery(9, Some(&serde_json::json!({ "item": "i-2" })))
        .expect("delivered");
    let given = ledger.message(9).unwrap().unwrap();
    assert_eq!(given.state, "delivered");
    assert_eq!(
        given.body,
        "Critical work: hard-problem.\n\nModify another file"
    );
}

/// What this version wrote itself is found as before: the task's brief at its own form.
#[test]
fn a_brief_this_version_wrote_is_still_known_at_a_move_and_an_edit() {
    let (_dir, mut ledger) = alpha13();
    // T-7 is open at Critical with its purpose: assigned now, its brief has today's line.
    let first = ledger.assign_task(1, 7, 4).expect("assigned");
    let brief = first.message.expect("its brief");
    assert_eq!(
        brief.body,
        "Critical work: hard-problem.\n\nModify another file"
    );
    ledger
        .release_task(1, 7, "the person reassigned it")
        .expect("released");
    let again = ledger.assign_task(1, 7, 4).expect("assigned again");
    let again = again.message.expect("its brief").body;
    assert!(
        again.starts_with("Critical work: hard-problem.\n\nModify another file"),
        "{again}"
    );
    assert_eq!(again.matches("Modify another file").count(), 1, "{again}");
    assert!(!again.contains("Kept from before"), "{again}");
}

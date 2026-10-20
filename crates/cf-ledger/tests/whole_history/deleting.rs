//! Deleting a project deletes its participants' conversations, and with each
//! its items' heads: a cascade of the ledger's keys, each conversation's heads
//! found by migration 0020's index of them by their conversation. So deleting
//! goes through each item once, and takes as many of SQLite's steps as there
//! are items, never as their square.

use rusqlite::{params, StatementStatus};

use super::old_logs::AT;
use super::{id_of, ledger_in, project};

/// How many items each conversation holds.
const ITEMS: i64 = 100;

/// SQLite's steps for deleting a closed project whose chief had
/// `conversations` conversations of [`ITEMS`] items each, as `delete_project`
/// deletes it; and that each of their rows and heads went with it.
fn steps_to_delete(conversations: i64) -> i64 {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger = ledger_in(&dir);
    let view = project(&mut ledger);
    let chief = id_of(&view, "chief");
    // Written in bulk as the copy writes them: each item's row, and its head.
    ledger
        .sqlite()
        .execute(
            "WITH RECURSIVE n (at) AS (SELECT 1 UNION ALL SELECT at + 1 FROM n WHERE at < ?1)
             INSERT INTO conversation (participant_id, harness, started_at, ended_at)
               SELECT ?2, 'claude-code', ?3, ?3 FROM n",
            params![conversations, chief, AT],
        )
        .expect("conversations");
    ledger
        .sqlite()
        .execute(
            "WITH RECURSIVE n (at) AS (SELECT 1 UNION ALL SELECT at + 1 FROM n WHERE at < ?1)
             INSERT INTO transcript (conversation_id, item_id, seq, role, text, complete, at, copied_at)
               SELECT c.id, 'i-' || n.at, n.at, 'user', 'said', 1, NULL, ?2
               FROM conversation c, n WHERE c.participant_id = ?3",
            params![ITEMS, AT, chief],
        )
        .expect("items");
    let items = ledger
        .sqlite()
        .execute(
            "INSERT INTO transcript_head (item, conversation_id, role, bytes, parts, id_shown, complete, at, compaction)
               SELECT rowid, conversation_id, role, 4, 0, item_id, 1, NULL, 0 FROM transcript",
            [],
        )
        .expect("heads");
    assert_eq!(
        i64::try_from(items).expect("a count"),
        conversations * ITEMS
    );
    ledger
        .set_project_state(view.id, "suspended")
        .expect("closed");
    let db = ledger.sqlite();
    let keyed: bool = db
        .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
        .expect("read");
    assert!(keyed, "the cascade is the keys'");
    // The one statement `delete_project` deletes with, its cascades within it.
    let mut delete = db
        .prepare("DELETE FROM project WHERE id = ?")
        .expect("a delete");
    delete.execute([view.id]).expect("deleted");
    let steps = delete.get_status(StatementStatus::VmStep);
    drop(delete);
    let left: i64 = db
        .query_row(
            "SELECT (SELECT COUNT(*) FROM transcript) + (SELECT COUNT(*) FROM transcript_head)
               + (SELECT COUNT(*) FROM conversation)",
            [],
            |row| row.get(0),
        )
        .expect("counted");
    assert_eq!(left, 0, "everything of the project went with it");
    i64::from(steps)
}

#[test]
fn deleting_a_project_of_1_000_conversations_of_100_000_items_takes_steps_as_the_items_are_many() {
    let half = steps_to_delete(500);
    let whole = steps_to_delete(1_000);
    println!("deleting 50,000 items: {half} steps; 100,000 items: {whole} steps");
    // Twice the items, twice the steps: a cascade that went through every head
    // for each conversation would take four times as many.
    assert!(
        whole < half * 5 / 2,
        "deleting grew faster than the items: {half} then {whole} steps"
    );
    // Some 80 an item; a cascade through every head for each conversation
    // would go through 1,000 heads an item, each a few steps.
    assert!(
        whole < 300 * 1_000 * ITEMS,
        "{whole} steps for 100,000 items"
    );
}

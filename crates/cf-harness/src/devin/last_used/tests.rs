//! What a Devin session ran on last: its row's model, never a message node's.

use rusqlite::Connection;

use super::used_in;
use crate::contract::Used;

/// A store whose session `s` began on `swe-2-medium`, its nodes saying so, and was switched
/// to `swe-2-high`, which its row says: the first node that names a model names the old.
fn store(model: Option<&str>) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("sessions.db");
    let db = Connection::open(&file).unwrap();
    db.execute_batch(
        "CREATE TABLE sessions (id TEXT PRIMARY KEY, main_chain_id TEXT, model TEXT);
         CREATE TABLE message_nodes (row_id INTEGER PRIMARY KEY, session_id TEXT, node_id TEXT,
           parent_node_id TEXT, chat_message TEXT, created_at REAL);",
    )
    .unwrap();
    db.execute("INSERT INTO sessions VALUES ('s', 'n2', ?)", [model])
        .unwrap();
    for (row, node, parent, generation) in [
        (1, "n1", None, "swe-2-medium"),
        (2, "n2", Some("n1"), "swe-2-high"),
    ] {
        let message = format!(
            r#"{{"role":"assistant","content":"Done","metadata":{{"generation_model":"{generation}"}}}}"#
        );
        db.execute(
            "INSERT INTO message_nodes VALUES (?, 's', ?, ?, ?, 0)",
            rusqlite::params![row, node, parent, message],
        )
        .unwrap();
    }
    (dir, file)
}

#[test]
fn the_sessions_row_says_the_model_and_never_its_first_node() {
    let (_dir, file) = store(Some("swe-2-high"));
    assert_eq!(
        used_in(&file, "s").unwrap(),
        Some(Used {
            model: "swe-2-high".to_owned(),
            effort: None,
            thinking: None,
            by_harness: false,
        })
    );
}

#[test]
fn a_session_the_store_has_no_row_or_no_model_of_says_nothing() {
    let (_dir, file) = store(None);
    assert_eq!(used_in(&file, "s").unwrap(), None);
    assert_eq!(used_in(&file, "elsewhere").unwrap(), None);
}

//! The chief's open question, read while the daemon runs, and a Devin chief's own
//! question dialog.

use std::fs;

use cf_evals::ledger_file;
use cf_evals::measure::{chief_open_question, devin_chief_questions, OpenQuestion};
use rusqlite::Connection;
use serde_json::json;

use crate::support::Folder;

/// A ledger in write-ahead mode and still open, as the daemon's is during a run.
fn while_open(name: &str) -> (Folder, Connection) {
    let folder = Folder::new();
    let ledger = ledger_file::load_ledger(&folder.ledger(), name, true, &[]).unwrap();
    (folder, ledger)
}

#[test]
fn the_chiefs_open_question_is_its_newest_asking_turn_end_since_the_owner_typed_read_from_a_copy() {
    // The chief asks while its tasks run; a result arrives, and its next turn asks
    // nothing: the question is still open.
    let (folder, _ledger) = while_open("open-question-asked");
    assert_eq!(
        chief_open_question(&folder.ledger()).unwrap(),
        Some(OpenQuestion {
            id: "a2".to_owned(),
            text: "Tasks are out. What is the final report called?".to_owned(),
        })
    );
}

#[test]
fn the_chiefs_open_question_is_none_before_the_chief_has_said_anything_and_none_once_the_owner_answered(
) {
    let (folder, _ledger) = while_open("parallel-unrun");
    assert_eq!(
        chief_open_question(&folder.ledger()).unwrap(),
        None,
        "no turn yet"
    );
    // The same record with the owner's answer and the chief's next turn after it.
    let (folder, _ledger) = while_open("open-question-answered");
    assert_eq!(
        chief_open_question(&folder.ledger()).unwrap(),
        None,
        "the owner answered"
    );
}

#[test]
fn a_devin_chiefs_own_dialog_is_its_newest_question_call_no_answer_followed_read_from_a_copy() {
    let folder = Folder::new();
    // The chief's conversation is bound to the session 'swift-owl', which Devin's store names.
    let file = folder.ledger();
    drop(ledger_file::load_ledger(&file, "devin-chief", false, &[]).unwrap());
    // Devin's data folder, on Windows (APPDATA) or elsewhere: dir/data/devin.
    let data = folder.path().join("data");
    let env = vec![
        (
            "HOME".to_owned(),
            folder.path().to_string_lossy().into_owned(),
        ),
        (
            "XDG_DATA_HOME".to_owned(),
            data.to_string_lossy().into_owned(),
        ),
        ("APPDATA".to_owned(), data.to_string_lossy().into_owned()),
    ];
    let cli = data.join("devin").join("cli");
    fs::create_dir_all(&cli).unwrap();
    let store = Connection::open(cli.join("sessions.db")).unwrap();
    store
        .execute_batch(
            "CREATE TABLE message_nodes (row_id INTEGER PRIMARY KEY, session_id TEXT, chat_message TEXT)",
        )
        .unwrap();
    let add = |message: serde_json::Value| {
        store
            .execute(
                "INSERT INTO message_nodes (session_id, chat_message) VALUES ('swift-owl', ?1)",
                [message.to_string()],
            )
            .unwrap();
    };
    let ask = |id: &str, question: &str| {
        json!({
            "role": "assistant",
            "tool_calls": [{
                "id": id,
                "name": "ask_user_question",
                "arguments": { "questions": [{ "question": question, "header": "File", "options": [{ "label": "a.html" }] }] },
            }],
        })
    };
    assert_eq!(
        devin_chief_questions(&file, &env).unwrap(),
        None,
        "nothing asked yet"
    );
    add(ask("q1", "Which file?"));
    add(json!({ "role": "tool", "tool_call_id": "q1", "content": "User answered your questions" }));
    assert_eq!(
        devin_chief_questions(&file, &env).unwrap(),
        None,
        "answered"
    );
    add(ask("q2", "Which language?"));
    assert_eq!(
        devin_chief_questions(&file, &env).unwrap(),
        Some(vec![json!({
            "question": "Which language?",
            "header": "File",
            "options": [{ "label": "a.html" }],
        })])
    );
}

#[test]
fn a_devin_chief_with_no_store_to_read_has_no_dialog_open() {
    let folder = Folder::new();
    let file = folder.ledger();
    drop(ledger_file::load_ledger(&file, "devin-chief", false, &[]).unwrap());
    let env = vec![
        (
            "HOME".to_owned(),
            folder.path().to_string_lossy().into_owned(),
        ),
        (
            "XDG_DATA_HOME".to_owned(),
            folder.path().join("nothing").to_string_lossy().into_owned(),
        ),
    ];
    assert_eq!(devin_chief_questions(&file, &env).unwrap(), None);
}

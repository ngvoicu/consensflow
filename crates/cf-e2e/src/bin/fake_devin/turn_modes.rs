//! A turn in each permission mode, as Devin 3000.11.92 had them: `/bypass` switches a
//! window and makes no turn of its own; in the normal mode the prompt file's call is
//! refused and a pasted one waits at its dialog; and the note of a quit goes on with the
//! message of the turn the quit cut.

use std::thread;
use std::time::Duration;

use rusqlite::Connection;
use serde_json::Value;

use super::*;
use crate::calls::MADE;
use crate::permission::{Answer, CANCELED, SKIPPED};

/// A window on a store and a wire of a folder of its own, in the normal mode or in full
/// permission, and what it said on its screen.
struct Case {
    folder: tempfile::TempDir,
    permission: Arc<Permission>,
    window: Window<Vec<u8>>,
}

fn case(bypass: bool) -> Case {
    let folder = tempfile::tempdir().unwrap();
    let store = Store::open(&folder.path().join("sessions.db"), "calm-river").unwrap();
    let wire = Wire::open(&folder.path().join("wire.jsonl"), "calm-river").unwrap();
    let permission = Arc::new(Permission::new(bypass));
    let window = Window::new(
        store,
        wire,
        Arc::new(Stop::default()),
        Vec::new(),
        Pace::default(),
    )
    .in_mode(Arc::clone(&permission), folder.path().to_path_buf());
    Case {
        folder,
        permission,
        window,
    }
}

impl Case {
    fn nodes(&self) -> Vec<Value> {
        let db = Connection::open(self.folder.path().join("sessions.db")).unwrap();
        let mut rows = db
            .prepare("SELECT chat_message FROM message_nodes ORDER BY row_id")
            .unwrap();
        rows.query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .map(|message| serde_json::from_str(&message.unwrap()).unwrap())
            .collect()
    }

    fn wire(&self) -> Vec<Value> {
        std::fs::read_to_string(self.folder.path().join("wire.jsonl"))
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    fn screen(&self) -> String {
        String::from_utf8(self.window.screen.clone()).unwrap()
    }

    fn made(&self) -> bool {
        std::fs::read_to_string(self.folder.path().join("made.txt")).is_ok_and(|text| text == MADE)
    }
}

/// The records of the wire that are `kind` of update.
fn updates<'a>(wire: &'a [Value], kind: &str) -> Vec<&'a Value> {
    wire.iter()
        .filter(|record| record["update"]["sessionUpdate"] == kind)
        .collect()
}

#[test]
fn bypass_alone_switches_the_window_and_makes_no_turn_no_record_and_no_prompt() {
    let mut case = case(false);
    case.window.take("/bypass").unwrap();
    assert_eq!(case.permission.mode(), "bypass");
    assert!(case.nodes().is_empty(), "nothing in the store");
    let wire = case.wire();
    assert!(wire
        .iter()
        .all(|record| record["method"] != "session/prompt"));
    assert!(wire.iter().all(|record| record.get("cause").is_none()));
    let changed = updates(&wire, "current_mode_update");
    assert_eq!(changed.len(), 1);
    assert_eq!(changed[0]["update"]["currentModeId"], "bypass");
    assert!(case.screen().contains("✓ Switched to Bypass mode"));
    // Again, in full permission: the same words, and no change on the wire.
    case.window.take("/bypass").unwrap();
    assert_eq!(updates(&case.wire(), "current_mode_update").len(), 1);
}

#[test]
fn a_prompt_files_call_in_the_normal_mode_is_refused_and_its_turn_answers_all_the_same() {
    let mut case = case(false);
    case.window
        .take_message("FILE made.txt Reply with exactly: DONE", true)
        .unwrap();
    assert!(!case.made(), "the tool never ran");
    let nodes = case.nodes();
    let roles: Vec<&str> = nodes
        .iter()
        .map(|node| node["role"].as_str().unwrap())
        .collect();
    assert_eq!(roles, ["user", "tool", "assistant"]);
    assert_eq!(nodes[1]["content"], SKIPPED);
    assert_eq!(nodes[2]["content"], "DONE", "it answers as if it had");
    let wire = case.wire();
    let failed = updates(&wire, "tool_call_update");
    assert_eq!(failed[0]["update"]["_meta"]["cognition.ai/rejected"], true);
    assert_eq!(wire.last().unwrap()["cause"], "complete");
}

#[test]
fn bypass_before_the_prompt_files_message_switches_the_window_before_the_message_runs() {
    let mut case = case(false);
    case.window
        .take_message("/bypass FILE made.txt Reply with exactly: DONE", true)
        .unwrap();
    assert!(case.made());
    let nodes = case.nodes();
    assert_eq!(
        nodes[0]["content"],
        "FILE made.txt Reply with exactly: DONE"
    );
    let wire = case.wire();
    let switched = wire
        .iter()
        .position(|record| record["update"]["sessionUpdate"] == "current_mode_update")
        .unwrap();
    let prompted = wire
        .iter()
        .position(|record| record["method"] == "session/prompt")
        .unwrap();
    assert!(
        switched < prompted,
        "the mode changed before the message ran"
    );
}

#[test]
fn a_pasted_call_in_the_normal_mode_waits_at_its_dialog_until_enter_or_escape() {
    for (key, made) in [(Answer::Approved, true), (Answer::Cancelled, false)] {
        let mut case = case(false);
        let permission = Arc::clone(&case.permission);
        let presser = thread::spawn(move || {
            while !permission.asking() {
                thread::sleep(Duration::from_millis(5));
            }
            assert!(permission.answer(key));
        });
        case.window
            .take("FILE made.txt Reply with exactly: DONE")
            .unwrap();
        presser.join().unwrap();
        assert!(
            case.screen().contains("❭ 1 Yes  (Approve once)"),
            "{}",
            case.screen()
        );
        assert_eq!(case.made(), made, "{key:?}");
        let wire = case.wire();
        if made {
            assert_eq!(wire.last().unwrap()["cause"], "complete");
        } else {
            // Left with Escape: the call failed, and so did the turn.
            let failed = updates(&wire, "tool_call_update");
            assert_eq!(
                failed[0]["update"]["content"][0]["content"]["text"],
                CANCELED
            );
            assert_eq!(failed[0]["update"]["_meta"]["cognition.ai/rejected"], false);
            assert_eq!(wire.last().unwrap()["cause"], "cancelled");
        }
    }
}

#[test]
fn the_note_of_a_quit_goes_on_with_the_message_of_the_turn_it_cut_without_its_wait() {
    let folder = tempfile::tempdir().unwrap();
    let db = folder.path().join("sessions.db");
    // The quit cut the first turn: its message is in the store, and no end of it.
    Store::open(&db, "calm-river")
        .unwrap()
        .user(
            "SLEEP 600 FILE made.txt Reply with exactly: DONE",
            "client-1",
        )
        .unwrap();
    let store = Store::open(&db, "calm-river").unwrap();
    let wire = Wire::open(&folder.path().join("wire.jsonl"), "calm-river").unwrap();
    let mut window = Window::new(
        store,
        wire,
        Arc::new(Stop::default()),
        Vec::new(),
        Pace::default(),
    )
    .in_mode(Arc::new(Permission::new(true)), folder.path().to_path_buf());
    let started = std::time::Instant::now();
    window
        .take_message(
            "[ConsensFlow m-2 · T-1 · note from ConsensFlow]\nConsensFlow quit while you were working on T-1. Go on where you stopped.",
            true,
        )
        .unwrap();
    assert!(started.elapsed() < Duration::from_secs(60), "not its wait");
    assert_eq!(
        std::fs::read_to_string(folder.path().join("made.txt")).unwrap(),
        MADE
    );
    assert_eq!(
        String::from_utf8(window.screen.clone())
            .unwrap()
            .lines()
            .last(),
        Some("DONE")
    );
}

//! What the owner reads of a board: the tasks, what changed, and how it is said.

use super::*;

fn board() -> Value {
    json!({
        "lanes": [
            {
                "participant": { "handle": "chief", "role": "chief" },
                "activity": { "state": "idle" },
                "tasks": [],
            },
            {
                "participant": { "handle": "zeus", "role": "worker" },
                "activity": { "state": "working" },
                "tasks": [{ "number": 1, "state": "working" }],
            },
        ],
        "open": [{ "number": 2, "state": "open" }],
        "gated": [],
    })
}

#[test]
fn the_tasks_are_those_of_the_lanes_then_the_open_ones() {
    let board = board();
    let tasks = tasks_of(&board);
    let numbers: Vec<i64> = tasks
        .iter()
        .filter_map(|task| task["number"].as_i64())
        .collect();
    assert_eq!(numbers, [1, 2]);
    assert!(tasks_of(&json!({})).is_empty());
    assert!(tasks_of(&json!({ "lanes": [{ "participant": {} }] })).is_empty());
}

#[test]
fn the_tasks_are_said_as_the_run_says_them_as_it_goes() {
    let board = board();
    assert_eq!(listed(&tasks_of(&board)), "T-1:working T-2:open");
    assert_eq!(listed(&[]), "none");
}

#[test]
fn what_changes_when_the_run_is_not_quiet_is_the_chiefs_state_the_tasks_and_every_window() {
    let before = board();
    let lane = Chief::lane_of(&before);
    let signature = signature_of(&before, &lane, &tasks_of(&before));
    assert_eq!(
        signature,
        r#"["idle",[[1,"working"],[2,"open"]],[["chief","idle"],["zeus","working"]]]"#
    );
    // A task's state, or a window's, makes another signature.
    let mut moved = board();
    moved["lanes"][1]["tasks"][0]["state"] = json!("done");
    let lane = Chief::lane_of(&moved);
    assert_ne!(signature_of(&moved, &lane, &tasks_of(&moved)), signature);
    let mut woke = board();
    woke["lanes"][0]["activity"]["state"] = json!("working");
    let lane = Chief::lane_of(&woke);
    assert_ne!(signature_of(&woke, &lane, &tasks_of(&woke)), signature);
    // What is the same is the same.
    let again = board();
    let lane = Chief::lane_of(&again);
    assert_eq!(signature_of(&again, &lane, &tasks_of(&again)), signature);
}

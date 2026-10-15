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

#[test]
fn the_files_are_read_at_the_first_look_that_sees_the_first_worker_task_s_result_in() {
    let tasks = |cards: Value| -> Vec<Value> { cards.as_array().unwrap().clone() };
    let first_in = |cards: Value| first_result_in(&tasks(cards).iter().collect::<Vec<_>>());
    // T-1 at work, then done, then accepted beside its follow-up.
    assert!(!first_in(
        json!([{ "number": 1, "pool": "worker", "state": "working" }])
    ));
    assert!(first_in(
        json!([{ "number": 1, "pool": "worker", "state": "done" }])
    ));
    assert!(first_in(json!([
        { "number": 2, "pool": null, "state": "working" },
        { "number": 1, "pool": "worker", "state": "accepted" },
    ])));
    // The follow-up's result is not T-1's, while T-1 waits for its own.
    assert!(!first_in(json!([
        { "number": 1, "pool": "worker", "state": "working" },
        { "number": 2, "pool": null, "state": "done" },
    ])));
    // A review, an advice, a test or an image done first is no worker's result.
    for pool in ["reviewer", "advisor", "tester", "designer"] {
        assert!(
            !first_in(json!([
                { "number": 1, "pool": pool, "state": "done" },
                { "number": 2, "pool": "worker", "state": "working" },
            ])),
            "{pool}"
        );
    }
    // A failed or cancelled T-1 brought no line in, and a board with no task has no result.
    assert!(!first_in(
        json!([{ "number": 1, "pool": "worker", "state": "failed" }])
    ));
    assert!(!first_in(
        json!([{ "number": 1, "pool": "worker", "state": "cancelled" }])
    ));
    assert!(!first_in(json!([])));
}

#[test]
fn the_files_kept_at_t_1_s_result_are_what_they_held_at_that_look_and_no_later_one() {
    let workspace = tempfile::tempdir().unwrap();
    let page = workspace.path().join("site/evaluare.html");
    cf_e2e::files::write(&page, "the owner's page").unwrap();
    let mut read = crate::measure::begun(workspace.path(), &["site/evaluare.html"]).unwrap();
    let mut kept = false;
    let look = |cards: Value, read: &mut BTreeMap<String, FileRead>, kept: &mut bool| {
        let cards = cards.as_array().unwrap().clone();
        let tasks: Vec<&Value> = cards.iter().collect();
        keep_at_result(read, kept, &tasks, workspace.path()).unwrap();
    };
    let at_result =
        |read: &BTreeMap<String, FileRead>| read["site/evaluare.html"].at_result.clone();
    // T-1 at work, its line not in yet: nothing kept.
    look(
        json!([{ "number": 1, "pool": "worker", "state": "working" }]),
        &mut read,
        &mut kept,
    );
    assert!(!kept);
    assert_eq!(at_result(&read), None);
    // Its result in, the line in the page: kept.
    cf_e2e::files::write(&page, "the owner's page, and the worker's line").unwrap();
    look(
        json!([{ "number": 1, "pool": "worker", "state": "done" }]),
        &mut read,
        &mut kept,
    );
    assert!(kept);
    assert_eq!(
        at_result(&read).as_deref(),
        Some("the owner's page, and the worker's line")
    );
    // The follow-up took the line out: what was kept stays, and the start too.
    cf_e2e::files::write(&page, "the owner's page").unwrap();
    look(
        json!([
            { "number": 1, "pool": "worker", "state": "accepted" },
            { "number": 2, "pool": null, "state": "done" },
        ]),
        &mut read,
        &mut kept,
    );
    assert_eq!(
        at_result(&read).as_deref(),
        Some("the owner's page, and the worker's line")
    );
    assert_eq!(
        read["site/evaluare.html"].start.as_deref(),
        Some("the owner's page")
    );
}

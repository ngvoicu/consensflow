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
fn t_1_s_result_is_in_once_the_first_worker_task_is_done_or_accepted() {
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
fn the_files_kept_at_t_1_s_result_are_what_they_held_as_the_owner_asks_for_the_next_step() {
    let played = Looks::new();
    // T-1 at work, its line not in yet, while the owner waits: nothing kept.
    played.look(T1_WORKING, "the owner's page", &Step::Wait);
    assert_eq!(played.at_result(), None);
    // Its result in, the line in the page, and the owner still waiting: nothing yet.
    played.look(T1_DONE, WITH_LINE, &Step::Wait);
    assert_eq!(played.at_result(), None);
    // The owner is about to ask for the undo: kept, before anything of it is asked.
    played.look(T1_DONE, WITH_LINE, &say());
    assert_eq!(played.at_result().as_deref(), Some(WITH_LINE));
    // The follow-up took the line out: what was kept stays, and the start too.
    played.look(T2_DONE, "the owner's page", &Step::Over);
    assert_eq!(played.at_result().as_deref(), Some(WITH_LINE));
    assert_eq!(played.start().as_deref(), Some("the owner's page"));
    // A later message of the owner's reads nothing again: T-1's page is kept once.
    played.look(T2_DONE, "the owner's page", &say());
    assert_eq!(played.at_result().as_deref(), Some(WITH_LINE));
    // A follow-up while T-1 still works keeps nothing: there is no result to keep.
    let early = Looks::new();
    early.look(T1_WORKING, WITH_LINE, &say());
    assert_eq!(early.at_result(), None);
    // Nor does a switch of the chief, which is no message of the owner's.
    let switched = Looks::new();
    switched.look(T1_DONE, WITH_LINE, &Step::Next(FollowUp::Switch));
    assert_eq!(switched.at_result(), None);
}

#[test]
fn a_chief_that_sends_the_undo_at_once_has_no_restored_page_taken_for_t_1_s_result() {
    // Sol, 2026-10-15: between two looks, five seconds apart, T-1 finished and a chief
    // that went on at once had T-2 take the line out. The look that first sees T-1's
    // result sees the page restored; it must not be kept as the page T-1 left.
    let raced = Looks::new();
    raced.look(T1_WORKING, "the owner's page", &Step::Wait);
    raced.look(T2_DONE, "the owner's page", &Step::Wait);
    raced.look(T2_DONE, "the owner's page", &Step::Over);
    assert_eq!(raced.at_result(), None);
}

const T1_WORKING: &str = r#"[{ "number": 1, "pool": "worker", "state": "working" }]"#;
const T1_DONE: &str = r#"[{ "number": 1, "pool": "worker", "state": "done" }]"#;
const T2_DONE: &str = r#"[
    { "number": 1, "pool": "worker", "state": "accepted" },
    { "number": 2, "pool": null, "state": "done" }
]"#;
const WITH_LINE: &str = "the owner's page, and the worker's line";

/// The owner's next message of the conversation.
fn say() -> Step {
    Step::Next(FollowUp::Say("the undo".to_owned()))
}

/// A run's looks at a workspace with one page in it, and what they kept of the page.
struct Looks {
    workspace: tempfile::TempDir,
    read: std::cell::RefCell<BTreeMap<String, FileRead>>,
    kept: std::cell::Cell<bool>,
}

impl Looks {
    fn new() -> Self {
        let workspace = tempfile::tempdir().unwrap();
        cf_e2e::files::write(
            &workspace.path().join("site/evaluare.html"),
            "the owner's page",
        )
        .unwrap();
        let read = crate::measure::begun(workspace.path(), &["site/evaluare.html"]).unwrap();
        Self {
            workspace,
            read: std::cell::RefCell::new(read),
            kept: std::cell::Cell::new(false),
        }
    }

    /// A look at a board whose tasks are `cards`, with the page holding `page`, where the
    /// owner's next step is `step`.
    fn look(&self, cards: &str, page: &str, step: &Step) {
        cf_e2e::files::write(&self.workspace.path().join("site/evaluare.html"), page).unwrap();
        let cards: Value = serde_json::from_str(cards).unwrap();
        let tasks: Vec<&Value> = cards.as_array().unwrap().iter().collect();
        let mut kept = self.kept.get();
        keep_at_result(
            &mut self.read.borrow_mut(),
            &mut kept,
            step,
            &tasks,
            self.workspace.path(),
        )
        .unwrap();
        self.kept.set(kept);
    }

    fn at_result(&self) -> Option<String> {
        self.read.borrow()["site/evaluare.html"].at_result.clone()
    }

    fn start(&self) -> Option<String> {
        self.read.borrow()["site/evaluare.html"].start.clone()
    }
}

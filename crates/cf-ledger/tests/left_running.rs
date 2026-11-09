//! What a window was running when the chief finished its task (T-691, 3.0.7; the field
//! report of 2026-10-26): `cf task finish` records the worker's last words as the result and
//! ends nothing its turn left running, and the close of the window after the accept ended a
//! parity run, silently. The finish keeps how many background jobs and agents the window was
//! counted running, with its move to done, for the accept to say; the worker's own result
//! waits for its work to end, so it keeps none.

#![allow(clippy::expect_used)]

use cf_ledger::{open_ledger, Ledger, NewChief, NewMember, NewProject, NewTask, Options};
use serde_json::json;

fn options() -> Options {
    let mut names = ["amber-pine", "brisk-birch", "calm-brook"]
        .into_iter()
        .cycle();
    Options {
        names: Box::new(move || names.next().unwrap_or_default().to_owned()),
        ..Options::default()
    }
}

/// A project at /work/app: apollo its chief, zeus its standard worker, with two tasks open for
/// a worker, each in a window of its own (`working`).
fn ledger_with_two_tasks_at_work(dir: &tempfile::TempDir) -> Ledger {
    let mut ledger = open_ledger(&dir.path().join("consensflow.db"), options()).expect("ledger");
    ledger
        .create_project(&NewProject {
            directory: "/work/app".into(),
            name: "app".into(),
            chief: NewChief {
                harness: "claude-code".into(),
                model: Some("apollo".into()),
                effort: None,
                model_name: None,
            },
            staff: vec![NewMember {
                model: None,
                effort: None,
                model_name: Some("zeus".into()),
                harness: "claude-code".into(),
                designer: false,
                roles: vec!["worker".into()],
                tier: "standard".into(),
            }],
            gate: false,
            wake_up: None,
            specs: None,
            staff_rules: None,
            compaction: None,
        })
        .expect("a project");
    let zeus = ledger
        .project(1)
        .expect("a project")
        .expect("project 1")
        .participants
        .into_iter()
        .find(|participant| participant.handle == "zeus")
        .expect("zeus")
        .id;
    for (number, body) in [(1, "Parser"), (2, "Lexer")] {
        ledger
            .create_task(
                1,
                &NewTask {
                    from: "chief".into(),
                    pool: Some("worker".into()),
                    tier: Some("standard".into()),
                    body: body.into(),
                    ..NewTask::default()
                },
            )
            .expect("a task");
        let brief = ledger
            .assign_task(1, number, zeus)
            .expect("assigned")
            .message
            .expect("its brief")
            .id;
        ledger.begin_delivery(brief).expect("its delivery begins");
        ledger
            .confirm_delivery(brief, Some(&json!({ "item": format!("i-{number}") })))
            .expect("its delivery is confirmed");
    }
    ledger
}

#[test]
fn a_finish_keeps_what_the_window_was_counted_running_for_the_accept_to_say() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger = ledger_with_two_tasks_at_work(&dir);
    assert_eq!(ledger.left_running(1, 1).expect("a count"), (0, 0));

    ledger
        .record_finished(1, 1, "Parser done", (2, 1))
        .expect("T-1 is finished");
    assert_eq!(ledger.left_running(1, 1).expect("a count"), (2, 1));
    ledger.accept_task(1, 1, "chief").expect("accepted");
    assert_eq!(
        ledger.left_running(1, 1).expect("a count"),
        (2, 1),
        "the accept is where it is said, and keeps it"
    );
    assert_eq!(
        ledger.left_running(1, 2).expect("a count"),
        (0, 0),
        "another task's window is none of it"
    );
}

#[test]
fn a_result_the_worker_gave_or_a_finish_that_counted_none_keeps_none() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger = ledger_with_two_tasks_at_work(&dir);
    ledger
        .record_result(1, 1, "Parser done")
        .expect("T-1 is done by its worker");
    assert_eq!(ledger.left_running(1, 1).expect("a count"), (0, 0));
    ledger
        .record_finished(1, 2, "Lexer done", (0, 0))
        .expect("T-2 is finished");
    assert_eq!(ledger.left_running(1, 2).expect("a count"), (0, 0));
}

#[test]
fn a_task_sent_back_and_finished_again_is_counted_by_its_latest_finish() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger = ledger_with_two_tasks_at_work(&dir);
    ledger
        .record_finished(1, 1, "Parser done", (3, 0))
        .expect("T-1 is finished");
    assert_eq!(ledger.left_running(1, 1).expect("a count"), (3, 0));

    // Sent back, taken in again, and the worker's own word ends it this time.
    let more = ledger
        .reopen_task(1, 1, "chief", "More")
        .expect("T-1 is sent back")
        .message
        .expect("its follow-up")
        .id;
    ledger.begin_delivery(more).expect("its delivery begins");
    ledger
        .confirm_delivery(more, Some(&json!({ "item": "i-9" })))
        .expect("its delivery is confirmed");
    ledger
        .record_result(1, 1, "Parser done again")
        .expect("T-1 is done by its worker");
    assert_eq!(
        ledger.left_running(1, 1).expect("a count"),
        (0, 0),
        "the earlier finish's count is not this result's"
    );
}

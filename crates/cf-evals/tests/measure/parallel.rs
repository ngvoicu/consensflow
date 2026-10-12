//! Measuring parallel work: how many windows had a brief and no result at once.

use cf_evals::measure::{measure, Options};

use crate::support::{measured, message, project_with, task, Folder};

#[test]
fn counts_only_windows_that_had_their_brief_tasks_still_on_the_board_never_ran() {
    let metrics = measured("parallel-unrun");
    assert_eq!(metrics.task_count, 3);
    assert_eq!(metrics.parallel, 1);
}

#[test]
fn ends_a_task_that_ended_without_a_result_where_it_ended() {
    let metrics = measured("parallel-cancelled");
    assert_eq!(metrics.parallel, 1);
}

#[test]
fn counts_a_window_at_work_from_the_delivery_of_its_brief_not_from_the_writing_of_it() {
    let folder = Folder::new();
    let (db, project) = project_with(&folder, &["zeus", "diana"]);
    // The participants are the human (1), the chief (2), zeus (3) and diana (4).
    let at = |time: &str| format!("2026-10-08T{time}.000Z");
    // T-1 is written at 10:00 and given to its window at 10:10, its result at 10:20;
    // T-2 is written at 10:01, given at 10:02, done at 10:05: before T-1 began.
    let one = task(&db, project, 1, 3, "done");
    message(
        &db,
        project,
        (3, 2),
        "task",
        one,
        "delivered",
        &at("10:00:00"),
        Some(&at("10:10:00")),
    );
    message(
        &db,
        project,
        (2, 3),
        "result",
        one,
        "delivered",
        &at("10:20:00"),
        None,
    );
    let two = task(&db, project, 2, 4, "done");
    message(
        &db,
        project,
        (4, 2),
        "task",
        two,
        "delivered",
        &at("10:01:00"),
        Some(&at("10:02:00")),
    );
    message(
        &db,
        project,
        (2, 4),
        "result",
        two,
        "delivered",
        &at("10:05:00"),
        None,
    );
    drop(db);
    assert_eq!(
        measure(&folder.ledger(), &Options::default())
            .unwrap()
            .parallel,
        1
    );
}

#[test]
fn two_windows_that_each_had_their_brief_before_either_had_given_its_result_ran_side_by_side() {
    let folder = Folder::new();
    let (db, project) = project_with(&folder, &["zeus", "diana"]);
    let at = |time: &str| format!("2026-10-08T{time}.000Z");
    let one = task(&db, project, 1, 3, "done");
    message(
        &db,
        project,
        (3, 2),
        "task",
        one,
        "delivered",
        &at("10:00:00"),
        Some(&at("10:01:00")),
    );
    message(
        &db,
        project,
        (2, 3),
        "result",
        one,
        "delivered",
        &at("10:20:00"),
        None,
    );
    let two = task(&db, project, 2, 4, "done");
    message(
        &db,
        project,
        (4, 2),
        "task",
        two,
        "delivered",
        &at("10:00:00"),
        Some(&at("10:02:00")),
    );
    message(
        &db,
        project,
        (2, 4),
        "result",
        two,
        "delivered",
        &at("10:05:00"),
        None,
    );
    drop(db);
    assert_eq!(
        measure(&folder.ledger(), &Options::default())
            .unwrap()
            .parallel,
        2
    );
}

//! A wake-up is given only when the chief has nothing on its way: no task
//! working, queued or waiting on a question, none open behind a task that is
//! not yet done. A task done and waiting for the chief's decision is the
//! chief's own work, and so is a board with nothing on it. A wake-up skipped
//! is not owed: the interval counts again from the skip.

use cf_ledger::{NewChief, NewMember, NewProject, NewTask, WakeUpUnit};

use crate::world::{world, World, DAY, MINUTE};

/// A world with the calls on, a minute apart, and the first wake-up delivered.
fn after_the_first() -> World {
    let mut w = world();
    w.turn_on(1, WakeUpUnit::Minutes);
    w.advance(MINUTE);
    w.wake_and_deliver();
    w
}

/// How many wake-ups the log says were skipped.
fn skipped(w: &World) -> usize {
    w.kinds()
        .iter()
        .filter(|kind| *kind == "project.wake_up_skipped")
        .count()
}

#[test]
fn a_task_queued_for_its_window_holds_the_wake_up_back() {
    let mut w = after_the_first();
    let given = w.give(&[]);
    assert_eq!(w.state(given.task.number), "queued");
    w.advance(DAY);
    assert!(w.due().is_none(), "work is on its way: no one to ask");
    assert_eq!(w.queued(), 1);
}

#[test]
fn a_task_working_holds_the_wake_up_back() {
    let mut w = after_the_first();
    let given = w.give(&[]);
    w.start(&given);
    w.advance(DAY);
    assert!(w.due().is_none());
    assert_eq!(w.queued(), 1);
}

#[test]
fn a_task_waiting_on_a_question_holds_the_wake_up_back() {
    let mut w = after_the_first();
    let given = w.give(&[]);
    w.start(&given);
    w.ask(given.task.number);
    w.advance(DAY);
    assert!(w.due().is_none());
    assert_eq!(w.queued(), 1);
}

#[test]
fn a_task_open_behind_one_that_is_not_yet_done_holds_the_wake_up_back() {
    let mut w = after_the_first();
    let first = w.give(&[]);
    w.start(&first);
    w.ledger
        .pause_task(w.project.id, first.task.number, Some("chief"), None)
        .expect("paused");
    assert_eq!(w.state(first.task.number), "paused");
    // A task paused is stopped, not working: the one open behind it is what is on its way.
    let behind = w.give(&[first.task.number as u64]);
    assert_eq!(w.state(behind.task.number), "open");
    assert_eq!(behind.task.blocked_by, [first.task.number]);
    w.advance(DAY);
    assert!(w.due().is_none());
    assert_eq!(w.queued(), 1);
}

#[test]
fn the_first_wake_up_after_the_calls_are_turned_on_is_held_back_by_work_on_its_way_too() {
    let mut w = world();
    let given = w.give(&[]);
    w.start(&given);
    w.turn_on(1, WakeUpUnit::Minutes);
    w.advance(DAY);
    assert!(
        w.due().is_none(),
        "the first has no more right than the rest"
    );
    assert_eq!(w.queued(), 0);
}

#[test]
fn a_task_done_and_waiting_for_the_chiefs_decision_is_the_chiefs_own_work_and_gets_a_wake_up() {
    let mut w = after_the_first();
    let given = w.give(&[]);
    w.start(&given);
    w.finish(given.task.number);
    w.advance(MINUTE);
    assert!(
        w.due().is_some(),
        "nothing is on its way but the chief's decision"
    );
}

#[test]
fn a_task_open_behind_one_done_and_undecided_waits_for_the_chief_too_and_gets_a_wake_up() {
    let mut w = after_the_first();
    let first = w.give(&[]);
    w.start(&first);
    w.finish(first.task.number);
    let behind = w.give(&[first.task.number as u64]);
    assert_eq!(w.state(behind.task.number), "open");
    w.advance(MINUTE);
    assert!(
        w.due().is_some(),
        "it waits for the chief to accept the first, which is the chief's to do"
    );
}

#[test]
fn a_task_open_behind_one_that_failed_or_was_called_off_is_the_chiefs_to_settle() {
    let mut w = after_the_first();
    let first = w.give(&[]);
    w.start(&first);
    let called_off = w.give(&[]);
    let behind = w.give(&[first.task.number as u64, called_off.task.number as u64]);
    assert_eq!(w.state(behind.task.number), "open");
    w.ledger
        .fail_task(w.project.id, first.task.number, "the window died")
        .expect("failed");
    w.ledger
        .cancel_task(w.project.id, called_off.task.number, "chief")
        .expect("called off");
    w.advance(MINUTE);
    assert!(
        w.due().is_some(),
        "nothing will finish them: the chief is the one left to act"
    );
}

#[test]
fn a_board_with_nothing_left_on_it_gets_a_wake_up_once_something_moved() {
    let mut w = after_the_first();
    let given = w.give(&[]);
    w.start(&given);
    w.finish(given.task.number);
    w.accept(given.task.number);
    w.advance(MINUTE);
    assert!(w.due().is_some());
}

#[test]
fn a_wake_up_skipped_is_not_owed_and_the_next_comes_an_interval_after_the_skip() {
    let mut w = after_the_first();
    let given = w.give(&[]);
    w.start(&given);
    w.advance(5 * MINUTE);
    assert!(w.due().is_none(), "skipped while the task works");
    assert_eq!(skipped(&w), 1);
    // The work ends, a good while after the interval passed: no wake-up is
    // owed for it, and the count began again at the skip.
    w.finish(given.task.number);
    assert!(w.due().is_none(), "not owed the moment the board clears");
    w.advance(MINUTE - 1);
    assert!(
        w.due().is_none(),
        "a minute from the skip, not from before it"
    );
    w.advance(1);
    assert!(w.due().is_some(), "the next interval, on the same rule");
    assert_eq!(w.queued(), 2);
}

#[test]
fn a_skip_is_logged_once_for_an_interval_however_often_it_is_asked_and_again_at_the_next() {
    let mut w = after_the_first();
    let given = w.give(&[]);
    w.start(&given);
    w.advance(MINUTE);
    for _ in 0..3 {
        assert!(w.due().is_none());
    }
    assert_eq!(skipped(&w), 1);
    w.advance(MINUTE);
    assert!(w.due().is_none());
    assert_eq!(skipped(&w), 2, "one for each interval the work lasts");
}

#[test]
fn work_on_its_way_in_another_project_holds_nothing_back() {
    let mut w = after_the_first();
    w.move_a_task();
    let other = w
        .ledger
        .create_project(&NewProject {
            directory: "/work/other".to_owned(),
            name: "other".to_owned(),
            chief: NewChief {
                harness: "claude-code".to_owned(),
                agent: Some("apollo".to_owned()),
            },
            staff: vec![NewMember {
                agent: "zeus".to_owned(),
                harness: "claude-code".to_owned(),
                designer: false,
                roles: vec!["worker".to_owned()],
                tier: "standard".to_owned(),
            }],
            gate: false,
            wake_up: None,
            specs: None,
        })
        .expect("another project");
    w.ledger
        .create_task(
            other.id,
            &NewTask {
                from: "chief".to_owned(),
                to: Some("zeus".to_owned()),
                body: "Write the lexer".to_owned(),
                ..NewTask::default()
            },
        )
        .expect("a task of its own");
    w.advance(MINUTE);
    assert!(w.due().is_some(), "that project's work is that chief's");
}

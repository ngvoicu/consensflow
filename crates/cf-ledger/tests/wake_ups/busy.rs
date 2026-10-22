//! A wake-up is given only when the chief has nothing on its way: no task
//! working or queued for its window, none waiting for an answer that is on its
//! way (one the gate holds for the human among them), none held for its
//! member's quota, none open behind one of those (down a chain of open tasks).
//! A task done and waiting for the chief's decision, a question the chief has
//! not answered, a task the chief paused, a task the chief holds itself, and a
//! board with nothing on it are the chief's own work. A wake-up skipped is not
//! owed: the interval counts again from the skip.

use cf_ledger::{NewChief, NewMember, NewProject, NewTask, WakeUpUnit};

use crate::world::{gated_world, world, World, DAY, MINUTE};

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
fn a_question_the_chief_has_not_answered_is_the_chiefs_to_answer_and_gets_a_wake_up() {
    let mut w = after_the_first();
    let given = w.give(&[]);
    w.start(&given);
    w.ask(given.task.number);
    w.advance(MINUTE);
    assert!(
        w.due().is_some(),
        "the task waits on the chief: nothing is on its way"
    );
}

#[test]
fn a_question_answered_and_the_answer_not_yet_received_holds_the_wake_up_back() {
    let mut w = after_the_first();
    let given = w.give(&[]);
    w.start(&given);
    let question = w.ask(given.task.number);
    w.answer(&question);
    assert_eq!(w.state(given.task.number), "waiting");
    w.advance(DAY);
    assert!(w.due().is_none(), "the answer is on its way to the window");
    assert_eq!(w.held_by(), [given.task.number]);
    assert_eq!(w.queued(), 1);
}

#[test]
fn a_task_open_behind_one_on_its_way_holds_the_wake_up_back_and_so_does_a_chain_of_them() {
    let mut w = after_the_first();
    let first = w.give(&[]);
    w.start(&first);
    let behind = w.give(&[first.task.number as u64]);
    assert_eq!(w.state(behind.task.number), "open");
    assert_eq!(behind.task.blocked_by, [first.task.number]);
    let last = w.give(&[behind.task.number as u64]);
    assert_eq!(w.state(last.task.number), "open");
    w.advance(DAY);
    assert!(w.due().is_none());
    assert_eq!(
        w.held_by(),
        [first.task.number, behind.task.number, last.task.number],
        "the skip names the working task and the two that wait behind it"
    );
    assert_eq!(w.queued(), 1);
}

#[test]
fn a_task_the_chief_paused_is_the_chiefs_to_resume_and_so_is_one_open_behind_it() {
    let mut w = after_the_first();
    let first = w.give(&[]);
    w.start(&first);
    w.pause(first.task.number);
    w.advance(MINUTE);
    let sent = w.due().expect("a task paused is on its way to nobody");
    w.deliver(&sent);
    let behind = w.give(&[first.task.number as u64]);
    assert_eq!(w.state(behind.task.number), "open");
    w.advance(MINUTE);
    assert!(
        w.due().is_some(),
        "the one open behind it waits for the chief's resume"
    );
}

#[test]
fn a_task_held_for_its_members_quota_goes_on_by_itself_and_holds_the_wake_up_back() {
    let mut w = after_the_first();
    let first = w.give(&[]);
    w.start(&first);
    w.hold(first.task.number);
    let behind = w.give(&[first.task.number as u64]);
    w.advance(MINUTE);
    assert!(w.due().is_none(), "it resumes at the reset: nobody to ask");
    assert_eq!(w.held_by(), [first.task.number, behind.task.number]);
    assert_eq!(w.queued(), 1);
}

#[test]
fn a_hold_the_daemon_ended_without_resuming_the_task_is_the_chiefs_again_and_gets_a_wake_up() {
    let mut w = after_the_first();
    let first = w.give(&[]);
    w.start(&first);
    w.hold(first.task.number);
    w.advance(MINUTE);
    assert!(w.due().is_none());
    w.release(first.task.number);
    w.advance(MINUTE);
    assert!(w.due().is_some(), "paused and held by nobody: the chief's");
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
fn tasks_open_in_a_chain_behind_one_done_and_undecided_get_a_wake_up_too() {
    let mut w = after_the_first();
    let first = w.give(&[]);
    w.start(&first);
    w.finish(first.task.number);
    let behind = w.give(&[first.task.number as u64]);
    let last = w.give(&[behind.task.number as u64]);
    let beside = w.give(&[first.task.number as u64]);
    assert_eq!(
        [
            w.state(behind.task.number),
            w.state(last.task.number),
            w.state(beside.task.number)
        ],
        ["open", "open", "open"]
    );
    w.advance(MINUTE);
    assert!(
        w.due().is_some(),
        "the chief's decision on the first is all the three wait for"
    );
}

#[test]
fn a_task_open_behind_one_that_failed_or_was_called_off_is_the_chiefs_to_settle() {
    let mut w = after_the_first();
    let first = w.give(&[]);
    w.start(&first);
    // A task others wait for is not cancelled by itself; its member's removal
    // still calls it off, and that is the one way left to such a need.
    w.ledger
        .add_member(
            w.project.id,
            &NewMember {
                agent: "hermes".to_owned(),
                harness: "claude-code".to_owned(),
                designer: false,
                roles: vec!["worker".to_owned()],
                tier: "standard".to_owned(),
            },
        )
        .expect("a second member");
    let called_off = w
        .ledger
        .create_task(
            w.project.id,
            &NewTask {
                from: "chief".to_owned(),
                to: Some("hermes".to_owned()),
                body: "Write the lexer".to_owned(),
                ..NewTask::default()
            },
        )
        .expect("hermes's task");
    let behind = w.give(&[first.task.number as u64, called_off.task.number as u64]);
    assert_eq!(w.state(behind.task.number), "open");
    w.ledger
        .fail_task(w.project.id, first.task.number, "the window died")
        .expect("failed");
    let removed = w
        .ledger
        .remove_member(w.project.id, "hermes")
        .expect("hermes leaves");
    assert_eq!(removed.cancelled, [called_off.task.number]);
    assert_eq!(w.state(behind.task.number), "open");
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

#[test]
fn an_answer_the_gate_holds_for_the_human_is_on_its_way_and_holds_the_wake_up_back() {
    let mut w = gated_world();
    w.turn_on(1, WakeUpUnit::Minutes);
    w.advance(MINUTE);
    w.wake_and_deliver();
    let given = w.give(&[]);
    assert_eq!(given.message.as_ref().unwrap().state, "gated");
    w.approve(given.message.as_ref().unwrap());
    w.start(&given);
    let question = w.ask(given.task.number);
    let answer = w.answer(&question);
    assert_eq!(
        answer.state, "gated",
        "the chief answered; the human decides"
    );
    assert_eq!(w.state(given.task.number), "waiting");
    w.advance(DAY);
    assert!(
        w.due().is_none(),
        "the chief has done its part: the answer waits for the human"
    );
    assert_eq!(w.held_by(), [given.task.number]);
}

#[test]
fn a_task_the_chief_holds_itself_and_works_on_is_the_chiefs_own_and_gets_a_wake_up() {
    let mut w = after_the_first();
    let own = w.give_self(&[]);
    w.start(&own);
    w.advance(MINUTE);
    assert!(
        w.due().is_some(),
        "the chief's turn can end in the middle of its own task: nobody else has it"
    );
}

#[test]
fn a_task_the_chief_holds_itself_does_not_let_the_staffs_work_go_unprotected() {
    let mut w = after_the_first();
    let own = w.give_self(&[]);
    w.start(&own);
    let theirs = w.give(&[]);
    w.start(&theirs);
    w.advance(MINUTE);
    assert!(w.due().is_none(), "zeus's task is on its way");
    assert_eq!(w.held_by(), [theirs.task.number]);
}

#[test]
fn a_task_open_behind_one_the_chief_holds_itself_waits_for_the_chief_too() {
    let mut w = after_the_first();
    let own = w.give_self(&[]);
    w.start(&own);
    let behind = w.give(&[own.task.number as u64]);
    assert_eq!(w.state(behind.task.number), "open");
    w.advance(MINUTE);
    assert!(
        w.due().is_some(),
        "the chief's task is all the other waits for"
    );
}

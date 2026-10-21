//! A wake-up is for a chief with nothing on its way: it is not given while a
//! task is working, queued for its window, waiting on a question, or open
//! behind a task that is not yet done. A task done and waiting for the chief's
//! decision, or a board with nothing left on it, is the chief's own, and the
//! chief is woken for it. A wake-up skipped is not owed: the interval counts
//! again from the skip.

use cf_engine::testing::Context;
use cf_ledger::{NewTask, ProjectView, WakeUpUnit};

use super::{at_rest, given, passes, queued, set, woken, HOUR, MINUTE};

/// A project whose chief has been woken once and answered, a minute apart.
fn woken_once() -> (Context, ProjectView) {
    let context = Context::new();
    let project = at_rest(&context);
    set(&context, &project, true, 1, WakeUpUnit::Minutes);
    context.advance(MINUTE);
    woken(&context, &project);
    assert_eq!(given(&context), 1);
    (context, project)
}

/// The state of task `number`.
fn state(context: &Context, project: &ProjectView, number: i64) -> String {
    context.task(project.id, number).task.state
}

/// Task `number` is on zeus's hands and working.
fn working(context: &Context, project: &ProjectView, number: i64) {
    passes(context);
    assert_eq!(state(context, project, number), "working");
}

/// A task for zeus that waits for the tasks `needs` to be accepted.
fn give_behind(context: &Context, project: &ProjectView, needs: &[u64]) {
    context.create_task(
        project.id,
        NewTask {
            from: "chief".to_owned(),
            to: Some("zeus".to_owned()),
            body: "Test the parser".to_owned(),
            needs: needs.to_vec(),
            ..NewTask::default()
        },
    );
}

/// zeus ends its task with its result, which reaches the chief, who answers it
/// and is at rest again with the task done and undecided.
fn done_and_undecided(context: &Context, project: &ProjectView, number: i64) {
    context.adapter.answer("zeus", "Parser done");
    passes(context);
    assert_eq!(state(context, project, number), "done");
    context.adapter.answer("chief", "Noted.");
    passes(context);
}

#[test]
fn a_task_queued_for_its_window_holds_the_wake_up_back() {
    let (context, project) = woken_once();
    context.give(project.id, "zeus", "Write the parser");
    context.advance(MINUTE);
    context.pass().unwrap();
    assert_eq!(state(&context, &project, 1), "queued");
    assert_eq!(given(&context), 1, "work is on its way: nobody to ask");
    assert_eq!(queued(&context, &project), 1);
}

#[test]
fn a_task_working_holds_the_wake_up_back_and_it_is_not_owed_when_the_work_is_over() {
    let (context, project) = woken_once();
    context.give(project.id, "zeus", "Write the parser");
    working(&context, &project, 1);
    context.advance(HOUR);
    passes(&context);
    assert_eq!(given(&context), 1, "a task is working");
    assert_eq!(queued(&context, &project), 1);
    // The work ends, hours after the interval passed: the chief has the result to
    // decide, and the count began again at the skip.
    done_and_undecided(&context, &project, 1);
    passes(&context);
    assert_eq!(given(&context), 1, "not owed the moment the work is over");
    context.advance(MINUTE - 1);
    passes(&context);
    assert_eq!(
        given(&context),
        1,
        "a minute from the skip, not from before it"
    );
    context.advance(1);
    passes(&context);
    assert_eq!(
        given(&context),
        2,
        "the next interval: a task done and undecided is the chief's own"
    );
}

#[test]
fn a_task_waiting_on_a_question_holds_the_wake_up_back() {
    let (context, project) = woken_once();
    context.give(project.id, "zeus", "Write the parser");
    working(&context, &project, 1);
    context.ask(project.id, "zeus", "chief", 1, "Which parser?");
    passes(&context);
    context.adapter.answer("chief", "Looking.");
    passes(&context);
    assert_eq!(state(&context, &project, 1), "waiting");
    context.advance(HOUR);
    passes(&context);
    assert_eq!(given(&context), 1, "the task waits for the chief's answer");
    assert_eq!(queued(&context, &project), 1);
}

#[test]
fn tasks_working_and_others_open_behind_them_as_in_the_field_hold_the_wake_up_back() {
    let (context, project) = woken_once();
    context.give(project.id, "zeus", "Write the parser");
    working(&context, &project, 1);
    give_behind(&context, &project, &[1]);
    give_behind(&context, &project, &[1, 2]);
    assert_eq!(
        [state(&context, &project, 2), state(&context, &project, 3)],
        ["open", "open"]
    );
    context.advance(HOUR);
    passes(&context);
    assert_eq!(given(&context), 1, "one working, two blocked behind it");
    assert_eq!(queued(&context, &project), 1);
}

#[test]
fn a_task_done_and_undecided_is_the_chiefs_own_and_so_is_one_open_behind_it() {
    let (context, project) = woken_once();
    context.give(project.id, "zeus", "Write the parser");
    working(&context, &project, 1);
    done_and_undecided(&context, &project, 1);
    give_behind(&context, &project, &[1]);
    assert_eq!(state(&context, &project, 2), "open");
    context.advance(MINUTE);
    passes(&context);
    assert_eq!(
        given(&context),
        2,
        "the chief's decision on T-1 is all that holds T-2"
    );
}

#[test]
fn a_board_with_nothing_left_on_it_gets_its_wake_up() {
    let (context, project) = woken_once();
    context.give(project.id, "zeus", "Write the parser");
    working(&context, &project, 1);
    done_and_undecided(&context, &project, 1);
    context
        .ledger
        .borrow_mut()
        .accept_task(project.id, 1, "chief")
        .unwrap();
    assert_eq!(state(&context, &project, 1), "accepted");
    context.advance(MINUTE);
    passes(&context);
    assert_eq!(given(&context), 2);
}

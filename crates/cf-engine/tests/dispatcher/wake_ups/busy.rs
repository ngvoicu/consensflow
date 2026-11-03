//! A wake-up is for a chief with nothing on its way: it is not given while a
//! task is working or queued for its window, waits for an answer that is on its
//! way, is held for its member's quota, or is open behind one of those. A task
//! done and waiting for the chief's decision, a question the chief has not
//! answered, or a board with nothing left on it, is the chief's own, and the
//! chief is woken for it. A wake-up skipped is not owed: the interval counts
//! again from the skip.

use cf_engine::testing::Context;
use cf_ledger::{NewTask, ProjectView, WakeUpUnit};

use crate::fixtures::soon;

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
fn a_question_the_chief_has_not_answered_is_the_chiefs_to_answer_and_gets_a_wake_up() {
    let (context, project) = woken_once();
    context.give(project.id, "zeus", "Write the parser");
    working(&context, &project, 1);
    context.ask(project.id, "zeus", "chief", 1, "Which parser?");
    passes(&context);
    // The chief's turn ends without `cf answer`.
    context.adapter.answer("chief", "Looking.");
    passes(&context);
    assert_eq!(state(&context, &project, 1), "waiting");
    context.advance(MINUTE);
    passes(&context);
    assert_eq!(
        given(&context),
        2,
        "the task waits for the chief's answer: nothing is on its way"
    );
}

#[test]
fn a_question_answered_and_the_answer_not_yet_received_holds_the_wake_up_back() {
    let (context, project) = woken_once();
    context.give(project.id, "zeus", "Write the parser");
    working(&context, &project, 1);
    let question = context.ask(project.id, "zeus", "chief", 1, "Which parser?");
    passes(&context);
    context.adapter.answer("chief", "Looking.");
    passes(&context);
    // The chief answers, and zeus is in its turn: the answer waits for the window.
    context.answer(project.id, question.id, "The new one");
    passes(&context);
    assert_eq!(state(&context, &project, 1), "waiting");
    context.advance(HOUR);
    passes(&context);
    assert_eq!(given(&context), 1, "the answer is on its way");
    assert_eq!(queued(&context, &project), 1);
}

#[test]
fn a_task_held_for_its_members_quota_holds_the_wake_up_back_until_the_hold_ends() {
    let (context, project) = woken_once();
    context.give(project.id, "zeus", "Write the parser");
    working(&context, &project, 1);
    let until = soon(&context, 100);
    context
        .ledger
        .borrow_mut()
        .hold_task(project.id, 1, &until, "out of quota")
        .unwrap();
    assert_eq!(state(&context, &project, 1), "paused");
    context.advance(HOUR);
    passes(&context);
    assert_eq!(given(&context), 1, "it goes on by itself at the reset");
    assert_eq!(queued(&context, &project), 1);
    // The daemon ends the hold without resuming it: the task is the chief's again.
    context
        .ledger
        .borrow_mut()
        .clear_hold(project.id, 1, "the member did not come back")
        .unwrap();
    context.advance(HOUR);
    passes(&context);
    assert_eq!(given(&context), 2);
}

#[test]
fn a_task_held_for_no_time_is_no_work_on_its_way_and_the_chief_is_woken_for_it() {
    // T-816 P2-2: a hold at `9999-12-31T23:59:59.99xZ` (an account unavailable, MUS-23, or a
    // model moved, MUS-24) ends at no time of its own: it waits for the human, and the chief,
    // who is the one to tell them, is woken meanwhile as it is for any task that waits on it.
    for sentinel in ["9999-12-31T23:59:59.998Z", "9999-12-31T23:59:59.999Z"] {
        let (context, project) = woken_once();
        context.give(project.id, "zeus", "Write the parser");
        working(&context, &project, 1);
        context
            .ledger
            .borrow_mut()
            .hold_task(project.id, 1, sentinel, "account unavailable")
            .unwrap();
        assert_eq!(state(&context, &project, 1), "paused");
        context.advance(HOUR);
        passes(&context);
        assert_eq!(given(&context), 2, "{sentinel}: the chief is woken");
    }
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
    give_behind(&context, &project, &[2]);
    assert_eq!(
        [state(&context, &project, 2), state(&context, &project, 3)],
        ["open", "open"]
    );
    context.advance(MINUTE);
    passes(&context);
    assert_eq!(
        given(&context),
        2,
        "the chief's decision on T-1 is all that holds T-2 and T-3 behind it"
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

#[test]
fn a_task_the_chief_holds_itself_is_the_chiefs_own_and_its_turn_that_ended_in_the_middle_of_it_is_woken(
) {
    let (context, project) = woken_once();
    context.create_task(
        project.id,
        NewTask {
            from: "chief".to_owned(),
            to: Some("chief".to_owned()),
            body: "Write the changelog".to_owned(),
            ..NewTask::default()
        },
    );
    working(&context, &project, 1);
    // The chief's turn ends in the middle of its task, on a limit say.
    context.adapter.answer("chief", "Out of quota.");
    passes(&context);
    assert_eq!(state(&context, &project, 1), "working");
    context.advance(MINUTE);
    passes(&context);
    assert_eq!(
        given(&context),
        2,
        "nobody else has the task: the wake-up is how the chief goes on"
    );
}

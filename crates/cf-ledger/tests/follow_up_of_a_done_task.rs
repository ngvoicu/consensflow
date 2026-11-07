//! A follow-up (`cf task add --after T-6`) of a task whose result is in and not
//! yet decided waits for that task to be accepted, as one of a task still at work
//! does (T-1118, from Gabriel's other board: T-8 went straight into the window
//! before the chief had decided T-6, and the CLI said only "continues in @…").
//! The window takes it when T-6 is accepted, in the order the follow-ups were
//! given; a task the chief reopens or cancels instead sends it nowhere before then.

#![allow(clippy::expect_used)]

use cf_ledger::{
    open_ledger, Ledger, NewChief, NewMember, NewProject, NewTask, Options, ProjectView,
};
use cf_proto::ledger::TaskView;
use serde_json::json;

/// Session names in a fixed order, so a test can say `zeus-amber-pine`.
fn options() -> Options {
    let mut names = ["amber-pine", "brisk-birch", "calm-brook"]
        .into_iter()
        .cycle();
    Options {
        names: Box::new(move || names.next().unwrap_or_default().to_owned()),
        ..Options::default()
    }
}

/// A project at /work/app: apollo its chief, zeus its standard worker.
fn project(ledger: &mut Ledger) -> ProjectView {
    ledger
        .create_project(&NewProject {
            directory: "/work/app".into(),
            name: "app".into(),
            chief: NewChief {
                harness: "claude-code".into(),
                model: Some("apollo".into()),
                effort: None,
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
        .expect("a project")
}

/// The chief opens T-`number` for a standard worker, and zeus's new session
/// takes it in: its brief is delivered and its turn ends with `result`, so it is
/// done (the result is in, and the chief has not decided).
fn done(ledger: &mut Ledger, view: &ProjectView, body: &str, result: &str) -> i64 {
    let created = ledger
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
    let member = view
        .participants
        .iter()
        .find(|participant| participant.handle == "zeus")
        .expect("zeus")
        .id;
    let brief = ledger
        .assign_task(1, created.task.number, member)
        .expect("assigned")
        .message
        .expect("its brief")
        .id;
    ledger.begin_delivery(brief).expect("its delivery begins");
    ledger
        .confirm_delivery(brief, Some(&json!({ "item": "i-1" })))
        .expect("its delivery is confirmed");
    ledger
        .record_result(1, created.task.number, result)
        .expect("its result");
    created.task.number
}

/// A follow-up of T-`after`, with `before` the tasks that come after it.
fn follow_up(
    ledger: &mut Ledger,
    after: i64,
    body: &str,
    before: &[u64],
) -> (TaskView, Option<i64>) {
    let created = ledger
        .create_task(
            1,
            &NewTask {
                from: "chief".into(),
                after: Some(after),
                body: body.into(),
                before: before.to_vec(),
                ..NewTask::default()
            },
        )
        .expect("a follow-up");
    (created.task, created.message.map(|message| message.id))
}

/// The window takes the brief of the queued T-`number` in, and its turn ends with `result`.
fn worked(ledger: &mut Ledger, number: i64, result: &str) {
    let brief = ledger
        .task(1, number)
        .expect("a thread")
        .expect("a task")
        .messages
        .iter()
        .find(|message| message.kind == "task" && message.state == "queued")
        .expect("its brief, queued for the window")
        .id;
    ledger.begin_delivery(brief).expect("its delivery begins");
    ledger
        .confirm_delivery(brief, Some(&json!({ "item": format!("i-{number}") })))
        .expect("its delivery is confirmed");
    ledger.record_result(1, number, result).expect("its result");
}

fn task(ledger: &Ledger, number: i64) -> TaskView {
    ledger
        .task(1, number)
        .expect("a thread")
        .expect("a task")
        .task
}

#[test]
fn a_follow_up_of_a_task_with_its_result_in_waits_for_it_to_be_accepted_and_goes_to_its_window_then(
) {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger = open_ledger(&dir.path().join("consensflow.db"), options()).expect("ledger");
    let view = project(&mut ledger);
    let first = done(&mut ledger, &view, "Parser", "Parser done");
    assert_eq!(task(&ledger, first).state, "done");
    let window = task(&ledger, first).assignee.expect("a window");

    let (followed, message) = follow_up(&mut ledger, first, "Now the lexer", &[]);
    assert_eq!(
        (
            followed.state.as_str(),
            followed.blocked_by.clone(),
            message
        ),
        ("open", vec![first], None),
        "it waits on the board for T-1, and nothing went into the window"
    );
    assert_eq!(
        followed.assignee.as_deref(),
        Some(window.as_str()),
        "bound to the window that did T-1"
    );

    // The chief decides: the window takes it then.
    ledger.accept_task(1, first, "chief").expect("accepted");
    let released = task(&ledger, followed.number);
    assert_eq!(released.state, "queued", "released by the acceptance");
    assert_eq!(released.blocked_by, Vec::<i64>::new());
    assert_eq!(released.assignee.as_deref(), Some(window.as_str()));
}

#[test]
fn follow_ups_of_a_task_with_its_result_in_go_in_their_order_and_one_the_chief_put_before_another_goes_first(
) {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger = open_ledger(&dir.path().join("consensflow.db"), options()).expect("ledger");
    let view = project(&mut ledger);
    let first = done(&mut ledger, &view, "Parser", "Parser done");

    let (tests, _) = follow_up(&mut ledger, first, "First the tests", &[]);
    let (docs, _) = follow_up(&mut ledger, first, "Then the docs", &[]);
    // The chief says this one goes ahead of the tests (the rule of 2026-10-15).
    let tests_number = u64::try_from(tests.number).expect("a number");
    let (lexer, _) = follow_up(&mut ledger, first, "Now the lexer", &[tests_number]);
    let states = |ledger: &Ledger| {
        [tests.number, docs.number, lexer.number].map(|number| task(ledger, number).state)
    };
    assert_eq!(
        states(&ledger),
        ["open", "open", "open"],
        "all wait for T-1"
    );
    assert_eq!(
        task(&ledger, tests.number).blocked_by,
        [first, lexer.number],
        "and the tests for the lexer too"
    );

    ledger.accept_task(1, first, "chief").expect("accepted");
    assert_eq!(
        states(&ledger),
        ["open", "queued", "open"],
        "the window has the docs, the first of those ready; one task at a time"
    );
    worked(&mut ledger, docs.number, "Docs done");
    ledger
        .accept_task(1, docs.number, "chief")
        .expect("the docs are accepted");
    assert_eq!(
        states(&ledger),
        ["open", "accepted", "queued"],
        "then the lexer, which the tests wait for"
    );
    worked(&mut ledger, lexer.number, "Lexer done");
    ledger
        .accept_task(1, lexer.number, "chief")
        .expect("the lexer is accepted");
    assert_eq!(
        states(&ledger),
        ["queued", "accepted", "accepted"],
        "and last the tests"
    );
}

#[test]
fn a_task_with_its_result_in_is_reopened_while_a_follow_up_waits_and_the_follow_up_goes_after_it() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger = open_ledger(&dir.path().join("consensflow.db"), options()).expect("ledger");
    let view = project(&mut ledger);
    let first = done(&mut ledger, &view, "Parser", "Parser done");
    let (followed, _) = follow_up(&mut ledger, first, "Now the lexer", &[]);

    // The chief is not satisfied: the window is not busy with the follow-up, which waits.
    ledger
        .reopen_task(1, first, "chief", "Mind the tests")
        .expect("the window takes T-1 back: a follow-up that waits does not make it busy");
    assert_eq!(task(&ledger, first).state, "queued");
    let waiting = task(&ledger, followed.number);
    assert_eq!(
        (waiting.state.as_str(), waiting.blocked_by),
        ("open", vec![first]),
        "the follow-up still waits for T-1, which was not accepted"
    );
}

#[test]
fn a_follow_up_of_an_accepted_task_goes_to_its_window_at_once_as_it_always_did() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger = open_ledger(&dir.path().join("consensflow.db"), options()).expect("ledger");
    let view = project(&mut ledger);
    let first = done(&mut ledger, &view, "Parser", "Parser done");
    ledger.accept_task(1, first, "chief").expect("accepted");

    let (followed, message) = follow_up(&mut ledger, first, "Now the lexer", &[]);
    assert_eq!(followed.state, "queued");
    assert!(message.is_some(), "its brief is queued for the window");
    assert_eq!(followed.blocked_by, Vec::<i64>::new());
}

#[test]
fn a_follow_up_of_an_older_accepted_task_waits_while_another_task_of_the_window_is_done_and_undecided(
) {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger = open_ledger(&dir.path().join("consensflow.db"), options()).expect("ledger");
    let view = project(&mut ledger);
    let first = done(&mut ledger, &view, "Parser", "Parser done");
    ledger.accept_task(1, first, "chief").expect("accepted");
    // T-2 follows T-1, which is accepted: it goes in, and its result is in.
    let (second, message) = follow_up(&mut ledger, first, "Tests", &[]);
    assert_eq!(second.state, "queued");
    assert!(message.is_some());
    worked(&mut ledger, second.number, "Tests done");
    assert_eq!(task(&ledger, second.number).state, "done");
    let window = task(&ledger, second.number).assignee.expect("a window");

    // The window holds a result the chief has not decided: a follow-up of the older,
    // accepted T-1 waits for it too, whichever task it names (T-1118).
    let (third, message) = follow_up(&mut ledger, first, "More", &[]);
    assert_eq!(
        (third.state.as_str(), third.blocked_by.clone(), message),
        ("open", vec![second.number], None),
        "it waits for T-2, and nothing went into the window"
    );
    assert_eq!(third.assignee.as_deref(), Some(window.as_str()));
    // The chief can still send T-2 back to its window.
    let reopened = ledger
        .reopen_task(1, second.number, "chief", "Handle empty input too")
        .expect("the window holds nothing but T-2");
    assert_eq!(reopened.task.state, "queued");

    // T-2 is over, accepted, and the follow-up goes in.
    worked(&mut ledger, second.number, "Tests done again");
    ledger
        .accept_task(1, second.number, "chief")
        .expect("accepted");
    assert_eq!(task(&ledger, third.number).state, "queued");
}

#[test]
fn follow_ups_wait_for_every_task_of_the_window_that_is_done_and_go_in_their_order() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger = open_ledger(&dir.path().join("consensflow.db"), options()).expect("ledger");
    let view = project(&mut ledger);
    let first = done(&mut ledger, &view, "Parser", "Parser done");
    ledger.accept_task(1, first, "chief").expect("accepted");
    let (second, _) = follow_up(&mut ledger, first, "Tests", &[]);
    worked(&mut ledger, second.number, "Tests done");

    let (docs, _) = follow_up(&mut ledger, first, "Docs", &[]);
    let (lexer, _) = follow_up(&mut ledger, first, "Lexer", &[]);
    assert_eq!(
        [docs.number, lexer.number].map(|number| task(&ledger, number).state),
        ["open", "open"]
    );
    ledger
        .accept_task(1, second.number, "chief")
        .expect("accepted");
    assert_eq!(
        [docs.number, lexer.number].map(|number| task(&ledger, number).state),
        ["queued", "open"],
        "the window takes one at a time, the first given first"
    );
    // The first of them has its result in: the next waits for its acceptance too.
    worked(&mut ledger, docs.number, "Docs done");
    assert_eq!(task(&ledger, lexer.number).state, "open");
    ledger
        .accept_task(1, docs.number, "chief")
        .expect("accepted");
    assert_eq!(task(&ledger, lexer.number).state, "queued");
}

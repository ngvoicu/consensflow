//! What waits on the chief for a chief that takes over (`chief_open_work`): a
//! question a member put to it is open only while someone waits for its
//! answer. Field report, 2026-10-21 (`sites`, T-157): a worker asked on T-9,
//! the chief answered and cancelled T-9 at once, and the cancel took the
//! answer's delivery back (the answer read `cancelled`). Four hours later the
//! human switched the chief, and the handoff sent the new chief to answer the
//! question: the list read an answer in no state but `gated` or `cancelled`
//! as none, and never looked at the question's task.
//!
//! The answer's side is here too (T-192): a question on a task that is over takes
//! no answer, whichever way the task ended, as the list says it is not open.

#![allow(clippy::expect_used)]

use cf_ledger::{
    open_ledger, Ledger, NewChief, NewMember, NewProject, NewQuestion, NewTask, Options,
};
use serde_json::json;

/// A project at /work/app: apollo its chief, zeus its worker, whose first
/// session is zeus-amber-pine.
fn project(ledger: &mut Ledger) -> i64 {
    ledger
        .create_project(&NewProject {
            directory: "/work/app".into(),
            name: "app".into(),
            chief: NewChief {
                harness: "claude-code".into(),
                agent: Some("apollo".into()),
            },
            staff: vec![NewMember {
                agent: "zeus".into(),
                harness: "claude-code".into(),
                designer: false,
                roles: vec!["worker".into()],
                tier: "standard".into(),
            }],
            gate: false,
            wake_up: None,
            specs: None,
        })
        .expect("a project")
        .id
}

fn ledger_in(dir: &tempfile::TempDir) -> Ledger {
    let mut names = ["amber-pine", "brisk-birch"].into_iter().cycle();
    open_ledger(
        &dir.path().join("consensflow.db"),
        Options {
            names: Box::new(move || names.next().unwrap_or_default().to_owned()),
            ..Options::default()
        },
    )
    .expect("a ledger")
}

/// A board with T-1 in zeus's window, which has taken its brief in.
fn board(ledger: &mut Ledger) -> i64 {
    let project = project(ledger);
    ledger
        .create_task(
            project,
            &NewTask {
                from: "chief".into(),
                pool: Some("worker".into()),
                tier: Some("standard".into()),
                body: "Parser".into(),
                ..NewTask::default()
            },
        )
        .expect("a task");
    let zeus = ledger
        .project(project)
        .expect("a project")
        .and_then(|found| found.participants.into_iter().find(|p| p.handle == "zeus"))
        .expect("zeus")
        .id;
    let brief = ledger
        .assign_task(project, 1, zeus)
        .expect("assigned")
        .message
        .expect("its brief")
        .id;
    ledger.begin_delivery(brief).expect("the brief goes in");
    ledger
        .confirm_delivery(brief, Some(&json!({ "item": "i-1" })))
        .expect("the brief is confirmed");
    project
}

/// The window asks the chief a question on `task` (none for one on no task) and
/// the chief's window is given it: the question's id.
fn asked(ledger: &mut Ledger, project: i64, task: Option<i64>) -> i64 {
    let question = ledger
        .ask(
            project,
            &NewQuestion {
                from: Some("zeus-amber-pine".into()),
                to: "chief".into(),
                body: Some("Which grammar?".into()),
                task,
                ..NewQuestion::default()
            },
        )
        .expect("a question")
        .id;
    ledger.begin_delivery(question).expect("it goes in");
    ledger
        .confirm_delivery(question, Some(&json!({ "item": "i-2" })))
        .expect("it is confirmed");
    question
}

/// The chief answers `question`; the answer waits in the queue for the window.
fn answered(ledger: &mut Ledger, project: i64, question: i64) -> i64 {
    let chief = chief(ledger, project);
    ledger
        .answer(question, chief, Some(&json!("The new one")), None)
        .expect("the chief answers")
        .id
}

fn chief(ledger: &Ledger, project: i64) -> i64 {
    ledger
        .project(project)
        .expect("a project")
        .and_then(|found| found.participants.into_iter().find(|p| p.handle == "chief"))
        .expect("the chief")
        .id
}

/// The ids of the questions that wait on the chief.
fn open(ledger: &Ledger, project: i64) -> Vec<i64> {
    ledger
        .chief_open_work(project)
        .expect("the open work")
        .questions
        .iter()
        .map(|question| question.id)
        .collect()
}

/// The result of T-1 reaches the chief, as the window's turn ends.
fn result_heard(ledger: &mut Ledger, project: i64) {
    let result = ledger
        .record_result(project, 1, "Parsed")
        .expect("its result")
        .message
        .expect("the result message")
        .id;
    ledger.begin_delivery(result).expect("the result goes in");
    ledger
        .confirm_delivery(result, Some(&json!({ "item": "i-3" })))
        .expect("the result is confirmed");
}

#[test]
fn a_question_on_a_task_cancelled_after_its_answer_is_not_open() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger = ledger_in(&dir);
    let project = board(&mut ledger);
    let question = asked(&mut ledger, project, Some(1));
    let answer = answered(&mut ledger, project, question);
    assert_eq!(open(&ledger, project), [] as [i64; 0], "answered: not open");

    // The cancel takes back the answer's delivery, as in the field (m-2487).
    ledger
        .cancel_task(project, 1, "chief")
        .expect("T-1 cancels");
    let answer = ledger.message(answer).expect("a read").expect("the answer");
    assert_eq!(answer.state, "cancelled");
    assert_eq!(
        open(&ledger, project),
        [] as [i64; 0],
        "nobody waits for the answer of a cancelled task"
    );
    ledger.close().expect("the file is given up");
}

#[test]
fn a_question_on_a_task_cancelled_with_no_answer_is_not_open() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger = ledger_in(&dir);
    let project = board(&mut ledger);
    let question = asked(&mut ledger, project, Some(1));
    assert_eq!(open(&ledger, project), [question], "asked: open");

    ledger
        .cancel_task(project, 1, "chief")
        .expect("T-1 cancels");
    assert_eq!(open(&ledger, project), [] as [i64; 0]);
    ledger.close().expect("the file is given up");
}

#[test]
fn a_question_on_a_task_the_chief_accepted_is_not_open() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger = ledger_in(&dir);
    let project = board(&mut ledger);
    let question = asked(&mut ledger, project, Some(1));
    result_heard(&mut ledger, project);
    assert_eq!(
        open(&ledger, project),
        [question],
        "the task is done and the question was never answered"
    );

    ledger
        .accept_task(project, 1, "chief")
        .expect("T-1 accepted");
    assert_eq!(open(&ledger, project), [] as [i64; 0]);
    ledger.close().expect("the file is given up");
}

#[test]
fn a_question_whose_answer_the_acceptance_took_back_is_not_open() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger = ledger_in(&dir);
    let project = board(&mut ledger);
    let question = asked(&mut ledger, project, Some(1));
    result_heard(&mut ledger, project);
    let answer = answered(&mut ledger, project, question);

    ledger
        .accept_task(project, 1, "chief")
        .expect("T-1 accepted");
    let answer = ledger.message(answer).expect("a read").expect("the answer");
    assert_eq!(answer.state, "cancelled", "{answer:?}");
    assert_eq!(open(&ledger, project), [] as [i64; 0]);
    ledger.close().expect("the file is given up");
}

#[test]
fn a_question_on_a_task_the_daemon_failed_is_not_open() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger = ledger_in(&dir);
    let project = board(&mut ledger);
    asked(&mut ledger, project, Some(1));

    ledger
        .fail_task(project, 1, "its window died")
        .expect("T-1 fails");
    assert_eq!(open(&ledger, project), [] as [i64; 0]);
    ledger.close().expect("the file is given up");
}

#[test]
fn a_question_on_a_task_still_waiting_or_paused_is_open() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger = ledger_in(&dir);
    let project = board(&mut ledger);
    let question = asked(&mut ledger, project, Some(1));
    let task = ledger.task(project, 1).expect("a read").expect("T-1");
    assert_eq!(task.task.state, "waiting", "its question holds it");
    assert_eq!(open(&ledger, project), [question]);

    ledger
        .pause_task(project, 1, Some("chief"), None)
        .expect("T-1 pauses");
    let task = ledger.task(project, 1).expect("a read").expect("T-1");
    assert_eq!(task.task.state, "paused");
    assert_eq!(open(&ledger, project), [question], "a pause is not an end");
    ledger.close().expect("the file is given up");
}

#[test]
fn a_question_whose_answer_was_withdrawn_while_its_task_goes_on_is_still_open() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger = ledger_in(&dir);
    let project = board(&mut ledger);
    let question = asked(&mut ledger, project, Some(1));
    let answer = answered(&mut ledger, project, question);
    ledger
        .cancel_message(answer, "withdrawn by the chief")
        .expect("the answer is withdrawn");

    // The task still waits for an answer: only an ended task makes the cancelled
    // answer none that anyone needs.
    assert_eq!(open(&ledger, project), [question]);
    ledger.close().expect("the file is given up");
}

#[test]
fn a_question_on_no_task_is_open_until_it_is_answered() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger = ledger_in(&dir);
    let project = board(&mut ledger);
    let question = asked(&mut ledger, project, None);
    assert_eq!(open(&ledger, project), [question]);

    answered(&mut ledger, project, question);
    assert_eq!(open(&ledger, project), [] as [i64; 0]);
    ledger.close().expect("the file is given up");
}

#[test]
fn only_the_question_on_the_ended_task_leaves_the_list() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger = ledger_in(&dir);
    let project = board(&mut ledger);
    let ended = asked(&mut ledger, project, Some(1));
    let free = asked(&mut ledger, project, None);
    assert_eq!(open(&ledger, project), [ended, free]);

    ledger
        .cancel_task(project, 1, "chief")
        .expect("T-1 cancels");
    assert_eq!(open(&ledger, project), [free]);
    ledger.close().expect("the file is given up");
}

/// The code and the words of the refusal an answer to the question gets, and
/// whether the question still has no answer on the board after it.
fn refused_answer(ledger: &mut Ledger, project: i64, question: i64) -> (String, String) {
    let chief = chief(ledger, project);
    let refused = ledger
        .answer(question, chief, Some(&json!("The new one")), None)
        .expect_err("the answer is refused");
    let answers = ledger
        .inbox(
            ledger
                .project(project)
                .expect("a project")
                .and_then(|found| {
                    found
                        .participants
                        .into_iter()
                        .find(|p| p.handle == "zeus-amber-pine")
                })
                .expect("the session")
                .id,
            100,
        )
        .expect("an inbox")
        .into_iter()
        .filter(|message| message.kind == "answer")
        .count();
    assert_eq!(answers, 0, "nothing was put to the window");
    (
        refused.code().expect("a code").to_owned(),
        refused.to_string(),
    )
}

#[test]
fn a_question_on_a_cancelled_task_takes_no_answer() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger = ledger_in(&dir);
    let project = board(&mut ledger);
    let question = asked(&mut ledger, project, Some(1));
    ledger
        .cancel_task(project, 1, "chief")
        .expect("T-1 cancels");

    let (code, words) = refused_answer(&mut ledger, project, question);
    assert_eq!(code, "task-cancelled");
    assert!(words.contains("T-1 is cancelled"), "{words}");
    ledger.close().expect("the file is given up");
}

#[test]
fn a_question_on_an_accepted_task_takes_no_answer() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger = ledger_in(&dir);
    let project = board(&mut ledger);
    let question = asked(&mut ledger, project, Some(1));
    result_heard(&mut ledger, project);
    ledger
        .accept_task(project, 1, "chief")
        .expect("T-1 accepted");

    let (code, words) = refused_answer(&mut ledger, project, question);
    assert_eq!(code, "task-accepted");
    assert!(
        words.contains("T-1 is accepted") && words.contains("finished"),
        "{words}"
    );
    ledger.close().expect("the file is given up");
}

#[test]
fn a_question_on_a_failed_task_takes_no_answer_and_says_to_reopen_the_task() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger = ledger_in(&dir);
    let project = board(&mut ledger);
    let question = asked(&mut ledger, project, Some(1));
    ledger
        .fail_task(project, 1, "its window died")
        .expect("T-1 fails");

    let (code, words) = refused_answer(&mut ledger, project, question);
    assert_eq!(code, "task-failed");
    assert!(
        words.contains("reopen T-1 to go on with it"),
        "the refusal says what to do instead: {words}"
    );
    ledger.close().expect("the file is given up");
}

#[test]
fn a_question_on_a_task_that_is_done_but_not_accepted_still_takes_its_answer() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger = ledger_in(&dir);
    let project = board(&mut ledger);
    let question = asked(&mut ledger, project, Some(1));
    result_heard(&mut ledger, project);

    // The result is waiting for the chief's decision: an answer that is late may
    // still be given (the acceptance takes back one that has not reached its window).
    let answer = answered(&mut ledger, project, question);
    assert!(answer > question);
    ledger.close().expect("the file is given up");
}

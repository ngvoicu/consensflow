//! The fact of a hold made for a retry (D3's N35), as the ledger keeps it: one row for each hold,
//! written with the hold, its message once when main's own Resume of that hold is queued, and
//! never changed after. The tries a window has used are read from the message last pasted, never
//! counted.

#![allow(clippy::expect_used)]

use cf_ledger::{
    open_ledger, Ledger, NewChief, NewMember, NewProject, NewTask, Options, ProjectView, RetryFact,
    RESUME_WORDS,
};

fn project(ledger: &mut Ledger) -> ProjectView {
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
            advisor_tool: false,
            prompt_suggestion: false,
            own_hands: false,
        })
        .expect("a project")
}

/// A ledger with T-1 working in the window of the worker, and that window's id and handle.
fn working() -> (tempfile::TempDir, Ledger, ProjectView, i64, String) {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger =
        open_ledger(&dir.path().join("ledger.db"), Options::default()).expect("a ledger");
    let project = project(&mut ledger);
    ledger
        .create_task(
            project.id,
            &NewTask {
                from: "chief".into(),
                pool: Some("worker".into()),
                tier: Some("standard".into()),
                body: "Do the work".into(),
                ..NewTask::default()
            },
        )
        .expect("a task");
    let worker = project
        .participants
        .iter()
        .find(|p| p.role == "worker")
        .expect("a worker")
        .clone();
    ledger
        .sqlite()
        .execute(
            "UPDATE task SET state = 'working', assignee_id = ?1 WHERE number = 1",
            [worker.id],
        )
        .expect("working");
    (dir, ledger, project, worker.id, worker.handle)
}

fn facts(ledger: &Ledger) -> Vec<(i64, String, i64, String, String, Option<i64>)> {
    let mut statement = ledger
        .sqlite()
        .prepare("SELECT task_id, kind, try, said, cause, message_id FROM turn_retry ORDER BY id")
        .expect("a statement");
    statement
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
            ))
        })
        .expect("rows")
        .collect::<Result<_, _>>()
        .expect("facts")
}

const UNTIL: &str = "2099-01-01T00:00:30.000Z";

fn hold(ledger: &mut Ledger, project: &ProjectView, session: i64, which: i64) {
    ledger
        .hold_task_for_retry(
            project.id,
            1,
            UNTIL,
            "its turn failed",
            &RetryFact {
                session,
                kind: "failed",
                which,
                said: "it dropped",
                cause: "k1",
            },
        )
        .expect("held for a retry");
}

#[test]
fn a_hold_for_a_retry_is_the_task_s_hold_and_one_fact_written_with_it() {
    let (_dir, mut ledger, project, session, _) = working();
    hold(&mut ledger, &project, session, 1);
    let thread = ledger.task(project.id, 1).expect("read").expect("a task");
    assert_eq!(thread.task.state, "paused");
    assert_eq!(thread.task.held_until.as_deref(), Some(UNTIL));
    assert_eq!(
        facts(&ledger),
        vec![(
            1,
            "failed".to_owned(),
            1,
            "it dropped".to_owned(),
            "k1".to_owned(),
            None
        )]
    );
    let retry = thread.task.retry.expect("the card reads the retry");
    assert_eq!(
        (retry.kind.as_str(), retry.which, retry.of),
        ("failed", 1, 2)
    );
    assert_eq!(retry.due_at.as_deref(), Some(UNTIL));
}

#[test]
fn a_hold_of_any_other_kind_writes_no_fact() {
    let (_dir, mut ledger, project, _, _) = working();
    ledger
        .hold_task(project.id, 1, UNTIL, "out of quota")
        .expect("held");
    assert!(facts(&ledger).is_empty());
    let thread = ledger.task(project.id, 1).expect("read").expect("a task");
    assert!(thread.task.retry.is_none());
}

#[test]
fn mains_own_resume_of_the_hold_carries_the_fact_once_and_the_card_follows_the_words() {
    let (_dir, mut ledger, project, session, _) = working();
    hold(&mut ledger, &project, session, 1);
    let moved = ledger
        .resume_held_task(project.id, 1, RESUME_WORDS)
        .expect("resumed");
    let message = moved.message.expect("main's words").id;
    assert_eq!(
        facts(&ledger)[0].5,
        Some(message),
        "the fact names its Resume"
    );
    assert_eq!(ledger.turn_retry_tries(message).expect("read"), 1);
    // The Resume is on its way: the card still says the retry, with no time.
    let on_its_way = ledger
        .task(project.id, 1)
        .expect("read")
        .expect("a task")
        .task
        .retry
        .expect("trying again");
    assert_eq!((on_its_way.which, on_its_way.due_at), (1, None));
    // Received: nothing waits.
    ledger
        .sqlite()
        .execute(
            "UPDATE message SET state = 'delivered', delivered_at = '2099-01-01T00:01:00.000Z'
              WHERE id = ?",
            [message],
        )
        .expect("delivered");
    let received = ledger.task(project.id, 1).expect("read").expect("a task");
    assert!(received.task.retry.is_none());
    // The window's tries are read from the message last pasted: this one carried the first.
    assert_eq!(ledger.turn_retry_tries(message).expect("read"), 1);
    assert_eq!(ledger.turn_retry_tries(message + 100).expect("read"), 0);
}

#[test]
fn a_persons_resume_of_the_hold_is_no_retry_and_links_no_fact() {
    let (_dir, mut ledger, project, session, _) = working();
    hold(&mut ledger, &project, session, 1);
    let moved = ledger
        .resume_task(project.id, 1, Some("chief"), "go on with the plan")
        .expect("resumed by the chief");
    let message = moved.message.expect("their words").id;
    assert_eq!(facts(&ledger)[0].5, None, "the fact links nothing");
    assert_eq!(ledger.turn_retry_tries(message).expect("read"), 0);
}

#[test]
fn a_resume_of_another_hold_than_the_fact_s_links_nothing() {
    let (_dir, mut ledger, project, session, _) = working();
    hold(&mut ledger, &project, session, 1);
    // The chief's pause takes the hold over; a later hold of the daemon's is another pause.
    ledger
        .pause_task(project.id, 1, Some("chief"), None)
        .expect("paused");
    let paused = ledger.task(project.id, 1).expect("read").expect("a task");
    assert_eq!(paused.task.held_until, None, "paused with no time");
    assert!(paused.task.retry.is_none(), "no retry waits any more");
    ledger
        .resume_task(project.id, 1, Some("chief"), "go on")
        .expect("resumed");
    ledger
        .sqlite()
        .execute("UPDATE task SET state = 'working' WHERE number = 1", [])
        .expect("working");
    ledger
        .hold_task(project.id, 1, UNTIL, "out of quota")
        .expect("held for its quota");
    let moved = ledger
        .resume_held_task(project.id, 1, RESUME_WORDS)
        .expect("resumed at its time");
    assert!(moved.message.is_some());
    assert_eq!(
        facts(&ledger)[0].5,
        None,
        "the stale fact was not taken for this hold"
    );
}

//! The fact of a hold made for a retry (D3's N35), as the ledger keeps it: one row for each hold,
//! written with the hold, its message once when main's own Resume of that hold is queued, and
//! never changed after. The tries a task has used in a row are a number on the task: raised in the
//! write that queues a numbered Resume, back to zero in the write of an action on the task, of
//! its result, or of a failure 30 minutes after the last numbered Resume was delivered.

#![allow(clippy::expect_used)]

use std::cell::Cell;
use std::rc::Rc;

use cf_base::time::Clock;
use cf_ledger::{
    open_ledger, Ledger, NewChief, NewMember, NewNote, NewProject, NewTask, Options, ProjectView,
    RetryFact, RetryHold, RetryPlan, RESUME_WORDS,
};

/// A clock that says what the test set it to.
struct At(Rc<Cell<i64>>);

impl Clock for At {
    fn now_ms(&mut self) -> i64 {
        self.0.get()
    }
}

const START: i64 = 1_792_058_400_000;
const PLAN: RetryPlan = RetryPlan {
    unit_ms: 30_000,
    second_units: 4,
    renew_after_ms: 30 * 60 * 1000,
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
    let (dir, ledger, project, worker, handle, _) = working_at();
    (dir, ledger, project, worker, handle)
}

/// [`working`], on a clock the test sets.
fn working_at() -> (
    tempfile::TempDir,
    Ledger,
    ProjectView,
    i64,
    String,
    Rc<Cell<i64>>,
) {
    let dir = tempfile::tempdir().expect("a folder");
    let clock = Rc::new(Cell::new(START));
    let mut ledger = open_ledger(
        &dir.path().join("ledger.db"),
        Options {
            clock: Box::new(At(clock.clone())),
            ..Options::default()
        },
    )
    .expect("a ledger");
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
    (dir, ledger, project, worker.id, worker.handle, clock)
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

/// The time a hold made now is due, as the plan says it (30 seconds, then 2 minutes).
fn due(clock: &Cell<i64>, wait_ms: i64) -> String {
    cf_base::time::iso(clock.get() + wait_ms)
}

fn hold(ledger: &mut Ledger, project: &ProjectView, session: i64) -> RetryHold {
    ledger
        .hold_task_for_retry(
            project.id,
            1,
            "its turn failed",
            &RetryFact {
                session,
                kind: "failed",
                said: "it dropped",
                cause: "k1",
            },
            PLAN,
        )
        .expect("held for a retry")
}

fn held(outcome: RetryHold) -> i64 {
    match outcome {
        RetryHold::Held { which, .. } => which,
        RetryHold::Spent => panic!("the tries were spent"),
    }
}

fn tries(ledger: &Ledger) -> i64 {
    ledger
        .sqlite()
        .query_row(
            "SELECT turn_retries FROM task WHERE number = 1",
            [],
            |row| row.get(0),
        )
        .expect("the count")
}

/// The task's turn fails: held for its retry; main's own Resume goes in at its time and is
/// received a second later. Which try that was.
fn failed_and_tried(
    ledger: &mut Ledger,
    project: &ProjectView,
    session: i64,
    clock: &Cell<i64>,
) -> i64 {
    let which = held(hold(ledger, project, session));
    clock.set(clock.get() + if which == 1 { 30_000 } else { 120_000 });
    let message = ledger
        .resume_held_task(project.id, 1, RESUME_WORDS)
        .expect("resumed")
        .message
        .expect("main's words")
        .id;
    clock.set(clock.get() + 1_000);
    ledger
        .sqlite()
        .execute(
            "UPDATE message SET state = 'delivered', delivered_at = ? WHERE id = ?",
            rusqlite::params![cf_base::time::iso(clock.get()), message],
        )
        .expect("delivered");
    ledger
        .sqlite()
        .execute("UPDATE task SET state = 'working' WHERE number = 1", [])
        .expect("working");
    which
}

#[test]
fn a_hold_for_a_retry_is_the_task_s_hold_and_one_fact_written_with_it() {
    let (_dir, mut ledger, project, session, _, clock) = working_at();
    assert_eq!(held(hold(&mut ledger, &project, session)), 1);
    let until = due(&clock, 30_000);
    let thread = ledger.task(project.id, 1).expect("read").expect("a task");
    assert_eq!(thread.task.state, "paused");
    assert_eq!(thread.task.held_until.as_deref(), Some(until.as_str()));
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
    assert_eq!(retry.due_at.as_deref(), Some(until.as_str()));
    assert_eq!(tries(&ledger), 0, "a hold uses no try: its Resume does");
}

#[test]
fn a_hold_of_any_other_kind_writes_no_fact() {
    let (_dir, mut ledger, project, _, _) = working();
    ledger
        .hold_task(project.id, 1, "2099-01-01T00:00:30.000Z", "out of quota")
        .expect("held");
    assert!(facts(&ledger).is_empty());
    let thread = ledger.task(project.id, 1).expect("read").expect("a task");
    assert!(thread.task.retry.is_none());
}

#[test]
fn mains_own_resume_of_the_hold_carries_the_fact_once_and_uses_a_try_in_the_same_write() {
    let (_dir, mut ledger, project, session, _) = working();
    held(hold(&mut ledger, &project, session));
    let moved = ledger
        .resume_held_task(project.id, 1, RESUME_WORDS)
        .expect("resumed");
    let message = moved.message.expect("main's words").id;
    assert_eq!(
        facts(&ledger)[0].5,
        Some(message),
        "the fact names its Resume"
    );
    assert_eq!(tries(&ledger), 1, "the Resume queued is the try used");
    // The Resume is on its way: the card still says the retry, with no time.
    let on_its_way = ledger
        .task(project.id, 1)
        .expect("read")
        .expect("a task")
        .task
        .retry
        .expect("trying again");
    assert_eq!((on_its_way.which, on_its_way.due_at), (1, None));
    // Received: nothing waits, and the count stands.
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
    assert_eq!(tries(&ledger), 1);
}

#[test]
fn a_third_failure_in_a_row_holds_nothing_and_writes_nothing() {
    let (_dir, mut ledger, project, session, _, clock) = working_at();
    assert_eq!(failed_and_tried(&mut ledger, &project, session, &clock), 1);
    assert_eq!(failed_and_tried(&mut ledger, &project, session, &clock), 2);
    assert_eq!(tries(&ledger), 2);
    let facts_before = facts(&ledger);
    assert!(matches!(
        hold(&mut ledger, &project, session),
        RetryHold::Spent
    ));
    assert_eq!(facts(&ledger), facts_before, "no third fact");
    let thread = ledger.task(project.id, 1).expect("read").expect("a task");
    assert_eq!(thread.task.state, "working", "the task was not held");
}

#[test]
fn a_resume_that_ends_a_quota_hold_is_neither_a_numbered_try_nor_a_reset() {
    let (_dir, mut ledger, project, session, _, clock) = working_at();
    failed_and_tried(&mut ledger, &project, session, &clock);
    failed_and_tried(&mut ledger, &project, session, &clock);
    // A two-minute quota hold, and main's Resume at its end.
    ledger
        .hold_task(
            project.id,
            1,
            &cf_base::time::iso(clock.get() + 120_000),
            "out of quota",
        )
        .expect("held for its quota");
    clock.set(clock.get() + 120_001);
    let moved = ledger
        .resume_held_task(project.id, 1, RESUME_WORDS)
        .expect("resumed at its time");
    assert!(moved.message.is_some());
    assert_eq!(
        tries(&ledger),
        2,
        "the count stands across the quota's Resume"
    );
    ledger
        .sqlite()
        .execute("UPDATE task SET state = 'working' WHERE number = 1", [])
        .expect("working");
    assert!(
        matches!(hold(&mut ledger, &project, session), RetryHold::Spent),
        "the next failure fails the task"
    );
}

#[test]
fn a_failure_30_minutes_after_the_last_numbered_resume_was_delivered_begins_again() {
    let (_dir, mut ledger, project, session, _, clock) = working_at();
    failed_and_tried(&mut ledger, &project, session, &clock);
    failed_and_tried(&mut ledger, &project, session, &clock);
    let delivered = clock.get();
    let at = |after: i64| clock.set(delivered + after);
    at(30 * 60_000 - 1);
    assert!(matches!(
        hold(&mut ledger, &project, session),
        RetryHold::Spent
    ));
    assert_eq!(tries(&ledger), 2);
    at(30 * 60_000);
    assert_eq!(held(hold(&mut ledger, &project, session)), 1);
    assert_eq!(tries(&ledger), 0, "reset in the write of the hold");
}

#[test]
fn a_clock_put_back_can_only_delay_the_end_of_the_30_minutes() {
    let (_dir, mut ledger, project, session, _, clock) = working_at();
    failed_and_tried(&mut ledger, &project, session, &clock);
    failed_and_tried(&mut ledger, &project, session, &clock);
    clock.set(clock.get() - 5 * 60_000);
    assert!(matches!(
        hold(&mut ledger, &project, session),
        RetryHold::Spent
    ));
    clock.set(clock.get() - 24 * 3_600_000);
    assert!(matches!(
        hold(&mut ledger, &project, session),
        RetryHold::Spent
    ));
    assert_eq!(tries(&ledger), 2);
}

#[test]
fn the_last_numbered_resume_is_the_last_by_the_ledger_s_order_not_by_its_wall_clock_time() {
    // The clock is put back ten minutes between the two tries: the second Resume was delivered
    // at an earlier wall-clock time than the first. The 30 minutes run from the second.
    let (_dir, mut ledger, project, session, _, clock) = working_at();
    failed_and_tried(&mut ledger, &project, session, &clock);
    clock.set(clock.get() - 10 * 60_000);
    failed_and_tried(&mut ledger, &project, session, &clock);
    clock.set(clock.get() + 30 * 60_000);
    assert_eq!(held(hold(&mut ledger, &project, session)), 1);
    assert_eq!(tries(&ledger), 0);
}

#[test]
fn a_count_with_no_delivered_resume_is_no_time_to_renew_from() {
    let (_dir, mut ledger, project, session, _, clock) = working_at();
    held(hold(&mut ledger, &project, session));
    ledger
        .resume_held_task(project.id, 1, RESUME_WORDS)
        .expect("resumed");
    ledger
        .sqlite()
        .execute("UPDATE task SET state = 'working' WHERE number = 1", [])
        .expect("working");
    clock.set(clock.get() + 3 * 3_600_000);
    assert_eq!(held(hold(&mut ledger, &project, session)), 2);
    assert_eq!(tries(&ledger), 1, "the count stands");
}

#[test]
fn the_30_minutes_run_from_the_last_delivered_resume_not_from_a_later_one_not_delivered() {
    // T-2246 finding 4: a second Resume queued and never delivered never hides the first.
    let (_dir, mut ledger, project, session, _, clock) = working_at();
    failed_and_tried(&mut ledger, &project, session, &clock);
    held(hold(&mut ledger, &project, session));
    ledger
        .resume_held_task(project.id, 1, RESUME_WORDS)
        .expect("resumed");
    assert_eq!(tries(&ledger), 2, "the queued Resume still counts");
    ledger
        .sqlite()
        .execute("UPDATE task SET state = 'working' WHERE number = 1", [])
        .expect("working");
    clock.set(clock.get() + 30 * 60_000);
    assert_eq!(held(hold(&mut ledger, &project, session)), 1);
    assert_eq!(tries(&ledger), 0);
}

#[test]
fn the_write_itself_refuses_a_count_past_two() {
    let (_dir, ledger, _, _, _) = working();
    let set = |count: i64| {
        ledger
            .sqlite()
            .execute("UPDATE task SET turn_retries = ?", [count])
    };
    set(2).expect("two tries");
    let refused = set(3).expect_err("a third is refused");
    assert!(refused.to_string().contains("CHECK"), "{refused}");
    assert_eq!(tries(&ledger), 2);
}

/// Two tries used, then what the test does; the count after it.
fn after(what: impl FnOnce(&mut Ledger, &ProjectView, &str, i64)) -> i64 {
    let (_dir, mut ledger, project, session, worker, clock) = working_at();
    failed_and_tried(&mut ledger, &project, session, &clock);
    failed_and_tried(&mut ledger, &project, session, &clock);
    assert_eq!(tries(&ledger), 2);
    what(&mut ledger, &project, &worker, session);
    tries(&ledger)
}

#[test]
fn a_tell_from_the_chief_to_the_window_begins_the_count_again() {
    assert_eq!(
        after(|ledger, project, worker, _| {
            ledger
                .note(
                    project.id,
                    &NewNote {
                        from: Some("chief".into()),
                        to: worker.into(),
                        body: "mind the edge case".into(),
                        task: Some(1),
                    },
                )
                .expect("told");
        }),
        0
    );
}

#[test]
fn a_tell_from_the_window_to_the_chief_does_not() {
    assert_eq!(
        after(|ledger, project, worker, _| {
            ledger
                .note(
                    project.id,
                    &NewNote {
                        from: Some(worker.into()),
                        to: "chief".into(),
                        body: "still at it".into(),
                        task: Some(1),
                    },
                )
                .expect("told");
        }),
        2
    );
}

#[test]
fn a_persons_resume_with_words_begins_the_count_again() {
    assert_eq!(
        after(|ledger, project, _, _| {
            ledger
                .pause_task(project.id, 1, Some("chief"), None)
                .expect("paused");
            ledger
                .resume_task(project.id, 1, Some("chief"), "go on with the plan")
                .expect("resumed");
        }),
        0
    );
}

#[test]
fn a_persons_resume_that_sends_the_task_back_to_the_board_begins_the_count_again() {
    // The task has no window to go on in: the Resume sends it back to the board, and no
    // message is queued to a window.
    assert_eq!(
        after(|ledger, project, _, _| {
            ledger
                .pause_task(project.id, 1, Some("chief"), None)
                .expect("paused");
            ledger
                .sqlite()
                .execute("UPDATE task SET assignee_id = NULL WHERE number = 1", [])
                .expect("no window");
            ledger
                .resume_task(project.id, 1, Some("chief"), "go on")
                .expect("resumed to the board");
        }),
        0
    );
}

#[test]
fn a_result_begins_the_count_again() {
    assert_eq!(
        after(|ledger, project, _, _| {
            ledger
                .record_result(project.id, 1, "done it")
                .expect("a result");
        }),
        0
    );
}

#[test]
fn a_reopen_begins_the_count_again() {
    assert_eq!(
        after(|ledger, project, _, _| {
            ledger
                .sqlite()
                .execute("UPDATE task SET state = 'failed' WHERE number = 1", [])
                .expect("failed");
            ledger
                .reopen_task(project.id, 1, "chief", "once more, rephrased")
                .expect("reopened");
        }),
        0
    );
}

#[test]
fn a_persons_resume_of_the_hold_is_no_retry_and_links_no_fact() {
    let (_dir, mut ledger, project, session, _) = working();
    held(hold(&mut ledger, &project, session));
    let moved = ledger
        .resume_task(project.id, 1, Some("chief"), "go on with the plan")
        .expect("resumed by the chief");
    assert!(moved.message.is_some());
    assert_eq!(facts(&ledger)[0].5, None, "the fact links nothing");
    assert_eq!(tries(&ledger), 0);
}

#[test]
fn a_resume_of_another_hold_than_the_fact_s_links_nothing() {
    let (_dir, mut ledger, project, session, _, clock) = working_at();
    held(hold(&mut ledger, &project, session));
    // The chief's pause takes the hold over; a later hold of the daemon's is another pause.
    clock.set(clock.get() + 1_000);
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
    clock.set(clock.get() + 1_000);
    ledger
        .hold_task(project.id, 1, "2099-01-01T00:00:30.000Z", "out of quota")
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
    assert_eq!(tries(&ledger), 0, "and it used no try");
}

/// Every row of every table that exists in both, as text: what a migration must keep.
fn dump(raw: &rusqlite::Connection) -> Vec<(String, Vec<String>)> {
    let tables: Vec<String> = raw
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
        .expect("tables")
        .query_map([], |row| row.get(0))
        .expect("names")
        .collect::<Result<_, _>>()
        .expect("table names");
    tables
        .into_iter()
        .filter(|table| table != "turn_retry")
        .map(|table| {
            let columns: Vec<String> = raw
                .prepare(&format!("SELECT name FROM pragma_table_info('{table}') WHERE name NOT IN ('turn_retries', 'retry_hold') ORDER BY cid"))
                .expect("columns")
                .query_map([], |row| row.get(0))
                .expect("names")
                .collect::<Result<_, _>>()
                .expect("column names");
            let select = columns
                .iter()
                .map(|c| format!("quote(\"{c}\")"))
                .collect::<Vec<_>>()
                .join(" || '|' || ");
            let rows = raw
                .prepare(&format!("SELECT {select} FROM \"{table}\" ORDER BY 1"))
                .expect("rows")
                .query_map([], |row| row.get::<_, String>(0))
                .expect("cells")
                .collect::<Result<_, _>>()
                .expect("row text");
            (table, rows)
        })
        .collect()
}

#[test]
fn the_migration_keeps_every_row_and_every_older_task_reads_zero() {
    let (dir, mut ledger, project, _, worker) = working();
    ledger
        .note(
            project.id,
            &NewNote {
                from: Some("chief".into()),
                to: worker,
                body: "a word".into(),
                task: Some(1),
            },
        )
        .expect("told");
    ledger.close().expect("closed");
    let file = dir.path().join("ledger.db");
    // As a build before 0045 left it: no count, no table, the version before.
    let raw = rusqlite::Connection::open(&file).expect("raw");
    cf_ledger::testing::undo_migration_0045(&raw).expect("0045 undone");
    raw.execute_batch(&format!(
        "PRAGMA user_version = {}",
        cf_ledger::SCHEMA_VERSION - 1
    ))
    .expect("version");
    let before = dump(&raw);
    assert!(before
        .iter()
        .any(|(table, rows)| table == "task" && rows.len() == 1));
    assert!(before
        .iter()
        .any(|(table, rows)| table == "message" && !rows.is_empty()));
    drop(raw);
    let ledger = open_ledger(&file, Options::default()).expect("migrated");
    assert_eq!(tries(&ledger), 0, "an older task reads zero");
    assert_eq!(dump(ledger.sqlite()), before, "every row kept");
    assert!(facts(&ledger).is_empty());
}

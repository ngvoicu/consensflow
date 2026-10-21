//! A task other tasks wait for is not cancelled (T-74, 3.0.6; the chief of
//! 2026-10-21, running this project on 3.0.5): `--needs T-n` is released when
//! T-n is *accepted*, so a task waiting on a task that was cancelled read
//! "blocked by T-n" for good, and nothing told anyone. The cancel is refused
//! while an open task waits for the task, in the words of `waited-for`; a
//! ledger 3.0.5 left with such a task tells the chief once, at its opening. A
//! member's removal is not refused for the tasks that wait for what it
//! cancels (the human who clicks Remove means it): the chief is told of each
//! of them in the removal's step (Calliope's review of a996f308, T-124).

#![allow(clippy::expect_used)]

use std::path::Path;

use cf_ledger::{
    open_ledger, Ledger, NewChief, NewMember, NewProject, NewTask, Options, ProjectView,
    TaskCreated,
};
use rusqlite::Connection;

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
fn project(ledger: &mut Ledger, gate: bool) -> ProjectView {
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
            gate,
            wake_up: None,
            specs: None,
        })
        .expect("a project")
}

/// The chief opens a task for a standard worker, waiting for `needs`.
fn open(ledger: &mut Ledger, project: i64, body: &str, needs: &[u64]) -> TaskCreated {
    ledger
        .create_task(
            project,
            &NewTask {
                from: "chief".into(),
                pool: Some("worker".into()),
                tier: Some("standard".into()),
                body: body.into(),
                needs: needs.to_vec(),
                ..NewTask::default()
            },
        )
        .expect("a task")
}

/// The chief gives a task to zeus by name, waiting for `needs`.
fn by_name(ledger: &mut Ledger, project: i64, body: &str, needs: &[u64]) -> TaskCreated {
    ledger
        .create_task(
            project,
            &NewTask {
                from: "chief".into(),
                to: Some("zeus".into()),
                body: body.into(),
                needs: needs.to_vec(),
                ..NewTask::default()
            },
        )
        .expect("a task given by name")
}

/// What cancelling `number` came to: the refusal's code and words, or none.
fn refused(ledger: &mut Ledger, number: i64) -> Option<(&'static str, String)> {
    ledger
        .cancel_task(1, number, "chief")
        .err()
        .map(|error| (error.code().expect("a refusal"), error.to_string()))
}

fn state(ledger: &Ledger, number: i64) -> String {
    ledger
        .task(1, number)
        .expect("a thread")
        .expect("a task")
        .task
        .state
}

#[test]
fn a_task_open_tasks_wait_for_is_not_cancelled_until_they_are() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger = open_ledger(&dir.path().join("consensflow.db"), options()).expect("ledger");
    project(&mut ledger, false);
    open(&mut ledger, 1, "Parser", &[]);
    // For a tier, by name, and paused before it began: each waits for T-1.
    open(&mut ledger, 1, "Docs", &[1]);
    by_name(&mut ledger, 1, "Lexer", &[1]);
    open(&mut ledger, 1, "Tests", &[1]);
    ledger
        .pause_task(1, 4, Some("chief"), None)
        .expect("T-4 is paused");
    // And one that waits for T-2, not for T-1.
    open(&mut ledger, 1, "Release", &[2]);

    assert_eq!(
        refused(&mut ledger, 1),
        Some((
            "waited-for",
            "T-1 cannot be cancelled while T-2, T-3, T-4 wait for it: cancel them first, or let T-1 finish"
                .to_owned()
        ))
    );
    assert_eq!(state(&ledger, 1), "open", "the refusal changed nothing");
    assert_eq!(
        ledger
            .task(1, 2)
            .expect("a thread")
            .expect("T-2")
            .task
            .blocked_by,
        [1]
    );

    // T-2 has a waiter of its own, and one waiter is said in the singular.
    assert_eq!(
        refused(&mut ledger, 2),
        Some((
            "waited-for",
            "T-2 cannot be cancelled while T-5 waits for it: cancel T-5 first, or let T-2 finish"
                .to_owned()
        ))
    );
    for waiter in [5, 4, 3, 2] {
        assert_eq!(refused(&mut ledger, waiter), None, "T-{waiter}");
    }
    assert_eq!(refused(&mut ledger, 1), None, "nothing waits for T-1 now");
    assert_eq!(state(&ledger, 1), "cancelled");
}

#[test]
fn a_task_nothing_waits_for_is_cancelled_as_before() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger = open_ledger(&dir.path().join("consensflow.db"), options()).expect("ledger");
    project(&mut ledger, false);
    open(&mut ledger, 1, "Parser", &[]);
    open(&mut ledger, 1, "Docs", &[1]);
    // T-3 waits for T-2 only: T-1's cancel does not look at it.
    open(&mut ledger, 1, "Lexer", &[2]);
    assert_eq!(
        refused(&mut ledger, 3),
        None,
        "T-3, which nothing waits for"
    );
    assert_eq!(state(&ledger, 3), "cancelled");
}

#[test]
fn the_humans_decline_of_a_brief_other_tasks_wait_for_is_refused_the_same_way() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger = open_ledger(&dir.path().join("consensflow.db"), options()).expect("ledger");
    project(&mut ledger, true);
    let brief = by_name(&mut ledger, 1, "Parser", &[])
        .message
        .expect("its brief")
        .id;
    open(&mut ledger, 1, "Docs", &[1]);

    let declined = ledger
        .decline_message(brief, "human")
        .expect_err("T-2 waits for T-1");
    assert_eq!(declined.code(), Some("waited-for"));
    assert_eq!(
        declined.to_string(),
        "T-1 cannot be cancelled while T-2 waits for it: cancel T-2 first, or pass T-1 on",
        "a brief that never started is passed on, not finished"
    );
    assert_eq!(state(&ledger, 1), "queued", "the task was not cancelled");
    let thread = ledger.task(1, 1).expect("a thread").expect("T-1");
    assert_eq!(
        thread
            .messages
            .iter()
            .map(|m| m.state.as_str())
            .collect::<Vec<_>>(),
        ["gated"],
        "and its brief still waits at the gate"
    );

    assert_eq!(refused(&mut ledger, 2), None);
    ledger
        .decline_message(brief, "human")
        .expect("nothing waits for T-1 now");
    assert_eq!(state(&ledger, 1), "cancelled");
}

/// What the 3.0.5 cancel left: T-1 cancelled with T-2 and T-3 still waiting
/// for it, T-4 waiting for nothing, T-5 cancelled itself while it waited for
/// T-1, and T-6 waiting for T-4.
fn stuck(file: &Path) {
    let mut ledger = open_ledger(file, options()).expect("a ledger");
    project(&mut ledger, false);
    open(&mut ledger, 1, "Parser", &[]);
    open(&mut ledger, 1, "Docs", &[1]);
    by_name(&mut ledger, 1, "Lexer", &[1]);
    open(&mut ledger, 1, "Tests", &[]);
    open(&mut ledger, 1, "Bench", &[1]);
    ledger.cancel_task(1, 5, "chief").expect("T-5");
    open(&mut ledger, 1, "Release", &[4]);
    ledger.close().expect("the file is given up");
    // What 3.0.5's cancel did: the task cancelled, the waiters left as they were.
    let db = Connection::open(file).expect("the file");
    db.execute("UPDATE task SET state = 'cancelled' WHERE number = 1", [])
        .expect("T-1 cancelled");
}

/// The chief's notes about tasks, as `T-n: words`, oldest first.
fn notes(ledger: &Ledger, chief: i64) -> Vec<String> {
    let mut found: Vec<_> = ledger
        .inbox(chief, 100)
        .expect("the inbox")
        .into_iter()
        .filter(|message| message.kind == "note")
        .collect();
    found.sort_by_key(|message| message.id);
    found
        .iter()
        .filter_map(|message| {
            let task = message.task_number?;
            Some(format!("T-{task}: {}", message.body))
        })
        .collect()
}

#[test]
fn a_ledger_that_holds_a_task_waiting_for_a_cancelled_one_tells_the_chief_once() {
    let dir = tempfile::tempdir().expect("a folder");
    let file = dir.path().join("consensflow.db");
    stuck(&file);

    let ledger = open_ledger(&file, options()).expect("the ledger, opened");
    let chief = ledger
        .project(1)
        .expect("a project")
        .expect("project 1")
        .participants
        .iter()
        .find(|participant| participant.role == "chief")
        .expect("its chief")
        .id;
    let expected = [
        "T-2: T-2 waits on cancelled T-1 and will never start.",
        "T-3: T-3 waits on cancelled T-1 and will never start.",
    ];
    assert_eq!(notes(&ledger, chief), expected);
    drop(ledger);

    // The next opening says nothing more of them.
    let ledger = open_ledger(&file, options()).expect("opened again");
    assert_eq!(notes(&ledger, chief), expected);
}

#[test]
fn a_ledger_with_no_task_waiting_for_a_cancelled_one_tells_the_chief_nothing() {
    let dir = tempfile::tempdir().expect("a folder");
    let file = dir.path().join("consensflow.db");
    let mut ledger = open_ledger(&file, options()).expect("a ledger");
    project(&mut ledger, false);
    open(&mut ledger, 1, "Parser", &[]);
    open(&mut ledger, 1, "Docs", &[1]);
    ledger.cancel_task(1, 2, "chief").expect("the waiter first");
    ledger.cancel_task(1, 1, "chief").expect("then T-1");
    ledger.close().expect("the file is given up");

    let ledger = open_ledger(&file, options()).expect("opened again");
    let chief = ledger
        .project(1)
        .expect("a project")
        .expect("project 1")
        .participants[1]
        .id;
    assert_eq!(notes(&ledger, chief), Vec::<String>::new());
}

#[test]
fn a_member_removed_with_a_task_others_wait_for_tells_the_chief_in_that_step_and_the_next_opening_says_nothing_more(
) {
    let dir = tempfile::tempdir().expect("a folder");
    let file = dir.path().join("consensflow.db");
    let mut ledger = open_ledger(&file, options()).expect("a ledger");
    let view = project(&mut ledger, false);
    let chief = view
        .participants
        .iter()
        .find(|participant| participant.role == "chief")
        .expect("its chief")
        .id;
    // T-1 is zeus's, and T-2 waits for it.
    by_name(&mut ledger, 1, "Parser", &[]);
    open(&mut ledger, 1, "Docs", &[1]);

    // The removal is not refused: the human who clicks Remove means it.
    let removed = ledger.remove_member(1, "zeus").expect("zeus leaves");
    assert_eq!(removed.cancelled, [1]);
    assert_eq!(state(&ledger, 1), "cancelled");
    assert_eq!(state(&ledger, 2), "open");
    // And the chief is told in that step, not at the next opening.
    let expected = ["T-2: T-2 waits on cancelled T-1 and will never start."];
    assert_eq!(notes(&ledger, chief), expected);
    ledger.close().expect("the file is given up");

    // The opening does not tell it again.
    let ledger = open_ledger(&file, options()).expect("opened again");
    assert_eq!(notes(&ledger, chief), expected);
}

#[test]
fn a_member_removed_with_a_task_nothing_waits_for_tells_the_chief_nothing_of_waiters() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger = open_ledger(&dir.path().join("consensflow.db"), options()).expect("ledger");
    let view = project(&mut ledger, false);
    let chief = view.participants[1].id;
    by_name(&mut ledger, 1, "Parser", &[]);
    open(&mut ledger, 1, "Docs", &[]);
    let removed = ledger.remove_member(1, "zeus").expect("zeus leaves");
    assert_eq!(removed.cancelled, [1]);
    assert_eq!(notes(&ledger, chief), Vec::<String>::new());
}

/// A waiter of several of the leaving member's tasks is told once, naming each
/// (Calliope's check T-126 of 9a7cc704): the removal cancels all its tasks and
/// then tells, so the note is written when every need of the waiter is
/// cancelled. Telling right after each cancel would name T-1 only, write the
/// mark, and leave T-2 out for good.
#[test]
fn a_waiter_of_several_tasks_a_removed_member_held_is_told_to_the_chief_once_naming_each() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger = open_ledger(&dir.path().join("consensflow.db"), options()).expect("ledger");
    let view = project(&mut ledger, false);
    let chief = view.participants[1].id;
    // T-1 and T-2 are zeus's, and T-3 waits for both.
    by_name(&mut ledger, 1, "Parser", &[]);
    by_name(&mut ledger, 1, "Lexer", &[]);
    open(&mut ledger, 1, "Docs", &[1, 2]);

    let removed = ledger.remove_member(1, "zeus").expect("zeus leaves");
    assert_eq!(removed.cancelled, [1, 2]);
    assert_eq!(state(&ledger, 3), "open");
    assert_eq!(
        notes(&ledger, chief),
        ["T-3: T-3 waits on cancelled T-1, T-2 and will never start."],
        "one note, naming both"
    );
}

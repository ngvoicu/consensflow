//! A rollback that fails is said, and a ledger left in a transaction refuses
//! (CLN-65). The rollback is refused by `refuse_next_rollback`, which stands for
//! SQLite's own refusal (T-2028 made one with an authorizer): no disk fault.

#![allow(clippy::expect_used)]

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use cf_ledger::{
    open_ledger, refuse_next_rollback, Ledger, NewChief, NewMember, NewNote, NewProject, NewTask,
    Options,
};

fn project(ledger: &mut Ledger) {
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
        .expect("a project");
}

fn task(project: i64, body: &str) -> NewTask {
    let _ = project;
    NewTask {
        from: "chief".into(),
        pool: Some("worker".into()),
        tier: Some("standard".into()),
        body: body.into(),
        ..NewTask::default()
    }
}

fn tasks(ledger: &Ledger) -> i64 {
    ledger
        .sqlite()
        .query_row("SELECT COUNT(*) FROM task", [], |row| row.get(0))
        .expect("a count")
}

fn tasks_in(file: &Path) -> i64 {
    let ledger = open_ledger(file, Options::default()).expect("reopened");
    tasks(&ledger)
}

#[test]
fn after_a_rollback_that_did_not_take_no_later_operation_seems_to_succeed() {
    let home = tempfile::tempdir().expect("a home");
    let file = home.path().join("consensflow.db");
    let mut ledger = open_ledger(&file, Options::default()).expect("a ledger");
    project(&mut ledger);
    let told = Rc::new(RefCell::new(Vec::new()));
    let hook = Rc::clone(&told);
    ledger.on_broken(Box::new(move |cause| {
        hook.borrow_mut().push(cause.to_owned())
    }));

    // An operation that fails on a project that is not there, and a rollback refused.
    refuse_next_rollback();
    let failed = ledger
        .create_task(99, &task(99, "nowhere"))
        .expect_err("no such project");
    let said = failed.to_string();
    assert!(
        said.contains("no project 99"),
        "the failure stays first: {said}"
    );
    assert!(said.contains("its rollback failed too"), "{said}");
    assert!(ledger.broken().is_some());
    assert_eq!(told.borrow().len(), 1);

    // On main the next insert reported success and showed its row, and was never committed.
    let later = ledger
        .create_task(1, &task(1, "later"))
        .expect_err("refused");
    assert_eq!(later.code(), Some("ledger-broken"), "{later}");
    assert!(
        ledger
            .sqlite()
            .query_row("SELECT COUNT(*) FROM task", [], |row| row.get::<_, i64>(0))
            .is_err(),
        "no row shows"
    );
    drop(ledger);
    assert_eq!(tasks_in(&file), 0, "a reopening holds nothing");
}

#[test]
fn a_ledger_whose_rollback_took_goes_on() {
    let home = tempfile::tempdir().expect("a home");
    let file = home.path().join("consensflow.db");
    let mut ledger = open_ledger(&file, Options::default()).expect("a ledger");
    project(&mut ledger);
    let failed = ledger
        .create_task(99, &task(99, "nowhere"))
        .expect_err("no such project");
    assert_eq!(failed.code(), Some("unknown-project"));
    assert!(ledger.broken().is_none());
    ledger.create_task(1, &task(1, "fine")).expect("a task");
    drop(ledger);
    assert_eq!(tasks_in(&file), 1);
}

#[test]
fn a_migration_whose_rollback_is_refused_says_so_after_its_own_failure() {
    let db = rusqlite::Connection::open_in_memory().expect("a connection");
    let migrations = ["CREATE TABLE a (x)", "THIS IS NOT SQL"];
    refuse_next_rollback();
    let failed = cf_ledger::migrate(&db, &migrations).expect_err("the second fails");
    let said = failed.to_string();
    assert!(
        said.contains("syntax error"),
        "the failure stays first: {said}"
    );
    assert!(said.contains("its rollback failed too"), "{said}");
    assert!(!db.is_autocommit(), "the transaction is open still");
}

#[test]
fn a_note_is_not_withdrawn_after_the_break_and_nothing_of_the_dirty_transaction_is_read() {
    let home = tempfile::tempdir().expect("a home");
    let file = home.path().join("consensflow.db");
    let mut ledger = open_ledger(&file, Options::default()).expect("a ledger");
    project(&mut ledger);
    let note = ledger
        .note(
            1,
            &NewNote {
                from: None,
                to: "chief".into(),
                body: "a note of ConsensFlow's".into(),
                task: None,
            },
        )
        .expect("a queued note");
    refuse_next_rollback();
    ledger
        .create_task(99, &task(99, "nowhere"))
        .expect_err("no such project");
    assert!(ledger.broken().is_some());

    // Resume's way of taking a note back: refused, not reported as done.
    let withdrawn = ledger
        .withdraw_note(&note, "its window is gone")
        .expect_err("refused");
    assert_eq!(withdrawn.code(), Some("ledger-broken"), "{withdrawn}");
    // And a read shows nothing of what the open transaction holds.
    assert!(ledger.message(note.id).is_err());
    drop(ledger);
    let reopened = open_ledger(&file, Options::default()).expect("reopened");
    let state: String = reopened
        .sqlite()
        .query_row("SELECT state FROM message WHERE id = ?", [note.id], |row| {
            row.get(0)
        })
        .expect("the note");
    assert_eq!(state, "queued", "the committed note is as it was");
}

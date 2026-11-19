//! Plans commit with their retry keys and keep each task's ordinary approval.

use super::fixture::{gated_world, world};
use cf_ledger::{NewTask, PlanTask};

fn section(name: &str) -> PlanTask {
    PlanTask {
        name: name.into(),
        task: NewTask {
            from: "chief".into(),
            pool: Some("worker".into()),
            tier: Some("standard".into()),
            body: format!("{name} brief"),
            ..NewTask::default()
        },
        needs: Vec::new(),
        before: Vec::new(),
    }
}

#[test]
fn a_plan_key_survives_reopening_and_cannot_be_reused_for_other_work() {
    let mut w = world();
    let mut second = section("second");
    second.needs.push("first".into());
    let tasks = [section("first"), second];
    let first = w
        .ledger
        .create_plan(w.project, &tasks, Some("key"), "hash one")
        .unwrap();
    assert_eq!(first.tasks.len(), 2);
    w.edit(|_| {});
    let retry = w
        .ledger
        .create_plan(w.project, &tasks, Some("key"), "hash one")
        .unwrap();
    assert!(retry.repeated);
    assert_eq!(retry.tasks, first.tasks);
    assert_eq!(w.ledger.board(w.project).unwrap().open.len(), 2);
    let refusal = w
        .ledger
        .create_plan(w.project, &tasks, Some("key"), "hash two")
        .unwrap_err();
    assert_eq!(refusal.code(), Some("key-used"));
    let mut different = tasks.clone();
    different[0].task.body = "different work".into();
    let refusal = w
        .ledger
        .create_plan(w.project, &different, Some("key"), "hash one")
        .unwrap_err();
    assert_eq!(refusal.code(), Some("key-used"));
}

#[test]
fn each_assigned_brief_of_a_plan_waits_for_its_own_approval() {
    let mut w = gated_world();
    let plan = w
        .ledger
        .create_plan(
            w.project,
            &[section("first"), section("second")],
            None,
            "hash",
        )
        .unwrap();
    let first = w.assign(plan.tasks[0].1, "zeus");
    let second = w.assign(plan.tasks[1].1, "diana");
    let first = first.message.unwrap();
    let second = second.message.unwrap();
    assert_ne!(first.id, second.id);
    assert_eq!(first.state, "gated");
    assert_eq!(second.state, "gated");
}

#[test]
fn a_later_refusal_leaves_no_task_or_key_and_the_key_can_be_tried_again() {
    let mut w = world();
    let mut invalid = section("second");
    invalid.task.pool = Some("advisor".into());
    let failure = w
        .ledger
        .create_plan(w.project, &[section("first"), invalid], Some("key"), "bad")
        .unwrap_err();
    assert!(failure.to_string().contains("second"), "{failure}");
    assert!(w.ledger.board(w.project).unwrap().open.is_empty());
    let made = w
        .ledger
        .create_plan(w.project, &[section("first")], Some("key"), "good")
        .unwrap();
    assert_eq!(made.tasks.len(), 1);
}

#[test]
fn migration_0040_keeps_an_older_ledger_and_an_older_build_refuses_it() {
    let mut w = world();
    let made = w
        .ledger
        .create_task(w.project, &section("first").task)
        .unwrap();
    w.edit(|db| {
        cf_ledger::testing::undo_migration_0040(db).unwrap();
        db.execute_batch("PRAGMA user_version = 39").unwrap();
        cf_ledger::migrate(db, &cf_ledger::MIGRATIONS[..39]).unwrap();
    });
    assert_eq!(
        w.ledger.board(w.project).unwrap().open[0].number,
        made.task.number
    );
    w.ledger.close_in_place().unwrap();
    let db = rusqlite::Connection::open(w.dir.path().join("consensflow.db")).unwrap();
    let version: i64 = db
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, 40);
    let error = cf_ledger::migrate(&db, &cf_ledger::MIGRATIONS[..39]).unwrap_err();
    assert_eq!(error.code(), Some("ledger-newer"));
}

#[test]
fn a_refused_plan_does_not_trace_events_that_were_rolled_back() {
    use std::{cell::RefCell, rc::Rc};
    let events = Rc::new(RefCell::new(Vec::new()));
    let out = Rc::clone(&events);
    let mut w = super::fixture::World::traced(Box::new(move |event| {
        out.borrow_mut().push(event.clone());
    }));
    events.borrow_mut().clear();
    let mut invalid = section("second");
    invalid.task.pool = Some("advisor".into());
    assert!(w
        .ledger
        .create_plan(w.project, &[section("first"), invalid], None, "")
        .is_err());
    assert!(
        events.borrow().is_empty(),
        "rolled back events must not reach the trace"
    );
    assert!(w.ledger.board(w.project).unwrap().open.is_empty());
    let made = w
        .ledger
        .create_plan(w.project, &[section("first")], None, "")
        .unwrap();
    assert!(
        !events.borrow().is_empty(),
        "committed events reach the trace"
    );
    events.borrow_mut().clear();
    assert!(w
        .ledger
        .decide_tasks(w.project, &[made.tasks[0].1, 999], "chief", "cancel", "")
        .is_err());
    assert!(
        events.borrow().is_empty(),
        "a refused decision leaves no trace of a cancel"
    );
    assert_eq!(w.ledger.board(w.project).unwrap().open.len(), 1);
}

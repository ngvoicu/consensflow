//! A batch that fails tells the trace nothing, even where SQLite ended its transaction under it.

use std::{cell::RefCell, rc::Rc};

use super::fixture::World;

const TRIGGER: &str =
    "CREATE TEMP TRIGGER test_end_write BEFORE UPDATE OF ended_at ON question_holding
    WHEN OLD.ended_at=-1 AND NEW.ended_at>0 BEGIN SELECT RAISE(%) ; END;";

/// A traced world with one task, assigned to zeus and delivered, and the events told so far
/// cleared: the number of the task and the events told after.
fn held() -> (World, i64, Rc<RefCell<Vec<cf_ledger::Event>>>) {
    let events = Rc::new(RefCell::new(Vec::new()));
    let out = Rc::clone(&events);
    let mut w = World::traced(Box::new(move |event| out.borrow_mut().push(event.clone())));
    let created = w.give("zeus", "Parser");
    let (task, brief) = (created.task.number, created.message.unwrap().id);
    w.deliver(brief);
    events.borrow_mut().clear();
    (w, task, events)
}

fn told_states(events: &Rc<RefCell<Vec<cf_ledger::Event>>>) -> usize {
    events
        .borrow()
        .iter()
        .filter(|event| event.kind == "task.state")
        .count()
}

#[test]
fn an_ordinary_batch_cancel_tells_the_trace_once() {
    let (mut w, task, events) = held();
    w.ledger
        .decide_tasks(w.project, &[task], "chief", "cancel", "")
        .unwrap();
    assert_eq!(told_states(&events), 1);
}

#[test]
fn a_batch_cancel_whose_end_date_write_aborts_tells_the_trace_nothing() {
    let (mut w, task, events) = held();
    w.ledger
        .sqlite()
        .execute_batch(&TRIGGER.replace('%', "ABORT,'test end write failed'"))
        .unwrap();
    assert!(w
        .ledger
        .decide_tasks(w.project, &[task], "chief", "cancel", "")
        .is_err());
    assert_eq!(told_states(&events), 0);
}

#[test]
fn a_batch_cancel_whose_end_date_write_rolls_the_transaction_back_tells_the_trace_nothing() {
    let (mut w, task, events) = held();
    w.ledger
        .sqlite()
        .execute_batch(&TRIGGER.replace('%', "ROLLBACK,'test end write failed'"))
        .unwrap();
    assert!(w
        .ledger
        .decide_tasks(w.project, &[task], "chief", "cancel", "")
        .is_err());
    assert_eq!(
        told_states(&events),
        0,
        "a cancel that failed reached the trace"
    );
}

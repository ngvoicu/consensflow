//! Work that throws before its first await lets go of its participant as
//! work that throws after it does (`begin`): its next step runs, the failure
//! is written down, and an operation waiting on it ends.

use std::cell::Cell;
use std::rc::Rc;

use cf_engine::testing::Context;
use cf_ledger::{LedgerError, NewTask};

use crate::fixtures::Tiers;
use crate::traces::held_to;

const SUITES: &[&str] = &["work that throws before its first await"];

#[test]
fn lets_go_of_its_participant_its_next_step_runs_the_failure_is_written_down_and_an_operation_waiting_on_it_ends(
) {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    context.adapter.answer("zeus", "Parser done");
    context.pass().unwrap();
    assert_eq!(
        tiers.task(1).task.state,
        "done",
        "the session's window closed with its task"
    );
    // One read of the ledger fails, in the work that opens the session's window.
    let fail = Rc::new(Cell::new(true));
    let failing = Rc::clone(&fail);
    context.ledger.borrow_mut().watch(Box::new(move |read, _| {
        if read == "projects" && failing.replace(false) {
            return Err(LedgerError::refused(
                "ledger-failed",
                "the ledger could not be read",
            ));
        }
        Ok(())
    }));
    context
        .open_window(tiers.project.id, "zeus-amber-pine")
        .unwrap();
    context.create_task(
        tiers.project.id,
        NewTask {
            from: "chief".to_owned(),
            after: Some(1),
            body: "Now the lexer".to_owned(),
            ..NewTask::default()
        },
    );
    let launches = context.adapter.prepared().len();
    context.pass().unwrap();
    let handles: Vec<_> = context.adapter.prepared()[launches..]
        .iter()
        .map(|request| request["participant"]["handle"].clone())
        .collect();
    assert_eq!(handles, ["zeus-amber-pine"], "its next step ran");
    // Written down: the test's own log keeps it, so nothing is failed unseen.
    let written: Vec<_> = std::mem::take(&mut *context.log.failures.borrow_mut());
    assert_eq!(written, ["the ledger could not be read"]);
    assert_eq!(
        context
            .close_project(tiers.project.id)
            .unwrap()
            .map(|project| project.state),
        Some("suspended".to_owned())
    );
    held_to(
        context.close(),
        SUITES,
        "lets go of its participant: its next step runs, the failure is written down, and an operation waiting on it ends",
    );
}

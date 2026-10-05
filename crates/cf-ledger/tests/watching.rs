//! What the engine's tests watch of the ledger (`test-support`): each read of
//! `projects` and `next_delivery`, and each write of `pause_task`, is told
//! before it is made, with the id it was given, and a call the watcher fails
//! fails.

#![cfg(feature = "test-support")]
// A test's own scaffolding expects, as the tests do.
#![allow(clippy::expect_used)]

use std::cell::RefCell;
use std::rc::Rc;

use cf_ledger::{open_ledger, LedgerError, NewChief, NewProject, Options};

fn project() -> NewProject {
    NewProject {
        directory: "/work/app".into(),
        name: "app".into(),
        chief: NewChief {
            harness: "claude-code".into(),
            agent: None,
        },
        staff: Vec::new(),
        gate: false,
    }
}

#[test]
fn a_watcher_is_told_each_read_it_watches_with_its_id_and_may_fail_one() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger =
        open_ledger(&dir.path().join("consensflow.db"), Options::default()).expect("a ledger");
    let created = ledger.create_project(&project()).expect("a project");
    let told = Rc::new(RefCell::new(Vec::new()));
    let (heard, failing) = (Rc::clone(&told), Rc::new(RefCell::new(true)));
    ledger.watch(Box::new(move |read, id| {
        heard.borrow_mut().push((read.to_owned(), id));
        if read == "projects" && failing.replace(false) {
            return Err(LedgerError::refused("watched", "the read was failed"));
        }
        Ok(())
    }));

    let refused = ledger.projects().expect_err("the first read fails");
    assert_eq!(refused.code(), Some("watched"));
    assert_eq!(ledger.projects().expect("the next reads").len(), 1);
    let chief = created.participants[1].id;
    ledger.next_delivery(chief).expect("a read");
    // What is not watched is not told.
    ledger.project(created.id).expect("a read");
    assert_eq!(
        *told.borrow(),
        [
            ("projects".to_owned(), None),
            ("projects".to_owned(), None),
            ("next_delivery".to_owned(), Some(chief)),
        ]
    );
}

#[test]
fn a_watcher_is_told_a_pause_by_its_tasks_number_before_it_is_written_and_may_fail_it() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger =
        open_ledger(&dir.path().join("consensflow.db"), Options::default()).expect("a ledger");
    let created = ledger.create_project(&project()).expect("a project");
    let told = Rc::new(RefCell::new(Vec::new()));
    let heard = Rc::clone(&told);
    ledger.watch(Box::new(move |call, id| {
        heard.borrow_mut().push((call.to_owned(), id));
        if call == "pause_task" {
            return Err(LedgerError::refused("watched", "the write was failed"));
        }
        Ok(())
    }));

    // Failed before the ledger looks for the task, which this one has not.
    let refused = ledger
        .pause_task(created.id, 7, None, None)
        .expect_err("the write fails");
    assert_eq!(refused.code(), Some("watched"));
    assert_eq!(*told.borrow(), [("pause_task".to_owned(), Some(7))]);
}

//! What the dispatcher's own shape must keep.

use std::cell::RefCell;
use std::rc::Rc;

use crate::testing::Context;
use crate::{Operation, SwitchTo, SwitchWhen};

#[test]
fn the_operations_the_engines_own_work_calls_are_told_where_they_begin() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    let told = Rc::new(RefCell::new(Vec::new()));
    let heard = Rc::clone(&told);
    context.dispatcher.on_operation(Rc::new(move |operation| {
        heard.borrow_mut().push(operation.clone());
    }));
    // A restart resumes the project open before it, and the pane host says a window ended.
    context.ledger.borrow_mut().suspend_for_restart().unwrap();
    context.resume_after_restart().unwrap();
    let chief = context.host.last("chief").unwrap().pane;
    context.exit("chief");
    assert_eq!(
        *told.borrow(),
        [
            Operation::ResumeProject(project.id),
            Operation::PaneExited(chief)
        ]
    );
}

#[test]
fn an_operations_work_is_small_where_it_is_made_its_steps_and_holds_on_the_heap() {
    // A step holds every wait below it: built inline and moved by value
    // through the hold, a pass's was 115 KB and filled a test's 2 MB stack.
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    let dispatcher = Rc::clone(&context.dispatcher);
    let to = SwitchTo {
        harness: "claude-code".to_owned(),
        agent: "apollo".to_owned(),
    };
    let sizes = [
        ("pass", std::mem::size_of_val(&dispatcher.pass())),
        (
            "switch_chief",
            std::mem::size_of_val(&dispatcher.switch_chief(project.id, to, SwitchWhen::Now, false)),
        ),
        (
            "close_project",
            std::mem::size_of_val(&dispatcher.close_project(project.id)),
        ),
    ];
    for (operation, size) in sizes {
        assert!(size < 8 * 1024, "{operation}'s future is {size} bytes");
    }
}

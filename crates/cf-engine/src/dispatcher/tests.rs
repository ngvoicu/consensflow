//! What the dispatcher's own shape must keep.

use std::rc::Rc;

use crate::testing::Context;
use crate::{SwitchTo, SwitchWhen};

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

//! A window that is not ready for a paste: the trace says so once, and the
//! message goes in when the window is (`describe('a window that is not ready
//! for a paste')`).

use std::cell::Cell;
use std::rc::Rc;

use cf_engine::testing::Context;
use cf_harness::contract::{Held, Readiness};
use cf_proto::trace::{Traced, WindowEvent};

use crate::fixtures::{assert_match, Tiers};
use crate::traces::held_to;

const SUITES: &[&str] = &["a window that is not ready for a paste"];

#[test]
fn says_so_once_in_the_trace_and_delivers_when_the_window_is_ready() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus", "diana"]);
    // A window that says no more than "not yet" until it is ready.
    let ready = Rc::new(Cell::new(false));
    let asked = Rc::clone(&ready);
    *context.adapter.ready.borrow_mut() = Some(Rc::new(move || {
        Ok(if asked.get() {
            Readiness::Ready
        } else {
            Readiness::Held(Held::Unsaid)
        })
    }));
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working");
    context.adapter.answer("zeus", "Parser done.");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "done");
    let chief = tiers.id("chief");
    let result = || {
        context
            .inbox(chief)
            .into_iter()
            .find(|message| message.kind == "result")
            .unwrap()
    };
    assert_eq!(
        result().state,
        "queued",
        "the chief is typing: the result waits"
    );
    let waiting = result().id;
    let held: Vec<(Option<String>, Option<i64>, String)> = context
        .trace
        .lines
        .borrow()
        .iter()
        .filter_map(|line| match &line.what {
            Traced::Window {
                project,
                participant,
                event: WindowEvent::DeliveryHeld { message, reason },
            } if *message == waiting => Some((participant.clone(), *project, reason.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(held.len(), 1, "said once, not every pass");
    assert_eq!((held[0].0.as_deref(), held[0].1), (Some("chief"), Some(1)));
    assert_match(&held[0].2, "not ready for a paste");
    ready.set(true);
    context.pass().unwrap();
    assert_ne!(
        result().state,
        "queued",
        "delivered once the window is ready"
    );
    held_to(
        context.close(),
        SUITES,
        "says so once in the trace, and delivers when the window is ready",
    );
}

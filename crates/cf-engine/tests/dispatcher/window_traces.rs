//! What the dispatcher tells of its windows: its transcript listeners when
//! one wrote more, and its trace each change of a window's activity
//! (`describe('the dispatcher traces what its windows do')`).

use std::cell::Cell;
use std::rc::Rc;

use cf_engine::testing::Context;
use cf_proto::trace::{TraceLine, Traced, WindowEvent};

use crate::fixtures::Tiers;
use crate::traces::held_to;

const SUITES: &[&str] = &["the dispatcher traces what its windows do"];

#[test]
fn tells_its_transcript_listeners_when_a_window_wrote_more_and_not_after_a_look_that_found_nothing_new(
) {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus", "diana"]);
    let told = Rc::new(Cell::new(0));
    let heard = Rc::clone(&told);
    context
        .dispatcher
        .on_transcript(Rc::new(move || heard.set(heard.get() + 1)));
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    assert!(told.get() > 0, "the brief its window took is new");
    let before = told.get();
    context.pass().unwrap();
    assert_eq!(told.get(), before, "nothing new: nothing told");
    context.adapter.answer("zeus", "Parser done");
    context.pass().unwrap();
    assert!(told.get() > before, "its answer is new");
    held_to(
        context.close(),
        SUITES,
        "tells its transcript listeners when a window wrote more, and not after a look that found nothing new",
    );
}

#[test]
fn tells_a_trace_each_change_of_a_windows_activity_by_participant() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus", "diana"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working");
    let activity: Vec<TraceLine> = context
        .trace
        .lines
        .borrow()
        .iter()
        .filter(|line| {
            matches!(
                &line.what,
                Traced::Window {
                    event: WindowEvent::Activity { .. },
                    ..
                }
            )
        })
        .cloned()
        .collect();
    let changes: Vec<(Option<&str>, &str)> = activity
        .iter()
        .filter_map(|line| match &line.what {
            Traced::Window {
                participant,
                event: WindowEvent::Activity { state, .. },
                ..
            } => Some((participant.as_deref(), state.as_str())),
            _ => None,
        })
        .take(2)
        .collect();
    assert_eq!(
        changes,
        [
            (Some("chief"), "idle"),
            (Some("zeus-amber-pine"), "working")
        ]
    );
    let written = serde_json::to_value(&activity[0]).unwrap();
    let mut keys: Vec<&String> = written.as_object().unwrap().keys().collect();
    keys.sort();
    assert_eq!(
        keys,
        ["at", "kind", "participant", "project", "reason", "state"]
    );
    held_to(
        context.close(),
        SUITES,
        "tells a trace each change of a window's activity, by participant",
    );
}

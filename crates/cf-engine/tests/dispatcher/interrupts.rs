//! A window whose task stopped stops too: Escape for an agent at work, as
//! many presses as its harness asks for, again while it still works, and
//! never into an idle window; and a window that works for hours is left
//! alone (`describe('the dispatcher')`).

use std::time::Duration;

use cf_engine::testing::Context;
use cf_engine::ActivityState;
use cf_harness::contract::{Interrupt, Pane};
use serde_json::{json, Value};

use crate::matching::found;
use crate::traces::held_to;

const SUITES: &[&str] = &["the dispatcher"];

/// What pressing Escape into `pane` is, as the host is asked it.
fn escape(pane: &Pane) -> Value {
    json!({ "id": pane.id, "generation": pane.generation, "bytes": [27] })
}

/// The pane of the last window of `handle`.
fn pane_of(context: &Context, handle: &str) -> Pane {
    context.host.last(handle).expect("its window").pane
}

#[test]
fn delivers_the_chiefs_tell_into_the_paused_window_once_the_agent_is_interrupted_and_collects_no_result_from_it(
) {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    context.adapter.busy("zeus");
    context.pause_task(project.id, 1);
    let told = context.tell(project.id, "zeus", 1, "Stop: use grammar v2");
    context.pass().unwrap();
    let pane = pane_of(&context, "zeus");
    assert_eq!(
        context.host.inputs(),
        [escape(&pane)],
        "the agent is interrupted first"
    );
    let seen = context.adapter.agent("zeus").items;
    assert!(
        !seen
            .iter()
            .any(|item| item.text.contains(&format!("m-{}", told.id))),
        "nothing pasted while it works"
    );
    context.adapter.answer("zeus", "Half a parser");
    context.pass().unwrap();
    let last = context.adapter.agent("zeus").items.pop().unwrap();
    assert!(
        found(
            r#"^\[ConsensFlow m-\d+ · T-1 · question from @chief\]\nStop: use grammar v2\n\nT-1 is paused for this\. Run in your shell: cf answer m-\d+ "…"; the chief resumes the task\.$"#,
            &last.text
        ),
        "the tell goes in once the window is idle"
    );
    assert_eq!(
        context.task(project.id, 1).task.state,
        "paused",
        "its half-done output was not a result"
    );
    context.pass().unwrap();
    assert_eq!(context.message(told.id).state, "delivered");
    held_to(
        context.close(),
        SUITES,
        "delivers the chief's tell into the paused window once the agent is interrupted, and collects no result from it",
    );
}

#[test]
fn interrupts_a_task_paused_again_after_a_resume_however_many_rounds_its_first_pause_took() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    // A harness that ignores the key: three rounds, then no more for this pause.
    context.adapter.busy("zeus");
    context.pause_task(project.id, 1);
    for _ in 0..4 {
        context.pass().unwrap();
        context.advance(3_100);
    }
    assert_eq!(
        context.host.inputs().len(),
        3,
        "three rounds for the first pause"
    );
    context.resume_task(project.id, 1, "Carry on");
    context.pass().unwrap();
    context.adapter.busy("zeus");
    context.advance(1_000);
    context.pause_task(project.id, 1);
    context.pass().unwrap();
    assert_eq!(
        context.host.inputs().len(),
        4,
        "the second pause is a stop of its own"
    );
    held_to(
        context.close(),
        SUITES,
        "interrupts a task paused again after a resume, however many rounds its first pause took",
    );
}

#[test]
fn leaves_the_window_alone_once_the_chiefs_tell_reaches_it_answered_or_not_until_the_task_goes_on()
{
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    context.adapter.busy("zeus");
    context.pause_task(project.id, 1);
    let told = context.tell(project.id, "zeus", 1, "Which file?");
    context.pass().unwrap();
    context.adapter.answer("zeus", "Stopped");
    context.pass().unwrap();
    assert_eq!(
        context.host.inputs().len(),
        1,
        "one Escape stopped the task"
    );
    // The tell is in; the agent works on its answer, past the next round's time.
    context.adapter.busy("zeus");
    context.advance(3_100);
    context.pass().unwrap();
    context.advance(3_100);
    context.pass().unwrap();
    assert_eq!(
        context.host.inputs().len(),
        1,
        "its answer to the tell is not interrupted"
    );
    // Answered, it ends its own turn: an Escape now would cut its last words.
    context
        .ledger
        .borrow_mut()
        .answer(told.id, told.recipient_id, Some(&json!("a.txt")), None)
        .unwrap();
    context.advance(3_100);
    context.pass().unwrap();
    assert_eq!(
        context.host.inputs().len(),
        1,
        "nor its wrap-up after the answer"
    );
    held_to(
        context.close(),
        SUITES,
        "leaves the window alone once the chief's tell reaches it, answered or not, until the task goes on",
    );
}

#[test]
fn never_stops_a_window_for_how_long_it_works() {
    let context = Context::new();
    let project = context.with_staff(&["zeus", "diana"]);
    context.give(project.id, "zeus", "Run the Rust tests");
    context.give(project.id, "diana", "Write the report");
    context.pass().unwrap();
    context.pass().unwrap();
    for handle in ["chief", "zeus", "diana"] {
        context.adapter.busy(handle);
    }
    // One command that runs for hours (Rust tests take 4 to 6), or a model
    // turn that goes on: nothing in any record moves, and nothing is stopped.
    context.pass().unwrap();
    for _ in 0..6 {
        context.advance(60 * 60_000);
        context.pass().unwrap();
    }
    assert!(context.host.killed().is_empty(), "no window closed");
    let states: Vec<String> = [1, 2]
        .map(|number| context.task(project.id, number).task.state)
        .to_vec();
    assert_eq!(states, ["working", "working"]);
    let open = context
        .ledger
        .borrow()
        .project(project.id)
        .unwrap()
        .unwrap();
    assert_eq!(open.state, "open");
    for handle in ["chief", "zeus", "diana"] {
        let id = context.id(project.id, handle);
        assert_eq!(
            context.dispatcher.activity(id).state,
            ActivityState::Working,
            "{handle}"
        );
    }
    let chief = context.id(project.id, "chief");
    let notes = context
        .inbox(chief)
        .into_iter()
        .filter(|message| message.kind == "note")
        .count();
    assert_eq!(notes, 0, "no note");
    held_to(
        context.close(),
        SUITES,
        "never stops a window for how long it works: a six-hour command or model turn is left alone",
    );
}

#[test]
fn presses_escape_twice_in_a_row_for_a_harness_that_asks_for_it() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.adapter.interrupt.set(Interrupt {
        presses: 2,
        close_after: None,
    });
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    context.pause_task(project.id, 1);
    context.pass().unwrap();
    let pane = pane_of(&context, "zeus");
    assert_eq!(context.host.inputs(), [escape(&pane), escape(&pane)]);
    held_to(
        context.close(),
        SUITES,
        "presses Escape twice in a row for a harness that asks for it",
    );
}

#[test]
fn presses_one_escape_more_a_moment_later_for_a_harness_whose_two_can_open_a_dialog() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.adapter.interrupt.set(Interrupt {
        presses: 2,
        close_after: Some(Duration::from_millis(20)),
    });
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    context.pause_task(project.id, 1);
    context.pass().unwrap();
    assert_eq!(context.host.inputs().len(), 3);
    // The Node test read the real clock; the third press is held to come
    // after the pause the adapter names, as the engine's sleeps are written.
    let steps: Vec<String> = context
        .recorder
        .events()
        .iter()
        .filter_map(|event| match event["seam"].as_str()? {
            "host" if event["args"][0] == "pane.input" => Some("pane.input".to_owned()),
            "time" => Some(format!("sleep {}", event["args"][0])),
            _ => None,
        })
        .collect();
    assert_eq!(
        steps,
        [
            "pane.input",
            "sleep 150",
            "pane.input",
            "sleep 20",
            "pane.input"
        ],
        "the third after the pause the adapter names"
    );
    held_to(
        context.close(),
        SUITES,
        "presses one Escape more a moment later for a harness whose two can open a dialog",
    );
}

#[test]
fn presses_no_escape_into_an_idle_window_whose_task_is_paused_and_gives_it_the_chiefs_tell_as_it_is(
) {
    // poker-lab, 2026-10-03: a tell's pause sent Escape twice to a Devin that
    // had stopped; its rewind opened, and the tell's Enter rewound the agent's
    // conversation.
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.adapter.interrupt.set(Interrupt {
        presses: 2,
        close_after: None,
    });
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    // Its turn is over, with nothing written yet: the task is still working.
    context.adapter.with("zeus", |agent| agent.settled = true);
    context.pass().unwrap();
    assert_eq!(context.task(project.id, 1).task.state, "working");
    context.pause_task(project.id, 1);
    context.tell(project.id, "zeus", 1, "Stop: use grammar v2");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.host.inputs(), Vec::<Value>::new());
    let last = context.adapter.agent("zeus").items.pop().unwrap();
    assert!(found(
        r"question from @chief\]\nStop: use grammar v2",
        &last.text
    ));
    held_to(
        context.close(),
        SUITES,
        "presses no Escape into an idle window whose task is paused, and gives it the chief's tell as it is",
    );
}

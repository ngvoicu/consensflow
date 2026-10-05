//! A cancelled task stops its window: an agent still at work on it is
//! interrupted, and the window closes as any that holds no task, unless the
//! human opened it; words on their way into it are given up on at once.

use cf_engine::testing::Context;
use cf_engine::ActivityState;
use cf_harness::records::Role;
use serde_json::{json, Value};

use crate::fixtures::{native_of, Tiers};
use crate::traces::held_to;

const SUITES: &[&str] = &["a cancelled task"];

/// The keys pressed and the windows killed since the recorder held `from`
/// events, in order: what the Node test noted by wrapping the host's calls.
fn steps(context: &Context, from: usize) -> Vec<Value> {
    context
        .recorder
        .events()
        .iter()
        .skip(from)
        .filter(|event| event["seam"] == "host")
        .filter_map(|event| match event["method"].as_str()? {
            "request" if event["args"][0] == "pane.input" => Some(json!([
                "keys",
                event["args"][1]["generation"],
                event["args"][1]["bytes"]
            ])),
            "kill" => Some(json!(["kill", event["args"][0]["generation"]])),
            _ => None,
        })
        .collect()
}

#[test]
fn stops_its_window_an_agent_at_work_on_it_is_interrupted_the_window_closes_and_the_session_stays()
{
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    let project = tiers.project.id;
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working");
    let session = tiers.assignee(1);
    let native = native_of(&context, tiers.id(&session));
    let pane = context.host.last("zeus").unwrap().pane;
    let begun = context.recorder.events().len();
    // One still on the board, and one given out whose window has not opened: they just cancel.
    tiers.open_body("Write the lexer");
    tiers.open_body("Write the docs");
    let zeus = tiers.id("zeus");
    context
        .ledger
        .borrow_mut()
        .assign_task(project, 3, zeus)
        .unwrap();
    for number in [2, 3, 1] {
        context
            .ledger
            .borrow_mut()
            .cancel_task(project, number, "human")
            .unwrap();
    }
    let opened = context.host.opened().len();
    context.pass().unwrap();
    assert_eq!(
        steps(&context, begun),
        [
            json!(["keys", pane.generation, [27]]),
            json!(["kill", pane.generation])
        ]
    );
    assert_eq!(
        context.host.opened().len(),
        opened,
        "no window opens for the other two"
    );
    let kept = tiers.participant(&session).unwrap();
    assert_eq!(kept.left_at, None, "the session stays on the board");
    context.open_window(project, &session).unwrap();
    let reopened = context.adapter.prepared().pop().unwrap();
    assert_eq!(
        [
            reopened["participant"]["handle"].clone(),
            reopened["resume"].clone(),
            reopened["message"].clone()
        ],
        [json!(session), json!(native), Value::Null],
        "and opens again on its own conversation"
    );
    held_to(
        context.close(),
        SUITES,
        "stops its window: an agent at work on it is interrupted, the window closes, and the session stays",
    );
}

#[test]
fn interrupts_a_window_the_human_opened_which_stays_and_lets_a_turn_the_human_began_there_since_go_on(
) {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    let project = tiers.project.id;
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    let session = tiers.assignee(1);
    context.open_window(project, &session).unwrap();
    let escapes = || context.host.inputs().len();
    context
        .ledger
        .borrow_mut()
        .cancel_task(project, 1, "human")
        .unwrap();
    context.pass().unwrap();
    assert_eq!(escapes(), 1, "interrupted");
    assert!(
        context.host.killed().is_empty(),
        "the human opened it: it stays open"
    );
    // A harness that ignores the key while it thinks is pressed again, a few seconds on.
    context.advance(3_100);
    context.pass().unwrap();
    assert_eq!(escapes(), 2);
    // The agent stops; the human asks it something in the window, and it works on that.
    context.adapter.answer("zeus", "Stopped.");
    context.pass().unwrap();
    let asked = context
        .adapter
        .item(Role::User, "What did you change so far?");
    context.adapter.with("zeus", |agent| {
        agent.items.push(asked);
        agent.settled = false;
    });
    for _ in 0..2 {
        context.advance(3_100);
        context.pass().unwrap();
    }
    assert_eq!(escapes(), 2, "the human's own turn is not interrupted");
    assert_eq!(
        context.dispatcher.activity(tiers.id(&session)).state,
        ActivityState::Working
    );
    assert_eq!(
        tiers.task(1).task.state,
        "cancelled",
        "and nothing it wrote became a result"
    );
    held_to(
        context.close(),
        SUITES,
        "interrupts a window the human opened, which stays, and lets a turn the human began there since go on",
    );
}

#[test]
fn closes_its_window_at_once_though_words_were_on_their_way_in_and_never_opens_one_for_it_again() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    let project = tiers.project.id;
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    context.pause_task(project, 1);
    context.pass().unwrap();
    context.adapter.answer("zeus", "Stopped.");
    context.pass().unwrap();
    // Resumed into its window, which has not shown the words yet.
    let message = context
        .ledger
        .borrow_mut()
        .resume_task(project, 1, Some("chief"), "Go on")
        .unwrap()
        .message
        .unwrap();
    context.adapter.with("zeus", |agent| agent.arrive = false);
    context.pass().unwrap();
    assert_eq!(context.message(message.id).state, "delivering");
    let pane = context.host.last("zeus").unwrap().pane;
    context
        .ledger
        .borrow_mut()
        .cancel_task(project, 1, "human")
        .unwrap();
    context.pass().unwrap();
    assert_eq!(
        context.host.killed().last(),
        Some(&pane),
        "closed on the next look, not once the words were given up on"
    );
    let opened = context.host.opened().len();
    for _ in 0..3 {
        context.advance(31_000);
        context.pass().unwrap();
    }
    assert_eq!(
        context.host.opened().len(),
        opened,
        "no window opens for it again"
    );
    assert_eq!(context.message(message.id).state, "cancelled");
    assert_eq!(
        tiers.notes("human"),
        Vec::<String>::new(),
        "and nothing failed for the human to hear of"
    );
    held_to(
        context.close(),
        SUITES,
        "closes its window at once though words were on their way in, and never opens one for it again",
    );
}

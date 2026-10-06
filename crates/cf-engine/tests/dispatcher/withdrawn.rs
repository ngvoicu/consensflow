//! A message withdrawn on its way into a window: what its window shows of it
//! after all confirms nothing, and a window that was getting ready is handed
//! nothing (`describe('a message withdrawn on its way into a window')`).

use std::rc::Rc;

use cf_engine::delivery_text::{delivery_text, marker_of};
use cf_engine::testing::Context;
use cf_harness::contract::Readiness;
use cf_harness::records::Role;
use cf_ledger::MessageView;
use serde_json::json;

use crate::fixtures::{assert_match, Tiers};
use crate::traces::held_to;

const SUITES: &[&str] = &["a message withdrawn on its way into a window"];

/// T-1 waits on its window's question; its answer goes to that window next
/// (`asked`).
fn asked(context: &Context) -> (Tiers<'_>, MessageView) {
    let tiers = Tiers::new(context, &["zeus", "diana"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    let question = context.ask(
        tiers.project.id,
        &tiers.assignee(1),
        "chief",
        1,
        "Which grammar?",
    );
    context.adapter.answer("zeus", "I asked.");
    context.pass().unwrap();
    let chief = tiers.id("chief");
    let answer = context
        .ledger
        .borrow_mut()
        .answer(question.id, chief, Some(&json!("The small one")), None)
        .unwrap();
    (tiers, answer)
}

#[test]
fn is_not_confirmed_when_it_shows_after_all_and_its_window_goes_with_its_work() {
    let context = Context::new();
    let (tiers, answer) = asked(&context);
    context.adapter.with("zeus", |agent| agent.arrive = false);
    context.pass().unwrap();
    assert_eq!(
        context.message(answer.id).state,
        "delivering",
        "handed over"
    );
    let pane = context.host.last("zeus").unwrap().pane;
    // The human takes the task back: the answer on its way is withdrawn,
    // and the window's harness shows it anyway.
    context
        .ledger
        .borrow_mut()
        .release_task(tiers.project.id, 1, "by @human")
        .unwrap();
    let shown = context
        .adapter
        .item(Role::User, &delivery_text(&context.message(answer.id), &[]));
    context
        .adapter
        .with("zeus", |agent| agent.items.push(shown));
    context.pass().unwrap();
    assert_eq!(
        context.message(answer.id).state,
        "cancelled",
        "it stays withdrawn"
    );
    assert!(
        context
            .host
            .killed()
            .iter()
            .any(|killed| killed.generation == pane.generation),
        "the window went with its work"
    );
    // And the task went on elsewhere, with what was on its way in the brief
    // of the window that took it, once. (Not held to Node's recording, which
    // let that go with the window.)
    assert_match(&tiers.assignee(1), "^diana-");
    let first = context.adapter.prepared().last().unwrap()["message"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(
        first.contains("(answer m-3 from @chief to m-2 of @zeus-amber-pine: Which grammar?)\nThe small one\n\nReassigned from @zeus-amber-pine (by @human)"),
        "{first}"
    );
}

#[test]
fn is_handed_nothing_when_it_was_withdrawn_while_its_window_got_ready_and_fails_nothing() {
    let context = Context::new();
    let (tiers, answer) = asked(&context);
    *context.adapter.ready.borrow_mut() = Some(Rc::new(|| Ok(Readiness::Ready)));
    let readying = context.adapter.ready_holds.hold(|_| true);
    context.pass().unwrap();
    context
        .ledger
        .borrow_mut()
        .cancel_task(tiers.project.id, 1, "chief")
        .unwrap();
    readying.open();
    context.settle();
    assert_eq!(context.message(answer.id).state, "cancelled");
    let marker = marker_of(answer.id);
    assert!(
        !context
            .adapter
            .agent("zeus")
            .items
            .iter()
            .any(|item| item.text.contains(&marker)),
        "nothing pasted"
    );
    held_to(
        context.close(),
        SUITES,
        "is handed nothing when it was withdrawn while its window got ready, and fails nothing",
    );
}

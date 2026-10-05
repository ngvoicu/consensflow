//! What the human typed in a window and has not sent: a message waits for
//! it, the board says so, and no Enter goes in after a paste on top of it
//! (`describe('what the human typed in a window and has not sent')`).

use std::cell::Cell;
use std::rc::Rc;

use cf_engine::testing::Context;
use cf_harness::contract::{Held, Readiness};
use serde_json::{json, Value};

use crate::fixtures::Tiers;
use crate::traces::held_to;

const SUITES: &[&str] = &["what the human typed in a window and has not sent"];

#[test]
fn holds_a_message_for_it_says_so_on_the_board_and_lets_it_go_once_the_text_is_sent() {
    // Poker-lab, 2026-10-03: "if I type, sometimes the daemon pastes over".
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus", "diana"]);
    let typed = Rc::new(Cell::new(true));
    let asked = Rc::clone(&typed);
    *context.adapter.ready.borrow_mut() = Some(Rc::new(move || {
        Ok(if asked.get() {
            Readiness::Held(Held::Unsent)
        } else {
            Readiness::Ready
        })
    }));
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
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
    assert_eq!(result().state, "queued");
    assert!(
        context.dispatcher.holding(chief).unwrap(),
        "the board says why"
    );
    typed.set(false);
    context.pass().unwrap();
    assert_ne!(result().state, "queued", "delivered once the text is sent");
    assert!(!context.dispatcher.holding(chief).unwrap());
    held_to(
        context.close(),
        SUITES,
        "holds a message for it, says so on the board, and lets it go once the text is sent",
    );
}

#[test]
fn presses_no_enter_again_into_a_window_where_the_human_has_typed_since_the_paste() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.adapter.with("chief", |agent| agent.arrive = false);
    context.note(project.id, "zeus", "chief", "One");
    context.pass().unwrap();
    context
        .host
        .set_snapshot(json!({ "outputQuietMs": 60_000, "unsent": true }));
    context.advance(11_000);
    context.pass().unwrap();
    let enters: Vec<Value> = context
        .host
        .inputs()
        .into_iter()
        .filter(|body| body["bytes"][0] == 13)
        .collect();
    assert_eq!(
        enters,
        Vec::<Value>::new(),
        "its Enter would send their text too"
    );
    held_to(
        context.close(),
        SUITES,
        "presses no Enter again into a window where the human has typed since the paste",
    );
}

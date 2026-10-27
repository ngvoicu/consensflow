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

/// A message held for words typed in its window and not sent, at the look `minutes`
/// minutes after the first that held it, given `context`'s words of what the box shows.
fn held_for(context: &Context, minutes: i64) {
    context.advance(minutes * 60_000);
    context.pass().unwrap();
}

/// T-839: a chief's results waited two hours for words the screen showed nowhere, and
/// nobody was told but by the chief's lane. Held past the bound, the human hears it
/// once, the window named and what its box seems to hold masked and cut short.
#[test]
fn a_message_held_past_the_bound_for_words_typed_tells_the_human_once_what_the_box_shows() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus", "diana"]);
    let asked = Rc::new(Cell::new(0));
    let counted = Rc::clone(&asked);
    *context.adapter.ready.borrow_mut() = Some(Rc::new(move || {
        counted.set(counted.get() + 1);
        Ok(Readiness::Held(Held::Unsent))
    }));
    *context.adapter.box_words.borrow_mut() = Some("half a thought about the parser".to_owned());
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    context.adapter.answer("zeus", "Parser done.");
    context.pass().unwrap();
    context.pass().unwrap();
    let chief = tiers.id("chief");
    assert!(context.dispatcher.holding(chief).unwrap());
    let told = || {
        tiers
            .notes("human")
            .into_iter()
            .filter(|note| note.contains("not sent"))
            .collect::<Vec<_>>()
    };
    let before = asked.get();
    held_for(&context, 9);
    assert!(asked.get() > before, "every look asks the window again");
    assert_eq!(told(), Vec::<String>::new(), "a human typing is no news");
    held_for(&context, 2);
    let notes = told();
    assert_eq!(notes.len(), 1, "{notes:?}");
    let note = &notes[0];
    assert!(note.contains("@chief"), "the window is named: {note}");
    assert!(note.contains("\"ha…\" (31 characters)"), "{note}");
    assert!(
        !note.contains("thought"),
        "never the words themselves: {note}"
    );
    held_for(&context, 30);
    assert_eq!(told().len(), 1, "told once");
}

/// A box no reader of the window's harness reads is said so, and the requester of the
/// task whose message waits hears it too.
#[test]
fn a_message_held_for_a_box_that_cannot_be_read_says_so_and_tells_its_requester() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus", "diana"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    *context.adapter.ready.borrow_mut() = Some(Rc::new(|| Ok(Readiness::Held(Held::Unsent))));
    let worker = tiers.assignee(1);
    context.tell(tiers.project.id, &worker, 1, "One more case.");
    context.pass().unwrap();
    context.adapter.with("zeus", |agent| agent.settled = true);
    context.pass().unwrap();
    context.pass().unwrap();
    assert!(context.dispatcher.holding(tiers.id(&worker)).unwrap());
    held_for(&context, 11);
    let human: Vec<String> = tiers
        .notes("human")
        .into_iter()
        .filter(|note| note.contains("not sent"))
        .collect();
    assert_eq!(human.len(), 1, "{human:?}");
    assert!(human[0].contains(&format!("@{worker}")), "{}", human[0]);
    assert!(human[0].contains("cannot be read"), "{}", human[0]);
    let chief: Vec<String> = tiers
        .notes("chief")
        .into_iter()
        .filter(|note| note.contains("not sent"))
        .collect();
    assert_eq!(chief.len(), 1, "the requester of T-1 hears it: {chief:?}");
}

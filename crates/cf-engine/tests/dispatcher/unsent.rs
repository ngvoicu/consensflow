//! What the human typed in a window and has not sent: a message waits for
//! it, the board says so, and no Enter goes in after a paste on top of it
//! (`describe('what the human typed in a window and has not sent')`).

use std::cell::Cell;
use std::rc::Rc;

use cf_engine::testing::Context;
use cf_harness::contract::{FirstMessage, Held, Readiness};
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

/// A chief at rest whose worker's result is held for words typed in its window: the
/// adapter answers as `held` says at each look, and the test's context.
fn a_result_held_at_the_chief<'a>(
    context: &'a Context,
    held: &Rc<Cell<Option<Held>>>,
) -> Tiers<'a> {
    let tiers = Tiers::new(context, &["zeus", "diana"]);
    // As Claude Code's: a window's first message too is pasted once it is up.
    context.adapter.first.set(FirstMessage::Delivered);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    // The worker has its brief; from now on the window the result goes to holds it.
    let asked = Rc::clone(held);
    *context.adapter.ready.borrow_mut() = Some(Rc::new(move || {
        Ok(asked
            .take()
            .map_or(Readiness::Held(Held::Unsent), Readiness::Held))
    }));
    *context.adapter.box_words.borrow_mut() = Some("half a thought".to_owned());
    context.adapter.answer("zeus", "Parser done.");
    context.pass().unwrap();
    context.pass().unwrap();
    assert!(context.dispatcher.holding(tiers.id("chief")).unwrap());
    tiers
}

/// The notes the human was told of a hold for words typed and not sent.
fn told_unsent(tiers: &Tiers) -> Vec<String> {
    tiers
        .notes("human")
        .into_iter()
        .filter(|note| note.contains("not sent"))
        .collect()
}

/// A look held for another reason (a snapshot that failed, on a loaded Mac) does not
/// start the ten minutes again: the hold is told at ten minutes from its first look
/// (Calliope's T-844).
#[test]
fn a_hold_a_failed_snapshot_interrupts_is_still_told_at_ten_minutes() {
    let context = Context::new();
    let once = Rc::new(Cell::new(None));
    let tiers = a_result_held_at_the_chief(&context, &once);
    held_for(&context, 5);
    once.set(Some(Held::Because(
        "the window cannot be read: no answer".to_owned(),
    )));
    held_for(&context, 1);
    held_for(&context, 5);
    assert_eq!(told_unsent(&tiers).len(), 1, "{:?}", tiers.notes("human"));
}

/// A daemon that restarts does not tell the human again of a hold it told them of
/// (Calliope's T-844): its note is found among theirs.
#[test]
fn a_hold_told_before_a_restart_is_not_told_again_after_it() {
    let context = Context::new();
    let never = Rc::new(Cell::new(None));
    let tiers = a_result_held_at_the_chief(&context, &never);
    held_for(&context, 11);
    assert_eq!(told_unsent(&tiers).len(), 1);
    let after = context.make();
    after.settle_after_restart().unwrap();
    for _ in 0..3 {
        context.advance(11 * 60_000);
        after.pass().unwrap();
    }
    let result = context
        .inbox(tiers.id("chief"))
        .into_iter()
        .find(|message| message.kind == "result")
        .unwrap();
    // Owed to the window the restart opened, as its first message, and held there.
    assert_eq!(result.state, "delivering", "still held");
    assert_eq!(told_unsent(&tiers).len(), 1, "{:?}", tiers.notes("human"));
}

/// The requester of the held message's task, a member's window, is told how many
/// characters the box shows, never any of them (Calliope's T-844).
#[test]
fn the_requester_is_told_how_many_characters_the_box_shows_and_none_of_them() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus", "diana"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    *context.adapter.ready.borrow_mut() = Some(Rc::new(|| Ok(Readiness::Held(Held::Unsent))));
    *context.adapter.box_words.borrow_mut() = Some("half a thought".to_owned());
    let worker = tiers.assignee(1);
    context.tell(tiers.project.id, &worker, 1, "One more case.");
    context.pass().unwrap();
    context.adapter.with("zeus", |agent| agent.settled = true);
    context.pass().unwrap();
    context.pass().unwrap();
    held_for(&context, 11);
    let human = told_unsent(&tiers);
    assert_eq!(human.len(), 1, "{human:?}");
    assert!(human[0].contains("\"ha…\" (14 characters)"), "{}", human[0]);
    let chief: Vec<String> = tiers
        .notes("chief")
        .into_iter()
        .filter(|note| note.contains("not sent"))
        .collect();
    assert_eq!(chief.len(), 1, "{chief:?}");
    assert!(chief[0].contains("shows 14 characters"), "{}", chief[0]);
    assert!(!chief[0].contains("ha…"), "{}", chief[0]);
}

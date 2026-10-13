//! A message handed over to a window that went into a turn after it (what a
//! harness is given while its agent is in a turn it may hold, and write to its
//! record when it gets to it) is waited for, however long its record takes,
//! and never pasted again. A paste into a window that stayed at rest and that
//! the record never shows is still tried again (`deliveries`).
//!
//! The report: 2026-10-13, ConsensFlow 3.0.3, a Devin chief on Windows. The
//! human cancelled T-17, the chief was sent two notes one right after the
//! other, and each reached its window twice in a row (m-186, m-186, m-187,
//! m-187): the record showed none of them within the minute, though the window
//! had been in a turn since and was at rest again, and the daemon took each
//! paste for lost. None of these is held to a Node trace: Node's dispatcher
//! counted the arrival window from the hand-over, turns or none.

use std::cell::Cell;
use std::rc::Rc;

use cf_engine::delivery_text::{delivery_text, marker_of};
use cf_engine::testing::Context;
use cf_harness::contract::Waiting;
use cf_harness::records::Role;
use cf_proto::ledger::MessageView;
use serde_json::{json, Value};

use crate::background::LOCK;
use crate::deliveries::enters;
use crate::fixtures::{given, Tiers};

/// The engine's arrival window in the kit, in milliseconds.
const ARRIVAL_MS: i64 = 30_000;

/// How many times a window was handed a message from now on: each paste the
/// engine made, whichever of them its record showed.
fn counting(context: &Context) -> Rc<Cell<usize>> {
    let made = Rc::new(Cell::new(0));
    let counted = Rc::clone(&made);
    *context.adapter.deliver.borrow_mut() = Some(Rc::new(move |taking| {
        counted.set(counted.get() + 1);
        Box::pin(async move { Ok(taking()) })
    }));
    made
}

/// `message` shows in the record of `handle`'s window: the harness took it
/// into a turn.
fn shows(context: &Context, handle: &str, message: &MessageView) {
    let item = context
        .adapter
        .item(Role::User, &delivery_text(message, &[]));
    context.adapter.with(handle, |agent| agent.items.push(item));
}

/// A project of the chief and zeus, the chief's window open and its harness
/// holding what it is given: a paste is taken and the record does not show it.
fn holding(context: &Context) -> i64 {
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    context.adapter.with("chief", |agent| agent.arrive = false);
    project.id
}

#[test]
fn waits_for_the_record_of_a_message_whose_window_went_into_a_turn_however_long_it_takes() {
    let context = Context::new();
    let project = holding(&context);
    let pasted = counting(&context);
    let note = context.note(project, "zeus", "chief", "hello");
    context.pass().unwrap();
    assert_eq!(context.message(note.id).state, "delivering");
    assert_eq!(pasted.get(), 1);

    // The chief is in a turn that takes four minutes, eight times the arrival
    // window, and its record shows nothing of what it holds meanwhile.
    context.adapter.busy("chief");
    for _ in 0..8 {
        context.advance(ARRIVAL_MS);
        context.pass().unwrap();
    }
    assert_eq!(context.message(note.id).attempts, 1, "not tried again");

    // The turn ends and the record still shows nothing, for ten windows more:
    // it is late, not lacking, and the window has the message.
    context.adapter.with("chief", |agent| agent.settled = true);
    for _ in 0..10 {
        context.advance(ARRIVAL_MS);
        context.pass().unwrap();
    }
    let waiting = context.message(note.id);
    assert_eq!(
        (waiting.state.as_str(), waiting.attempts),
        ("delivering", 1),
        "still on its way, not tried again"
    );
    assert_eq!(pasted.get(), 1, "not pasted again");

    // The record shows it at last.
    shows(&context, "chief", &waiting);
    context.pass().unwrap();
    let delivered = context.message(note.id);
    assert_eq!(
        (delivered.state.as_str(), delivered.attempts),
        ("delivered", 1)
    );
    assert_eq!(pasted.get(), 1);
}

#[test]
fn pastes_again_a_message_whose_window_stayed_at_rest_and_not_one_whose_window_went_into_a_turn() {
    let context = Context::new();
    let project = holding(&context);
    let pasted = counting(&context);
    let first = context.note(project, "zeus", "chief", "hello");
    context.pass().unwrap();

    // At rest all the arrival window, nothing shown: the paste did nothing, and
    // is sent again.
    context.advance(ARRIVAL_MS + 1_000);
    context.pass().unwrap();
    assert_eq!(context.message(first.id).attempts, 2);
    assert_eq!(pasted.get(), 2, "pasted again once");

    // A turn of a single look, and at rest again: the window had the second
    // paste, and the minutes after it change nothing.
    context.adapter.busy("chief");
    context.pass().unwrap();
    context.adapter.with("chief", |agent| agent.settled = true);
    for _ in 0..10 {
        context.advance(ARRIVAL_MS);
        context.pass().unwrap();
    }
    let waiting = context.message(first.id);
    assert_eq!(
        (waiting.state.as_str(), waiting.attempts),
        ("delivering", 2)
    );
    assert_eq!(pasted.get(), 2, "not pasted a third time");
}

#[test]
fn takes_a_window_waiting_on_a_dialog_of_its_own_for_a_window_in_a_turn() {
    let context = Context::new();
    let project = holding(&context);
    let pasted = counting(&context);
    let note = context.note(project, "zeus", "chief", "hello");
    context.pass().unwrap();

    // Its own question dialog is open: the turn waits for the human's answer, and
    // is in it still, whatever its record says of the turn it began.
    context.adapter.with("chief", |agent| {
        agent.waiting = Some(Waiting {
            reason: Some("its own question dialog is open".to_owned()),
        });
    });
    for _ in 0..4 {
        context.advance(ARRIVAL_MS);
        context.pass().unwrap();
    }
    assert_eq!(context.message(note.id).attempts, 1);
    assert_eq!(pasted.get(), 1, "not pasted again");

    // The human answers: the window is at rest, and went through a turn.
    context.adapter.with("chief", |agent| agent.waiting = None);
    for _ in 0..4 {
        context.advance(ARRIVAL_MS);
        context.pass().unwrap();
    }
    assert_eq!(context.message(note.id).attempts, 1);
    assert_eq!(pasted.get(), 1, "not pasted again");
}

#[test]
fn presses_enter_again_for_a_message_only_into_a_window_at_rest_and_once_for_each_rest_after_a_turn(
) {
    let context = Context::new();
    let project = holding(&context);
    context.host.set_snapshot(json!({ "outputQuietMs": 5_000 }));
    context.note(project, "zeus", "chief", "hello");
    context.pass().unwrap();

    // A turn is no paste left unsent: its window has the message, or holds it.
    context.adapter.busy("chief");
    context.advance(15_000);
    context.pass().unwrap();
    assert_eq!(enters(&context), 0, "not into a window in a turn");

    // At rest again, nothing shown: the paste may be sitting in the input, as a
    // turn that ignored its Enter left it.
    context.adapter.with("chief", |agent| agent.settled = true);
    context.pass().unwrap();
    assert_eq!(enters(&context), 1, "pressed once, at rest");
    context.advance(2_000);
    context.pass().unwrap();
    assert_eq!(enters(&context), 1, "not again for the same rest");

    // Another turn, and another rest: another Enter, and no paste.
    context.adapter.busy("chief");
    context.advance(2_000);
    context.pass().unwrap();
    context.adapter.with("chief", |agent| agent.settled = true);
    context.pass().unwrap();
    assert_eq!(enters(&context), 2, "once for each rest after a turn");
}

#[test]
fn sends_the_enter_that_a_turn_ignored_and_the_record_shows_the_message_once() {
    let context = Context::new();
    let project = holding(&context);
    let pasted = counting(&context);
    context.host.set_snapshot(json!({ "outputQuietMs": 5_000 }));
    let note = context.note(project, "zeus", "chief", "hello");
    context.pass().unwrap();
    context.adapter.busy("chief");
    context.pass().unwrap();

    // The turn ends with the paste still in the input: Enter sends it, and the
    // record shows it, which no second paste was needed for.
    let (adapter, header) = (Rc::downgrade(&context.adapter), delivery_text(&note, &[]));
    *context.host.on_request.borrow_mut() = Some(Rc::new(move |op: &str, body: &Value| {
        let enter = op == "pane.input" && body["bytes"][0] == 13;
        if let (true, Some(adapter)) = (enter, adapter.upgrade()) {
            let item = adapter.item(Role::User, &header);
            adapter.with("chief", |agent| agent.items.push(item));
        }
    }));
    context.adapter.with("chief", |agent| agent.settled = true);
    context.advance(11_000);
    context.pass().unwrap();
    context.pass().unwrap();
    let delivered = context.message(note.id);
    assert_eq!(
        (delivered.state.as_str(), delivered.attempts),
        ("delivered", 1)
    );
    assert_eq!(enters(&context), 1);
    assert_eq!(pasted.get(), 1, "pasted once");
}

#[test]
fn hands_over_each_of_two_notes_once_that_come_together_after_a_cancel_to_a_chief_in_a_turn() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working");
    // Its window has work running when it closes (a cancel closes it).
    context.host.exits_leaving(&[LOCK]);
    let pasted = counting(&context);
    context.adapter.with("chief", |agent| agent.arrive = false);

    // The human cancels T-1: the chief is told of it, and, as the worker's
    // window closes, of the work it left. Both are on their way to it together.
    context
        .ledger
        .borrow_mut()
        .cancel_task(tiers.project.id, 1, "human")
        .unwrap();
    context.pass().unwrap();
    context.pass().unwrap();
    let notes: Vec<MessageView> = context
        .inbox(tiers.id("chief"))
        .into_iter()
        .rev()
        .filter(|message| message.kind == "note")
        .collect();
    assert_eq!(notes.len(), 2, "the cancel's note and the closed window's");
    let (cancelled, closed) = (&notes[0], &notes[1]);
    assert!(
        cancelled.body.contains("cancelled T-1"),
        "{}",
        cancelled.body
    );
    assert!(closed.body.contains("background work"), "{}", closed.body);
    assert_eq!(context.message(cancelled.id).state, "delivering");
    assert_eq!(
        pasted.get(),
        1,
        "the first is on its way, the second waits for it"
    );

    // The chief's harness takes the first into a turn, and its record shows
    // it while the turn has not yet written a word: the window reads at rest.
    shows(&context, "chief", cancelled);
    context.pass().unwrap();
    assert_eq!(context.message(cancelled.id).state, "delivered");
    assert_eq!(context.message(closed.id).state, "delivering");
    assert_eq!(
        pasted.get(),
        2,
        "the second is pasted into the turn the first began"
    );

    // The turn goes on for minutes, and the harness holds the second meanwhile.
    context.adapter.busy("chief");
    for _ in 0..8 {
        context.advance(ARRIVAL_MS);
        context.pass().unwrap();
    }
    let waiting = context.message(closed.id);
    assert_eq!(
        (waiting.state.as_str(), waiting.attempts),
        ("delivering", 1)
    );
    assert_eq!(pasted.get(), 2, "neither was pasted again");

    // The turn ends and takes the second.
    shows(&context, "chief", &waiting);
    context.adapter.answer("chief", "noted");
    context.pass().unwrap();
    for note in [cancelled, closed] {
        let found = context.message(note.id);
        assert_eq!(
            (found.state.as_str(), found.attempts),
            ("delivered", 1),
            "m-{}",
            note.id
        );
    }
    assert_eq!(pasted.get(), 2);
    let items = given(&context, "chief");
    for note in [cancelled, closed] {
        let id = marker_of(note.id);
        let shown = items.iter().filter(|text| text.contains(&id)).count();
        assert_eq!(shown, 1, "{id} shows once in what the chief was given");
    }
}

#[test]
fn pastes_nothing_more_in_the_passes_that_find_a_paste_still_going_in() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    // The chief's harness takes long over the paste, as a host that waits for
    // the window's echo does: it is not through when the next passes look.
    let launch = context.adapter.agent("chief").launch;
    let release = context
        .adapter
        .deliver_holds
        .hold(move |asked| asked[0]["launch"]["launchId"] == launch.as_str());
    let note = context.note(project.id, "zeus", "chief", "hello");
    context.pass().unwrap();
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.message(note.id).state, "delivering");
    assert!(
        context.adapter.agent("chief").items.is_empty(),
        "nothing is in the window yet"
    );
    release.open();
    context.settle();
    context.pass().unwrap();
    assert_eq!(context.message(note.id).state, "delivered");
    assert_eq!(
        given(&context, "chief").len(),
        1,
        "the one paste is all the window was given"
    );
}

#[test]
fn pastes_a_note_about_a_task_its_recipient_has_no_task_message_for_alone_and_a_second_one_alone() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    // Two notes about T-1 for the chief, which was never sent a task message
    // about it: nothing for them to ride in.
    let first = context.note_from_consensflow(tiers.project.id, "chief", "T-1 was looked at");
    let second =
        context.note_from_consensflow(tiers.project.id, "chief", "T-1 was looked at again");
    for _ in 0..4 {
        context.pass().unwrap();
        context.adapter.answer("chief", "noted");
    }
    let items = given(&context, "chief");
    assert_eq!(items.len(), 2, "{items:?}");
    for (text, note) in items.iter().zip([&first, &second]) {
        assert_eq!(text, &delivery_text(&context.message(note.id), &[]));
        assert_eq!(text.matches("[ConsensFlow m-").count(), 1, "{text}");
    }
}

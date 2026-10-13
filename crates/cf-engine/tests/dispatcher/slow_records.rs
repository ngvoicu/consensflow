//! A message handed over to a window whose harness holds what it is given while
//! its agent is in a turn (what comes in waits, and the harness's record shows it
//! when the agent runs it, a step or a turn on) is waited for, never pasted again
//! for a record that is late: only the window's rest counts towards the arrival
//! window, so the turns the window goes through are no time the record had to show
//! the message. And a harness that says its own queue holds the message
//! (`Window::took`) is waited for as one that queued it itself is. A paste into a
//! window that stayed at rest and that the record never shows is still tried
//! again (`deliveries`), whatever turns the window went through since: the wait is
//! bounded by the window's rest, and nothing a window does stops the queue behind it.
//! The one more Enter a paste is given (`deliveries`) is pressed in the first arrival
//! window only, whatever the wait after it.
//!
//! The report: 2026-10-13, ConsensFlow 3.0.3, a Devin chief on Windows. The
//! human cancelled T-17, the chief was sent two notes one right after the
//! other, and each reached its window twice in a row (m-186, m-186, m-187,
//! m-187): the record showed none of them within the minute, since the chief
//! was in a turn that kept them, and the daemon took each paste for lost. None of
//! these is held to a Node trace: Node's dispatcher counted the arrival window
//! from the hand-over, turns or none.

use std::cell::Cell;
use std::rc::Rc;

use cf_engine::delivery_text::{delivery_text, marker_of};
use cf_engine::testing::Context;
use cf_harness::contract::Waiting;
use cf_harness::records::Role;
use cf_proto::ledger::MessageView;
use serde_json::json;

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

/// `message` shows in the record of `handle`'s window: the harness ran it.
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

/// The reason of the last `delivery.retried` of `project`.
fn why_retried(context: &Context, project: i64) -> String {
    let events = context.ledger.borrow().events(project, 0, 500).unwrap();
    events
        .iter()
        .rfind(|event| event.kind == "delivery.retried")
        .and_then(|event| event.data["reason"].as_str().map(str::to_owned))
        .unwrap_or_default()
}

#[test]
fn waits_for_the_record_of_a_message_while_its_window_is_in_a_turn_and_pastes_it_once() {
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
    let waiting = context.message(note.id);
    assert_eq!(
        (waiting.state.as_str(), waiting.attempts),
        ("delivering", 1),
        "still on its way, not tried again"
    );
    assert_eq!(pasted.get(), 1, "not pasted again");

    // The turn ends and takes the note: the record shows it.
    shows(&context, "chief", &waiting);
    context.adapter.answer("chief", "noted");
    context.pass().unwrap();
    let delivered = context.message(note.id);
    assert_eq!(
        (delivered.state.as_str(), delivered.attempts),
        ("delivered", 1)
    );
    assert_eq!(pasted.get(), 1);
}

#[test]
fn counts_the_arrival_window_in_the_time_its_window_is_at_rest_and_begins_it_again_after_a_turn() {
    let context = Context::new();
    let project = holding(&context);
    let pasted = counting(&context);
    let note = context.note(project, "zeus", "chief", "hello");
    context.pass().unwrap();

    // Twenty seconds at rest, then a turn of twenty: the count is begun again
    // by the turn, so the forty seconds are no arrival window.
    context.advance(20_000);
    context.pass().unwrap();
    context.adapter.busy("chief");
    context.advance(20_000);
    context.pass().unwrap();
    assert_eq!(context.message(note.id).attempts, 1);

    // At rest again, with nothing shown: the window is given its whole length.
    context.adapter.with("chief", |agent| agent.settled = true);
    context.pass().unwrap();
    context.advance(ARRIVAL_MS - 1_000);
    context.pass().unwrap();
    let waiting = context.message(note.id);
    assert_eq!(
        (waiting.state.as_str(), waiting.attempts),
        ("delivering", 1)
    );
    assert_eq!(pasted.get(), 1);

    // Past it at rest, the paste is taken for lost and sent again, and the board
    // says it was after the turn.
    context.advance(2_000);
    context.pass().unwrap();
    let again = context.message(note.id);
    assert_eq!((again.state.as_str(), again.attempts), ("delivering", 2));
    assert_eq!(pasted.get(), 2, "pasted again once");
    assert_eq!(
        why_retried(&context, project),
        "the harness record never showed it in the 30 s its window was at rest since its last turn"
    );
}

#[test]
fn counts_no_rest_while_its_window_waits_on_a_dialog_of_its_own() {
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

    // The human answers: the window is at rest, and the window is counted from there.
    context.adapter.with("chief", |agent| agent.waiting = None);
    context.pass().unwrap();
    context.advance(ARRIVAL_MS - 1_000);
    context.pass().unwrap();
    assert_eq!(pasted.get(), 1);
    context.advance(2_000);
    context.pass().unwrap();
    assert_eq!(context.message(note.id).attempts, 2);
    assert_eq!(pasted.get(), 2);
}

#[test]
fn presses_no_enter_again_after_the_arrival_window_for_a_message_its_window_goes_on_waiting_for() {
    let context = Context::new();
    let project = holding(&context);
    let note = context.note(project, "zeus", "chief", "hello");
    context.pass().unwrap();

    // The chief is in a turn and printing when the paste has been waited for the
    // arrival window: the one Enter the paste is given is not pressed into it.
    context.adapter.busy("chief");
    context.host.set_snapshot(json!({ "outputQuietMs": 500 }));
    context.advance(ARRIVAL_MS);
    context.pass().unwrap();
    assert_eq!(enters(&context), 0, "not into a window that just printed");

    // Quiet at last, minutes on, and the record is still waited for: that window
    // is past, and no Enter is pressed into a turn for the paste it holds.
    context.host.set_snapshot(json!({ "outputQuietMs": 5_000 }));
    for _ in 0..6 {
        context.advance(ARRIVAL_MS);
        context.pass().unwrap();
    }
    assert_eq!(enters(&context), 0, "none once the arrival window is past");
    assert_eq!(context.message(note.id).attempts, 1);
}

#[test]
fn pastes_a_lost_paste_again_after_a_turn_of_its_windows_own_and_the_queue_behind_it_goes_on() {
    let context = Context::new();
    let project = holding(&context);
    let pasted = counting(&context);
    let first = context.note(project, "zeus", "chief", "first");
    let second = context.note(project, "zeus", "chief", "second");
    context.pass().unwrap();
    assert_eq!(context.message(first.id).state, "delivering");
    assert_eq!(pasted.get(), 1, "the second waits behind the first");

    // The paste was lost. The window goes into a turn that is none of its own
    // (the human typed to it, or it began one by itself) and that goes on for
    // five minutes: it is no sign the paste landed.
    context.adapter.busy("chief");
    for _ in 0..10 {
        context.advance(ARRIVAL_MS);
        context.pass().unwrap();
    }
    assert_eq!(context.message(first.id).attempts, 1);
    assert_eq!(context.message(second.id).state, "queued");

    // The turn ends, and the record gets a whole window of rest from there: not
    // a moment less, and then the paste is sent again, which lands this time.
    context.adapter.with("chief", |agent| agent.settled = true);
    context.pass().unwrap();
    context.advance(ARRIVAL_MS - 1_000);
    context.pass().unwrap();
    assert_eq!(context.message(first.id).attempts, 1);
    context.adapter.with("chief", |agent| agent.arrive = true);
    context.advance(2_000);
    context.pass().unwrap();
    assert_eq!(pasted.get(), 2, "pasted again once");
    context.pass().unwrap();
    let delivered = context.message(first.id);
    assert_eq!(
        (delivered.state.as_str(), delivered.attempts),
        ("delivered", 2)
    );

    // Nothing stalled: the next message goes in as soon as the window is free.
    context.adapter.answer("chief", "noted");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.message(second.id).state, "delivered");
    let items = given(&context, "chief");
    assert_eq!(items.len(), 2, "{items:?}");
    for (text, note) in items.iter().zip([&first, &second]) {
        assert_eq!(text.matches(&marker_of(note.id)).count(), 1, "{text}");
    }
}

#[test]
fn waits_on_a_message_the_harness_says_its_queue_holds_whatever_its_window_does_and_its_record_shows(
) {
    let context = Context::new();
    let project = holding(&context);
    let pasted = counting(&context);
    let note = context.note(project, "zeus", "chief", "hello");
    context.pass().unwrap();
    assert_eq!(pasted.get(), 1);

    // The harness has said nothing of it, and the window is at rest: a paste to
    // be tried again. Then its queue says it has it (Devin's wire log shows its TUI
    // put the message to its agent), and the window stays at rest, for minutes.
    context.adapter.with("chief", |agent| agent.took = true);
    context.pass().unwrap();
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

    // The harness runs it, and its record shows it.
    shows(&context, "chief", &waiting);
    context.pass().unwrap();
    assert_eq!(context.message(note.id).state, "delivered");
    assert_eq!(pasted.get(), 1);
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

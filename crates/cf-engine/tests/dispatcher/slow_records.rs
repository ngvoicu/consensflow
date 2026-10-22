//! A message handed over to a window whose harness holds what it is given while
//! its agent is in a turn (what comes in waits, and the harness's record shows it
//! when the agent runs it, a step or a turn on) is waited for, never pasted again
//! for a record that is late: only the window's rest counts towards the arrival
//! window, so the turns the window goes through are no time the record had to show
//! the message. A paste into a
//! window that stayed at rest and that the record never shows is still tried
//! again (`deliveries`), whatever turns the window went through since: the wait is
//! bounded by the window's rest, and nothing a window does stops the queue behind it.
//! However often the window goes into a turn, a paste its record never showed is
//! pasted again at the first rest past half an hour from its hand-over. A window
//! whose harness says the paste is in a turn it is in (`Window::held`: the turn it
//! went into, or the one the harness began with it) is in that turn, however at rest
//! it reads, as long as the harness says so, and the paste never goes again
//! meanwhile, however old: once that turn ends, its record has a whole arrival window
//! of rest, and the half hour runs from the first look that found it held no longer.
//! One whose harness says it put the paste to its agent (`Window::took`) is waited
//! for until its record shows it. The one more Enter a paste is given (`deliveries`)
//! is pressed in the first arrival window only, whatever the wait after it.
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
fn pastes_again_at_its_next_rest_a_paste_never_shown_in_half_an_hour_of_short_turns() {
    let context = Context::new();
    let project = holding(&context);
    let pasted = counting(&context);
    let note = context.note(project, "zeus", "chief", "hello");
    context.pass().unwrap();

    // Twenty seconds of rest and a short turn, over and over: no rest is ever a
    // whole arrival window, and nothing shows the paste.
    for _ in 0..60 {
        context.adapter.with("chief", |agent| agent.settled = true);
        context.advance(20_000);
        context.pass().unwrap();
        context.adapter.busy("chief");
        context.advance(10_000);
        context.pass().unwrap();
    }
    assert_eq!(context.message(note.id).attempts, 1);
    assert_eq!(pasted.get(), 1);

    // Past half an hour, the next look at rest pastes it again and says why.
    context.adapter.with("chief", |agent| agent.settled = true);
    context.advance(1_000);
    context.pass().unwrap();
    let again = context.message(note.id);
    assert_eq!((again.state.as_str(), again.attempts), ("delivering", 2));
    assert_eq!(pasted.get(), 2, "pasted again once");
    assert_eq!(
        why_retried(&context, project),
        "the harness record never showed it in the 30 min since it was handed over"
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

/// The report of 2026-10-15 (B5): a result went into a Devin chief's window twice. A
/// turn whose agent says nothing reads at rest, and what was pasted into it is held
/// until it ends.
#[test]
fn counts_no_rest_while_the_harness_holds_the_paste_in_the_turn_it_went_into_and_pastes_it_once() {
    let context = Context::new();
    let project = holding(&context);
    let pasted = counting(&context);
    let note = context.note(project, "zeus", "chief", "hello");
    context.pass().unwrap();
    assert_eq!(pasted.get(), 1);

    // The window reads at rest for five minutes, ten arrival windows, and its harness
    // says the paste went into the turn it is in still.
    context.adapter.with("chief", |agent| agent.held = true);
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

    // The turn ends: the record gets a whole arrival window of rest from there, and
    // the harness runs the message within it.
    context.adapter.with("chief", |agent| agent.held = false);
    context.pass().unwrap();
    context.advance(ARRIVAL_MS - 1_000);
    context.pass().unwrap();
    assert_eq!(context.message(note.id).attempts, 1);
    shows(&context, "chief", &waiting);
    context.pass().unwrap();
    let delivered = context.message(note.id);
    assert_eq!(
        (delivered.state.as_str(), delivered.attempts),
        ("delivered", 1)
    );
    assert_eq!(pasted.get(), 1);
}

/// The reviews of 2026-10-16: past half an hour, a paste held in a turn whose agent
/// said nothing went again into that turn, and both ran when it ended.
#[test]
fn never_pastes_again_a_paste_held_in_a_turn_however_long_and_gives_its_record_a_window_when_it_ends(
) {
    let context = Context::new();
    let project = holding(&context);
    let pasted = counting(&context);
    let note = context.note(project, "zeus", "chief", "hello");
    let behind = context.note(project, "zeus", "chief", "behind");
    context.pass().unwrap();

    // A turn whose agent never says a word holds the paste, and the window reads at
    // rest all the while: an hour of it, and the queue behind it waits.
    context.adapter.with("chief", |agent| agent.held = true);
    for _ in 0..120 {
        context.advance(ARRIVAL_MS);
        context.pass().unwrap();
    }
    let waiting = context.message(note.id);
    assert_eq!(
        (waiting.state.as_str(), waiting.attempts),
        ("delivering", 1)
    );
    assert_eq!(pasted.get(), 1, "never pasted again while it is held");
    assert_eq!(context.message(behind.id).state, "queued");

    // The turn ends (the human stopped it): the record has a whole arrival window
    // of rest from there, the half hour long past as it is.
    context.adapter.with("chief", |agent| agent.held = false);
    context.pass().unwrap();
    context.advance(ARRIVAL_MS - 1_000);
    context.pass().unwrap();
    assert_eq!(pasted.get(), 1, "not at the turn's end");
    // It never shows: it goes again, once, and says why.
    context.advance(2_000);
    context.pass().unwrap();
    let again = context.message(note.id);
    assert_eq!((again.state.as_str(), again.attempts), ("delivering", 2));
    assert_eq!(pasted.get(), 2, "pasted again once");
    assert_eq!(
        why_retried(&context, project),
        format!(
            "the harness record never showed it in the {} s its window was at rest since its last turn",
            ARRIVAL_MS / 1000
        )
    );
}

/// A turn of the chief's holds the paste for a minute, two looks of it, and ends.
fn held_for_a_minute(context: &Context) {
    context.adapter.with("chief", |agent| agent.held = true);
    for _ in 0..2 {
        context.advance(ARRIVAL_MS);
        context.pass().unwrap();
    }
    context.adapter.with("chief", |agent| agent.held = false);
}

/// Twenty seconds of rest, looked at every five, and a turn of ten that holds
/// nothing, `rounds` times: fifteen seconds the looks saw at rest each.
fn short_turns(context: &Context, rounds: usize) {
    for _ in 0..rounds {
        context.adapter.with("chief", |agent| agent.settled = true);
        for _ in 0..4 {
            context.advance(5_000);
            context.pass().unwrap();
        }
        context.adapter.busy("chief");
        context.advance(10_000);
        context.pass().unwrap();
    }
}

/// The reviews of 2026-10-16: Devin dropped a paste, and the turn the human began next
/// held it. Once that turn ended, turns of ten seconds twenty seconds apart kept every
/// rest short of an arrival window, and the paste never went again, its queue behind
/// it. The half hour runs from the first look that found the paste held no longer, in
/// time and not in rest, which no such rhythm of turns ever adds up to: a dropped paste
/// goes again within a bound of the turn that held it (`schedules`' never-for-good,
/// 41 minutes), which rest the looks saw (round 8's count, an hour here) would break. A
/// second turn that holds it begins the half hour again.
#[test]
fn pastes_again_half_an_hour_after_the_turn_that_held_it_ended_a_paste_short_turns_keep_unshown() {
    let context = Context::new();
    let project = holding(&context);
    let pasted = counting(&context);
    let note = context.note(project, "zeus", "chief", "hello");
    let behind = context.note(project, "zeus", "chief", "behind");
    context.pass().unwrap();

    // A turn holds it, then just under half an hour of short turns, then a turn holds
    // it again.
    held_for_a_minute(&context);
    short_turns(&context, 59);
    held_for_a_minute(&context);

    // Half an hour of short turns since the first look that found it free, and nothing
    // shows the paste.
    short_turns(&context, 60);
    assert_eq!(context.message(note.id).attempts, 1);
    assert_eq!(pasted.get(), 1);
    assert_eq!(context.message(behind.id).state, "queued");

    // Past half an hour of it, the next look at rest pastes it again and says why.
    context.adapter.with("chief", |agent| {
        agent.settled = true;
        agent.arrive = true;
    });
    context.advance(5_000);
    context.pass().unwrap();
    assert_eq!(
        pasted.get(),
        1,
        "half an hour to the millisecond is not past it"
    );
    context.advance(5_000);
    context.pass().unwrap();
    assert_eq!(pasted.get(), 2, "pasted again once");
    assert_eq!(
        why_retried(&context, project),
        "the harness record never showed it in the 30 min since the turn that held it ended"
    );
    context.pass().unwrap();
    let delivered = context.message(note.id);
    assert_eq!(
        (delivered.state.as_str(), delivered.attempts),
        ("delivered", 2)
    );

    // The queue behind it goes on.
    context.adapter.answer("chief", "noted");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.message(behind.id).state, "delivered");
}

/// The paste of a window whose looks stop for 31 minutes and go on at rest is pasted
/// again only after a whole arrival window of rest from the first look after.
fn pasted_again_only_after_a_real_rest(context: &Context, project: i64, pasted: &Cell<usize>) {
    context.advance(31 * 60_000);
    context.adapter.with("chief", |agent| agent.settled = true);
    context.pass().unwrap();
    assert_eq!(pasted.get(), 1, "not at the first look after the gap");
    context.advance(ARRIVAL_MS - 1_000);
    context.pass().unwrap();
    assert_eq!(pasted.get(), 1, "nor before a whole arrival window of rest");
    context.advance(2_000);
    context.pass().unwrap();
    assert_eq!(pasted.get(), 2);
    assert_eq!(
        why_retried(context, project),
        format!(
            "the harness record never showed it in the {} s its window was at rest since its last turn",
            ARRIVAL_MS / 1000
        )
    );
}

/// A review of 2026-10-16: the looks stopped for 31 minutes (a laptop asleep) while a
/// turn held the paste, and the turn ended just before the first look after them.
/// Counted from the last look that found it held, the half hour was past, and the
/// paste went again at once, with no rest: Devin ran both copies.
#[test]
fn counts_no_rest_in_a_gap_in_the_looks_that_began_in_the_turn_that_held_the_paste() {
    let context = Context::new();
    let project = holding(&context);
    let pasted = counting(&context);
    context.note(project, "zeus", "chief", "hello");
    context.pass().unwrap();
    context.adapter.with("chief", |agent| agent.held = true);
    context.advance(ARRIVAL_MS);
    context.pass().unwrap();

    // No look for 31 minutes, and the turn ends just before the next.
    context.adapter.with("chief", |agent| agent.held = false);
    pasted_again_only_after_a_real_rest(&context, project, &pasted);
}

/// The same of a hand-over into a turn that holds the paste, with no look between: the
/// harness says at the hand-over that it holds it, so the time from the hand-over is no
/// rest of a paste that was free (Astra's review of 2026-10-17: the looks stopped right
/// after the hand-over, the turn ended just before the next, and the paste went again
/// while Devin had it queued).
#[test]
fn counts_no_rest_in_a_gap_in_the_looks_from_a_hand_over_into_a_turn_that_held_the_paste() {
    let context = Context::new();
    let project = holding(&context);
    let pasted = counting(&context);
    context.adapter.with("chief", |agent| agent.held = true);
    context.note(project, "zeus", "chief", "hello");
    context.pass().unwrap();
    assert_eq!(pasted.get(), 1);

    // No look for 31 minutes, and the turn ends just before the next.
    context.adapter.with("chief", |agent| agent.held = false);
    pasted_again_only_after_a_real_rest(&context, project, &pasted);
}

/// A gap that began after the turn that held the paste ended is time the paste was free:
/// the half hour counts it. A paste Devin did not drop is in the window by then, and its
/// harness says so (`Window::took`), so it is never pasted again, however long the looks
/// stop; one it dropped goes again at the first look at rest past the half hour.
fn free_then_a_gap(context: &Context, took: bool) -> (i64, Rc<Cell<usize>>) {
    let project = holding(context);
    let pasted = counting(context);
    context.note(project, "zeus", "chief", "hello");
    context.pass().unwrap();
    context.adapter.with("chief", |agent| agent.held = true);
    context.advance(ARRIVAL_MS);
    context.pass().unwrap();
    // The turn that held it ends, and Devin runs the paste or drops it.
    context.adapter.with("chief", |agent| {
        agent.held = false;
        agent.took = took;
    });
    context.advance(5_000);
    context.pass().unwrap();

    // A turn of its own or of the human's, and no look for 31 minutes.
    context.adapter.busy("chief");
    context.advance(5_000);
    context.pass().unwrap();
    context.advance(31 * 60_000);
    context.adapter.with("chief", |agent| agent.settled = true);
    context.pass().unwrap();
    (project, pasted)
}

#[test]
fn pastes_again_after_a_gap_in_the_looks_a_paste_its_harness_dropped_after_the_turn_that_held_it() {
    let context = Context::new();
    let (project, pasted) = free_then_a_gap(&context, false);
    assert_eq!(
        pasted.get(),
        2,
        "pasted again at the first look at rest past the half hour"
    );
    assert_eq!(
        why_retried(&context, project),
        "the harness record never showed it in the 30 min since the turn that held it ended"
    );
}

#[test]
fn never_pastes_again_after_a_gap_in_the_looks_a_paste_its_harness_took_after_the_turn_that_held_it(
) {
    let context = Context::new();
    let (_, pasted) = free_then_a_gap(&context, true);
    for _ in 0..4 {
        context.advance(ARRIVAL_MS);
        context.pass().unwrap();
    }
    context.advance(31 * 60_000);
    context.pass().unwrap();
    assert_eq!(pasted.get(), 1, "its record shows it when it does");
}

/// A paste Devin ran at once between two looks, its record late (Vali's schedules of
/// 2026-10-17: a regression against main, which waited once the wire said the TUI had
/// put the paste to its agent): the arrival window from the hand-over is past before
/// the record shows it, and the harness's word that its agent has it is what waits.
#[test]
fn waits_for_the_record_of_a_paste_its_harness_put_to_its_agent_however_late_it_shows() {
    let context = Context::new();
    let project = holding(&context);
    let pasted = counting(&context);
    let note = context.note(project, "zeus", "chief", "hello");
    context.pass().unwrap();
    context.adapter.with("chief", |agent| agent.took = true);
    for _ in 0..3 {
        context.advance(ARRIVAL_MS);
        context.pass().unwrap();
    }
    assert_eq!(pasted.get(), 1, "not pasted again");
    shows(&context, "chief", &context.message(note.id));
    context.pass().unwrap();
    let delivered = context.message(note.id);
    assert_eq!(
        (delivered.state.as_str(), delivered.attempts),
        ("delivered", 1)
    );
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

/// The reviews of 2026-10-18: Devin's TUI put the paste to its agent (`Window::took`), and
/// the human ran `/new` before the record showed it. The window was followed to the new
/// conversation with the paste still on its way, whose record never shows it, and the
/// window's queue waited behind it for good. The conversation it went into says what
/// came of it.
fn taken_then_switched(context: &Context) -> (i64, MessageView, Rc<Cell<usize>>, String) {
    switched(context, true)
}

/// A paste handed over, then the human's `/new`: `took` where its harness said it took
/// the paste first.
fn switched(context: &Context, took: bool) -> (i64, MessageView, Rc<Cell<usize>>, String) {
    let project = holding(context);
    let pasted = counting(context);
    let note = context.note(project, "zeus", "chief", "hello");
    context.pass().unwrap();
    context.adapter.with("chief", |agent| agent.took = took);
    context.advance(5_000);
    context.pass().unwrap();
    let left = context.adapter.with("chief", |agent| agent.native.clone());
    context.adapter.switch_to("chief", "native-new");
    context.advance(1_000);
    context.pass().unwrap();
    assert_eq!(pasted.get(), 1);
    (project, note, pasted, left)
}

#[test]
fn gives_back_to_its_queue_a_paste_taken_in_a_conversation_the_human_left_once_its_record_rests_without_it(
) {
    let context = Context::new();
    let (project, note, pasted, _) = taken_then_switched(&context);
    // The first look on the conversation it shows now reads the one left at rest.
    context.advance(1_000);
    context.pass().unwrap();
    context.advance(ARRIVAL_MS - 1_000);
    context.pass().unwrap();
    assert_eq!(
        pasted.get(),
        1,
        "its record has had no arrival window of rest yet"
    );
    context.advance(2_000);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(
        pasted.get(),
        2,
        "pasted into the conversation the window shows now"
    );
    assert_eq!(
        why_retried(&context, project),
        "its window was switched to another conversation, whose record never showed it"
    );
    let again = context.message(note.id);
    assert_eq!(
        (again.state.as_str(), again.attempts),
        ("delivering", 1),
        "given back with its attempt"
    );
}

#[test]
fn confirms_a_paste_taken_in_a_conversation_the_human_left_when_its_record_shows_it() {
    let context = Context::new();
    let (_, note, pasted, left) = taken_then_switched(&context);
    // The conversation left runs it: its record, at work, shows it, however late.
    context.adapter.with("chief", |agent| {
        agent.records_settled.insert(left.clone(), false);
    });
    for _ in 0..4 {
        context.advance(ARRIVAL_MS);
        context.pass().unwrap();
    }
    assert_eq!(pasted.get(), 1, "a record at work is waited for");
    assert_eq!(context.message(note.id).state, "delivering");
    let item = context
        .adapter
        .item(Role::User, &delivery_text(&context.message(note.id), &[]));
    context.adapter.with("chief", |agent| {
        agent.records.entry(left.clone()).or_default().push(item);
    });
    context.pass().unwrap();
    let delivered = context.message(note.id);
    assert_eq!(
        (delivered.state.as_str(), delivered.attempts),
        ("delivered", 1)
    );
    assert_eq!(pasted.get(), 1, "never pasted again");
}

/// The review of c2e29f2d (P1): Devin's reader answers unknown for a session its store
/// holds no row of, which it writes with the session's first message. The human ran
/// `/new`, a paste went into the fresh conversation, and the human switched again before
/// Devin ran it: that conversation's record could never be read, and the paste waited
/// for good. A record that cannot be read is at rest with nothing in it, as a look reads
/// one: the paste goes back after a whole arrival window.
#[test]
fn gives_back_a_paste_taken_in_a_conversation_the_human_left_whose_record_cannot_be_read() {
    let context = Context::new();
    let (project, note, pasted, left) = taken_then_switched(&context);
    context
        .adapter
        .with("chief", |agent| agent.records_unreadable.push(left.clone()));
    context.advance(1_000);
    context.pass().unwrap();
    context.advance(ARRIVAL_MS - 1_000);
    context.pass().unwrap();
    assert_eq!(pasted.get(), 1, "a whole arrival window first");
    context.advance(2_000);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(pasted.get(), 2, "pasted into the conversation shown now");
    assert_eq!(
        why_retried(&context, project),
        "its window was switched to another conversation, whose record never showed it"
    );
    assert_eq!(context.message(note.id).attempts, 1, "with its attempt");
}

/// The review of c2e29f2d (P2): a conversation the human left in the middle of a turn may
/// read at work for good, no end ever written for it. Its record unchanged for half an
/// hour, the paste goes back; a record that changes is a turn that goes on.
#[test]
fn gives_back_a_paste_in_a_conversation_left_at_work_once_its_record_has_not_changed_for_half_an_hour(
) {
    // A harness that says nothing of having taken it: the half hour is its bound.
    let context = Context::new();
    let (project, note, pasted, left) = switched(&context, false);
    context.adapter.with("chief", |agent| {
        agent.records_settled.insert(left.clone(), false);
    });
    for _ in 0..50 {
        context.advance(ARRIVAL_MS);
        context.pass().unwrap();
    }
    assert_eq!(pasted.get(), 1, "25 minutes at work");
    // It writes something: a turn that goes on, and the half hour starts again.
    let item = context.adapter.item(Role::Assistant, "still at it");
    context.adapter.with("chief", |agent| {
        agent.records.entry(left.clone()).or_default().push(item);
    });
    for _ in 0..50 {
        context.advance(ARRIVAL_MS);
        context.pass().unwrap();
    }
    assert_eq!(pasted.get(), 1, "25 minutes from its last change");
    for _ in 0..12 {
        context.advance(ARRIVAL_MS);
        context.pass().unwrap();
    }
    context.pass().unwrap();
    assert_eq!(pasted.get(), 2, "half an hour unchanged: the switch cut it");
    assert_eq!(
        why_retried(&context, project),
        "its window was switched to another conversation, whose record read a turn at work, unchanged, and never showed it"
    );
    assert_eq!(context.message(note.id).attempts, 1);
}

/// The review of c2e29f2d (P3): the window closed while a paste it had taken was in the
/// conversation the human left. Its end read the conversation shown, which never holds
/// it, and the next window was given it again, though the conversation left had run it.
#[test]
fn a_window_that_closes_with_a_paste_in_the_conversation_left_settles_it_by_that_record() {
    let context = Context::new();
    let (_, note, pasted, left) = taken_then_switched(&context);
    let item = context
        .adapter
        .item(Role::User, &delivery_text(&context.message(note.id), &[]));
    context.adapter.with("chief", |agent| {
        agent.records_settled.insert(left.clone(), false);
        agent.records.entry(left.clone()).or_default().push(item);
    });
    context.exit("chief");
    for _ in 0..3 {
        context.pass().unwrap();
        context.advance(ARRIVAL_MS);
    }
    let delivered = context.message(note.id);
    assert_eq!(
        (delivered.state.as_str(), delivered.attempts),
        ("delivered", 1)
    );
    assert_eq!(pasted.get(), 1, "never pasted again");
}

/// The review of 6c8fc595: Devin's `/new` cuts no turn (T-107's probe): it switches once
/// the turn ends, and runs the paste queued behind it in the conversation left. A paste
/// its harness said it took, behind a step longer than half an hour, was given back at the
/// half hour and then run in the old conversation as well. Its harness's word holds it:
/// the record is waited for, however long it reads at work, and confirms it when it shows.
#[test]
fn waits_past_half_an_hour_on_a_paste_its_harness_took_in_a_conversation_left_at_work() {
    let context = Context::new();
    let (_, note, pasted, left) = taken_then_switched(&context);
    context.adapter.with("chief", |agent| {
        agent.records_settled.insert(left.clone(), false);
    });
    for _ in 0..130 {
        context.advance(ARRIVAL_MS);
        context.pass().unwrap();
    }
    assert_eq!(pasted.get(), 1, "an hour at work: never given back");
    assert_eq!(context.message(note.id).state, "delivering");
    let item = context
        .adapter
        .item(Role::User, &delivery_text(&context.message(note.id), &[]));
    context.adapter.with("chief", |agent| {
        agent.records.entry(left.clone()).or_default().push(item);
    });
    context.pass().unwrap();
    let delivered = context.message(note.id);
    assert_eq!(
        (delivered.state.as_str(), delivered.attempts),
        ("delivered", 1)
    );
    assert_eq!(pasted.get(), 1);
}

/// The review of 6c8fc595 (the second half of c2e29f2d's P3): a paste its harness took,
/// the human's `/new`, the conversation left ran the paste, and the daemon stopped before
/// a look read that record. The start read the conversation the window shows now, which
/// never holds it, gave it back and pasted it again there. The ledger keeps the
/// conversation each delivery went into, and the start reads that one.
#[test]
fn a_restart_settles_a_paste_by_the_conversation_it_went_into_not_the_one_the_window_shows_now() {
    let context = Context::new();
    let (project, note, pasted, left) = taken_then_switched(&context);
    let item = context
        .adapter
        .item(Role::User, &delivery_text(&context.message(note.id), &[]));
    context.adapter.with("chief", |agent| {
        agent.records.entry(left.clone()).or_default().push(item);
    });
    context.ledger.borrow_mut().suspend_for_restart().unwrap();
    let after = context.make();
    after.settle_after_restart().unwrap();
    let settled = context.message(note.id);
    assert_eq!(
        (settled.state.as_str(), settled.attempts),
        ("delivered", 1),
        "the conversation it went into ran it"
    );
    after.resume_project(project).unwrap();
    after.pass().unwrap();
    after.pass().unwrap();
    assert_eq!(pasted.get(), 1, "never pasted again");
}

/// Calliope's look at round 3 (T-275): Devin holds a paste queued behind a turn and writes
/// it to its record only when the turn takes it in. A paste its TUI took, the human's
/// `/new`, and the daemon stopped while the conversation left still worked: its record
/// did not show the paste yet, the start gave it back, and it went into the new
/// conversation as well as the old. Devin's store has the history row of what its TUI
/// took, which the start reads with no window open, and the paste is delivered.
#[test]
fn a_restart_delivers_a_paste_its_harness_took_though_the_conversation_it_went_into_does_not_show_it_yet(
) {
    let context = Context::new();
    let (project, note, pasted, left) = taken_then_switched(&context);
    let text = delivery_text(&context.message(note.id), &[]);
    context.adapter.with("chief", |agent| {
        agent.records_settled.insert(left.clone(), false);
        agent.stored.insert(left.clone(), vec![text]);
    });
    context.ledger.borrow_mut().suspend_for_restart().unwrap();
    let after = context.make();
    after.settle_after_restart().unwrap();
    let settled = context.message(note.id);
    assert_eq!(
        (settled.state.as_str(), settled.attempts),
        ("delivered", 1),
        "its harness took it"
    );
    after.resume_project(project).unwrap();
    after.pass().unwrap();
    after.pass().unwrap();
    assert_eq!(pasted.get(), 1, "never pasted again");
}

/// The same, with a store that has no row of the paste: its harness never took it, and
/// it goes back to its queue as it always did.
#[test]
fn a_restart_gives_back_a_paste_its_harness_says_it_never_took() {
    let context = Context::new();
    let (_, note, _, left) = taken_then_switched(&context);
    context.adapter.with("chief", |agent| {
        agent
            .stored
            .insert(left.clone(), vec!["another prompt".to_owned()]);
    });
    context.ledger.borrow_mut().suspend_for_restart().unwrap();
    context.make().settle_after_restart().unwrap();
    let settled = context.message(note.id);
    assert_eq!((settled.state.as_str(), settled.attempts), ("queued", 0));
}

//! Paced delivery (3.0.7 chief-meeting, CMT-03 to CMT-08): a message is typed into
//! a window only once its harness reads it at rest, from a record that was read,
//! the same rest on the same last item for the settle, with no key of the human's
//! in that time, and a fresh look just before the paste still reads it so. A look
//! that reads anything else starts the settle again, and nothing is spent: no
//! delivery begins, and no attempt is counted. A launch's prelude and the chief's
//! urgent tell go as they did, at the first look at rest.

use std::rc::Rc;

use cf_engine::testing::{Context, Made};
use cf_harness::contract::{Admission, InputBox, Readiness};
use cf_harness::records::Role;
use serde_json::{json, Value};

const SETTLE: i64 = 3_000;

fn paced() -> Context {
    Context::made(Made {
        settle_ms: SETTLE,
        ..Made::default()
    })
}

/// The messages pasted into the window of `handle`, by their ids, oldest first.
fn pasted(context: &Context, handle: &str) -> Vec<i64> {
    context
        .adapter
        .agent(handle)
        .items
        .iter()
        .filter(|item| item.role == Role::User)
        .filter_map(|item| {
            let rest = item.text.strip_prefix("[ConsensFlow m-")?;
            rest.split(' ').next()?.parse().ok()
        })
        .collect()
}

/// The ledger's events of `kind`, as it logged their data.
fn logged(context: &Context, kind: &str) -> Vec<Value> {
    context
        .recorder
        .events()
        .into_iter()
        .filter(|event| event["seam"] == "event" && event["event"]["kind"] == kind)
        .map(|event| event["event"]["data"].clone())
        .collect()
}

/// A project whose chief's window is up and at rest, its staff zeus.
fn chief_at_rest(context: &Context) -> i64 {
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    context.adapter.with("chief", |agent| agent.settled = true);
    context.pass().unwrap();
    project.id
}

#[test]
fn a_message_waits_until_the_window_has_read_at_rest_for_the_settle() {
    let context = paced();
    let project = chief_at_rest(&context);
    let note = context.note(project, "zeus", "chief", "One");
    context.pass().unwrap();
    assert_eq!(
        pasted(&context, "chief"),
        Vec::<i64>::new(),
        "its rest begins"
    );
    context.advance(SETTLE - 1);
    context.pass().unwrap();
    assert_eq!(pasted(&context, "chief"), Vec::<i64>::new(), "not yet");
    context.advance(1);
    context.pass().unwrap();
    assert_eq!(pasted(&context, "chief"), [note.id]);
    assert_eq!(context.message(note.id).attempts, 1);
}

#[test]
fn two_messages_go_a_settle_apart_each_after_a_turn_has_ended() {
    let context = paced();
    let project = chief_at_rest(&context);
    let first = context.note(project, "zeus", "chief", "One");
    let second = context.note(project, "zeus", "chief", "Two");
    context.pass().unwrap();
    context.advance(SETTLE);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(pasted(&context, "chief"), [first.id]);
    assert_eq!(context.message(first.id).state, "delivered");
    // The chief's turn on the first ends: the second waits a whole settle from then.
    context.adapter.answer("chief", "Noted.");
    context.pass().unwrap();
    context.advance(SETTLE - 1);
    context.pass().unwrap();
    assert_eq!(pasted(&context, "chief"), [first.id]);
    context.advance(1);
    context.pass().unwrap();
    assert_eq!(pasted(&context, "chief"), [first.id, second.id]);
    assert_eq!(context.message(second.id).attempts, 1);
}

#[test]
fn a_new_last_item_starts_the_settle_again() {
    let context = paced();
    let project = chief_at_rest(&context);
    let note = context.note(project, "zeus", "chief", "One");
    context.pass().unwrap();
    context.advance(SETTLE - 1_000);
    // The record has an item more, its turn read ended still (a turn the harness
    // began and ended between two looks).
    context.adapter.answer("chief", "Something of its own.");
    context.pass().unwrap();
    context.advance(SETTLE - 1_000);
    context.pass().unwrap();
    assert_eq!(pasted(&context, "chief"), Vec::<i64>::new());
    context.advance(1_000);
    context.pass().unwrap();
    assert_eq!(pasted(&context, "chief"), [note.id]);
}

#[test]
fn a_look_that_reads_the_window_at_work_starts_the_settle_again() {
    let context = paced();
    let project = chief_at_rest(&context);
    let note = context.note(project, "zeus", "chief", "One");
    context.pass().unwrap();
    context.advance(SETTLE - 1_000);
    context.adapter.busy("chief");
    context.pass().unwrap();
    context.adapter.with("chief", |agent| agent.settled = true);
    context.pass().unwrap();
    context.advance(SETTLE - 1_000);
    context.pass().unwrap();
    assert_eq!(pasted(&context, "chief"), Vec::<i64>::new());
    context.advance(1_000);
    context.pass().unwrap();
    assert_eq!(pasted(&context, "chief"), [note.id]);
}

#[test]
fn a_key_of_the_humans_within_the_settle_holds_the_message() {
    let context = paced();
    let project = chief_at_rest(&context);
    let note = context.note(project, "zeus", "chief", "One");
    context.pass().unwrap();
    context.advance(SETTLE * 2);
    context
        .host
        .set_snapshot(json!({ "typedQuietMs": SETTLE - 1 }));
    context.pass().unwrap();
    assert_eq!(pasted(&context, "chief"), Vec::<i64>::new());
    assert_eq!(context.message(note.id).state, "queued");
    assert_eq!(context.message(note.id).attempts, 0, "no attempt is spent");
    context.host.set_snapshot(json!({ "typedQuietMs": SETTLE }));
    context.pass().unwrap();
    assert_eq!(pasted(&context, "chief"), [note.id]);
}

#[test]
fn a_fresh_look_before_the_paste_that_finds_a_new_item_hands_nothing_over() {
    let context = paced();
    let project = chief_at_rest(&context);
    let note = context.note(project, "zeus", "chief", "One");
    context.pass().unwrap();
    context.advance(SETTLE);
    // Between the look that let it go and the paste, the window began and ended a
    // turn of its own: the record's last item is another.
    let adapter = Rc::downgrade(&context.adapter);
    *context.adapter.ready.borrow_mut() = Some(Rc::new(move || {
        if let Some(adapter) = adapter.upgrade() {
            if adapter.agent("chief").items.is_empty() {
                adapter.answer("chief", "A turn of its own.");
            }
        }
        Ok(Readiness::Ready)
    }));
    context.pass().unwrap();
    assert_eq!(pasted(&context, "chief"), Vec::<i64>::new());
    assert_eq!(context.message(note.id).state, "queued");
    assert_eq!(context.message(note.id).attempts, 0, "no attempt is spent");
    assert_eq!(logged(&context, "delivery.begun"), Vec::<Value>::new());
    context.pass().unwrap();
    context.advance(SETTLE);
    context.pass().unwrap();
    assert_eq!(pasted(&context, "chief"), [note.id]);
}

#[test]
fn a_fresh_look_before_the_paste_that_finds_the_window_at_work_hands_nothing_over() {
    let context = paced();
    let project = chief_at_rest(&context);
    let note = context.note(project, "zeus", "chief", "One");
    context.pass().unwrap();
    context.advance(SETTLE);
    let adapter = Rc::downgrade(&context.adapter);
    *context.adapter.ready.borrow_mut() = Some(Rc::new(move || {
        if let Some(adapter) = adapter.upgrade() {
            adapter.busy("chief");
        }
        Ok(Readiness::Ready)
    }));
    context.pass().unwrap();
    assert_eq!(pasted(&context, "chief"), Vec::<i64>::new());
    assert_eq!(context.message(note.id).attempts, 0, "no attempt is spent");
}

/// The rule is the same with no settle: a record that could not be read says
/// nothing of the window, so nothing goes in on it.
#[test]
fn an_unreadable_record_is_never_rest_with_a_settle_or_without() {
    for context in [paced(), Context::new()] {
        let project = chief_at_rest(&context);
        context.adapter.unread("chief", true);
        let note = context.note(project, "zeus", "chief", "One");
        for _ in 0..3 {
            context.advance(SETTLE);
            context.pass().unwrap();
        }
        assert_eq!(pasted(&context, "chief"), Vec::<i64>::new());
        assert_eq!(context.message(note.id).attempts, 0);
        context.adapter.unread("chief", false);
        context.pass().unwrap();
        context.advance(SETTLE);
        context.pass().unwrap();
        assert_eq!(pasted(&context, "chief"), [note.id]);
    }
}

#[test]
fn the_look_that_lets_a_message_go_is_in_its_delivery_begun() {
    let context = paced();
    let project = chief_at_rest(&context);
    context.adapter.answer("chief", "Ready.");
    let last = context
        .adapter
        .agent("chief")
        .items
        .last()
        .unwrap()
        .id
        .clone();
    context.host.set_snapshot(json!({ "typedQuietMs": 9_000 }));
    let note = context.note(project, "zeus", "chief", "One");
    context.pass().unwrap();
    context.advance(SETTLE + 500);
    context.pass().unwrap();
    let begun = logged(&context, "delivery.begun");
    assert_eq!(begun.len(), 1);
    let look = &begun[0]["look"];
    assert_eq!(begun[0]["message"], json!(note.id));
    assert_eq!(look["status"], json!("idle"));
    assert_eq!(look["restMs"], json!(SETTLE + 500));
    assert_eq!(look["typedQuietMs"], json!(9_000));
    assert_eq!(look["lastItem"], json!(&*last));
    assert_eq!(look["items"], json!(1));
}

#[test]
fn a_harness_with_no_idle_word_of_its_own_waits_for_a_quiet_screen_too_and_says_so() {
    let context = paced();
    context.adapter.idle_word.set(false);
    let project = chief_at_rest(&context);
    let chief = context.id(project, "chief");
    assert_eq!(
        context.dispatcher.activity(chief).reason.as_deref(),
        Some("at rest by its record and a quiet screen: its harness says nothing of its own")
    );
    let note = context.note(project, "zeus", "chief", "One");
    context
        .host
        .set_snapshot(json!({ "outputQuietMs": SETTLE - 1 }));
    context.pass().unwrap();
    context.advance(SETTLE);
    context.pass().unwrap();
    assert_eq!(pasted(&context, "chief"), Vec::<i64>::new());
    context
        .host
        .set_snapshot(json!({ "outputQuietMs": SETTLE }));
    context.pass().unwrap();
    assert_eq!(pasted(&context, "chief"), [note.id]);
}

/// Codex's broker says its thread is at work before its rollout shows the turn: the
/// window is given nothing on its record's word alone (CMT-06).
#[test]
fn a_window_whose_harness_says_it_is_at_work_is_given_nothing_whatever_its_record_says() {
    let context = paced();
    let project = chief_at_rest(&context);
    context.adapter.with("chief", |agent| agent.at_work = true);
    let note = context.note(project, "zeus", "chief", "One");
    for _ in 0..3 {
        context.advance(SETTLE);
        context.pass().unwrap();
    }
    assert_eq!(pasted(&context, "chief"), Vec::<i64>::new());
    context.adapter.with("chief", |agent| agent.at_work = false);
    context.pass().unwrap();
    assert_eq!(
        pasted(&context, "chief"),
        Vec::<i64>::new(),
        "its settle begins"
    );
    context.advance(SETTLE);
    context.pass().unwrap();
    assert_eq!(pasted(&context, "chief"), [note.id]);
}

/// Devin kept words in its input box that the human's keys counted as sent (a line typed
/// with its Enter in one burst): a paste would go in behind them, in one message with them
/// (the live pacing run on Devin, 2026-10-27). The fresh look reads the box, and a box that
/// holds words takes no paste.
#[test]
fn a_box_that_still_holds_words_takes_no_paste() {
    let context = paced();
    let project = chief_at_rest(&context);
    context.adapter.input_box.set(Some(InputBox::Holds));
    let note = context.note(project, "zeus", "chief", "One");
    for _ in 0..3 {
        context.advance(SETTLE);
        context.pass().unwrap();
    }
    assert_eq!(pasted(&context, "chief"), Vec::<i64>::new());
    assert_eq!(context.message(note.id).attempts, 0, "no attempt is spent");
    context.adapter.input_box.set(Some(InputBox::Empty));
    context.pass().unwrap();
    context.advance(SETTLE);
    context.pass().unwrap();
    assert_eq!(pasted(&context, "chief"), [note.id]);
}

/// Pi's extension refuses a message while Pi is busy (D9): nothing went in, so the
/// attempt is given back, and the settle starts again.
#[test]
fn a_harness_that_refuses_a_message_for_being_busy_spends_no_attempt() {
    let context = paced();
    let project = chief_at_rest(&context);
    let busy = Rc::new(std::cell::Cell::new(true));
    let refusing = Rc::clone(&busy);
    *context.adapter.deliver.borrow_mut() = Some(Rc::new(move |taking| {
        let admission = if refusing.get() {
            Admission::Busy {
                reason: "Pi is at work".to_owned(),
            }
        } else {
            taking()
        };
        Box::pin(async move { Ok(admission) })
    }));
    let note = context.note(project, "zeus", "chief", "One");
    context.pass().unwrap();
    context.advance(SETTLE);
    context.pass().unwrap();
    assert_eq!(context.message(note.id).state, "queued");
    assert_eq!(
        context.message(note.id).attempts,
        0,
        "the attempt is given back"
    );
    busy.set(false);
    context.pass().unwrap();
    assert_eq!(
        pasted(&context, "chief"),
        Vec::<i64>::new(),
        "its settle begins again"
    );
    context.advance(SETTLE);
    context.pass().unwrap();
    assert_eq!(pasted(&context, "chief"), [note.id]);
    assert_eq!(context.message(note.id).attempts, 1);
}

/// The chief's `cf tell` stops a window's work at once: it goes at the first look
/// that reads the window at rest, as before.
#[test]
fn an_urgent_tell_goes_at_the_first_look_at_rest() {
    let context = paced();
    let project = chief_at_rest(&context);
    context.give(project, "zeus", "Parser");
    context.pass().unwrap();
    context.advance(SETTLE);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.task(project, 1).task.state, "working");
    let session = context.task(project, 1).messages[0].recipient.clone();
    let tell = context.tell(project, &session, 1, "Stop: the grammar changed");
    context.adapter.with("zeus", |agent| agent.settled = true);
    context.pass().unwrap();
    assert_eq!(pasted(&context, "zeus").last(), Some(&tell.id));
}

/// Calliope's review of `9aa34baf6` (T-842, P1): a paste whose Enter was withheld stays
/// in its window's input box, and the key that empties it is pressed before the next
/// paste. The fresh look read that box as holding words, so the step returned before the
/// key, every pass: every message for the window waited for good, its lane idle. With a
/// settle, as the daemon runs: the box the engine empties first holds nothing for the
/// look.
#[test]
fn a_box_holding_a_paste_whose_enter_was_withheld_is_emptied_and_the_next_paste_goes() {
    let context = paced();
    let tiers = crate::resume::at_work(&context);
    let project = tiers.project.id;
    context.adapter.with("zeus-amber-pine", |agent| {
        agent.settled = true;
        agent.arrive = false;
    });
    let words = crate::withheld_enter::note_from(
        &context,
        project,
        None,
        "zeus-amber-pine",
        &cf_ledger::continuation(1),
    );
    for _ in 0..2 {
        context.advance(SETTLE);
        context.pass().unwrap();
    }
    assert_eq!(context.message(words.id).state, "delivering");
    crate::withheld_enter::enter_again_with(&context, "p1-zeus-amber-pine", || {
        context.human_pause(project, 1);
    });
    assert_eq!(crate::deliveries::enters(&context), 0, "no key that sends");
    // The box holds the paste whose Enter was withheld.
    context.adapter.input_box.set(Some(InputBox::Holds));
    context.resume_task(project, 1, "Carry on");
    let next = tiers.task(1).messages.last().cloned().expect("the words");
    for _ in 0..4 {
        context.advance(SETTLE);
        context.pass().unwrap();
    }
    let seam = "adapter:claude-code";
    assert_eq!(
        (
            crate::withheld_enter::clears(&context),
            crate::withheld_enter::times_handed(&context, seam, next.id)
        ),
        (1, 1),
        "the box emptied, then the words pasted"
    );
}

/// The reasons the gate held message `id` for, as the trace was told them, in order.
fn held_reasons(context: &Context, id: i64) -> Vec<String> {
    use cf_proto::trace::{Traced, WindowEvent};
    context
        .trace
        .lines
        .borrow()
        .iter()
        .filter_map(|line| match &line.what {
            Traced::Window {
                event: WindowEvent::DeliveryHeld { message, reason },
                ..
            } if *message == id => Some(reason.clone()),
            _ => None,
        })
        .collect()
}

/// T-842's P2: the gate's waits were told nowhere, the lane idle throughout. Each reason
/// is told the trace once, while it holds, and the board's reader says it
/// (`Dispatcher::held_back`), since the message became held; once it goes, nothing is
/// held.
#[test]
fn a_message_the_gate_holds_says_why_once_a_reason_and_since_when() {
    let context = paced();
    let project = chief_at_rest(&context);
    let chief = context.id(project, "chief");
    let note = context.note(project, "zeus", "chief", "One");
    context.pass().unwrap();
    let held = context.dispatcher.held_back(chief).expect("held");
    assert_eq!(
        (held.message, held.reason.as_str()),
        (note.id, "it has not rested 3 s since its last turn")
    );
    let since = held.since_ms;
    context.advance(SETTLE);
    context.host.set_snapshot(json!({ "typedQuietMs": 100 }));
    for _ in 0..3 {
        context.pass().unwrap();
    }
    let held = context.dispatcher.held_back(chief).expect("held");
    assert_eq!(held.reason, "the human typed in it within the last 3 s");
    assert_eq!(held.since_ms, since, "held since it was first held");
    assert_eq!(
        held_reasons(&context, note.id),
        [
            "it has not rested 3 s since its last turn",
            "the human typed in it within the last 3 s"
        ],
        "once a reason, not once a look"
    );
    context.host.set_snapshot(json!({ "typedQuietMs": SETTLE }));
    context.pass().unwrap();
    assert_eq!(pasted(&context, "chief"), [note.id]);
    assert_eq!(context.dispatcher.held_back(chief), None);
}

/// T-842's P2: the gate's snapshot had the arrival's 5 s to answer, where `ready()`'s own
/// has the bridge's: under load every snapshot came late and nothing went, unsaid. A late
/// answer is the host's word all the same.
#[test]
fn a_pane_host_that_answers_late_holds_nothing_back() {
    let context = paced();
    let project = chief_at_rest(&context);
    let note = context.note(project, "zeus", "chief", "One");
    context.pass().unwrap();
    context.advance(SETTLE);
    let looking = context
        .host
        .request_holds
        .hold(|args| args[0] == "pane.snapshot");
    let passing = context.begin_pass();
    context.settle();
    context.advance(6_000);
    looking.open();
    context.settle();
    passing.take().unwrap().unwrap();
    context.pass().unwrap();
    assert_eq!(pasted(&context, "chief"), [note.id]);
}

/// T-842's P2 (item 3): a record that cannot be read lets no message in (CMT-05), and the
/// board read the window idle while nothing went. Its lane says the record cannot be
/// read, and so does the reason the message is held for.
#[test]
fn a_window_whose_record_cannot_be_read_says_so_on_its_lane_and_in_its_held_message() {
    let context = paced();
    let project = chief_at_rest(&context);
    let chief = context.id(project, "chief");
    context.adapter.unread("chief", true);
    let note = context.note(project, "zeus", "chief", "One");
    for _ in 0..3 {
        context.advance(SETTLE);
        context.pass().unwrap();
    }
    let said = "its record cannot be read (the transcript could not be read): nothing is typed into it until it can be";
    assert_eq!(
        context.dispatcher.activity(chief).reason.as_deref(),
        Some(said)
    );
    let held = context.dispatcher.held_back(chief).expect("held");
    assert_eq!((held.message, held.reason.as_str()), (note.id, said));
}

/// T-842's P2: Codex's broker missed a `turn/completed`, and said the thread at work for
/// good while its rollout read the turn over; nothing went in, ever. Once the record has
/// read the turn over for a settle while the harness says at work, the harness is asked
/// again (no clock decides: Codex's own answer does), and what it says then stands.
#[test]
fn a_window_whose_harness_says_at_work_past_a_settle_of_its_turn_read_over_is_asked_again() {
    let context = paced();
    let project = chief_at_rest(&context);
    context.adapter.with("chief", |agent| {
        agent.at_work = true;
        agent.at_work_after_recheck = Some(true);
    });
    let note = context.note(project, "zeus", "chief", "One");
    context.pass().unwrap();
    context.advance(SETTLE - 1);
    context.pass().unwrap();
    assert_eq!(
        context.adapter.agent("chief").rechecked,
        0,
        "not before a settle"
    );
    context.advance(1);
    context.pass().unwrap();
    assert_eq!(context.adapter.agent("chief").rechecked, 1);
    context.pass().unwrap();
    assert_eq!(
        context.adapter.agent("chief").rechecked,
        1,
        "once a settle, not once a look"
    );
    assert_eq!(
        pasted(&context, "chief"),
        Vec::<i64>::new(),
        "it says at work still"
    );
    // The notice was lost: asked again, the harness says idle.
    context
        .adapter
        .with("chief", |agent| agent.at_work_after_recheck = Some(false));
    context.advance(SETTLE);
    context.pass().unwrap();
    assert_eq!(context.adapter.agent("chief").rechecked, 2);
    context.pass().unwrap();
    context.advance(SETTLE);
    context.pass().unwrap();
    assert_eq!(pasted(&context, "chief"), [note.id]);
}

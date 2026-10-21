//! What the input box holds after an Enter pressed once more was withheld (Calliope's
//! review of 2026-10-21, round 11). The human may send the paste left there: the
//! window's record then shows it, and it is taken for arrived, never pasted again.
//! A row of a paste that arrived reads delivered though it was withdrawn after its
//! hand-over. The key that empties the box goes only into a window quiet and holding
//! nothing the human typed, after the delivery is begun, so that a paste always
//! follows it; a box it emptied counts as empty until something goes in; and a box
//! that will not take the key fails its delivery as a refused paste does.

use std::cell::Cell;
use std::rc::Rc;

use cf_engine::delivery_text::marker_of;
use cf_engine::testing::Context;
use cf_harness::contract::{Admission, Held, Readiness};
use cf_harness::records::Role;
use cf_ledger::MessageView;
use serde_json::json;

use crate::deliveries::enters;
use crate::fixtures::Tiers;
use crate::resume::at_work;
use crate::withheld_enter::{clears, enter_again_with, handed_to, note, note_from, times_handed};

const SEAM: &str = "adapter:claude-code";
const ZEUS: &str = "p1-zeus-amber-pine";

/// What was pasted into zeus's window for `message`, its first paste.
fn pasted(context: &Context, pane: &str, message: i64) -> String {
    handed_to(context, SEAM, pane)
        .into_iter()
        .find(|text| text.contains(&marker_of(message)))
        .expect("it was pasted")
}

/// How many pastes into the window of `pane` said `body`, on a line of its own or
/// carried under another message's header.
fn said(context: &Context, pane: &str, body: &str) -> usize {
    handed_to(context, SEAM, pane)
        .iter()
        .filter(|text| text.lines().any(|line| line == body))
        .count()
}

/// The human presses Enter in the window of `handle` on what its box holds, `text`:
/// its record shows it given, and the window answers and comes to rest.
fn human_sends(context: &Context, handle: &str, text: &str) {
    let given = context.adapter.item(Role::User, text);
    let answer = context.adapter.item(Role::Assistant, "ok");
    context.adapter.with(handle, |agent| {
        agent.items.push(given);
        agent.items.push(answer);
        agent.settled = true;
    });
}

/// T-1's words to go on, carrying the chief's note, sat unsent in zeus's box, and
/// the chief paused T-1 again while the Enter for them looked at the screen,
/// taking the note back: no key went, the words went back to their queue, and the
/// paste stays in the box (round 11's sequence). The words, and the note.
fn carrier_withheld(context: &Context) -> (Tiers<'_>, MessageView, MessageView) {
    let tiers = at_work(context);
    let project = tiers.project.id;
    context.pause_task(project, 1);
    context.pass().unwrap();
    context.adapter.with("zeus-amber-pine", |agent| {
        agent.settled = true;
        agent.arrive = false;
    });
    let note = note(context, project, "zeus-amber-pine", "Use tabs");
    context.resume_task(project, 1, "Carry on");
    let words = tiers.task(1).messages.last().cloned().expect("the words");
    for _ in 0..2 {
        context.pass().unwrap();
    }
    assert_eq!(context.message(words.id).state, "delivering");
    enter_again_with(context, ZEUS, || context.pause_task(project, 1));
    assert_eq!(enters(context), 0);
    assert_eq!(context.message(note.id).state, "cancelled");
    assert_eq!(context.message(words.id).state, "queued");
    (tiers, words, note)
}

/// The chief resumes T-1 once more, zeus taking what it is given now: the words.
fn resumed_again(context: &Context, tiers: &Tiers<'_>) -> MessageView {
    context.resume_task(tiers.project.id, 1, "Carry on again");
    context
        .adapter
        .with("zeus-amber-pine", |agent| agent.arrive = true);
    tiers.task(1).messages.last().cloned().expect("the words")
}

/// Calliope's review of 2026-10-21, round 11 (P2-1): the words whose Enter was
/// withheld sat in zeus's box while T-1 was paused, and the human pressed Enter
/// on them there. The chief resumed T-1, the engine emptied a box that held
/// nothing and pasted the words again, carried in the new ones. The window's
/// record shows the words given: they arrived, the note in them too, and nothing
/// of them is pasted again; the box is empty, and nothing is pressed into it.
#[test]
fn words_the_human_sent_from_the_box_are_taken_for_arrived_and_never_pasted_again() {
    let context = Context::new();
    let (tiers, words, withdrawn) = carrier_withheld(&context);
    human_sends(
        &context,
        "zeus-amber-pine",
        &pasted(&context, ZEUS, words.id),
    );
    context.pass().unwrap();

    let again = resumed_again(&context, &tiers);
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(said(&context, ZEUS, &words.body), 1, "the words went once");
    assert_eq!(context.message(words.id).state, "delivered");
    assert_eq!(
        context.message(withdrawn.id).state,
        "delivered",
        "the note went in the paste the human sent"
    );
    assert_eq!(
        times_handed(&context, SEAM, again.id),
        1,
        "the new words go"
    );
    assert_eq!(clears(&context), 0, "nothing pressed into an empty box");
}

/// The same across a Close: the window the Close could not close had its Enter
/// withheld while closed, and the human sent the note in its box before the Resume.
#[test]
fn a_note_the_human_sent_from_the_box_while_closed_is_taken_for_arrived_at_the_resume() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.adapter.with("chief", |agent| agent.arrive = false);
    let note = context.note(project.id, "zeus", "chief", "hello");
    context.pass().unwrap();
    assert_eq!(context.message(note.id).state, "delivering");
    context.host.refuse_kills.set(true);
    assert!(
        context.close_project(project.id).is_err(),
        "its windows stay"
    );
    context.host.refuse_kills.set(false);
    context.advance(11_000);
    context.host.set_snapshot(json!({ "outputQuietMs": 5_000 }));
    context.pass().unwrap();
    assert_eq!(
        context.message(note.id).state,
        "queued",
        "withheld, given back"
    );
    let chief = context.host.last("chief").expect("its window").pane.id;
    human_sends(&context, "chief", &pasted(&context, &chief, note.id));
    context.pass().unwrap();

    context.resume_project(project.id).unwrap();
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(
        times_handed(&context, SEAM, note.id),
        1,
        "the note went once"
    );
    assert_eq!(context.message(note.id).state, "delivered");
    assert_eq!((enters(&context), clears(&context)), (0, 0));
}

/// The same of a continuation withdrawn while the screen was looked at, its task
/// paused: the human sent the word to go on left in the box. The engine never
/// pastes it again, presses nothing into the empty box, and the record says it
/// arrived.
#[test]
fn a_withdrawn_continuation_the_human_sent_from_the_box_is_never_pasted_again() {
    let context = Context::new();
    let tiers = at_work(&context);
    let project = tiers.project.id;
    context.adapter.with("zeus-amber-pine", |agent| {
        agent.settled = true;
        agent.arrive = false;
    });
    let go_on = note_from(
        &context,
        project,
        None,
        "zeus-amber-pine",
        &cf_ledger::continuation(1),
    );
    context.pass().unwrap();
    assert_eq!(context.message(go_on.id).state, "delivering");
    enter_again_with(&context, ZEUS, || context.human_pause(project, 1));
    assert_eq!(context.message(go_on.id).state, "cancelled");
    human_sends(
        &context,
        "zeus-amber-pine",
        &pasted(&context, ZEUS, go_on.id),
    );
    context.pass().unwrap();

    let again = resumed_again(&context, &tiers);
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(
        times_handed(&context, SEAM, go_on.id),
        1,
        "never pasted again"
    );
    assert_eq!(
        times_handed(&context, SEAM, again.id),
        1,
        "the words to resume go"
    );
    assert_eq!(clears(&context), 0, "nothing pressed into an empty box");
    let go_on = context.message(go_on.id);
    assert_eq!(go_on.state, "delivered", "the record says it arrived");
    assert_eq!(go_on.receipt["cancelled"], "after it was handed over");
}

/// Calliope's review of 2026-10-21, round 11 (P2-2): T-1's words to go on,
/// carrying the chief's note, were handed over to zeus, and the chief paused T-1
/// again, which took the note back; then zeus's record showed the words, the
/// note in them. The words were confirmed and the note read cancelled. It reads
/// delivered, its receipt naming the words that carried it.
#[test]
fn a_note_taken_back_after_its_carrier_was_handed_over_reads_delivered_once_the_carrier_arrives() {
    let context = Context::new();
    let tiers = at_work(&context);
    let project = tiers.project.id;
    context.pause_task(project, 1);
    context.pass().unwrap();
    context.adapter.with("zeus-amber-pine", |agent| {
        agent.settled = true;
        agent.arrive = false;
    });
    let note = note(&context, project, "zeus-amber-pine", "Use tabs");
    context.resume_task(project, 1, "Carry on");
    let words = tiers.task(1).messages.last().cloned().expect("the words");
    for _ in 0..2 {
        context.pass().unwrap();
    }
    assert_eq!(context.message(words.id).state, "delivering");
    context.pause_task(project, 1);
    assert_eq!(context.message(note.id).state, "cancelled");
    let given = context
        .adapter
        .item(Role::User, &pasted(&context, ZEUS, words.id));
    context
        .adapter
        .with("zeus-amber-pine", |agent| agent.items.push(given));
    context.pass().unwrap();

    assert_eq!(context.message(words.id).state, "delivered");
    let note = context.message(note.id);
    assert_eq!(note.state, "delivered", "its words arrived");
    assert_eq!(note.receipt["carrier"], words.id);
    assert_eq!(note.receipt["cancelled"], "after it was handed over");
}

/// Calliope's review of 2026-10-21, round 11 (P3-3): Claude Code ends at a second
/// Control-C in a row. The key that empties the box goes only into a window that
/// printed nothing for a while (a human who just cleared their own draft with one
/// made Claude print its warning) and holds nothing the human typed since the
/// window was read ready; otherwise the pass waits.
#[test]
fn the_box_is_emptied_only_while_the_window_is_quiet_and_holds_nothing_typed() {
    for (snapshot, why) in [
        (json!({ "outputQuietMs": 500 }), "printing"),
        (
            json!({ "outputQuietMs": 5_000, "unsent": true }),
            "typed into",
        ),
    ] {
        let context = Context::new();
        let (tiers, _, _) = carrier_withheld(&context);
        let again = resumed_again(&context, &tiers);
        context.host.set_snapshot(snapshot);
        for _ in 0..2 {
            context.pass().unwrap();
        }
        assert_eq!(clears(&context), 0, "{why}: nothing pressed");
        assert_eq!(
            times_handed(&context, SEAM, again.id),
            0,
            "{why}: nothing pasted"
        );
        assert_eq!(
            context.message(again.id).attempts,
            0,
            "{why}: no attempt spent"
        );

        context
            .host
            .set_snapshot(json!({ "outputQuietMs": 5_000, "unsent": false }));
        context.pass().unwrap();
        assert_eq!(
            clears(&context),
            1,
            "{why}: pressed once the window is quiet"
        );
        assert_eq!(
            times_handed(&context, SEAM, again.id),
            1,
            "{why}: then pasted"
        );
    }
}

/// The same review (P3-3): a key that emptied the box was followed by no paste,
/// and Claude, read at rest by an interrupt, pressed its own Control-C at the
/// next one. A box the engine emptied counts as empty until something goes in:
/// a paste the host refused after the key, and the one after it, are each told
/// so, and the key is pressed once.
#[test]
fn a_box_emptied_counts_as_empty_until_a_paste_goes_in() {
    let context = Context::new();
    let (tiers, _, _) = carrier_withheld(&context);
    let refusals = Rc::new(Cell::new(1_u32));
    let left = Rc::clone(&refusals);
    *context.adapter.deliver.borrow_mut() = Some(Rc::new(move |taking| {
        let refused = left.get() > 0;
        left.set(left.get().saturating_sub(1));
        Box::pin(async move {
            Ok(if refused {
                Admission::Refused {
                    reason: "the pane host refused the paste".to_owned(),
                }
            } else {
                taking()
            })
        })
    }));
    let again = resumed_again(&context, &tiers);
    let before = context.adapter.cleared.borrow().len();
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(refusals.get(), 0, "the first paste was refused");
    assert_eq!(context.message(again.id).state, "delivered");
    assert_eq!(clears(&context), 1, "one key");
    assert_eq!(
        context.adapter.cleared.borrow()[before..],
        [true, true],
        "the refused paste and the one after it were each told the box is empty"
    );
}

/// The same review (P3-5): a pane that would not take the key that empties its
/// box held its queue silently for ever. Each key refused spends an attempt of
/// the delivery, as a refused paste does, and the last fails it, the human told.
#[test]
fn a_box_that_will_not_take_its_key_fails_its_delivery_as_a_refused_paste_does() {
    let context = Context::new();
    let (tiers, _, _) = carrier_withheld(&context);
    context.host.refuse_keys.set(true);
    let again = resumed_again(&context, &tiers);
    for _ in 0..6 {
        context.pass().unwrap();
    }
    assert_eq!(times_handed(&context, SEAM, again.id), 0, "nothing pasted");
    let again = context.message(again.id);
    assert_eq!(again.state, "failed");
    assert_eq!(again.attempts, 3);
    assert!(
        again
            .reason
            .as_deref()
            .is_some_and(|reason| reason.contains("input box could not be emptied")),
        "{:?}",
        again.reason
    );
    assert_eq!(context.task(tiers.project.id, 1).task.state, "failed");
}

/// Calliope's review of 2026-10-21, round 12 (P3-1): a window whose screen never
/// went quiet for the key that empties its box held its queue for good, no attempt
/// spent, the human told nothing. Once the wait outlasts the arrival window, the
/// delivery is begun and the attempt fails; the last fails it, as a key the host
/// did not take does.
#[test]
fn a_window_that_never_goes_quiet_for_the_key_fails_its_delivery_in_the_end() {
    let context = Context::new();
    let (tiers, _, _) = carrier_withheld(&context);
    let again = resumed_again(&context, &tiers);
    context.host.set_snapshot(json!({ "outputQuietMs": 500 }));
    for _ in 0..16 {
        context.advance(11_000);
        context.pass().unwrap();
    }
    assert_eq!(
        clears(&context),
        0,
        "nothing pressed into a printing window"
    );
    assert_eq!(times_handed(&context, SEAM, again.id), 0, "nothing pasted");
    let again = context.message(again.id);
    assert_eq!((again.state.as_str(), again.attempts), ("failed", 3));
    assert!(
        again
            .reason
            .as_deref()
            .is_some_and(|reason| reason.contains("never went quiet")),
        "{:?}",
        again.reason
    );
    assert_eq!(context.task(tiers.project.id, 1).task.state, "failed");
}

/// The same review (P3-2): the window's record, copied by ConsensFlow, held the
/// note's header from before its Enter was withheld (an earlier copy of it, or the
/// human quoting it). That was taken for the human sending it from the box: the
/// note read delivered and was never pasted, the stale text left in the box. Only
/// what the record was given since the Enter was withheld counts.
#[test]
fn an_earlier_copy_of_a_withheld_paste_is_not_taken_for_the_human_sending_it() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.adapter.with("chief", |agent| agent.arrive = false);
    let note = context.note(project.id, "zeus", "chief", "hello");
    context.pass().unwrap();
    assert_eq!(context.message(note.id).state, "delivering");
    let chief = context.id(project.id, "chief");
    let conversation = context
        .ledger
        .borrow()
        .current_conversation(chief)
        .unwrap()
        .expect("the chief's conversation")
        .id;
    let quoted = format!("{} as I said", marker_of(note.id));
    context
        .ledger
        .borrow_mut()
        .copy_transcript(
            conversation,
            &[json!({ "id": "earlier", "role": "user", "text": quoted })],
            1_000,
        )
        .unwrap();
    context.host.refuse_kills.set(true);
    assert!(
        context.close_project(project.id).is_err(),
        "its windows stay"
    );
    context.host.refuse_kills.set(false);
    context.advance(11_000);
    context.host.set_snapshot(json!({ "outputQuietMs": 5_000 }));
    context.pass().unwrap();
    assert_eq!(
        context.message(note.id).state,
        "queued",
        "withheld, given back"
    );

    context.resume_project(project.id).unwrap();
    context.adapter.with("chief", |agent| agent.arrive = true);
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(
        (times_handed(&context, SEAM, note.id), clears(&context)),
        (2, 1),
        "the box emptied, and the note pasted again"
    );
    let note = context.message(note.id);
    assert_eq!(note.state, "delivered");
    assert_ne!(note.receipt["item"], "earlier");
}

/// The same review (P3-3): the window's record showed the words the human sent
/// from the box, and the write that takes them for arrived failed. The mark was
/// dropped first, and the next words were pasted carrying the words again. The
/// mark goes only once the ledger has them arrived, in one write: the next pass
/// asks again.
#[test]
fn a_failed_write_of_words_sent_from_the_box_leaves_them_to_be_taken_for_arrived_next_pass() {
    let context = Context::new();
    let (tiers, words, _) = carrier_withheld(&context);
    human_sends(
        &context,
        "zeus-amber-pine",
        &pasted(&context, ZEUS, words.id),
    );
    context.pass().unwrap();
    // The write that takes them for arrived fails at its last step, as a ledger
    // that fails inside a write does: nothing of it stays.
    context
        .ledger
        .borrow_mut()
        .write_raw(&format!(
            "CREATE TRIGGER injected BEFORE UPDATE OF state ON message
             WHEN NEW.state = 'delivered' AND OLD.id = {}
             BEGIN SELECT RAISE(ABORT, 'injected'); END;",
            words.id
        ))
        .unwrap();

    let again = resumed_again(&context, &tiers);
    let _ = context.pass();
    assert_eq!(
        context.message(words.id).state,
        "queued",
        "nothing of the failed write stays"
    );
    assert_eq!(times_handed(&context, SEAM, again.id), 0, "nothing pasted");
    context
        .ledger
        .borrow_mut()
        .write_raw("DROP TRIGGER injected;")
        .unwrap();
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(context.message(words.id).state, "delivered");
    assert_eq!(said(&context, ZEUS, &words.body), 1, "the words went once");
    assert_eq!(
        times_handed(&context, SEAM, again.id),
        1,
        "the new words go"
    );
    assert_eq!(clears(&context), 0);
}

/// The same review (P3-4): words whose Enter was withheld went back to their queue
/// (their hand-over undone), and a wordless resume superseded them; the human had
/// sent them from the box. They stayed withdrawn, the note in them too, though
/// both arrived. The paste was admitted, or nothing would be in the box: they read
/// delivered.
#[test]
fn words_superseded_after_the_human_sent_them_from_the_box_read_delivered_with_their_note() {
    let context = Context::new();
    let tiers = at_work(&context);
    let project = tiers.project.id;
    context.pause_task(project, 1);
    context.pass().unwrap();
    context.adapter.with("zeus-amber-pine", |agent| {
        agent.settled = true;
        agent.arrive = false;
    });
    let note = note(&context, project, "zeus-amber-pine", "Use tabs");
    context.resume_task(project, 1, cf_ledger::RESUME_WORDS);
    let words = tiers.task(1).messages.last().cloned().expect("the words");
    for _ in 0..2 {
        context.pass().unwrap();
    }
    enter_again_with(&context, ZEUS, || context.pause_task(project, 1));
    assert_eq!(context.message(words.id).state, "queued");
    human_sends(
        &context,
        "zeus-amber-pine",
        &pasted(&context, ZEUS, words.id),
    );
    context.pass().unwrap();

    context.resume_task(project, 1, cf_ledger::RESUME_WORDS);
    assert_eq!(
        context.message(words.id).state,
        "cancelled",
        "superseded by the new words"
    );
    context
        .adapter
        .with("zeus-amber-pine", |agent| agent.arrive = true);
    for _ in 0..3 {
        context.pass().unwrap();
    }
    let (words, note) = (context.message(words.id), context.message(note.id));
    assert_eq!(
        (words.state.as_str(), note.state.as_str()),
        ("delivered", "delivered")
    );
    assert_eq!(words.receipt["cancelled"], "after it was handed over");
    assert_eq!(clears(&context), 0);
}

/// The same review (P3-6): a box the engine emptied counted as empty after the
/// human typed into it, so an interrupt of ours that put their message back there
/// would have it ride with the next paste, Claude pressing no Control-C of its own.
/// The human's typing ends it: the next paste is not told the box is empty.
#[test]
fn a_box_the_human_typed_into_since_it_was_emptied_is_not_counted_empty() {
    let context = Context::new();
    let (tiers, _, _) = carrier_withheld(&context);
    let refusals = Rc::new(Cell::new(1_u32));
    let left = Rc::clone(&refusals);
    *context.adapter.deliver.borrow_mut() = Some(Rc::new(move |taking| {
        let refused = left.get() > 0;
        left.set(left.get().saturating_sub(1));
        Box::pin(async move {
            Ok(if refused {
                Admission::Refused {
                    reason: "the pane host refused the paste".to_owned(),
                }
            } else {
                taking()
            })
        })
    }));
    let again = resumed_again(&context, &tiers);
    context.pass().unwrap();
    assert_eq!(refusals.get(), 0, "the paste after the key was refused");
    assert_eq!(clears(&context), 1);

    *context.adapter.ready.borrow_mut() = Some(Rc::new(|| Ok(Readiness::Held(Held::Unsent))));
    context.pass().unwrap();
    *context.adapter.ready.borrow_mut() = None;
    let before = context.adapter.cleared.borrow().len();
    for _ in 0..2 {
        context.pass().unwrap();
    }
    assert_eq!(context.message(again.id).state, "delivered");
    assert_eq!(
        context.adapter.cleared.borrow()[before..],
        [false],
        "the paste after the human typed is not told the box is empty"
    );
}

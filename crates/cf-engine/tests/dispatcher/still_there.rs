//! A re-paste needs proof of loss (3.0.7 chief-meeting, CMT-24, T-760's F4): a paste its
//! window's record never showed goes again only where it is gone from the screen as from
//! the record. A window whose screen printed within the last seconds is at work, or taking
//! it in; a window whose input box holds it is pressed Enter once more. Either way it waits
//! a whole wait more, and nothing is pasted.

use cf_engine::testing::Context;
use cf_harness::contract::InputBox;
use serde_json::{json, Value};

/// The kit's arrival window, and a moment past it.
const PAST_ARRIVAL: i64 = 31_000;

/// A note to the chief whose paste its record will never show.
fn a_note_that_never_shows(context: &Context) -> i64 {
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    context.adapter.with("chief", |agent| agent.arrive = false);
    let note = context.note(project.id, "zeus", "chief", "Lost");
    context.pass().unwrap();
    assert_eq!(context.message(note.id).state, "delivering");
    note.id
}

/// How many times the ledger began a delivery of `id`.
fn begun(context: &Context, id: i64) -> usize {
    context
        .recorder
        .events()
        .into_iter()
        .filter(|event| {
            event["seam"] == "event"
                && event["event"]["kind"] == "delivery.begun"
                && event["event"]["data"]["message"] == json!(id)
        })
        .count()
}

/// The Enters pressed into windows.
fn enters(context: &Context) -> usize {
    context
        .host
        .inputs()
        .iter()
        .filter(|body: &&Value| body["bytes"][0] == 13)
        .count()
}

#[test]
fn a_paste_its_input_box_still_holds_is_pressed_enter_again_and_not_pasted_again() {
    let context = Context::new();
    let note = a_note_that_never_shows(&context);
    context.host.set_snapshot(json!({ "outputQuietMs": 60_000 }));
    context.adapter.input_box.set(Some(InputBox::Holds));
    let before = enters(&context);
    context.advance(PAST_ARRIVAL);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(begun(&context, note), 1, "not pasted again");
    assert_eq!(context.message(note).state, "delivering");
    assert_eq!(enters(&context), before + 1, "its Enter, once more");
    // Not asked again before a whole wait more.
    context.advance(1_000);
    context.pass().unwrap();
    assert_eq!(enters(&context), before + 1);
    // Its box empty and its screen quiet, a whole wait later: it is gone, and goes again.
    context.adapter.input_box.set(Some(InputBox::Empty));
    context.advance(PAST_ARRIVAL);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(begun(&context, note), 2, "gone from the screen as from the record");
}

#[test]
fn a_window_whose_screen_printed_within_the_last_seconds_is_not_pasted_again() {
    let context = Context::new();
    let note = a_note_that_never_shows(&context);
    context.host.set_snapshot(json!({ "outputQuietMs": 500 }));
    let before = enters(&context);
    context.advance(PAST_ARRIVAL);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(begun(&context, note), 1, "at work, or taking it in");
    assert_eq!(enters(&context), before, "and no Enter into a window that draws");
    context.host.set_snapshot(json!({ "outputQuietMs": 60_000 }));
    context.advance(PAST_ARRIVAL);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(begun(&context, note), 2, "quiet, its box read empty: it is gone");
}

/// Claude Code stopped right after the paste (`cargo xtask live stopped`): its screen has
/// printed nothing since the paste was written, so it has not read it, and it is not pasted
/// again; a screen that printed after the paste, quiet now, its box empty, has lost it.
#[test]
fn a_window_that_has_printed_nothing_since_the_paste_was_written_is_not_pasted_again() {
    let context = Context::new();
    let note = a_note_that_never_shows(&context);
    context
        .host
        .set_snapshot(json!({ "outputQuietMs": 40_000, "pasteQuietMs": 31_000 }));
    let before = enters(&context);
    context.advance(PAST_ARRIVAL);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(begun(&context, note), 1, "its program has not read it");
    assert_eq!(enters(&context), before, "and no Enter goes into a window that reads nothing");
    context
        .host
        .set_snapshot(json!({ "outputQuietMs": 20_000, "pasteQuietMs": 62_000 }));
    context.adapter.input_box.set(Some(InputBox::Empty));
    context.advance(PAST_ARRIVAL);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(begun(&context, note), 2, "drawn since, and gone");
}

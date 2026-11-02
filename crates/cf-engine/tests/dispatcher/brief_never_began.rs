//! A brief its window's record shows, and no turn begun on it (the field bug T-1130,
//! 2026-11-02): T-9, added `--after T-8` for a session whose window ConsensFlow had
//! closed, went to a window opened again on the session's conversation. Its record showed
//! the brief, m-399, and nothing after it, and the window never began: the board said
//! "working" for an hour, and nobody was told. The record's showing of the brief is what
//! makes a task working (`receipt::reconcile`), so every wait of the delivery's was over;
//! and a turn's end found no answer to take and said nothing.
//!
//! What should happen: a brief its window shows and begins nothing on for a whole arrival
//! window, the window's screen quiet all along, is noticed. Enter is pressed once more into
//! a window that takes pastes (the record has the brief: what it lacks is the Enter that
//! sends it). If a whole arrival window more begins nothing, the task leaves `working`:
//! ConsensFlow pauses it, and its requester is told once what was sent, when, and what
//! the window shows, as the board shows the pause's words. `cf task resume` sends its
//! words into the same window, where the brief is above them.
//!
//! The window that never begins reads at rest (Devin's and Muse's readers, a record that
//! ends on a message of the user's past their first moments) or in flight for ever
//! (Claude Code's, Codex's and Pi's): both are here. The control is a window whose screen
//! prints: a model on its way to its first word, which is left be.

use cf_engine::testing::Context;
use regex::Regex;
use serde_json::{json, Value};

use crate::fixtures::{given, Tiers};

/// The kit's arrival window, and a moment past it.
const PAST_ARRIVAL: i64 = 31_000;

/// The Enters pressed into windows.
fn enters(context: &Context) -> usize {
    context
        .host
        .inputs()
        .iter()
        .filter(|body: &&Value| body["bytes"][0] == 13)
        .count()
}

/// The id of the message whose header `text` begins with.
fn message_of(text: &str) -> i64 {
    Regex::new(r"m-(\d+) · T-2 · task")
        .unwrap()
        .captures(text)
        .and_then(|found| found[1].parse().ok())
        .unwrap_or_else(|| panic!("no header of T-2's in {text:?}"))
}

/// The field's shape: zeus answers T-1 and its window closes; T-2, added `--after T-1`,
/// goes once T-1 is accepted to a window opened again on the conversation, whose record
/// shows its brief and which begins nothing on it (`at_rest` says how its reader reads it).
/// T-2 is working. The id of its brief.
fn a_follow_up_whose_window_never_begins(context: &Context, tiers: &Tiers, at_rest: bool) -> i64 {
    let project = tiers.project.id;
    context.host.exits_showing(Some(0), None, None);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working");
    context.adapter.answer("zeus", "Parser done");
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "done");
    context.create_task(
        project,
        cf_ledger::NewTask {
            from: "chief".to_owned(),
            after: Some(1),
            body: "Now the lexer".to_owned(),
            ..cf_ledger::NewTask::default()
        },
    );
    context.adapter.never_begins.set(Some(at_rest));
    context
        .ledger
        .borrow_mut()
        .accept_task(project, 1, "chief")
        .unwrap();
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(tiers.task(2).task.state, "working", "its record shows the brief");
    let brief = given(context, "zeus")
        .into_iter()
        .rev()
        .find(|text| text.contains("· T-2 · task"))
        .expect("the brief is in the record");
    message_of(&brief)
}

/// The notes the chief was sent that say T-2 is paused.
fn pause_notes(tiers: &Tiers) -> Vec<String> {
    tiers
        .notes("chief")
        .into_iter()
        .filter(|note| note.starts_with("T-2 is paused"))
        .collect()
}

fn never_begun_is_pressed_enter_once_then_paused_and_told(at_rest: bool) {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    let brief = a_follow_up_whose_window_never_begins(&context, &tiers, at_rest);
    // The window printed nothing since long before.
    context
        .host
        .set_snapshot(json!({ "outputQuietMs": 600_000, "tail": ["Ready for the next step.", "❯ "] }));
    let before = enters(&context);

    // Within its arrival window it is waited for: a model on its way to its first word.
    context.advance(10_000);
    context.pass().unwrap();
    assert_eq!(enters(&context), before);
    assert_eq!(tiers.task(2).task.state, "working");

    // A whole arrival window of nothing: Enter, once more, and the task still works.
    context.advance(PAST_ARRIVAL);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(enters(&context), before + 1, "its Enter, pressed once more");
    assert_eq!(tiers.task(2).task.state, "working");
    assert!(pause_notes(&tiers).is_empty());

    // A whole arrival window more, and nothing began: the task leaves `working`, and the
    // chief is told once what was sent, and what the window shows.
    context.advance(PAST_ARRIVAL);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(enters(&context), before + 1, "the Enter is pressed once");
    let task = tiers.task(2).task;
    assert_eq!(task.state, "paused");
    let notes = pause_notes(&tiers);
    assert_eq!(notes.len(), 1, "{notes:#?}");
    let note = &notes[0];
    assert!(note.contains(&format!("m-{brief}")), "{note}");
    assert!(note.contains("began no turn"), "{note}");
    assert!(note.contains("Enter"), "{note}");
    assert!(note.contains("its screen ended with"), "{note}");
    assert!(note.contains("cf task resume T-2"), "{note}");
    for _ in 0..3 {
        context.advance(PAST_ARRIVAL);
        context.pass().unwrap();
    }
    assert_eq!(pause_notes(&tiers).len(), 1, "told once");
    assert_eq!(enters(&context), before + 1);
}

#[test]
fn a_brief_a_window_at_rest_never_began_is_pressed_enter_once_then_its_task_is_paused_and_its_requester_told(
) {
    never_begun_is_pressed_enter_once_then_paused_and_told(true);
}

#[test]
fn a_brief_a_window_read_in_flight_never_began_is_pressed_enter_once_then_its_task_is_paused_and_its_requester_told(
) {
    never_begun_is_pressed_enter_once_then_paused_and_told(false);
}

/// The way on: `cf task resume` sends its words into the same window, not a new one, and
/// a window that begins on them works the task again.
#[test]
fn a_task_paused_for_a_brief_never_begun_resumes_in_the_same_window() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    a_follow_up_whose_window_never_begins(&context, &tiers, true);
    context
        .host
        .set_snapshot(json!({ "outputQuietMs": 600_000 }));
    for _ in 0..2 {
        context.advance(PAST_ARRIVAL);
        context.pass().unwrap();
        context.pass().unwrap();
    }
    assert_eq!(tiers.task(2).task.state, "paused");
    let opened = context.host.opened().len();

    context.adapter.never_begins.set(None);
    context.resume_task(tiers.project.id, 2, "Begin it now");
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(context.host.opened().len(), opened, "no window opened");
    assert_eq!(tiers.task(2).task.state, "working");
    let last = given(&context, "zeus").pop().unwrap();
    assert!(last.contains("Begin it now"), "{last}");
}

/// The control: a window whose screen prints is on its way to its first word, however
/// long, and is neither pressed nor paused.
#[test]
fn a_brief_whose_window_prints_is_left_be() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    a_follow_up_whose_window_never_begins(&context, &tiers, false);
    context.host.set_snapshot(json!({ "outputQuietMs": 200 }));
    let before = enters(&context);
    for _ in 0..4 {
        context.advance(PAST_ARRIVAL);
        context.pass().unwrap();
        context.pass().unwrap();
    }
    assert_eq!(enters(&context), before);
    assert_eq!(tiers.task(2).task.state, "working");
    assert!(pause_notes(&tiers).is_empty());
}

/// A window that began is never pressed: its turn's first word ends the watch.
#[test]
fn a_brief_whose_window_begins_late_is_neither_pressed_again_nor_paused() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    a_follow_up_whose_window_never_begins(&context, &tiers, false);
    context
        .host
        .set_snapshot(json!({ "outputQuietMs": 600_000 }));
    let before = enters(&context);
    context.advance(PAST_ARRIVAL);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(enters(&context), before + 1);
    // The Enter began it: its agent writes, and answers.
    context.adapter.answer("zeus", "Lexer done");
    for _ in 0..3 {
        context.advance(PAST_ARRIVAL);
        context.pass().unwrap();
    }
    assert_eq!(tiers.task(2).task.state, "done");
    assert!(pause_notes(&tiers).is_empty());
    assert_eq!(enters(&context), before + 1);
}

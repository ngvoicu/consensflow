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

use crate::fixtures::{given, native_of, Tiers};

/// The kit's arrival window, and a moment past it.
pub(crate) const PAST_ARRIVAL: i64 = 31_000;

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
pub(crate) fn a_follow_up_whose_window_never_begins(
    context: &Context,
    tiers: &Tiers,
    at_rest: bool,
) -> i64 {
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
    assert_eq!(
        tiers.task(2).task.state,
        "working",
        "its record shows the brief"
    );
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
    context.host.set_snapshot(
        json!({ "outputQuietMs": 600_000, "screenQuietMs": 600_000, "tail": ["Ready for the next step.", "❯ "] }),
    );
    let before = enters(&context);
    let killed = context.host.killed().len();

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
    assert!(note.contains("Enter was pressed once more"), "{note}");
    assert!(note.contains("its screen ended with"), "{note}");
    assert!(note.contains("cf task resume T-2"), "{note}");
    for _ in 0..3 {
        context.advance(PAST_ARRIVAL);
        context.pass().unwrap();
    }
    assert_eq!(pause_notes(&tiers).len(), 1, "told once");
    assert_eq!(enters(&context), before + 1);
    // The window is kept for the resume: nothing ran in it, and no key is owed it.
    // A window at rest pays the pause's stop at once, and is kept for the resume. One read in
    // flight is pressed the stop's rounds, which a Claude Code reads at rest by (`by_press`);
    // one that reads in flight still is closed, and the resume opens it on its conversation.
    if at_rest {
        assert_eq!(context.host.killed().len(), killed, "the window is kept");
    }
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

/// A window at rest that repaints what it shows without changing it (Muse Code 1.4.14 after a
/// command it ran in the background was stopped: colour resets every 50 ms, T-1081, T-1139)
/// prints all the time and shows the same: its screen is quiet, and the brief it began no
/// turn on is given its Enter once, as any quiet window's is (T-1083's screen clock; T-1209).
#[test]
fn a_brief_a_window_that_repaints_at_rest_never_began_is_pressed_enter_once() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    a_follow_up_whose_window_never_begins(&context, &tiers, true);
    // It printed 40 ms ago, and what it shows has not changed for ten minutes.
    context.host.set_snapshot(
        json!({ "outputQuietMs": 40, "screenQuietMs": 600_000, "tail": ["Ready for the next step.", "❯ "] }),
    );
    let before = enters(&context);
    context.advance(PAST_ARRIVAL);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(enters(&context), before + 1, "its Enter, pressed once more");
    assert_eq!(tiers.task(2).task.state, "working");
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
        .set_snapshot(json!({ "outputQuietMs": 600_000, "screenQuietMs": 600_000 }));
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
    context
        .host
        .set_snapshot(json!({ "outputQuietMs": 200, "screenQuietMs": 200 }));
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
        .set_snapshot(json!({ "outputQuietMs": 600_000, "screenQuietMs": 600_000 }));
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

/// A turn that began is the turn's, however quiet its screen: a call that prints nothing
/// for hours is never pressed into, nor its task paused.
#[test]
fn a_brief_whose_turn_began_is_never_pressed_however_quiet_its_screen() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    a_follow_up_whose_window_never_begins(&context, &tiers, false);
    context.adapter.writes(
        "zeus",
        "Running the lexer's tests",
        "2026-11-02T22:35:10.000Z",
    );
    context
        .host
        .set_snapshot(json!({ "outputQuietMs": 600_000, "screenQuietMs": 600_000 }));
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

/// A window inside one long model call (T-1396: in a measurement run of 2026-11-10 a Devin
/// window thought 146 s on a send-back, its record showing nothing after the message and its
/// screen drawing nothing, and the task was pressed Enter at 61 s and paused at 126 s, its
/// window at work). Devin 3000.12 says the message back as its turn takes it in, on the
/// window's own launch's log (T-1417), and the window's look says so of the message
/// (`Observed::begun`): the turn is the turn's, however long its record and its screen stay
/// quiet. Nothing is pressed, the task stays working, and nobody is told. Once the turn ended
/// with nothing after the message, the window is one that began nothing: its Enter, then the
/// pause, as ever.
fn a_brief_its_window_said_back_is_neither_pressed_nor_paused(at_rest: bool) {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    let brief = a_follow_up_whose_window_never_begins(&context, &tiers, at_rest);
    context
        .adapter
        .with("zeus", |agent| agent.begun = vec![brief]);
    // The screen of the field's window: its last answer, its idle box, its status line.
    context.host.set_snapshot(json!({
        "outputQuietMs": 600_000,
        "screenQuietMs": 600_000,
        "tail": ["❭ Ask Devin to build features, fix bugs, or work on your code", "SWE-2 Max · bypass"],
    }));
    let before = enters(&context);
    for _ in 0..4 {
        context.advance(PAST_ARRIVAL);
        context.pass().unwrap();
        context.pass().unwrap();
    }
    assert_eq!(enters(&context), before, "no Enter into a turn at work");
    assert_eq!(tiers.task(2).task.state, "working");
    assert!(pause_notes(&tiers).is_empty(), "nobody is told");

    // The turn ends, and nothing follows the message: a turn that came to nothing, watched
    // from the look that finds it so.
    context.adapter.with("zeus", |agent| agent.begun.clear());
    context.pass().unwrap();
    context.advance(PAST_ARRIVAL);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(enters(&context), before + 1, "its Enter, pressed once more");
    assert_eq!(tiers.task(2).task.state, "working");
    context.advance(PAST_ARRIVAL);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(2).task.state, "paused");
    let notes = pause_notes(&tiers);
    assert_eq!(notes.len(), 1, "{notes:#?}");
    assert!(notes[0].contains(&format!("m-{brief}")), "{}", notes[0]);
}

/// The window read in flight, as a Devin's is while its turn is open on its log.
#[test]
fn a_brief_its_window_said_back_into_an_open_turn_is_neither_pressed_nor_paused() {
    a_brief_its_window_said_back_is_neither_pressed_nor_paused(false);
}

/// The same, the window read at rest by its items.
#[test]
fn a_brief_its_window_said_back_into_an_open_turn_is_left_be_read_at_rest() {
    a_brief_its_window_said_back_is_neither_pressed_nor_paused(true);
}

/// Another message said back is no word of this one (T-1405, the review of T-1396, where an
/// older launch's word that a turn worked switched the rule off): a turn open on the window's
/// log that took in another message says nothing of T-2's brief.
/// The window reads at rest or in flight and begins no turn on it: it is pressed Enter once
/// and its task paused, its requester told.
#[test]
fn a_brief_whose_window_said_back_only_another_message_is_still_pressed_and_paused() {
    for at_rest in [true, false] {
        let context = Context::new();
        let tiers = Tiers::new(&context, &["zeus"]);
        let brief = a_follow_up_whose_window_never_begins(&context, &tiers, at_rest);
        // The board's message before the brief: whatever it was, it is not the brief.
        let first = brief - 1;
        context
            .adapter
            .with("zeus", |agent| agent.begun = vec![first]);
        context
            .host
            .set_snapshot(json!({ "outputQuietMs": 600_000, "screenQuietMs": 600_000 }));
        let before = enters(&context);
        context.advance(PAST_ARRIVAL);
        context.pass().unwrap();
        context.pass().unwrap();
        assert_eq!(
            enters(&context),
            before + 1,
            "at rest {at_rest}: its Enter, pressed once more"
        );
        assert_eq!(tiers.task(2).task.state, "working");
        context.advance(PAST_ARRIVAL);
        context.pass().unwrap();
        context.pass().unwrap();
        assert_eq!(tiers.task(2).task.state, "paused", "at rest {at_rest}");
        let notes = pause_notes(&tiers);
        assert_eq!(notes.len(), 1, "at rest {at_rest}: {notes:#?}");
        assert!(notes[0].contains(&format!("m-{brief}")), "{}", notes[0]);
    }
}

/// The live word apart from the record (Codex's broker, `Window::at_work`) is no word of a turn
/// begun: the broker marks the thread at work itself as it hands a message over, before Codex
/// says a turn began, and a turn that never begins leaves it so. A brief such a window began no
/// turn on is pressed Enter once and its task paused all the same.
#[test]
fn a_brief_whose_window_reads_at_work_only_by_a_live_word_apart_from_its_record_is_still_pressed_and_paused(
) {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    a_follow_up_whose_window_never_begins(&context, &tiers, false);
    context.adapter.with("zeus", |agent| agent.at_work = true);
    context
        .host
        .set_snapshot(json!({ "outputQuietMs": 600_000, "screenQuietMs": 600_000 }));
    let before = enters(&context);
    for _ in 0..2 {
        context.advance(PAST_ARRIVAL);
        context.pass().unwrap();
        context.pass().unwrap();
    }
    assert_eq!(enters(&context), before + 1, "its Enter, pressed once more");
    assert_eq!(tiers.task(2).task.state, "paused");
    assert_eq!(pause_notes(&tiers).len(), 1);
}

/// Words the human typed into the window and did not send would go with an Enter: none is
/// pressed, and the task is paused all the same, its note saying why nothing was pressed.
#[test]
fn a_window_holding_words_the_human_did_not_send_is_pressed_nothing_and_its_task_paused() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    a_follow_up_whose_window_never_begins(&context, &tiers, true);
    context.host.set_snapshot(
        json!({ "outputQuietMs": 600_000, "screenQuietMs": 600_000, "unsent": true, "tail": ["Ready for the next step."] }),
    );
    let before = enters(&context);
    for _ in 0..2 {
        context.advance(PAST_ARRIVAL);
        context.pass().unwrap();
        context.pass().unwrap();
    }
    assert_eq!(enters(&context), before, "nothing pressed");
    assert_eq!(tiers.task(2).task.state, "paused");
    let notes = pause_notes(&tiers);
    assert_eq!(notes.len(), 1, "{notes:#?}");
    assert!(
        notes[0].contains("no Enter was pressed") && notes[0].contains("not sent"),
        "{}",
        notes[0]
    );
}

/// A harness that takes its messages over a channel of its own (Pi's inbox, Codex's broker)
/// is pressed nothing: its window is paused after the same two waits, its note saying so.
#[test]
fn a_brief_a_channel_harness_never_began_is_pressed_nothing_and_its_task_paused() {
    let context = Context::new();
    let project = context
        .open_project(json!({
            "directory": "/work/app",
            "name": "app",
            "chief": { "harness": "claude-code", "model": "apollo" },
            "staff": [{ "agent": "ares", "harness": "pi", "role": "worker", "tier": "standard" }],
        }))
        .unwrap()
        .expect("the project");
    context.adapter.never_begins.set(Some(false));
    context.give(project.id, "ares", "Write the parser");
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(context.task(project.id, 1).task.state, "working");
    context
        .host
        .set_snapshot(json!({ "outputQuietMs": 600_000, "screenQuietMs": 600_000, "tail": ["Ready for the next step."] }));
    let before = enters(&context);
    for _ in 0..2 {
        context.advance(PAST_ARRIVAL);
        context.pass().unwrap();
        context.pass().unwrap();
    }
    assert_eq!(enters(&context), before, "nothing pressed");
    assert_eq!(context.task(project.id, 1).task.state, "paused");
    let chief = context.id(project.id, "chief");
    let notes: Vec<String> = context
        .inbox(chief)
        .into_iter()
        .map(|message| message.body)
        .filter(|body| body.starts_with("T-1 is paused"))
        .collect();
    assert_eq!(notes.len(), 1, "{notes:#?}");
    assert!(
        notes[0].contains("its harness takes no Enter"),
        "{}",
        notes[0]
    );
}

/// A window read in flight that never began, whose rounds of ConsensFlow's own stop it ignored
/// too, running nothing, is relaunched on its conversation (T-1130's mend, kept by T-1199):
/// nothing of a brief never begun can be lost. The task, in its hands still, is resumed with
/// its brief again and goes on there by itself, and its requester is told of a relaunch,
/// never of a close.
#[test]
fn a_task_paused_for_a_brief_its_window_read_in_flight_never_began_is_relaunched_on_its_conversation_and_goes_on_by_itself(
) {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    a_follow_up_whose_window_never_begins(&context, &tiers, false);
    context
        .host
        .set_snapshot(json!({ "outputQuietMs": 600_000, "screenQuietMs": 600_000 }));
    for _ in 0..2 {
        context.advance(PAST_ARRIVAL);
        context.pass().unwrap();
        context.pass().unwrap();
    }
    assert_eq!(tiers.task(2).task.state, "paused");
    let session = tiers.assignee(2);
    let native = native_of(&context, tiers.id(&session));
    let killed = context.host.killed().len();
    context.adapter.never_begins.set(None);
    context.host.exits_showing(Some(0), None, None);
    for _ in 0..6 {
        context.advance(5_000);
        context.pass().unwrap();
    }
    assert_eq!(context.host.killed().len(), killed + 1, "relaunched");
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(tiers.task(2).task.state, "working", "it goes on by itself");
    assert_eq!(tiers.assignee(2), session, "the same session");
    assert_eq!(
        native_of(&context, tiers.id(&session)),
        native,
        "on the same conversation"
    );
    let last = given(&context, "zeus").pop().unwrap();
    assert!(
        last.contains("· T-2 · task from @chief]") && last.contains("Now the lexer"),
        "the very brief, again: {last}"
    );
    let told: Vec<String> = tiers
        .notes("chief")
        .into_iter()
        .filter(|note| note.contains("relaunched"))
        .collect();
    assert_eq!(told.len(), 1, "{told:#?}");
    assert!(
        told[0].starts_with(&format!("T-2's window (@{session}) began no turn on m-"))
            && told[0].contains("so ConsensFlow relaunched it on its conversation and gave it m-")
            && told[0].ends_with("again. T-2 goes on by itself."),
        "{}",
        told[0]
    );
    let all = [tiers.notes("chief"), tiers.notes("human")].concat();
    assert!(
        all.iter()
            .all(|note| !note.contains("closed it") && !note.contains("did not stop")),
        "{all:#?}"
    );
}

/// The chief's pause of a task whose window never began its brief is a pause: the window that
/// ignores its rounds is kept and told, never relaunched; only ConsensFlow's own stop of a
/// brief never begun relaunches.
#[test]
fn the_chiefs_pause_of_a_window_that_never_began_its_brief_keeps_it_and_relaunches_nothing() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    a_follow_up_whose_window_never_begins(&context, &tiers, false);
    context
        .host
        .set_snapshot(json!({ "outputQuietMs": 600_000, "screenQuietMs": 600_000 }));
    let killed = context.host.killed().len();
    let opened = context.host.opened().len();
    context.pause_task(tiers.project.id, 2);
    for _ in 0..10 {
        context.advance(5_000);
        context.pass().unwrap();
    }
    assert_eq!(context.host.killed().len(), killed, "kept");
    assert_eq!(context.host.opened().len(), opened, "nothing relaunched");
    assert!(context
        .dispatcher
        .unstopped(tiers.id(&tiers.assignee(2)))
        .is_some());
    assert_eq!(tiers.task(2).task.state, "paused");
    let kept: Vec<String> = tiers
        .notes("chief")
        .into_iter()
        .filter(|note| note.contains("did not stop"))
        .collect();
    assert_eq!(kept.len(), 1, "{kept:#?}");
    assert!(tiers
        .notes("chief")
        .iter()
        .all(|note| !note.contains("relaunched")));
}

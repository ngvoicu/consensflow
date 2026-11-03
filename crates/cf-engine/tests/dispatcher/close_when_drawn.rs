//! A window the engine closes because its task is done stays open until its screen has been
//! drawn (T-1051, from T-1038's decision 2): Devin's agent ends its turn some seconds before
//! its screen has typed the answer out, and a window closed as the result was taken went
//! mid-typing, frozen on "Typing", for whoever watched. The result is taken and delivered at
//! once as ever; only the close waits, until the pane has printed nothing for a moment, or
//! for the bound. A pause, a cancel, a failure or a hold for quota close at once, whatever
//! the screen shows.
//!
//! Not held to a Node recording: Node's dispatcher closed the window as the result was
//! taken, and looked at no screen to do it.

use cf_engine::testing::Context;
use cf_harness::contract::Pane;
use serde_json::{json, Value};

use crate::fixtures::{hold_notes, Tiers};

/// How long the pane must have printed nothing: `windows.rs`' `DRAWN_QUIET_MS`.
const STILL_MS: i64 = 1_500;

/// The longest the window waits from the look that found it free: `windows.rs`'
/// `STILL_BOUND_MS`.
const BOUND_MS: i64 = 10_000;

/// Zeus has T-1, delivered and at work in a window of its own: its pane.
fn at_work(context: &Context, project: i64) -> Pane {
    context.give(project, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.task(project, 1).task.state, "working");
    context.host.last("zeus").unwrap().pane
}

/// The pane printed `ms` ago, as the host's snapshot says it.
fn printed(context: &Context, ms: i64) {
    context.host.set_snapshot(json!({ "outputQuietMs": ms }));
}

/// Zeus's agent answers, and the engine looks at it twice.
fn answers(context: &Context) {
    context.adapter.answer("zeus", "Parser done");
    context.pass().unwrap();
    context.pass().unwrap();
}

/// Whether the window has not been closed.
fn open(context: &Context, window: &Pane) -> bool {
    !context.host.killed().contains(window)
}

/// Whether the chief was given T-`number`'s result, with these words in it.
fn has_result(context: &Context, project: i64, number: i64, words: &str) -> bool {
    context
        .task(project, number)
        .messages
        .iter()
        .any(|message| message.kind == "result" && message.body.contains(words))
}

#[test]
fn a_window_whose_task_is_done_stays_open_while_its_screen_is_being_drawn_and_closes_once_it_has_been_still(
) {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    let window = at_work(&context, project.id);
    // The harness printed 300 ms ago: its screen is being drawn as its agent's turn ends.
    printed(&context, 300);
    answers(&context);
    // The answer is the chief's at once; only the close waits.
    assert_eq!(context.task(project.id, 1).task.state, "done");
    assert!(has_result(&context, project.id, 1, "Parser done"));
    assert!(open(&context, &window), "its screen is being drawn");
    assert!(
        !context.host.quiet_looks().is_empty(),
        "the engine looked at the screen"
    );

    // A second on the screen has printed again, a moment ago, and one more is still short.
    context.advance(1_000);
    printed(&context, 120);
    context.pass().unwrap();
    assert!(open(&context, &window), "it printed again");
    context.advance(1_000);
    printed(&context, STILL_MS - 1);
    context.pass().unwrap();
    assert!(
        open(&context, &window),
        "a millisecond short of being still"
    );

    // Still for the whole of it: the window closes in that step, and once.
    printed(&context, STILL_MS);
    context.pass().unwrap();
    assert_eq!(context.host.killed(), std::slice::from_ref(&window));
    context.pass().unwrap();
    assert_eq!(context.host.killed(), [window], "closed once");
    assert_eq!(context.task(project.id, 1).task.state, "done");
    context.close();
}

#[test]
fn a_screen_that_never_rests_holds_the_window_for_the_bound_from_the_look_that_found_it_free_and_no_longer(
) {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    let window = at_work(&context, project.id);
    printed(&context, 300);
    answers(&context);
    assert_eq!(context.task(project.id, 1).task.state, "done");

    // It prints on, whatever time goes by: the window waits the bound out.
    context.advance(BOUND_MS - 1);
    context.pass().unwrap();
    context.pass().unwrap();
    assert!(open(&context, &window), "a millisecond before the bound");
    assert_eq!(context.task(project.id, 1).task.state, "done");

    // The bound is over: the window closes, its screen drawing still.
    context.advance(1);
    context.pass().unwrap();
    assert_eq!(context.host.killed(), [window]);
    assert!(has_result(&context, project.id, 1, "Parser done"));
    context.close();
}

#[test]
fn a_window_whose_screen_is_already_still_or_whose_host_cannot_tell_closes_as_soon_as_its_task_is_done(
) {
    let screens: [(&str, Value); 4] = [
        ("printed a minute ago", json!({ "outputQuietMs": 60_000 })),
        (
            "printed just long enough ago",
            json!({ "outputQuietMs": STILL_MS }),
        ),
        ("a host that does not say", json!({})),
        (
            "a pane that has printed nothing",
            json!({ "outputQuietMs": null }),
        ),
    ];
    for (name, screen) in screens {
        let context = Context::new();
        let project = context.with_staff(&["zeus"]);
        let window = at_work(&context, project.id);
        context.host.set_snapshot(screen);
        answers(&context);
        assert_eq!(context.task(project.id, 1).task.state, "done", "{name}");
        assert_eq!(
            context.host.killed(),
            [window],
            "{name}: closed in the step that took the result"
        );
        context.close();
    }
}

#[test]
fn a_task_the_chief_accepts_while_its_window_waits_does_not_end_the_wait() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    let window = at_work(&context, project.id);
    printed(&context, 300);
    answers(&context);
    assert!(open(&context, &window), "its screen is being drawn");

    // The chief takes the result within a second: the task is accepted, its screen drawing.
    context.advance(1_000);
    context
        .ledger
        .borrow_mut()
        .accept_task(project.id, 1, "chief")
        .unwrap();
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.task(project.id, 1).task.state, "accepted");
    assert!(open(&context, &window), "accepted, it still waits");

    printed(&context, STILL_MS);
    context.pass().unwrap();
    assert_eq!(context.host.killed(), [window]);
    context.close();
}

#[test]
fn a_cancelled_task_closes_its_window_at_once_whatever_its_screen_shows() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    let window = at_work(&context, project.id);
    printed(&context, 300);
    context
        .ledger
        .borrow_mut()
        .cancel_task(project.id, 1, "human")
        .unwrap();
    context.pass().unwrap();
    assert_eq!(context.host.killed(), [window]);
    assert!(
        context.host.quiet_looks().is_empty(),
        "no look at a screen for a close that waits for nothing"
    );
    context.close();
}

#[test]
fn the_next_task_of_the_session_waits_for_the_same_screen_and_its_requester_is_told_nothing_of_it()
{
    let context = Context::new();
    // The host proves each window's program gone, as the app's does: the session's next task
    // goes on with its conversation (`ownership`), as this test is about something else.
    context.host.exits_showing(Some(0), None, None);
    let project = context.with_staff(&["zeus"]);
    let window = at_work(&context, project.id);
    printed(&context, 300);
    answers(&context);
    // T-2 for zeus is queued behind T-1, which is done: no other task goes into its window.
    context.give(project.id, "zeus", "Lexer");
    let opened = context.host.opened().len();
    context.pass().unwrap();
    context.pass().unwrap();
    assert!(open(&context, &window), "its screen is being drawn");
    assert_eq!(context.host.opened().len(), opened);
    assert_eq!(context.task(project.id, 2).task.state, "queued");
    let told: Vec<String> = context
        .task(project.id, 2)
        .messages
        .iter()
        .filter(|message| message.kind == "note")
        .map(|message| message.body.clone())
        .collect();
    assert!(told.is_empty(), "{told:?}");

    // The screen rests: the window closes, and T-2 opens a window of its own.
    printed(&context, STILL_MS);
    context.pass().unwrap();
    assert_eq!(context.host.killed(), std::slice::from_ref(&window));
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.host.opened().len(), opened + 1);
    assert_ne!(context.host.last("zeus").unwrap().pane, window);
    assert_eq!(context.task(project.id, 2).task.state, "working");
    context.close();
}

/// The same, where the host proves nothing of the exit: the window closes as it rests, and T-2,
/// a follow-up no window had, is held for the chief's word instead of opening a window unasked.
#[test]
fn the_next_task_of_the_session_waits_for_the_same_screen_and_a_close_with_no_proof_holds_it_for_the_chiefs_word(
) {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    let window = at_work(&context, project.id);
    printed(&context, 300);
    answers(&context);
    context.give(project.id, "zeus", "Lexer");
    let opened = context.host.opened().len();
    context.pass().unwrap();
    context.pass().unwrap();
    assert!(open(&context, &window), "its screen is being drawn");
    assert_eq!(context.host.opened().len(), opened);
    assert_eq!(context.task(project.id, 2).task.state, "queued");
    assert!(
        hold_notes(&context, project.id, 2).is_empty(),
        "told nothing while it waits"
    );

    // The screen rests: the window closes, with no proof its program ended.
    printed(&context, STILL_MS);
    context.pass().unwrap();
    assert_eq!(context.host.killed(), std::slice::from_ref(&window));
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.host.opened().len(), opened, "no window unasked");
    assert_eq!(context.task(project.id, 2).task.state, "paused");
    assert_eq!(hold_notes(&context, project.id, 2).len(), 1);

    // The chief's word sends it on, and T-2 opens a window of its own.
    context.resume_task(project.id, 2, "Go on in a window of your own.");
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(context.host.opened().len(), opened + 1);
    assert_ne!(context.host.last("zeus").unwrap().pane, window);
    assert_eq!(context.task(project.id, 2).task.state, "working");
    context.close();
}

#[test]
fn the_next_task_of_the_session_waits_no_longer_than_the_bound_for_a_screen_that_never_rests() {
    let context = Context::new();
    // The host proves each window's program gone, as the app's does: the session's next task
    // goes on with its conversation (`ownership`), as this test is about something else.
    context.host.exits_showing(Some(0), None, None);
    let project = context.with_staff(&["zeus"]);
    let window = at_work(&context, project.id);
    printed(&context, 300);
    answers(&context);
    context.give(project.id, "zeus", "Lexer");
    let opened = context.host.opened().len();
    context.pass().unwrap();
    assert!(open(&context, &window));

    // The bound runs from the first look that found the window free, however often the
    // next task, queued behind it, keeps the window from closing for itself.
    context.advance(BOUND_MS - 1);
    context.pass().unwrap();
    context.pass().unwrap();
    assert!(open(&context, &window), "a millisecond before the bound");
    context.advance(1);
    context.pass().unwrap();
    assert_eq!(context.host.killed(), std::slice::from_ref(&window));
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.host.opened().len(), opened + 1);
    assert_eq!(context.task(project.id, 2).task.state, "working");
    context.close();
}

/// The same, where the host proves nothing of the exit: at the bound the window closes, and T-2
/// is held for the chief's word instead of opening a window unasked.
#[test]
fn the_next_task_of_the_session_waits_no_longer_than_the_bound_and_a_close_with_no_proof_holds_it_for_the_chiefs_word(
) {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    let window = at_work(&context, project.id);
    printed(&context, 300);
    answers(&context);
    context.give(project.id, "zeus", "Lexer");
    let opened = context.host.opened().len();
    context.pass().unwrap();
    assert!(open(&context, &window));

    context.advance(BOUND_MS - 1);
    context.pass().unwrap();
    context.pass().unwrap();
    assert!(open(&context, &window), "a millisecond before the bound");
    context.advance(1);
    context.pass().unwrap();
    assert_eq!(context.host.killed(), std::slice::from_ref(&window));
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.host.opened().len(), opened, "no window unasked");
    assert_eq!(context.task(project.id, 2).task.state, "paused");
    assert_eq!(hold_notes(&context, project.id, 2).len(), 1);

    context.resume_task(project.id, 2, "Go on in a window of your own.");
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(context.host.opened().len(), opened + 1);
    assert_eq!(context.task(project.id, 2).task.state, "working");
    context.close();
}

#[test]
fn a_task_in_hand_again_ends_the_wait_and_the_next_time_it_is_done_the_window_waits_its_own_time() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    let window = at_work(&context, project.id);
    printed(&context, 300);
    answers(&context);
    context.advance(6_000);
    context.pass().unwrap();
    assert!(open(&context, &window), "six seconds into the wait");

    // The chief follows up on T-1 in the window that is still open: the screen rests, the
    // words go in, and T-1 is at work again.
    context
        .ledger
        .borrow_mut()
        .reopen_task(project.id, 1, "chief", "The tests fail")
        .unwrap();
    printed(&context, 5_000);
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(context.task(project.id, 1).task.state, "working");
    assert!(open(&context, &window), "a task in hand keeps its window");

    // Fifteen seconds on, more than the bound after the first wait began, T-1 is done again
    // with its screen drawing: the window waits again, from this look.
    context.advance(15_000);
    printed(&context, 300);
    context
        .ledger
        .borrow_mut()
        .record_result(project.id, 1, "The tests pass")
        .unwrap();
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.task(project.id, 1).task.state, "done");
    assert!(open(&context, &window), "its own wait has begun");
    context.advance(BOUND_MS - 1);
    context.pass().unwrap();
    assert!(open(&context, &window), "a millisecond before its bound");
    context.advance(1);
    context.pass().unwrap();
    assert_eq!(context.host.killed(), [window]);
    context.close();
}

#[test]
fn a_window_opened_since_waits_its_own_time_not_the_bound_of_the_one_that_closed() {
    let context = Context::new();
    // The host proves each window's program gone, as the app's does: the session's next task
    // goes on with its conversation (`ownership`), as this test is about something else.
    context.host.exits_showing(Some(0), None, None);
    let project = context.with_staff(&["zeus"]);
    let first = at_work(&context, project.id);
    printed(&context, 300);
    answers(&context);
    context.advance(BOUND_MS);
    context.pass().unwrap();
    assert_eq!(
        context.host.killed(),
        std::slice::from_ref(&first),
        "the bound is over"
    );

    // T-2 opens a window, and is done in it with the screen drawing: that window waits.
    printed(&context, 5_000);
    context.give(project.id, "zeus", "Lexer");
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(context.task(project.id, 2).task.state, "working");
    let second = context.host.last("zeus").unwrap().pane;
    assert_ne!(second, first);
    printed(&context, 300);
    context.adapter.answer("zeus", "Lexer done");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.task(project.id, 2).task.state, "done");
    assert!(open(&context, &second), "it waits its own time");
    context.advance(BOUND_MS);
    context.pass().unwrap();
    assert!(!open(&context, &second));
    context.close();
}

/// The same, where the host proves nothing of the exit: T-2, a follow-up no window had, is held
/// for the chief's word when the first window has closed, and the window it is then sent on in
/// waits its own time, as before.
#[test]
fn a_window_opened_since_waits_its_own_time_and_a_first_close_with_no_proof_holds_the_task_for_the_chiefs_word(
) {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    let first = at_work(&context, project.id);
    printed(&context, 300);
    answers(&context);
    context.advance(BOUND_MS);
    context.pass().unwrap();
    assert_eq!(
        context.host.killed(),
        std::slice::from_ref(&first),
        "the bound is over"
    );

    printed(&context, 5_000);
    context.give(project.id, "zeus", "Lexer");
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(context.task(project.id, 2).task.state, "paused");
    assert_eq!(hold_notes(&context, project.id, 2).len(), 1);
    assert_eq!(
        context.host.last("zeus").unwrap().pane,
        first,
        "no second window yet"
    );

    context.resume_task(project.id, 2, "Go on in a window of your own.");
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(context.task(project.id, 2).task.state, "working");
    let second = context.host.last("zeus").unwrap().pane;
    assert_ne!(second, first);
    printed(&context, 300);
    context.adapter.answer("zeus", "Lexer done");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.task(project.id, 2).task.state, "done");
    assert!(open(&context, &second), "it waits its own time");
    context.advance(BOUND_MS);
    context.pass().unwrap();
    assert!(!open(&context, &second));
    context.close();
}

#[test]
fn a_window_the_human_opens_on_a_finished_session_waits_its_own_time_once_hidden_not_the_bound_of_the_one_that_closed(
) {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    let project = tiers.project.id;
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working");
    let session = tiers.assignee(1);
    let first = context.host.last(&session).unwrap().pane;
    printed(&context, 300);
    context.adapter.answer("zeus", "Parser done");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "done");
    assert!(open(&context, &first), "its screen is being drawn");
    context.advance(BOUND_MS);
    context.pass().unwrap();
    assert!(!open(&context, &first), "the bound is over");

    // The human shows the session's window again, a window of its own that stays while they
    // watch it, and hides it while its screen is drawing: it waits from then, not from the
    // look that found the window before it free.
    printed(&context, 5_000);
    context.open_window(project, &session).unwrap();
    context.pass().unwrap();
    let shown = context.host.last(&session).unwrap().pane;
    assert_ne!(shown, first);
    assert!(open(&context, &shown), "shown, it stays");
    printed(&context, 300);
    context.hide_window(project, &session).unwrap();
    context.pass().unwrap();
    assert!(open(&context, &shown), "hidden, its screen is being drawn");
    context.advance(BOUND_MS - 1);
    context.pass().unwrap();
    assert!(open(&context, &shown), "a millisecond before its bound");
    context.advance(1);
    context.pass().unwrap();
    assert!(!open(&context, &shown), "its bound is over");
    context.close();
}

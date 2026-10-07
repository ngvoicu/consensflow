//! A pause asks a stop of its task's window, and the window owes it until it
//! has paid it, at rest, in the engine's own record of that window: one Escape
//! for a turn that is still at work, none for one that is not, and no
//! forgiveness for a pause that came in a moment nothing could see. The
//! rules are those of `stops.rs`. None of these tests is held to a Node
//! recording: the rule is Node's no more.

use cf_engine::testing::Context;
use cf_engine::{ActivityState, Unstopped};
use cf_harness::contract::Pane;
use serde_json::{json, Value};

use crate::fixtures::{after, assert_match, exhausted};

/// What pressing Escape into `pane` is, as the host is asked it.
fn escape(pane: &Pane) -> Value {
    json!({ "id": pane.id, "generation": pane.generation, "bytes": [27] })
}

/// The pane of the last window of `handle`.
fn pane_of(context: &Context, handle: &str) -> Pane {
    context.host.last(handle).expect("its window").pane
}

/// A project with zeus at work on T-1, its brief received.
fn working(context: &Context) -> i64 {
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.task(project.id, 1).task.state, "working");
    project.id
}

/// The notes of `who` that say a window did not stop.
fn ignored(context: &Context, project: i64, who: &str) -> Vec<String> {
    context
        .inbox(context.id(project, who))
        .into_iter()
        .filter(|message| message.kind == "note" && message.body.contains("did not stop"))
        .map(|message| message.body)
        .collect()
}

#[test]
fn a_message_in_flight_when_the_task_is_paused_and_resumed_in_one_tick_costs_one_escape_and_the_words_wait_for_the_turn_to_end(
) {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    // The brief is pasted and not yet seen to have arrived.
    context.pause_task(project.id, 1);
    context.resume_task(project.id, 1, "Carry on");
    context.pass().unwrap();
    let pane = pane_of(&context, "zeus");
    assert_eq!(
        context.task(project.id, 1).task.state,
        "queued",
        "the brief arrived, and the words that resume it have not"
    );
    assert_eq!(
        context.host.inputs(),
        [escape(&pane)],
        "the turn the brief began is the one the pause stopped"
    );

    // The old turn ends; the words go in once the window is at rest, and no key with them.
    context.adapter.answer("zeus", "Parser, a first draft");
    context.pass().unwrap();
    let words = context.adapter.agent("zeus").items.pop().unwrap();
    assert_match(&words.text, r"T-1 · task from @chief\]\nResumed: Carry on$");
    assert_eq!(context.host.inputs().len(), 1, "no second key");
    context.pass().unwrap();
    assert_eq!(context.task(project.id, 1).task.state, "working");

    // The result is the turn after the words, not the draft before them.
    context.adapter.answer("zeus", "Parser, finished");
    context.pass().unwrap();
    let thread = context.task(project.id, 1);
    assert_eq!(thread.task.state, "done");
    let result = thread.messages.iter().find(|m| m.kind == "result").unwrap();
    assert_eq!(result.body, "Parser, finished");
}

#[test]
fn a_stall_a_resume_and_a_fresh_window_press_no_escape() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    // The window closed mid-task: the task is paused, and the pause is a stop of a window that is gone.
    context.exit("zeus");
    assert_eq!(context.task(project.id, 1).task.state, "paused");
    context.resume_task(project.id, 1, "Carry on");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.task(project.id, 1).task.state, "working");
    assert_eq!(
        context.host.inputs(),
        Vec::<Value>::new(),
        "a window that opened after the pause owes nothing for it"
    );
}

#[test]
fn three_rounds_ignored_exhaust_the_stop_which_is_said_once_tried_again_every_minute_and_paid_at_rest(
) {
    let context = Context::new();
    let project = working(&context);
    let zeus = context.id(project, "zeus");
    context.adapter.busy("zeus");
    context.pause_task(project, 1);
    let pane = pane_of(&context, "zeus");
    // The harness ignores every key: three rounds, three seconds apart.
    for _ in 0..3 {
        context.pass().unwrap();
        context.advance(3_100);
    }
    assert_eq!(context.host.inputs().len(), 3);
    assert_eq!(
        context.dispatcher.unstopped(zeus),
        None,
        "not yet given up on"
    );
    // The look after the third: nothing pressed, and the stop is given up on.
    context.pass().unwrap();
    assert_eq!(context.host.inputs().len(), 3);
    assert_eq!(
        context.dispatcher.unstopped(zeus),
        Some(Unstopped { task: 1, rounds: 3 })
    );
    let (human, chief) = (
        ignored(&context, project, "human"),
        ignored(&context, project, "chief"),
    );
    assert_eq!((human.len(), chief.len()), (1, 1), "each is told once");
    assert_match(
        &human[0],
        r"^@zeus did not stop for T-1: it ignored the interrupt 3 times and is still on its earlier turn\. What is for it waits until that turn ends, and what it writes before then is not T-1's result\. To stop it now, cancel T-1, or reassign it if it was given by tier\.$",
    );
    assert_match(
        &chief[0],
        r"^T-1's window \(@zeus\) did not stop: it ignored the interrupt and is still on its earlier turn\. Your words wait until that turn ends; what it writes before then is not taken as T-1's result \(cf task get T-1 --transcript shows it\)\. To stop it now: cf task cancel T-1\.$",
    );

    // A minute later, one round more; and the notes are not said again.
    for _ in 0..18 {
        context.advance(3_100);
        context.pass().unwrap();
    }
    assert_eq!(context.host.inputs().len(), 3, "less than a minute: none");
    context.advance(10_000);
    context.pass().unwrap();
    assert_eq!(context.host.inputs(), vec![escape(&pane); 4]);
    assert_eq!(
        context.dispatcher.unstopped(zeus),
        Some(Unstopped { task: 1, rounds: 4 }),
        "the board says how often"
    );
    assert_eq!(ignored(&context, project, "human").len(), 1);

    // Its turn ends: the stop is paid with no key, and what the board said is gone.
    context.adapter.answer("zeus", "Where I was");
    context.pass().unwrap();
    assert_eq!(context.dispatcher.unstopped(zeus), None);
    assert_eq!(context.host.inputs().len(), 4);
    // An idle window is given no key, however long ago the stop was asked.
    context.advance(10 * 60_000);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.host.inputs().len(), 4);
    assert_eq!(context.task(project, 1).task.state, "paused");
}

#[test]
fn a_harness_that_takes_its_turn_up_again_at_the_reset_is_interrupted_before_the_resume_goes_in() {
    let context = Context::new();
    let project = working(&context);
    let zeus = context.id(project, "zeus");
    // The harness is out of quota and waits to try again, so its window never
    // rests while the task is held: the stop the hold asked is never paid.
    context.adapter.busy("zeus");
    let resets_at = after(&context, 2 * 3_600_000);
    context
        .adapter
        .quota("zeus", Some(exhausted(None, Some(&resets_at))));
    context.pass().unwrap();
    assert_eq!(context.task(project, 1).task.state, "paused", "held");
    context.pass().unwrap();
    let pressed = context.host.inputs().len();
    assert!(
        pressed >= 1,
        "its window at work is interrupted while it is out"
    );
    assert_eq!(context.dispatcher.activity(zeus).state, ActivityState::Out);

    // The reset: its turn is up again by itself, and the daemon resumes the task.
    context.advance(2 * 3_600_000 + 1_000);
    context.adapter.quota("zeus", None);
    context.pass().unwrap();
    assert_eq!(
        context.task(project, 1).task.state,
        "queued",
        "the words wait for the turn to end"
    );
    assert!(
        context.host.inputs().len() > pressed,
        "interrupted again, before the words go in"
    );
    let words = context.adapter.agent("zeus").items.pop().unwrap();
    assert!(
        !words.text.contains("Resumed: Go on where you stopped"),
        "nothing was pasted into the working turn"
    );
    // It stops; the words go in, once.
    context.adapter.answer("zeus", "Stopped at the lexer");
    context.pass().unwrap();
    let words = context.adapter.agent("zeus").items.pop().unwrap();
    assert_match(&words.text, r"Resumed: Go on where you stopped\.$");
    context.pass().unwrap();
    assert_eq!(context.task(project, 1).task.state, "working");
}

#[test]
fn a_second_pause_before_the_window_is_seen_at_rest_costs_no_second_key_and_is_paid_at_that_look() {
    let context = Context::new();
    let project = working(&context);
    context.adapter.busy("zeus");
    context.pause_task(project, 1);
    context.pass().unwrap();
    assert_eq!(context.host.inputs().len(), 1, "Escape, once");
    // The key worked, and the chief paused again before any look saw it at rest.
    context.adapter.answer("zeus", "Stopped");
    context.resume_task(project, 1, "Use JSON");
    context.pause_task(project, 1);
    context.pass().unwrap();
    assert_eq!(
        context.host.inputs().len(),
        1,
        "the window is at rest: no key"
    );
    assert_eq!(context.task(project, 1).task.state, "paused");

    // Resumed once more, the words go in, and the task goes on with no key at all.
    context.resume_task(project, 1, "Use JSON, and test it");
    context.pass().unwrap();
    let words = context.adapter.agent("zeus").items.pop().unwrap();
    assert_match(&words.text, r"Resumed: Use JSON, and test it$");
    context.pass().unwrap();
    assert_eq!(context.task(project, 1).task.state, "working");
    assert_eq!(context.host.inputs().len(), 1);
}

#[test]
fn a_window_out_of_quota_and_still_at_work_is_interrupted_while_its_activity_says_it_is_out() {
    let context = Context::new();
    let project = working(&context);
    let zeus = context.id(project, "zeus");
    context.adapter.busy("zeus");
    let resets_at = after(&context, 3 * 3_600_000);
    context
        .adapter
        .quota("zeus", Some(exhausted(None, Some(&resets_at))));
    context.pass().unwrap();
    let pane = pane_of(&context, "zeus");
    context.pass().unwrap();
    assert_eq!(context.dispatcher.activity(zeus).state, ActivityState::Out);
    assert_eq!(
        context.host.inputs(),
        [escape(&pane)],
        "what its look finds it doing is what counts, not what the board says"
    );
    // Its turn ends on the refusal: the stop is paid with no key.
    context.adapter.answer("zeus", "Out of quota");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.host.inputs().len(), 1);
    assert_eq!(context.dispatcher.unstopped(zeus), None);
}

#[test]
fn a_pause_of_a_task_the_window_holds_but_does_not_work_on_presses_no_key_and_its_own_task_is_stopped(
) {
    let context = Context::new();
    let project = working(&context);
    // A second task given to the same participant: its words wait for the
    // first to be over, and the window works on the first.
    context.give(project, "zeus", "Docs");
    context.pass().unwrap();
    assert_eq!(context.task(project, 2).task.state, "queued");
    context.adapter.busy("zeus");
    context.pause_task(project, 2);
    context.pass().unwrap();
    assert_eq!(
        context.host.inputs(),
        Vec::<Value>::new(),
        "T-2 was never given to the window at work on T-1"
    );

    context.pause_task(project, 1);
    context.pass().unwrap();
    let pane = pane_of(&context, "zeus");
    assert_eq!(
        context.host.inputs(),
        [escape(&pane)],
        "the pause of the task it works on is a stop of it"
    );
}

#[test]
fn the_words_that_first_reach_a_window_for_a_task_paused_before_they_came_answer_that_pause() {
    let context = Context::new();
    let project = working(&context);
    // T-2 is given to the window, which works on T-1, and is paused before any
    // words of it reach the window: nothing in the window is for it to stop.
    context.give(project, "zeus", "Docs");
    context.pause_task(project, 2);
    context.pass().unwrap();
    assert_eq!(context.host.inputs(), Vec::<Value>::new());
    // T-1 ends paused, and its window, seen at rest, pays that stop with no key.
    context.pause_task(project, 1);
    context.adapter.answer("zeus", "Stopped");
    context.pass().unwrap();
    assert_eq!(context.host.inputs(), Vec::<Value>::new());

    // The words that resume T-2 are the first it is given, a window at rest
    // taking them: no turn of the window was ever for T-2's pause to stop.
    context.resume_task(project, 2, "Go on");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.task(project, 2).task.state, "working");
    context.pass().unwrap();
    assert_eq!(
        context.host.inputs(),
        Vec::<Value>::new(),
        "the turn the words began is not interrupted for a pause that came before them"
    );
}

#[test]
fn a_stop_paid_for_one_task_the_window_held_never_hides_the_stop_of_the_task_it_goes_on_with() {
    let context = Context::new();
    let project = working(&context);
    context.give(project, "zeus", "Docs");
    // T-1 is paused and its turn ends before a look finds it at work: the
    // window, seen at rest, pays the stop with no key.
    context.pause_task(project, 1);
    context.adapter.answer("zeus", "Stopped");
    context.pass().unwrap();
    // The words of T-2 go in, and the window works on it.
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.task(project, 2).task.state, "working");
    assert_eq!(context.task(project, 1).task.state, "paused");
    assert_eq!(context.host.inputs(), Vec::<Value>::new());

    // T-2 is paused in its turn: its stop is owed, whatever was paid for T-1.
    context.adapter.busy("zeus");
    context.pause_task(project, 2);
    context.pass().unwrap();
    let pane = pane_of(&context, "zeus");
    assert_eq!(context.host.inputs(), [escape(&pane)]);
}

#[test]
fn a_turn_stopped_before_its_first_word_is_paid_at_rest_and_the_resume_goes_in_once_and_ends_with_its_result(
) {
    let context = Context::new();
    let project = working(&context);
    let zeus = context.id(project, "zeus");
    assert_eq!(
        context.adapter.agent("zeus").interrupted,
        0,
        "a window nobody pressed a key into was never told"
    );
    // The turn is in the hooks of its prompt, before a word of its answer. A
    // harness that writes no record of a turn interrupted there reads at rest
    // once it has stopped, its record as it was (the brief, and nothing after).
    context.adapter.busy_until_interrupted("zeus");
    context.pause_task(project, 1);
    let pane = pane_of(&context, "zeus");
    context.pass().unwrap();
    assert_eq!(context.host.inputs(), [escape(&pane)]);
    assert_eq!(
        context.adapter.agent("zeus").interrupted,
        1,
        "the window is told of the press: it reads the look that follows by it"
    );

    // The looks that follow find it at rest: paid, with no second key, and
    // nobody is told it ignored the stop.
    for _ in 0..3 {
        context.advance(3_100);
        context.pass().unwrap();
    }
    assert_eq!(context.host.inputs().len(), 1);
    assert_eq!(context.dispatcher.unstopped(zeus), None);
    assert_eq!(ignored(&context, project, "human"), Vec::<String>::new());
    assert_eq!(ignored(&context, project, "chief"), Vec::<String>::new());
    let thread = context.task(project, 1);
    assert_eq!(thread.task.state, "paused");
    assert!(
        thread
            .messages
            .iter()
            .all(|message| message.kind != "result"),
        "no result comes of a turn that was stopped"
    );

    // The words that resume the task go in at once, once, and the result is
    // the turn after them.
    context.resume_task(project, 1, "Carry on");
    context.pass().unwrap();
    let words = context.adapter.agent("zeus").items.pop().unwrap();
    assert_match(&words.text, r"T-1 · task from @chief\]\nResumed: Carry on$");
    assert_eq!(context.host.inputs().len(), 1, "no key with them");
    context.pass().unwrap();
    assert_eq!(context.task(project, 1).task.state, "working");
    context.adapter.answer("zeus", "Parser, finished");
    context.pass().unwrap();
    let thread = context.task(project, 1);
    assert_eq!(thread.task.state, "done");
    let result = thread.messages.iter().find(|m| m.kind == "result").unwrap();
    assert_eq!(result.body, "Parser, finished");
}

#[test]
fn keys_the_host_refused_are_no_press_and_the_window_is_not_told_until_a_round_goes_in() {
    let context = Context::new();
    let project = working(&context);
    // The window stops when it is told it was interrupted, and not otherwise.
    context.adapter.busy_until_interrupted("zeus");
    context.host.refuse_keys.set(true);
    context.pause_task(project, 1);
    let pane = pane_of(&context, "zeus");
    context.pass().unwrap();
    assert_eq!(context.host.inputs(), [escape(&pane)], "the key was tried");
    assert_eq!(
        context.adapter.agent("zeus").interrupted,
        0,
        "none was taken: no interrupt was pressed, and a window idle in its hooks must not be read at rest for it"
    );

    // The next round is taken: now it is told, and the window reads at rest.
    context.host.refuse_keys.set(false);
    context.advance(3_100);
    context.pass().unwrap();
    assert_eq!(context.host.inputs().len(), 2);
    assert_eq!(context.adapter.agent("zeus").interrupted, 1);
    context.pass().unwrap();
    assert_eq!(
        context.dispatcher.unstopped(context.id(project, "zeus")),
        None
    );
    assert_eq!(
        context.host.inputs().len(),
        2,
        "paid at rest, with no more keys"
    );
}

#[test]
fn each_round_of_keys_is_told_to_the_window_and_a_round_given_up_on_is_not() {
    let context = Context::new();
    let project = working(&context);
    context.adapter.busy("zeus");
    context.pause_task(project, 1);
    for round in 1..=3_u32 {
        context.pass().unwrap();
        assert_eq!(context.host.inputs().len(), round as usize);
        assert_eq!(context.adapter.agent("zeus").interrupted, round);
        context.advance(3_100);
    }
    // The look after the third: the stop is given up on, and no key goes in.
    context.pass().unwrap();
    assert_eq!(context.host.inputs().len(), 3);
    assert_eq!(context.adapter.agent("zeus").interrupted, 3);
}

//! A pause asks a stop of its task's window, and the window owes it until it
//! has paid it, at rest, in the engine's own record of that window: one Escape
//! for a turn that is still at work, none for one that is not, and no
//! forgiveness for a pause that came in a moment nothing could see. The
//! rules are those of `stops.rs`. None of these tests is held to a Node
//! recording: the rule is Node's no more.

use cf_engine::testing::{Context, Gate};
use cf_engine::{ActivityState, Unstopped};
use cf_harness::contract::Pane;
use serde_json::{json, Value};

use cf_ledger::NewTask;

use crate::fixtures::{after, assert_match, exhausted, Tiers};

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

/// What the human and the chief are told of a window closed for a pause it
/// ignored, while its task is still paused.
const CLOSED_PAUSED: [&str; 2] = [
    "@zeus did not stop for T-1: it ignored the interrupt 3 times, so ConsensFlow closed its window. T-1 stays paused, and its window reopens on its conversation when it is resumed.",
    "T-1's window (@zeus) did not stop: it ignored the interrupt, so ConsensFlow closed it. T-1 stays paused, and its window reopens on its conversation when it is resumed.",
];

/// What they are told of it once the task was resumed while the window ignored the rounds.
const CLOSED_RESUMED: [&str; 2] = [
    "@zeus did not stop for T-1: it ignored the interrupt 3 times, so ConsensFlow closed its window. The words that resume T-1 go in once its window reopens on its conversation.",
    "T-1's window (@zeus) did not stop: it ignored the interrupt, so ConsensFlow closed it. Your words go in once it reopens on its conversation.",
];

/// The notes the human and the chief were given that a window did not stop.
fn told(context: &Context, project: i64) -> [Vec<String>; 2] {
    [
        ignored(context, project, "human"),
        ignored(context, project, "chief"),
    ]
}

/// One note of a window that did not stop, to the chief who gave the task, in the
/// chief's words of `said` ([human's, chief's]), and none to the human: two notes of
/// one event said it twice (T-502's m-3943 and m-3944, 2026-10-25).
fn to_the_chief(said: [&str; 2]) -> [Vec<String>; 2] {
    [Vec::new(), vec![said[1].to_owned()]]
}

/// The window at work on T-1 ignores every key, as a harness hung in a model
/// request does (Pi on zeewin's long run, 2026-10-19): T-1 is paused and its three
/// rounds are pressed, three seconds apart. The pane it was pressed into.
fn ignored_three_rounds(context: &Context, project: i64) -> Pane {
    context.adapter.busy("zeus");
    context.pause_task(project, 1);
    let pane = pane_of(context, "zeus");
    for _ in 0..3 {
        context.pass().unwrap();
        context.advance(3_100);
    }
    assert_eq!(context.host.inputs(), vec![escape(&pane); 3]);
    assert!(
        context.host.killed().is_empty(),
        "not closed before the third round is over"
    );
    pane
}

#[test]
fn three_rounds_ignored_close_the_window_the_task_stays_paused_and_its_resume_reopens_it_on_its_conversation(
) {
    let context = Context::new();
    let project = working(&context);
    let zeus = context.id(project, "zeus");
    let native = context.adapter.agent("zeus").native;
    let launches = context.adapter.prepared().len();
    let pane = ignored_three_rounds(&context, project);

    // The look after the third: no key, and the window is closed, as the host proves.
    context.host.exits_showing(Some(0), None, None);
    context.pass().unwrap();
    assert_eq!(context.host.inputs().len(), 3);
    assert_eq!(context.host.killed(), [pane]);
    assert_eq!(
        context.dispatcher.activity(zeus).state,
        ActivityState::Closed
    );
    assert_eq!(
        context.dispatcher.unstopped(zeus),
        None,
        "a closed window has nothing to show of the stop"
    );
    assert_eq!(context.task(project, 1).task.state, "paused");
    assert_eq!(told(&context, project), to_the_chief(CLOSED_PAUSED));

    // Nothing more while it is paused: no window opens for it, nothing is said again.
    context.advance(10 * 60_000);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.adapter.prepared().len(), launches);
    assert_eq!(context.host.inputs().len(), 3);
    assert_eq!(told(&context, project).map(|notes| notes.len()), [0, 1]);
    assert_eq!(context.task(project, 1).task.state, "paused");

    // Resumed: the window reopens on its conversation, told its program before is
    // gone, with the words as its first message and no key.
    context.resume_task(project, 1, "Carry on");
    context.pass().unwrap();
    let back = context.adapter.prepared().pop().unwrap();
    assert_eq!(back["resume"], json!(native), "the same conversation");
    assert_eq!(context.adapter.previous_gone().last(), Some(&true));
    assert_match(
        back["message"].as_str().unwrap(),
        r"T-1 · task from @chief\]\nResumed: Carry on$",
    );
    context.pass().unwrap();
    assert_eq!(context.task(project, 1).task.state, "working");
    assert_eq!(
        context.host.inputs().len(),
        3,
        "the new window owes nothing for the pause"
    );
    context.adapter.answer("zeus", "Parser, finished");
    context.pass().unwrap();
    let thread = context.task(project, 1);
    assert_eq!(thread.task.state, "done");
    let result = thread.messages.iter().find(|m| m.kind == "result").unwrap();
    assert_eq!(result.body, "Parser, finished");
}

#[test]
fn a_task_resumed_while_its_window_ignores_the_rounds_has_its_words_go_into_the_window_that_reopens(
) {
    let context = Context::new();
    let project = working(&context);
    let native = context.adapter.agent("zeus").native;
    context.adapter.busy("zeus");
    context.pause_task(project, 1);
    context.pass().unwrap();
    // The chief resumes it at once, as zeewin's did 16 s after its pause: the words
    // wait for a window at rest, which this one never is.
    context.resume_task(project, 1, "Carry on");
    for _ in 0..3 {
        context.advance(3_100);
        context.pass().unwrap();
    }
    assert_eq!(context.host.inputs().len(), 3);
    assert_eq!(
        context.host.killed().len(),
        1,
        "closed after its third round"
    );
    assert_eq!(context.task(project, 1).task.state, "queued");
    assert_eq!(told(&context, project), to_the_chief(CLOSED_RESUMED));

    // The next pass opens it again, on its conversation, with the words.
    context.pass().unwrap();
    let back = context.adapter.prepared().pop().unwrap();
    assert_eq!(back["resume"], json!(native));
    assert_match(
        back["message"].as_str().unwrap(),
        r"T-1 · task from @chief\]\nResumed: Carry on$",
    );
    context.pass().unwrap();
    assert_eq!(context.task(project, 1).task.state, "working");
    context.adapter.answer("zeus", "Parser, finished");
    context.pass().unwrap();
    assert_eq!(context.task(project, 1).task.state, "done");
}

#[test]
fn a_window_held_for_quota_that_ignores_the_rounds_is_closed_and_the_reset_reopens_it_and_gives_the_words_once(
) {
    let context = Context::new();
    let project = working(&context);
    let native = context.adapter.agent("zeus").native;
    let launches = context.adapter.prepared().len();
    // Out of quota, its harness hung in the turn: the task is held, and the window
    // never rests.
    context.adapter.busy("zeus");
    let resets_at = after(&context, 2 * 3_600_000);
    context
        .adapter
        .quota("zeus", Some(exhausted(None, Some(&resets_at))));
    context.pass().unwrap();
    assert_eq!(context.task(project, 1).task.state, "paused", "held");
    let pane = pane_of(&context, "zeus");
    for _ in 0..3 {
        context.pass().unwrap();
        context.advance(3_100);
    }
    assert_eq!(context.host.inputs(), vec![escape(&pane); 3]);
    context.host.exits_showing(Some(0), None, None);
    context.pass().unwrap();
    assert_eq!(
        context.host.killed(),
        [pane],
        "closed while its member is out"
    );
    assert_eq!(context.task(project, 1).task.state, "paused");
    assert_eq!(told(&context, project), to_the_chief(CLOSED_PAUSED));

    // Held until the reset: no window opens, and nothing goes in.
    context.advance(60 * 60_000);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.adapter.prepared().len(), launches);
    assert_eq!(context.task(project, 1).task.state, "paused");

    // The reset: the daemon resumes the task, and the window reopens on its
    // conversation with the hold's words as its first message.
    context.advance(60 * 60_000 + 1_000);
    context.pass().unwrap();
    let back = context.adapter.prepared().pop().unwrap();
    assert_eq!(context.adapter.prepared().len(), launches + 1);
    assert_eq!(back["resume"], json!(native), "the same conversation");
    assert_match(
        back["message"].as_str().unwrap(),
        r"T-1 · task from ConsensFlow\]\nResumed: Go on where you stopped\.$",
    );
    context.pass().unwrap();
    assert_eq!(context.task(project, 1).task.state, "working");
    context.adapter.answer("zeus", "Parser, finished");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.task(project, 1).task.state, "done");
    let given = context
        .adapter
        .agent("zeus")
        .items
        .iter()
        .filter(|item| item.text.contains("Resumed: Go on where you stopped"))
        .count();
    assert_eq!(given, 1, "the words went in once");
    assert_eq!(
        context.host.inputs().len(),
        3,
        "and no key into the new window"
    );
}

#[test]
fn a_task_cancelled_while_its_window_is_closed_is_said_to_be_cancelled_and_no_words_are_promised() {
    // Astra's order on round 1: the close is asked of the host, which takes its
    // time, and the task is cancelled before it answers.
    let context = Context::new();
    let project = working(&context);
    let pane = ignored_three_rounds(&context, project);
    let held = Gate::default();
    *context.host.hold_kill.borrow_mut() = Some((pane.generation, held.clone()));
    let stepping = context.begin_pass();
    context.settle();
    context
        .ledger
        .borrow_mut()
        .cancel_task(project, 1, "human")
        .unwrap();
    held.open();
    context.settle();
    stepping.take().unwrap().unwrap();
    assert_eq!(context.host.killed(), [pane]);
    assert_eq!(context.task(project, 1).task.state, "cancelled");
    assert_eq!(
        told(&context, project),
        to_the_chief([
            "@zeus did not stop for T-1: it ignored the interrupt 3 times, so ConsensFlow closed its window. T-1 was cancelled.",
            "T-1's window (@zeus) did not stop: it ignored the interrupt, so ConsensFlow closed it. T-1 was cancelled.",
        ])
    );
}

#[test]
fn a_close_the_host_refuses_is_shown_said_once_and_tried_again_every_minute_with_no_key() {
    let context = Context::new();
    let project = working(&context);
    let zeus = context.id(project, "zeus");
    let pane = ignored_three_rounds(&context, project);
    context.host.refuse_kills.set(true);
    context.pass().unwrap();
    assert_eq!(
        context.host.killed(),
        std::slice::from_ref(&pane),
        "the close was tried"
    );
    assert_eq!(
        context.dispatcher.unstopped(zeus),
        Some(Unstopped { task: 1, rounds: 3 }),
        "the board shows the window that did not stop and is still open"
    );
    assert_eq!(
        told(&context, project),
        to_the_chief([
            "@zeus did not stop for T-1: it ignored the interrupt 3 times, and its window would not close. ConsensFlow tries to close it again every minute; until then what is for it waits, and what it writes is not T-1's result.",
            "T-1's window (@zeus) did not stop: it ignored the interrupt, and would not close. ConsensFlow tries again every minute; until then your words wait, and what it writes is not taken as T-1's result (cf task get T-1 --transcript shows it).",
        ])
    );

    // Less than a minute: nothing; a minute: the close again, no key, nothing said.
    for _ in 0..18 {
        context.advance(3_100);
        context.pass().unwrap();
    }
    assert_eq!(context.host.killed().len(), 1);
    context.advance(10_000);
    context.pass().unwrap();
    assert_eq!(context.host.killed(), vec![pane.clone(); 2]);
    assert_eq!(
        context.host.inputs().len(),
        3,
        "no key once every round was ignored"
    );
    assert_eq!(told(&context, project).map(|notes| notes.len()), [0, 1]);

    // The host takes the next: closed, and said so; the task stays paused.
    context.host.refuse_kills.set(false);
    context.advance(60_000);
    context.pass().unwrap();
    assert_eq!(context.host.killed(), vec![pane; 3]);
    assert_eq!(
        context.dispatcher.activity(zeus).state,
        ActivityState::Closed
    );
    assert_eq!(context.dispatcher.unstopped(zeus), None);
    let [human, chief] = told(&context, project);
    assert_eq!(human, Vec::<String>::new());
    assert_eq!(chief.len(), 2, "{chief:?}");
    assert!(
        chief.iter().any(|note| note == CLOSED_PAUSED[1]),
        "{chief:?}"
    );
    assert_eq!(context.task(project, 1).task.state, "paused");
}

#[test]
fn a_window_kept_open_whose_turn_ends_before_its_close_pays_the_stop_and_stays() {
    let context = Context::new();
    let project = working(&context);
    let zeus = context.id(project, "zeus");
    ignored_three_rounds(&context, project);
    context.host.refuse_kills.set(true);
    context.pass().unwrap();
    assert!(context.dispatcher.unstopped(zeus).is_some());
    context.host.refuse_kills.set(false);
    // Its turn ends by itself before the minute is out: paid at rest, never closed.
    context.adapter.answer("zeus", "Where I was");
    context.pass().unwrap();
    context.advance(10 * 60_000);
    context.pass().unwrap();
    assert_eq!(context.dispatcher.unstopped(zeus), None);
    assert_eq!(
        context.host.killed().len(),
        1,
        "only the close the host refused"
    );
    assert_eq!(context.dispatcher.activity(zeus).state, ActivityState::Idle);
    assert_eq!(context.task(project, 1).task.state, "paused");
}

#[test]
fn a_window_that_stops_at_its_second_round_is_never_closed_and_nobody_is_told() {
    let context = Context::new();
    let project = working(&context);
    let launches = context.adapter.prepared().len();
    context.adapter.busy("zeus");
    context.pause_task(project, 1);
    context.pass().unwrap();
    context.advance(3_100);
    context.pass().unwrap();
    assert_eq!(context.host.inputs().len(), 2);
    // The second round stopped it, slowly: its turn ends a little later.
    context.advance(2_000);
    context.adapter.answer("zeus", "Stopped");
    for _ in 0..5 {
        context.advance(3_100);
        context.pass().unwrap();
    }
    context.advance(10 * 60_000);
    context.pass().unwrap();
    assert_eq!(context.host.inputs().len(), 2);
    assert!(
        context.host.killed().is_empty(),
        "the window stopped: it stays"
    );
    assert_eq!(told(&context, project), [Vec::<String>::new(), Vec::new()]);
    assert_eq!(context.task(project, 1).task.state, "paused");
    // Resumed, the words go into the same window.
    context.resume_task(project, 1, "Carry on");
    context.pass().unwrap();
    assert_eq!(context.adapter.prepared().len(), launches, "no new window");
    let words = context.adapter.agent("zeus").items.pop().unwrap();
    assert_match(&words.text, r"Resumed: Carry on$");
}

#[test]
fn a_window_the_human_keeps_open_is_closed_all_the_same_and_is_theirs_again_once_it_reopens() {
    let context = Context::new();
    let tiers = crate::background::working(&context);
    let project = tiers.project.id;
    let session = tiers.assignee(1);
    context.open_window(project, &session).unwrap();
    let pane = context.host.last("zeus").unwrap().pane;
    context.adapter.busy("zeus");
    context.pause_task(project, 1);
    for _ in 0..4 {
        context.pass().unwrap();
        context.advance(3_100);
    }
    assert_eq!(
        context.host.killed(),
        [pane],
        "the pause is the human's or the chief's own word: the window that ignores it goes"
    );
    assert_eq!(context.task(project, 1).task.state, "paused");

    // Resumed, it reopens; its task done, it stays open, as the human had it.
    context.resume_task(project, 1, "Carry on");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.task(project, 1).task.state, "working");
    context.adapter.answer("zeus", "Parser, finished");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.task(project, 1).task.state, "done");
    assert_eq!(
        context.host.killed().len(),
        1,
        "the human holds the window that came back"
    );
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

    // The words that resume T-2 do not go into T-1's window, which stays for T-1's
    // resumption (`ownership`); once T-1 is given up, the window closes, and they are
    // the first a window of T-2's own is given: no turn of it was ever for T-2's
    // pause to stop.
    context.resume_task(project, 2, "Go on");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.task(project, 2).task.state, "queued");
    context
        .ledger
        .borrow_mut()
        .cancel_task(project, 1, "chief")
        .unwrap();
    for _ in 0..3 {
        context.pass().unwrap();
    }
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
    // The words of T-2 wait for a window of their own: T-1's stays for its resumption
    // (`ownership`). T-1 is given up, its window closes, and T-2 is worked on in the
    // window that opens.
    context.pass().unwrap();
    assert_eq!(context.task(project, 2).task.state, "queued");
    assert_eq!(context.task(project, 1).task.state, "paused");
    context
        .ledger
        .borrow_mut()
        .cancel_task(project, 1, "chief")
        .unwrap();
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(context.task(project, 2).task.state, "working");
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

#[test]
fn the_words_that_first_reach_a_window_the_human_opened_with_no_task_answer_a_pause_asked_before_them(
) {
    // Calliope on 0eec91d2: a session's window the human opens (Show) with no message and
    // no conversation (its first launch never came up) is no task's window, and another
    // task's first words go into it. A pause of that task asked before its words arrived
    // was none the window could stop: its words answer it, and the turn they begin is not
    // interrupted for it.
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    context.host.refuse.set(true);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(
        tiers.task(1).task.state,
        "failed",
        "its window never came up"
    );
    let session = tiers.assignee(1);
    context.host.refuse.set(false);
    context.open_window(tiers.project.id, &session).unwrap();
    context.pass().unwrap();
    let opened = context.adapter.prepared();
    let opened = opened.last().unwrap();
    assert_eq!(
        (&opened["message"], &opened["resume"]),
        (&Value::Null, &Value::Null),
        "opened with nothing, on no conversation"
    );

    // T-2 for the session, paused before its words reach the window.
    context.create_task(
        tiers.project.id,
        NewTask {
            from: "chief".to_owned(),
            to: Some(session.clone()),
            body: "Write the docs".to_owned(),
            ..NewTask::default()
        },
    );
    context.pause_task(tiers.project.id, 2);
    context.pass().unwrap();
    assert_eq!(tiers.task(2).task.state, "paused");
    assert_eq!(context.host.inputs(), Vec::<Value>::new());

    // Resumed, its words are the first the window is given.
    context.resume_task(tiers.project.id, 2, "Go on");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(2).task.state, "working");
    context.pass().unwrap();
    assert_eq!(
        context.host.inputs(),
        Vec::<Value>::new(),
        "the turn the words began is not interrupted for a pause that came before them"
    );
}

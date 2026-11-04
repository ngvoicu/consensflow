//! A pause asks a stop of its task's window, and the window owes it until it
//! has paid it, at rest, in the engine's own record of that window: one Escape
//! for a turn that is still at work, none for one that is not, and no
//! forgiveness for a pause that came in a moment nothing could see. The
//! rules are those of `stops.rs`. None of these tests is held to a Node
//! recording: the rule is Node's no more.

use cf_engine::testing::Context;
use cf_engine::{ActivityState, Unstopped};
use cf_harness::contract::Pane;
use cf_harness::records::Role;
use serde_json::{json, Value};

use cf_ledger::NewTask;

use crate::fixtures::{after, assert_match, exhausted, hold_notes, now, Tiers};

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

/// What the human and the chief who gave T-1 are told of zeus's window kept after its
/// three rounds, running nothing ConsensFlow can see, its task paused.
const KEPT_PAUSED: [&str; 2] = [
    "@zeus did not stop for T-1: its turn went on through the interrupt 3 times, and ConsensFlow can see nothing it runs: the host lists no work of its turn, and its harness no job or agent, so its harness may be hung (in a model request, or a tool it does not name). ConsensFlow keeps the window open and T-1 paused, closes nothing, and interrupts the turn again every minute. To end it now, delete @zeus's session.",
    "T-1's window (@zeus) did not stop: its turn went on through the interrupt, and ConsensFlow can see nothing it runs (the host lists no work of its turn, its harness no job or agent): its harness may be hung. ConsensFlow keeps the window open and T-1 paused, closes nothing, and interrupts the turn again every minute; until then what it writes is not taken as T-1's result. `cf task resume T-1 \"…\"` gives the window your words once its turn ends; only the human can end it sooner, by deleting its session.",
];

/// What they are told of it once the task was resumed while the window ignored the rounds.
const KEPT_RESUMED: [&str; 2] = [
    "@zeus did not stop for T-1: its turn went on through the interrupt 3 times, and ConsensFlow can see nothing it runs: the host lists no work of its turn, and its harness no job or agent, so its harness may be hung (in a model request, or a tool it does not name). ConsensFlow keeps the window open, closes nothing, and interrupts the turn again every minute. To end it now, pause T-1 and delete @zeus's session.",
    "T-1's window (@zeus) did not stop: its turn went on through the interrupt, and ConsensFlow can see nothing it runs (the host lists no work of its turn, its harness no job or agent): its harness may be hung. ConsensFlow keeps the window open, closes nothing, and interrupts the turn again every minute; until then what it writes is not taken as T-1's result. Your words go in once its turn ends; only the human can end it sooner, by deleting its session.",
];

/// The notes the human and the chief were given that a window did not stop.
fn told(context: &Context, project: i64) -> [Vec<String>; 2] {
    [
        ignored(context, project, "human"),
        ignored(context, project, "chief"),
    ]
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
fn three_rounds_ignored_keep_the_window_said_once_its_task_paused_and_asked_again_every_minute() {
    // A pause never closes a window (the owner's rule, T-1199): one that ignores every
    // round, running nothing ConsensFlow can see, is kept, its task paused, and the chief
    // who gave the task and the human are told once that only the human can end it.
    let context = Context::new();
    let project = working(&context);
    let zeus = context.id(project, "zeus");
    let launches = context.adapter.prepared().len();
    let pane = ignored_three_rounds(&context, project);

    // The look after the third: no key, and nothing closed.
    context.pass().unwrap();
    assert_eq!(context.host.inputs().len(), 3);
    assert!(context.host.killed().is_empty(), "a pause closes nothing");
    assert_eq!(
        context.dispatcher.unstopped(zeus),
        Some(Unstopped { task: 1, rounds: 3 }),
        "the board shows the window that did not stop"
    );
    assert_eq!(context.task(project, 1).task.state, "paused");
    assert_eq!(
        told(&context, project),
        KEPT_PAUSED.map(|said| vec![said.to_owned()])
    );

    // A minute on, three fresh rounds, a few seconds apart, and nothing said again; ten
    // minutes on, the same, and no window opens for it.
    for _ in 0..19 {
        context.advance(3_100);
        context.pass().unwrap();
    }
    assert_eq!(context.host.inputs().len(), 3, "less than a minute: no key");
    context.advance(2_000);
    context.pass().unwrap();
    assert_eq!(context.host.inputs(), vec![escape(&pane); 4], "asked again");
    for _ in 0..3 {
        context.advance(3_100);
        context.pass().unwrap();
    }
    assert_eq!(
        context.host.inputs().len(),
        6,
        "three fresh rounds, no more"
    );
    context.advance(10 * 60_000);
    context.pass().unwrap();
    assert!(context.host.killed().is_empty());
    assert_eq!(context.adapter.prepared().len(), launches);
    assert_eq!(told(&context, project).map(|notes| notes.len()), [1, 1]);
    assert_eq!(context.task(project, 1).task.state, "paused");

    // Its turn ends: the stop is paid, and the resume's words go into the same window.
    context.adapter.answer("zeus", "Where I was");
    context.pass().unwrap();
    assert_eq!(context.dispatcher.unstopped(zeus), None);
    context.resume_task(project, 1, "Carry on");
    context.pass().unwrap();
    assert_eq!(
        context.adapter.prepared().len(),
        launches,
        "the same window"
    );
    let words = context.adapter.agent("zeus").items.pop().unwrap();
    assert_match(&words.text, r"Resumed: Carry on$");
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
fn a_turn_that_writes_and_is_compacted_through_its_rounds_is_no_turn_passed_and_is_kept() {
    // A stop is paid by a message of the user's after the one its turn began with
    // (T-807): the turn is over. A turn that goes on writing its own words, or whose
    // harness writes a compaction's summary in the user's place, is that same turn, at
    // work through every round: it is kept as any that ignores them, never closed.
    let context = Context::new();
    let project = working(&context);
    let zeus = context.id(project, "zeus");
    context.adapter.busy("zeus");
    context.pause_task(project, 1);
    let pane = pane_of(&context, "zeus");
    for round in 0..3 {
        context.pass().unwrap();
        context.adapter.writes(
            "zeus",
            &format!("Still at it, {round}"),
            &after(&context, 0),
        );
        let mut summary = context
            .adapter
            .item(Role::User, "This session is being continued");
        summary.compaction = true;
        context
            .adapter
            .with("zeus", |agent| agent.items.push(summary));
        context.advance(3_100);
    }
    context.pass().unwrap();
    assert_eq!(context.host.inputs(), vec![escape(&pane); 3]);
    assert!(context.host.killed().is_empty(), "a hung turn is kept");
    assert!(context.dispatcher.unstopped(zeus).is_some());
    assert_eq!(context.task(project, 1).task.state, "paused");
}

#[test]
fn a_task_resumed_while_its_window_ignores_the_rounds_has_its_words_go_into_that_window_once_its_turn_ends(
) {
    let context = Context::new();
    let project = working(&context);
    let launches = context.adapter.prepared().len();
    context.adapter.busy("zeus");
    context.pause_task(project, 1);
    context.pass().unwrap();
    // The chief resumes it at once, as zeewin's did 16 s after its pause: the words
    // wait for a window at rest, which this one is not.
    context.resume_task(project, 1, "Carry on");
    for _ in 0..3 {
        context.advance(3_100);
        context.pass().unwrap();
    }
    assert_eq!(context.host.inputs().len(), 3);
    assert!(
        context.host.killed().is_empty(),
        "kept after its third round"
    );
    assert_eq!(context.task(project, 1).task.state, "queued");
    assert_eq!(
        told(&context, project),
        KEPT_RESUMED.map(|said| vec![said.to_owned()])
    );

    // Its turn ends: the words go into the same window, and the task goes on there.
    context.adapter.answer("zeus", "Where I was");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.adapter.prepared().len(), launches, "no new window");
    let words = context.adapter.agent("zeus").items.pop().unwrap();
    assert_match(&words.text, r"Resumed: Carry on$");
    context.pass().unwrap();
    assert_eq!(context.task(project, 1).task.state, "working");
    context.adapter.answer("zeus", "Parser, finished");
    context.pass().unwrap();
    assert_eq!(context.task(project, 1).task.state, "done");
}

#[test]
fn a_window_held_for_quota_at_work_while_out_is_pressed_nothing_and_closed_never_and_its_words_go_in_once_it_rests(
) {
    // T-1199's bug 10: a hold asked a stop, a window that read at work while its member
    // was out (a harness that waits to try again) was pressed three rounds, ignored them,
    // and was closed, its task left paused. A hold asks no stop.
    let context = Context::new();
    let project = working(&context);
    let zeus = context.id(project, "zeus");
    let launches = context.adapter.prepared().len();
    context.adapter.busy("zeus");
    let resets_at = after(&context, 2 * 3_600_000);
    context
        .adapter
        .quota("zeus", Some(exhausted(None, Some(&resets_at))));
    context.pass().unwrap();
    assert_eq!(context.task(project, 1).task.state, "paused", "held");
    for _ in 0..10 {
        context.pass().unwrap();
        context.advance(3_100);
    }
    assert_eq!(
        context.host.inputs(),
        Vec::<Value>::new(),
        "no key for a hold"
    );
    assert!(context.host.killed().is_empty(), "and no close");
    assert_eq!(context.dispatcher.unstopped(zeus), None);
    assert_eq!(told(&context, project), [Vec::<String>::new(), Vec::new()]);

    // The reset: the daemon resumes the task, and its words wait for the turn at work.
    context.advance(2 * 3_600_000);
    context.adapter.quota("zeus", None);
    context.pass().unwrap();
    assert_eq!(context.task(project, 1).task.state, "queued");
    // The turn ends: the words go into the same window, once, and the task goes on.
    context.adapter.answer("zeus", "Waited for the limit");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(
        context.adapter.prepared().len(),
        launches,
        "the same window"
    );
    let given = context
        .adapter
        .agent("zeus")
        .items
        .iter()
        .filter(|item| item.text.contains("Resumed: Go on where you stopped"))
        .count();
    assert_eq!(given, 1, "the words went in once");
    context.pass().unwrap();
    assert_eq!(context.task(project, 1).task.state, "working");
    context.adapter.answer("zeus", "Parser, finished");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.task(project, 1).task.state, "done");
    assert!(context.host.inputs().is_empty());
}

#[test]
fn a_task_cancelled_while_its_window_is_kept_for_its_pause_is_cancelled_as_ever_and_its_window_holding_nothing_closes(
) {
    // A cancel keeps what it did before T-1199: the task is called off, its turn about the
    // task is pressed a round (`interrupt_cancelled`), and its window, holding nothing now,
    // closes at that look as any window with no task in hand does (`close_if_free`),
    // whatever its turn; no note says a pause closed it.
    let context = Context::new();
    let project = working(&context);
    let zeus = context.id(project, "zeus");
    let pane = ignored_three_rounds(&context, project);
    context.pass().unwrap();
    assert!(context.dispatcher.unstopped(zeus).is_some(), "kept");
    assert!(context.host.killed().is_empty());
    context
        .ledger
        .borrow_mut()
        .cancel_task(project, 1, "human")
        .unwrap();
    context.host.exits_showing(Some(0), None, None);
    context.pass().unwrap();
    assert_eq!(context.task(project, 1).task.state, "cancelled");
    assert_eq!(context.host.killed(), [pane], "holding nothing, it closes");
    assert_eq!(
        context.host.inputs().len(),
        4,
        "the pause's three rounds, and the cancel's first, at the look that closes it"
    );
    assert_eq!(told(&context, project).map(|notes| notes.len()), [1, 1]);
}

#[test]
fn a_kept_window_whose_turn_ends_pays_the_stop_and_stays() {
    let context = Context::new();
    let project = working(&context);
    let zeus = context.id(project, "zeus");
    ignored_three_rounds(&context, project);
    context.pass().unwrap();
    assert!(context.dispatcher.unstopped(zeus).is_some());
    // Its turn ends by itself before the minute is out: paid at rest, never closed.
    context.adapter.answer("zeus", "Where I was");
    context.pass().unwrap();
    context.advance(10 * 60_000);
    context.pass().unwrap();
    assert_eq!(context.dispatcher.unstopped(zeus), None);
    assert!(context.host.killed().is_empty());
    assert_eq!(context.host.inputs().len(), 3, "no key once it is at rest");
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
fn a_window_the_human_keeps_open_that_ignores_a_pause_is_kept_and_stays_theirs() {
    let context = Context::new();
    let tiers = crate::background::working(&context);
    let project = tiers.project.id;
    let session = tiers.assignee(1);
    context.open_window(project, &session).unwrap();
    context.adapter.busy("zeus");
    context.pause_task(project, 1);
    for _ in 0..4 {
        context.pass().unwrap();
        context.advance(3_100);
    }
    assert!(context.host.killed().is_empty(), "a pause closes nothing");
    assert_eq!(context.task(project, 1).task.state, "paused");

    // Resumed, the words wait for the turn; it ends, they go in; its task done, the
    // window stays open, as the human had it.
    context.resume_task(project, 1, "Carry on");
    context.adapter.answer("zeus", "Where I was");
    context.pass().unwrap();
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.task(project, 1).task.state, "working");
    context.adapter.answer("zeus", "Parser, finished");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.task(project, 1).task.state, "done");
    assert!(
        context.host.killed().is_empty(),
        "the human holds the window"
    );
}

#[test]
fn a_harness_that_takes_its_turn_up_again_at_the_reset_is_pressed_nothing_and_the_resume_goes_in_after_it(
) {
    let context = Context::new();
    let project = working(&context);
    let zeus = context.id(project, "zeus");
    // The harness is out of quota and waits to try again, so its window never rests
    // while the task is held: a hold asks no stop, and nothing is pressed into it.
    context.adapter.busy("zeus");
    let resets_at = after(&context, 2 * 3_600_000);
    context
        .adapter
        .quota("zeus", Some(exhausted(None, Some(&resets_at))));
    context.pass().unwrap();
    assert_eq!(context.task(project, 1).task.state, "paused", "held");
    context.pass().unwrap();
    assert_eq!(context.host.inputs().len(), 0, "a hold presses no key");
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
    assert_eq!(context.host.inputs().len(), 0, "nor is the resume a stop");
    let words = context.adapter.agent("zeus").items.pop().unwrap();
    assert!(
        !words.text.contains("Resumed: Go on where you stopped"),
        "nothing was pasted into the working turn"
    );
    // Its turn ends; the words go in, once.
    context.adapter.answer("zeus", "Back at the lexer");
    context.pass().unwrap();
    let words = context.adapter.agent("zeus").items.pop().unwrap();
    assert_match(&words.text, r"Resumed: Go on where you stopped\.$");
    context.pass().unwrap();
    assert_eq!(context.task(project, 1).task.state, "working");
    assert_eq!(context.host.inputs().len(), 0);
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
fn a_window_out_of_quota_and_still_at_work_is_pressed_nothing_while_its_activity_says_it_is_out() {
    let context = Context::new();
    let project = working(&context);
    let zeus = context.id(project, "zeus");
    context.adapter.busy("zeus");
    let resets_at = after(&context, 3 * 3_600_000);
    context
        .adapter
        .quota("zeus", Some(exhausted(None, Some(&resets_at))));
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.dispatcher.activity(zeus).state, ActivityState::Out);
    assert_eq!(
        context.host.inputs(),
        Vec::<Value>::new(),
        "a quota wait never interrupts"
    );
    // Its turn ends on the refusal: nothing owed, nothing pressed.
    context.adapter.answer("zeus", "Out of quota");
    context.pass().unwrap();
    context.pass().unwrap();
    assert!(context.host.inputs().is_empty());
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
    // The host proves each window's program gone, as the app's does: the session's next task
    // goes on with its conversation (`ownership`), as this test is about something else.
    context.host.exits_showing(Some(0), None, None);
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

/// The same, where the host proves nothing of the exit of the window that closed: the words that
/// resumed T-2 are not the first a window of its own is given; T-2 is held for the chief's word
/// first, and the words that send it on are, with no turn of it to interrupt for the earlier pause.
#[test]
fn the_words_that_first_reach_a_window_for_a_task_paused_before_they_came_are_held_for_the_chiefs_word_when_the_window_before_closed_with_no_proof(
) {
    let context = Context::new();
    let project = working(&context);
    context.give(project, "zeus", "Docs");
    context.pause_task(project, 2);
    context.pass().unwrap();
    assert_eq!(context.host.inputs(), Vec::<Value>::new());
    context.pause_task(project, 1);
    context.adapter.answer("zeus", "Stopped");
    context.pass().unwrap();
    assert_eq!(context.host.inputs(), Vec::<Value>::new());

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
    assert_eq!(
        context.task(project, 2).task.state,
        "paused",
        "held for the chief's word: T-1's window closed with no proof"
    );
    assert_eq!(hold_notes(&context, project, 2).len(), 1);

    context.resume_task(project, 2, "Go on in a window of your own.");
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
    // The host proves each window's program gone, as the app's does: the session's next task
    // goes on with its conversation (`ownership`), as this test is about something else.
    context.host.exits_showing(Some(0), None, None);
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

/// The same, where the host proves nothing of the exit of the window that closed: T-2 is held for
/// the chief's word, no key is pressed for anything, and once sent on its stop is owed all the same.
#[test]
fn a_stop_paid_for_one_task_never_hides_the_stop_of_the_task_the_chief_sends_on_after_a_hold() {
    let context = Context::new();
    let project = working(&context);
    context.give(project, "zeus", "Docs");
    context.pause_task(project, 1);
    context.adapter.answer("zeus", "Stopped");
    context.pass().unwrap();
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
    assert_eq!(
        context.task(project, 2).task.state,
        "paused",
        "held for the chief's word: T-1's window closed with no proof"
    );
    assert_eq!(hold_notes(&context, project, 2).len(), 1);
    assert_eq!(context.host.inputs(), Vec::<Value>::new());

    context.resume_task(project, 2, "Go on in a window of your own.");
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

/// The window the human opened for T-1's session, which stays open whatever its task:
/// the project and the session.
fn held_open(context: &Context) -> (i64, String) {
    let tiers = crate::background::working(context);
    let project = tiers.project.id;
    let session = tiers.assignee(1);
    context.open_window(project, &session).unwrap();
    (project, session)
}

/// The window begins a turn of its own (Claude Code's meta prompt once its limit resets,
/// or what the human typed): at work on it.
fn its_own_turn(context: &Context, words: &str) {
    let at = now(context);
    context.adapter.begins_its_own_turn("zeus", words, &at);
}

/// The messages of the user's in zeus's record that say `words`.
fn given_zeus(context: &Context, words: &str) -> usize {
    context
        .adapter
        .agent("zeus")
        .items
        .iter()
        .filter(|item| item.role == Role::User && item.text.contains(words))
        .count()
}

#[test]
fn a_reopen_after_a_hold_its_window_waited_out_at_work_presses_nothing_and_its_words_go_in_after_the_turn(
) {
    // T-1199's bug 9 on its task's way: T-21 was held for quota while its window waited at
    // work to continue (m-444), went on by itself (m-446) and was done (m-447); reopened
    // while its window was on a turn after its result, it was pressed three rounds and
    // closed (m-450). The hold had asked a stop no look paid, which waited through the
    // done task (`stop_of` counts none of a done task) for the reopen.
    let context = Context::new();
    let (project, _) = held_open(&context);
    let launches = context.adapter.prepared().len();
    context.adapter.busy("zeus");
    let resets_at = after(&context, 60_000);
    context
        .adapter
        .quota("zeus", Some(exhausted(None, Some(&resets_at))));
    for _ in 0..4 {
        context.pass().unwrap();
        context.advance(3_100);
    }
    assert_eq!(context.task(project, 1).task.state, "paused", "held");
    assert_eq!(
        context.host.inputs(),
        Vec::<Value>::new(),
        "a hold presses nothing"
    );

    // Quota is back: the window goes on by itself, and its answer is the result.
    context.advance(60_000);
    context.adapter.quota("zeus", None);
    its_own_turn(
        &context,
        "Your claude.ai usage limit has reset. Continue the task you were working on.",
    );
    context.pass().unwrap();
    let at = now(&context);
    context
        .adapter
        .ends_its_own_turn("zeus", "Parser, finished", &at);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.task(project, 1).task.state, "done");

    // A turn of the window's own after its result, and the chief reopens T-1: nothing is
    // pressed into the turn, nothing closed, and the words wait for its end.
    its_own_turn(&context, "One more look at the parser");
    context.pass().unwrap();
    context
        .ledger
        .borrow_mut()
        .reopen_task(project, 1, "chief", "Fix the README")
        .unwrap();
    for _ in 0..10 {
        context.pass().unwrap();
        context.advance(3_100);
    }
    assert_eq!(
        context.host.inputs(),
        Vec::<Value>::new(),
        "a reopen presses nothing"
    );
    assert!(context.host.killed().is_empty(), "and closes nothing");
    assert_eq!(
        given_zeus(&context, "Fix the README"),
        0,
        "nothing into the turn"
    );
    assert_eq!(told(&context, project), [Vec::<String>::new(), Vec::new()]);
    context.adapter.answer("zeus", "Looked");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(
        given_zeus(&context, "Fix the README"),
        1,
        "its words, after the turn"
    );
    assert_eq!(
        context.adapter.prepared().len(),
        launches,
        "the same window"
    );
    context.pass().unwrap();
    assert_eq!(context.task(project, 1).task.state, "working");
}

#[test]
fn a_resume_an_answer_a_follow_up_and_a_reopen_given_a_window_at_work_press_nothing_and_wait_for_its_turn(
) {
    // Only a pause, a tell, a cancel or the human's own stop asks a window to stop
    // (T-1199): any other words wait for the turn at work to end, as a message does.
    let context = Context::new();
    let (project, _) = held_open(&context);
    let launches = context.adapter.prepared().len();
    let quiet = |context: &Context, what: &str| {
        for _ in 0..10 {
            context.pass().unwrap();
            context.advance(3_100);
        }
        assert_eq!(
            context.host.inputs(),
            Vec::<Value>::new(),
            "{what} pressed a key"
        );
        assert!(context.host.killed().is_empty(), "{what} closed the window");
    };

    // A resume: T-1 paused at rest, and resumed while the window is on a turn of its own.
    context.adapter.answer("zeus", "Halfway");
    context.pause_task(project, 1);
    context.pass().unwrap();
    its_own_turn(&context, "What the human typed");
    context.pass().unwrap();
    context.resume_task(project, 1, "Carry on");
    quiet(&context, "a resume");
    assert_eq!(given_zeus(&context, "Resumed: Carry on"), 0);
    context.adapter.answer("zeus", "Typed back");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(given_zeus(&context, "Resumed: Carry on"), 1);

    // An answer to the window's question, given while it works.
    context.pass().unwrap();
    context.adapter.busy("zeus");
    let question = context.ask(
        project,
        &context.task(project, 1).task.assignee.unwrap(),
        "chief",
        1,
        "Which grammar?",
    );
    context.answer(project, question.id, "The v2 one");
    quiet(&context, "an answer");
    context.adapter.answer("zeus", "Parser, finished");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(
        given_zeus(&context, "The v2 one"),
        1,
        "the answer, after the turn"
    );
    context.pass().unwrap();
    context.adapter.answer("zeus", "Parser, finished with v2");
    context.pass().unwrap();
    context.pass().unwrap();

    // A reopen of T-1, done, given while the window is on a turn of its own.
    assert_eq!(context.task(project, 1).task.state, "done");
    its_own_turn(&context, "Check the parser once more");
    context.pass().unwrap();
    context
        .ledger
        .borrow_mut()
        .reopen_task(project, 1, "chief", "Handle tabs too")
        .unwrap();
    quiet(&context, "a reopen");
    assert_eq!(given_zeus(&context, "Handle tabs too"), 0);
    context.adapter.answer("zeus", "Checked");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(given_zeus(&context, "Handle tabs too"), 1, "after the turn");
    context.pass().unwrap();
    context.adapter.answer("zeus", "Tabs handled");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.task(project, 1).task.state, "done");

    // A follow-up of T-1 for the same window, given while it is on a turn of its own:
    // nothing is pressed, and nothing goes into the turn.
    context
        .ledger
        .borrow_mut()
        .accept_task(project, 1, "chief")
        .unwrap();
    its_own_turn(&context, "Tidy the notes");
    context.pass().unwrap();
    context.create_task(
        project,
        NewTask {
            from: "chief".to_owned(),
            after: Some(1),
            body: "Now the lexer".to_owned(),
            ..NewTask::default()
        },
    );
    quiet(&context, "a follow-up");
    assert_eq!(given_zeus(&context, "Now the lexer"), 0);
    assert_eq!(
        context.adapter.prepared().len(),
        launches,
        "one window throughout"
    );
}

#[test]
fn a_pause_of_a_window_at_work_whose_member_goes_out_meanwhile_is_pressed_while_its_activity_says_it_is_out(
) {
    // A pause asks a stop, a hold none: the chief's pause of a window at work goes on with
    // its rounds when the member is out meanwhile, whatever its activity says; what its
    // look finds it doing is what counts.
    let context = Context::new();
    let project = working(&context);
    let zeus = context.id(project, "zeus");
    context.adapter.busy("zeus");
    context.pause_task(project, 1);
    context.pass().unwrap();
    assert_eq!(context.host.inputs().len(), 1, "the pause's first round");
    let resets_at = after(&context, 3 * 3_600_000);
    context
        .adapter
        .quota("zeus", Some(exhausted(None, Some(&resets_at))));
    // The look that reads it out presses nothing; the looks after it, out, go on with the
    // rounds.
    for _ in 0..3 {
        context.advance(3_100);
        context.pass().unwrap();
    }
    assert_eq!(context.dispatcher.activity(zeus).state, ActivityState::Out);
    assert_eq!(
        context.host.inputs().len(),
        3,
        "its rounds go on while its activity says it is out"
    );
}

#[test]
fn a_pause_whose_turn_a_refusal_for_quota_ended_before_any_look_presses_no_key() {
    // The chief's pause asks a stop; the refusal ended the turn before the window was
    // looked at, and its record reads the turn in flight past it (T-502's reader): the turn
    // is over, and the stop is paid with no key.
    let context = Context::new();
    let project = working(&context);
    let zeus = context.id(project, "zeus");
    context.adapter.busy("zeus");
    context.pause_task(project, 1);
    let resets_at = after(&context, 2 * 3_600_000);
    context
        .adapter
        .refuse_unsettled("zeus", exhausted(Some(&now(&context)), Some(&resets_at)));
    for _ in 0..6 {
        context.pass().unwrap();
        context.advance(3_100);
    }
    assert_eq!(context.host.inputs(), Vec::<Value>::new(), "no key");
    assert!(context.host.killed().is_empty());
    assert_eq!(context.dispatcher.unstopped(zeus), None);
    assert_eq!(context.task(project, 1).task.state, "paused");
}

#[test]
fn a_stop_owed_when_the_chief_finishes_its_task_is_settled_and_a_reopen_presses_nothing_into_the_turn_after(
) {
    // The chief pauses T-1 while its window works and goes on through the rounds, and
    // finishes it at what the window said last: done with the pause's stop unpaid, and the
    // window, the human's, still on that turn (bug 9's window, still on its turn after its
    // result). The chief reopens T-1: what T-1 was asked before it was done is settled, and
    // nothing is pressed into the turn; the words wait for its end (T-1199).
    let context = Context::new();
    let (project, _) = held_open(&context);
    context.adapter.busy("zeus");
    context.pause_task(project, 1);
    context.pass().unwrap();
    let pressed = context.host.inputs().len();
    assert_eq!(pressed, 1, "the pause's first round");
    // A complete answer, and the window still at work after it.
    let item = context
        .adapter
        .item(Role::Assistant, "Half the parser, and the tests");
    context.adapter.with("zeus", |agent| {
        agent.items.push(item);
        agent.settled = false;
    });
    let finished = context.finish_task(project, 1, "chief").unwrap();
    assert_eq!(finished.task.state, "done");
    context.adapter.with("zeus", |agent| agent.settled = false);
    context.pass().unwrap();
    context
        .ledger
        .borrow_mut()
        .reopen_task(project, 1, "chief", "Fix the README")
        .unwrap();
    for _ in 0..10 {
        context.pass().unwrap();
        context.advance(3_100);
    }
    assert_eq!(
        context.host.inputs().len(),
        pressed,
        "nothing pressed after the finish"
    );
    assert!(context.host.killed().is_empty());
    context.adapter.answer("zeus", "Looked");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(
        given_zeus(&context, "Fix the README"),
        1,
        "its words, after the turn"
    );
}

#[test]
fn a_pause_and_a_resume_whose_task_ends_at_a_turn_end_before_the_words_went_in_owe_nothing_to_a_reopen(
) {
    // The chief's question on T-1199: paused, then resumed, and the task done at a turn end
    // before the resume's words went in. That turn end is a look at rest, which pays the
    // stop: a reopen owes none of it.
    let context = Context::new();
    let (project, _) = held_open(&context);
    context.adapter.busy("zeus");
    context.pause_task(project, 1);
    context.pass().unwrap();
    let pressed = context.host.inputs().len();
    context.resume_task(project, 1, "Carry on");
    context.adapter.answer("zeus", "Parser, finished");
    for _ in 0..3 {
        context.pass().unwrap();
    }
    let state = context.task(project, 1).task.state;
    if state != "done" {
        // Its words went in at that rest: answered, the task is done.
        context.adapter.answer("zeus", "Parser, finished again");
        context.pass().unwrap();
        context.pass().unwrap();
    }
    assert_eq!(context.task(project, 1).task.state, "done");
    its_own_turn(&context, "One more look at the parser");
    context.pass().unwrap();
    context
        .ledger
        .borrow_mut()
        .reopen_task(project, 1, "chief", "Fix the README")
        .unwrap();
    for _ in 0..10 {
        context.pass().unwrap();
        context.advance(3_100);
    }
    assert_eq!(context.host.inputs().len(), pressed, "nothing pressed");
    assert!(context.host.killed().is_empty());
}

#[test]
fn a_tell_to_a_task_held_for_quota_asks_its_stop_and_the_reset_resumes_nothing() {
    // T-1220's P1-1: a tell to a held task records a stop as any tell does, and takes the
    // task out of its hold: no resume at the quota's reset.
    let context = Context::new();
    let project = working(&context);
    context.adapter.busy("zeus");
    let resets_at = after(&context, 60_000);
    context
        .adapter
        .quota("zeus", Some(exhausted(None, Some(&resets_at))));
    context.pass().unwrap();
    assert_eq!(context.task(project, 1).task.state, "paused", "held");
    assert!(context.host.inputs().is_empty(), "a hold presses nothing");
    let session = context.task(project, 1).task.assignee.unwrap();
    context.tell(project, &session, 1, "Use the v2 grammar");
    for _ in 0..2 {
        context.pass().unwrap();
        context.advance(3_100);
    }
    assert!(
        !context.host.inputs().is_empty(),
        "the tell's stop is pressed into the turn at work"
    );
    // The reset: the task is the chief's now, and stays paused.
    context.advance(120_000);
    context.adapter.quota("zeus", None);
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(
        context.task(project, 1).task.state,
        "paused",
        "no resume of its own"
    );
}

#[test]
fn a_finish_and_a_reopen_between_two_looks_settle_the_stop_and_a_pause_after_is_owed() {
    // T-1220's P2-4: the chief finishes T-1 and reopens it at once, with no look between;
    // the old stop is settled by the finish, and a pause asked after the reopen is owed.
    let context = Context::new();
    let (project, _) = held_open(&context);
    context.adapter.busy("zeus");
    context.pause_task(project, 1);
    context.pass().unwrap();
    let pressed = context.host.inputs().len();
    assert_eq!(pressed, 1, "the pause's first round");
    let item = context
        .adapter
        .item(Role::Assistant, "Half the parser, and the tests");
    context.adapter.with("zeus", |agent| {
        agent.items.push(item);
        agent.settled = false;
    });
    context.finish_task(project, 1, "chief").unwrap();
    context
        .ledger
        .borrow_mut()
        .reopen_task(project, 1, "chief", "Fix the README")
        .unwrap();
    for _ in 0..10 {
        context.pass().unwrap();
        context.advance(3_100);
    }
    assert_eq!(
        context.host.inputs().len(),
        pressed,
        "nothing pressed after the finish"
    );
    // The control: a pause asked now is owed, and pressed.
    context.pause_task(project, 1);
    context.pass().unwrap();
    assert_eq!(
        context.host.inputs().len(),
        pressed + 1,
        "a new pause is pressed"
    );
}

#[test]
fn a_projects_close_closes_a_kept_window_as_the_humans_own_shutdown() {
    // T-1199 round 3, the chief's 8: a pause never closes a window, but the human's Close of
    // the project (and the app's quit) is no pause: it closes every window, a kept one too,
    // and the task stays paused for the project's Resume.
    let context = Context::new();
    let project = working(&context);
    let zeus = context.id(project, "zeus");
    let pane = ignored_three_rounds(&context, project);
    context.pass().unwrap();
    assert!(context.dispatcher.unstopped(zeus).is_some(), "kept");
    assert!(context.host.killed().is_empty());
    context.host.exits_showing(Some(0), None, None);
    context.close_project(project).unwrap();
    assert!(
        context.host.killed().contains(&pane),
        "the Close closes it, with every window of the project: {:?}",
        context.host.killed()
    );
    assert_eq!(context.task(project, 1).task.state, "paused");
}

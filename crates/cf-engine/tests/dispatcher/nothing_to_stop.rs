//! A window whose turn is over has nothing for an interrupt to stop, and is never
//! closed for keys nothing in it could heed (T-551). On 2026-10-25, 3.0.6-alpha.2:
//! an account's weekly limit refused seven Claude windows, ConsensFlow held each
//! task, and pressed Escape into T-502's window, whose look did not read it at
//! rest; three rounds later it closed the window, and the close ended the plant run
//! and the test run it had left in the background (m-3943 to m-3945, two notes of
//! the one close among them). At ~21:05 a `cf tell` of T-604 stopped a turn whose
//! three background agents still ran: Claude says busy while they run, the
//! interrupted turn's record counts none of them, and the window was closed as one
//! that ignored the interrupt (m-4188).
//!
//! The cases are those shapes, and the three ways a task whose turn ended is paused:
//! a refusal for quota, the chief's pause and the chief's tell. In each the window
//! stays open with what it left running, nobody is told it did not stop, and the
//! words that send the task on go into the same window once it rests.

use cf_engine::testing::Context;
use cf_harness::contract::Pane;
use cf_harness::records::{Pending, Role};
use serde_json::{json, Value};

use crate::fixtures::{after, exhausted, now, Tiers};

/// What pressing Escape into `pane` is, as the host is asked it.
fn escape(pane: &Pane) -> Value {
    json!({ "id": pane.id, "generation": pane.generation, "bytes": [27] })
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

/// Every note of the project that says a window did not stop, or that a close
/// ended what a window left running, whoever it went to.
fn closes_told(context: &Context, project: i64) -> Vec<String> {
    ["human", "chief"]
        .into_iter()
        .flat_map(|who| context.inbox(context.id(project, who)))
        .filter(|message| message.kind == "note")
        .map(|message| message.body)
        .filter(|body| body.contains("did not stop") || body.contains("its close ended"))
        .collect()
}

/// How many messages of the window `handle` say `words`.
fn given(context: &Context, handle: &str, words: &str) -> usize {
    context
        .adapter
        .agent(handle)
        .items
        .iter()
        .filter(|item| item.role == Role::User && item.text.contains(words))
        .count()
}

/// The look reads `handle`'s window at rest again: Claude's status went idle.
fn rests(context: &Context, handle: &str) {
    context.adapter.with(handle, |agent| agent.settled = true);
}

/// Ten passes, three seconds and more apart: every round a stop could have.
fn rounds(context: &Context) {
    for _ in 0..10 {
        context.pass().unwrap();
        context.advance(3_100);
    }
}

#[test]
fn an_accounts_weekly_limit_closes_none_of_its_windows_and_each_goes_on_in_its_own_once_quota_is_back(
) {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus", "diana", "juno"]);
    for body in ["Parser", "Lexer", "Printer"] {
        tiers.open_body(body);
    }
    context.pass().unwrap();
    context.pass().unwrap();
    let sessions: Vec<String> = (1..=3).map(|number| tiers.assignee(number)).collect();
    let launches = context.adapter.prepared().len();
    let resets_at = after(&context, 46 * 3_600_000);
    let limit = exhausted(Some(&now(&context)), Some(&resets_at));
    // The weekly limit ends two windows' turns, read at rest on the refusal.
    context.adapter.refuse(&sessions[1], limit.clone());
    context.adapter.refuse(&sessions[2], limit.clone());
    // T-502's: its turn ends on the refusal with a command it left running in the
    // background, and its look does not read it at rest.
    context.adapter.refuse_unsettled(&sessions[0], limit);
    context.adapter.with(&sessions[0], |agent| {
        agent.pending = Pending {
            jobs: 1,
            ..Pending::default()
        };
    });
    rounds(&context);
    for number in 1..=3 {
        assert_eq!(tiers.task(number).task.state, "paused", "T-{number} held");
    }
    assert_eq!(
        context.host.inputs(),
        Vec::<Value>::new(),
        "no key: nothing in a refused turn is left to stop"
    );
    assert!(
        context.host.killed().is_empty(),
        "every window waits with its work"
    );
    assert_eq!(
        closes_told(&context, tiers.project.id),
        Vec::<String>::new()
    );
    let waits = tiers
        .notes("chief")
        .into_iter()
        .filter(|note| note.contains(" waits with @"))
        .count();
    assert_eq!(waits, 3, "each requester is told its task waits, once");

    // Quota is back: each task goes on in its own window, which reads at rest again.
    for member in ["zeus", "diana", "juno"] {
        context
            .back_from_quota(tiers.project.id, member)
            .expect("back");
    }
    rests(&context, &sessions[0]);
    for _ in 0..3 {
        context.pass().unwrap();
    }
    for (number, session) in (1..=3).zip(&sessions) {
        assert_eq!(tiers.task(number).task.state, "working", "T-{number}");
        assert_eq!(
            given(&context, session, "Resumed: Go on where you stopped"),
            1,
            "{session}"
        );
    }
    assert_eq!(
        context.adapter.prepared().len(),
        launches,
        "no window opened again"
    );
    assert_eq!(context.host.inputs(), Vec::<Value>::new());
}

#[test]
fn a_tell_whose_escape_stopped_a_turn_with_background_agents_keeps_the_window_and_goes_in_once_it_rests(
) {
    let context = Context::new();
    let project = working(&context);
    let launches = context.adapter.prepared().len();
    // T-604's: its turn started agents in the background and works on; the interrupt
    // ends the turn in its record, and Claude stays busy while the agents run.
    context.adapter.busy_with_agents_until_interrupted("zeus");
    context.tell(project, "zeus", 1, "Drop the per-member cap");
    rounds(&context);
    let pane = context.host.last("zeus").expect("its window").pane;
    assert_eq!(
        context.host.inputs(),
        [escape(&pane)],
        "one Escape, which stopped the turn"
    );
    assert!(
        context.host.killed().is_empty(),
        "the turn stopped: no close"
    );
    assert_eq!(closes_told(&context, project), Vec::<String>::new());
    assert_eq!(context.task(project, 1).task.state, "paused");

    // The agents end: the window rests, and the words go into it.
    rests(&context, "zeus");
    context.pass().unwrap();
    assert_eq!(given(&context, "zeus", "Drop the per-member cap"), 1);
    assert_eq!(
        context.adapter.prepared().len(),
        launches,
        "the same window"
    );
    assert_eq!(context.host.inputs().len(), 1, "no key with them");
}

#[test]
fn a_pause_whose_escape_stopped_a_turn_with_background_agents_keeps_the_window_and_its_resume_goes_in(
) {
    let context = Context::new();
    let project = working(&context);
    let launches = context.adapter.prepared().len();
    context.adapter.busy_with_agents_until_interrupted("zeus");
    context.pause_task(project, 1);
    rounds(&context);
    assert_eq!(
        context.host.inputs().len(),
        1,
        "one Escape, which stopped the turn"
    );
    assert!(context.host.killed().is_empty());
    assert_eq!(closes_told(&context, project), Vec::<String>::new());

    context.resume_task(project, 1, "Go on");
    rests(&context, "zeus");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(given(&context, "zeus", "Resumed: Go on"), 1);
    assert_eq!(context.task(project, 1).task.state, "working");
    assert_eq!(
        context.adapter.prepared().len(),
        launches,
        "the same window"
    );
}

/// zeus's turn on T-1 ended and left a job running in the background: the task
/// goes on in the background, held for it.
fn held_for_its_job(context: &Context) -> i64 {
    let project = working(context);
    context
        .adapter
        .answer_leaving_a_job("zeus", "The plants run; I report when they end");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.task(project, 1).task.state, "working", "held");
    project
}

#[test]
fn a_pause_of_a_task_held_for_its_job_presses_no_key_and_its_resume_goes_into_the_same_window() {
    let context = Context::new();
    let project = held_for_its_job(&context);
    let launches = context.adapter.prepared().len();
    context.pause_task(project, 1);
    rounds(&context);
    assert_eq!(context.task(project, 1).task.state, "paused");
    assert_eq!(context.host.inputs(), Vec::<Value>::new());
    assert!(context.host.killed().is_empty(), "its job runs on");
    assert_eq!(closes_told(&context, project), Vec::<String>::new());

    context.resume_task(project, 1, "Go on");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(given(&context, "zeus", "Resumed: Go on"), 1);
    assert_eq!(context.task(project, 1).task.state, "working");
    assert_eq!(context.adapter.prepared().len(), launches);
    assert_eq!(context.host.inputs(), Vec::<Value>::new());
}

#[test]
fn a_tell_of_a_task_held_for_its_job_presses_no_key_and_its_words_go_into_the_same_window() {
    let context = Context::new();
    let project = held_for_its_job(&context);
    let launches = context.adapter.prepared().len();
    context.tell(project, "zeus", 1, "Run the daemon table too");
    rounds(&context);
    assert_eq!(given(&context, "zeus", "Run the daemon table too"), 1);
    assert_eq!(context.host.inputs(), Vec::<Value>::new());
    assert!(context.host.killed().is_empty(), "its job runs on");
    assert_eq!(closes_told(&context, project), Vec::<String>::new());
    assert_eq!(context.adapter.prepared().len(), launches);
}

#[test]
fn a_task_held_for_its_job_whose_next_turn_is_refused_presses_no_key_and_goes_on_there_at_the_reset(
) {
    let context = Context::new();
    let project = held_for_its_job(&context);
    let launches = context.adapter.prepared().len();
    // The job ends, the window takes a turn for it, and the limit refuses that turn;
    // a second job still runs, and the look does not read the window at rest.
    context.adapter.wake(
        "zeus",
        "<task-notification><status>completed</status></task-notification>",
    );
    context.pass().unwrap();
    let resets_at = after(&context, 2 * 3_600_000);
    context
        .adapter
        .refuse_unsettled("zeus", exhausted(Some(&now(&context)), Some(&resets_at)));
    context.adapter.with("zeus", |agent| {
        agent.pending = Pending {
            jobs: 1,
            ..Pending::default()
        };
    });
    rounds(&context);
    assert_eq!(context.task(project, 1).task.state, "paused", "held");
    assert_eq!(context.host.inputs(), Vec::<Value>::new());
    assert!(context.host.killed().is_empty(), "its job runs on");
    assert_eq!(closes_told(&context, project), Vec::<String>::new());

    // The reset: the words go into the same window once it reads at rest.
    context.advance(2 * 3_600_000);
    rests(&context, "zeus");
    context.adapter.quota("zeus", None);
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(
        given(&context, "zeus", "Resumed: Go on where you stopped"),
        1
    );
    assert_eq!(context.task(project, 1).task.state, "working");
    assert_eq!(context.adapter.prepared().len(), launches);
}

#[test]
fn a_window_closed_for_a_stop_it_ignored_is_told_once_to_the_human_who_gave_its_task() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.create_task(
        project.id,
        cf_ledger::NewTask {
            from: "human".to_owned(),
            to: Some("zeus".to_owned()),
            body: "Parser".to_owned(),
            ..cf_ledger::NewTask::default()
        },
    );
    context.pass().unwrap();
    context.pass().unwrap();
    context.adapter.busy("zeus");
    context.human_pause(project.id, 1);
    rounds(&context);
    context.host.exits_showing(Some(0), None, None);
    context.pass().unwrap();
    assert_eq!(context.host.killed().len(), 1, "a hung turn is closed");
    assert_eq!(
        closes_told(&context, project.id),
        ["@zeus did not stop for T-1: it ignored the interrupt 3 times, so ConsensFlow closed its window. T-1 stays paused, and its window reopens on its conversation when it is resumed."],
        "one note, in the human's words, and none to the chief"
    );
}

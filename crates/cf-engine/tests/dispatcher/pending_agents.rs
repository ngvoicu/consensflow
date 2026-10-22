//! A turn that ends while background agents or workflows it started still run
//! (2026-10-11: a chief's four research agents, and a result that waited twelve
//! minutes for the last one). The harness's own record says so of the turn that
//! ended (`pending` of the reading), and its window takes input between turns: a
//! message for it goes in then, whoever the window is for. A worker's turn like that
//! keeps its task working (A2, turn-end's smallest rule of 2026-10-21, after dip-agentos's
//! T-20 and poker-lab's T-442): its words are progress, the requester is told once that it
//! goes on in the background, and the result is the final answer of the turn that ends
//! with nothing pending; nothing is ended. A chief's window is not held. Node's
//! dispatcher had neither, so none of these is held to a Node trace.

use cf_engine::delivery_text::marker_of;
use cf_engine::testing::Context;
use cf_harness::records::Pending;

use crate::fixtures::{given, Tiers};

/// The notification Claude Code begins a turn with when an agent ends.
const NOTICE: &str =
    "<task-notification>\n<task-id>a1</task-id>\n<status>completed</status>\n</task-notification>";

/// What the chief is told when T-1 goes on in the background: the opening of the note.
fn goes_on(handle: &str, running: &str) -> String {
    format!("T-1 goes on in the background: @{handle} ended its turn with {running} running")
}

/// T-1 given to zeus, whose window has taken it in.
fn working(context: &Context) -> Tiers<'_> {
    let tiers = Tiers::new(context, &["zeus"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working");
    tiers
}

/// The results the chief was given or is to be given.
fn results(context: &Context, tiers: &Tiers<'_>) -> Vec<String> {
    context
        .inbox(tiers.id("chief"))
        .into_iter()
        .filter(|message| message.kind == "result")
        .map(|message| message.body)
        .collect()
}

#[test]
fn a_message_waiting_for_a_chief_goes_in_when_its_turn_ends_with_agents_pending() {
    let context = Context::new();
    let tiers = working(&context);

    // The chief is in a turn of its own when zeus finishes: the result waits.
    context.adapter.busy("chief");
    context.adapter.answer("zeus", "Parser done");
    context.pass().unwrap();
    context.pass().unwrap();
    let result = context
        .inbox(tiers.id("chief"))
        .into_iter()
        .find(|message| message.kind == "result")
        .expect("a result for the chief");
    assert_eq!(result.state, "queued", "a busy chief is not interrupted");

    // Its turn ends with four research agents still running, which its window
    // takes input beside: the result goes in, and does not wait for them.
    context
        .adapter
        .answer_leaving_agents("chief", "Four agents are researching it");
    context.pass().unwrap();
    context.pass().unwrap();

    assert!(
        given(&context, "chief")
            .iter()
            .any(|text| text.contains(&marker_of(result.id))),
        "{:?}",
        given(&context, "chief")
    );
    assert_ne!(context.message(result.id).state, "queued");
}

#[test]
fn a_chief_that_ends_turn_after_turn_with_agents_pending_is_given_what_waits_at_each_end() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    context
        .adapter
        .answer_leaving_agents("chief", "Three agents are researching it");
    context.pass().unwrap();

    // Each time it ends a turn leaving fewer, what came meanwhile goes in.
    for (agents, word) in [(3, "one"), (2, "two"), (1, "three")] {
        context.adapter.busy("chief");
        context.note_from_consensflow(tiers.project.id, "chief", &format!("Word {word}"));
        context
            .adapter
            .answer_leaving_agents("chief", &format!("{agents} agents run"));
        context.pass().unwrap();
        context.pass().unwrap();
        assert!(
            given(&context, "chief")
                .iter()
                .any(|text| text.contains(&format!("Word {word}"))),
            "{word}: {:?}",
            given(&context, "chief")
        );
    }
}

#[test]
fn a_chiefs_turn_that_ends_with_agents_pending_is_asked_nothing_of_the_host() {
    // A chief's window is not touched: no job of its, and no agent of its, is ended.
    let context = Context::new();
    let _tiers = working(&context);
    context
        .adapter
        .answer_leaving_agents("chief", "Four agents are researching it");
    context.pass().unwrap();
    context.pass().unwrap();
}

#[test]
fn a_workers_turn_that_ends_with_agents_pending_keeps_its_task_working_and_the_requester_is_told_once(
) {
    let context = Context::new();
    let tiers = working(&context);
    let session = tiers.assignee(1);

    // zeus starts three research agents and ends its turn: its words are progress.
    context
        .adapter
        .answer_leaving_agents("zeus", "Started three research agents");
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working", "held for the agents");
    assert_eq!(results(&context, &tiers), Vec::<String>::new());
    let told = tiers.notes("chief");
    assert_eq!(told.len(), 1, "{told:?}");
    assert!(
        told[0].starts_with(&goes_on(&session, "1 background agent or workflow")),
        "{told:?}"
    );
    assert!(told[0].contains("cf task finish T-1"), "{told:?}");
    assert!(
        told[0].ends_with("Started three research agents"),
        "{told:?}"
    );

    // An agent's end wakes a turn that ends with the others still running: no word more.
    context.adapter.wake("zeus", NOTICE);
    context.pass().unwrap();
    context
        .adapter
        .answer_leaving_agents("zeus", "One reader is in, two to go");
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working");
    assert_eq!(tiers.notes("chief").len(), 1, "told once");

    // The last one's end wakes the turn that reports: its final answer is the result.
    context.adapter.wake("zeus", NOTICE);
    context.pass().unwrap();
    context
        .adapter
        .answer("zeus", "All three are in: the parser is sound");
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "done");
    assert_eq!(
        results(&context, &tiers),
        ["All three are in: the parser is sound"]
    );
}

#[test]
fn a_reviewer_or_an_advisor_whose_turn_ends_with_agents_pending_is_held_as_a_worker_is() {
    // 2026-10-11's agents were an advisor's: the rule is no worker's alone.
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    tiers.open_for("reviewer", "standard", "Review the parser");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working");

    context
        .adapter
        .answer_leaving_agents("calliope", "Started two readers");
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working");
    assert_eq!(results(&context, &tiers), Vec::<String>::new());

    context.adapter.wake("calliope", NOTICE);
    context.pass().unwrap();
    context
        .adapter
        .answer("calliope", "Both readers agree: two findings");
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "done");
    assert_eq!(
        results(&context, &tiers),
        ["Both readers agree: two findings"]
    );
}

#[test]
fn a_turn_that_left_a_job_and_agents_is_held_for_both_and_ends_neither() {
    let context = Context::new();
    let tiers = working(&context);
    let session = tiers.assignee(1);
    context.adapter.answer_leaving(
        "zeus",
        "Started the run and the research",
        Pending {
            agents: 1,
            jobs: 1,
            retired: 0,
            noticed: 0,
        },
    );
    context.pass().unwrap();

    assert_eq!(tiers.task(1).task.state, "working");
    let told = tiers.notes("chief");
    assert!(
        told[0].starts_with(&goes_on(
            &session,
            "1 background job and 1 background agent or workflow"
        )),
        "{told:?}"
    );
    assert!(told[0].contains("which does not end them"), "{told:?}");
}

#[test]
fn a_turn_that_ends_with_agents_pending_is_not_a_failure_and_a_failed_turn_is_whatever_it_left() {
    let context = Context::new();
    let tiers = working(&context);
    // The harness reports the turn failed, with agents still running.
    context.adapter.with("zeus", |agent| agent.failed = true);
    context
        .adapter
        .answer_leaving_agents("zeus", "Started the research");
    context.pass().unwrap();

    assert_eq!(tiers.task(1).task.state, "failed");
    assert_eq!(results(&context, &tiers), Vec::<String>::new());
    let told = tiers.notes("chief");
    assert!(
        told.iter().all(|note| !note.contains("background")),
        "{told:?}"
    );
}

#[test]
fn an_agent_that_wakes_in_a_window_kept_open_after_its_task_was_finished_runs_a_turn_that_is_no_result(
) {
    // The human keeps the window open and finishes the held task: the agents go on, and
    // what one's end wakes is a turn of a worker whose task is done. Nothing is recorded,
    // nothing is failed, nothing more is said.
    let context = Context::new();
    let tiers = working(&context);
    context
        .open_window(tiers.project.id, &tiers.assignee(1))
        .unwrap();
    context
        .adapter
        .answer_leaving_agents("zeus", "Started the research");
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working");
    let finished = context
        .finish_task(tiers.project.id, 1, "human")
        .expect("finished");
    assert_eq!(finished.task.state, "done");
    assert_eq!(finished.agents, 1);
    let told = tiers.notes("chief");

    context.adapter.busy("zeus");
    context.pass().unwrap();
    context
        .adapter
        .answer("zeus", "All three are in: the parser is done");
    context.pass().unwrap();
    context.pass().unwrap();

    assert_eq!(tiers.task(1).task.state, "done");
    let given = results(&context, &tiers);
    assert_eq!(given.len(), 1, "{given:?}");
    assert!(given[0].starts_with("Started the research"), "{given:?}");
    assert_eq!(tiers.notes("chief"), told);
}

//! A turn that ends while background agents or workflows it started still run
//! (2026-10-11: a chief's four research agents, and a result that waited twelve
//! minutes for the last one). The harness's own record says so of the turn that
//! ended (`agents_pending` of the reading), and its window takes input between
//! turns: a message for it goes in then, whoever the window is for. What a turn
//! like that is not, is a task's result: an agent's end wakes a turn of its own,
//! and the end of the turn that leaves none running is the result (the same
//! hold that a job a worker left running has, `background`). Node's dispatcher
//! had neither, so none of these is held to a Node trace.

use cf_engine::delivery_text::marker_of;
use cf_engine::testing::Context;
use cf_engine::{Activity, ActivityState};

use crate::fixtures::{given, Tiers};

const LOCK: &str = "python3 lock.py record --run 7";

/// T-1 given to zeus, whose window has taken it in.
fn working(context: &Context) -> Tiers<'_> {
    let tiers = Tiers::new(context, &["zeus"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working");
    tiers
}

/// What the window of T-1 is doing.
fn doing(context: &Context, tiers: &Tiers<'_>) -> Activity {
    context.dispatcher.activity(tiers.id(&tiers.assignee(1)))
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
fn a_workers_turn_that_ends_with_agents_pending_is_not_its_result_and_the_next_that_ends_with_none_is(
) {
    let context = Context::new();
    let tiers = working(&context);

    // zeus starts three research agents and ends its turn.
    context
        .adapter
        .answer_leaving_agents("zeus", "Started three research agents");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working", "no result yet");
    assert_eq!(results(&context, &tiers), Vec::<String>::new());
    assert_eq!(
        doing(&context, &tiers),
        Activity::because(ActivityState::Working, "background agents running")
    );

    // The first agent ends, which wakes it: a turn of its own, that ends with
    // two agents still running.
    context.adapter.busy("zeus");
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working");
    assert_eq!(
        doing(&context, &tiers),
        Activity::of(ActivityState::Working)
    );
    context
        .adapter
        .answer_leaving_agents("zeus", "One report is in");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working");
    assert_eq!(results(&context, &tiers), Vec::<String>::new());

    // The last ends: the turn it wakes leaves none, and is the end of the task.
    context.adapter.busy("zeus");
    context.pass().unwrap();
    context
        .adapter
        .answer("zeus", "All three are in: the parser is done");
    context.pass().unwrap();

    assert_eq!(tiers.task(1).task.state, "done");
    assert_eq!(
        results(&context, &tiers),
        ["Started three research agents\n\nOne report is in\n\nAll three are in: the parser is done"]
    );
}

#[test]
fn a_reviewer_or_an_advisor_whose_turn_ends_with_agents_pending_is_held_as_a_worker_is() {
    // 2026-10-11's agents were an advisor's: the hold is no worker's alone.
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
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working", "no findings yet");
    assert_eq!(results(&context, &tiers), Vec::<String>::new());
    assert_eq!(
        doing(&context, &tiers),
        Activity::because(ActivityState::Working, "background agents running")
    );

    context.adapter.busy("calliope");
    context.pass().unwrap();
    context
        .adapter
        .answer("calliope", "No test for empty input.");
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "done");
    assert_eq!(
        results(&context, &tiers),
        ["Started two readers\n\nNo test for empty input."]
    );
}

#[test]
fn agents_hold_the_result_for_as_long_as_they_run_whatever_the_time_and_whoever_the_window_is_for()
{
    let context = Context::new();
    let tiers = working(&context);
    context
        .adapter
        .answer_leaving_agents("zeus", "Started the research");
    context.pass().unwrap();

    // A day, and a week: a window is never stopped for how long it works, and
    // no settle time ends a wait for agents (it ends one for a job, which has
    // no turn to wake).
    for hours in [24, 24 * 6] {
        context.advance(hours * 3_600_000);
        context.pass().unwrap();
        assert_eq!(tiers.task(1).task.state, "working");
    }
    assert_eq!(results(&context, &tiers), Vec::<String>::new());
}

#[test]
fn a_worker_with_agents_pending_is_given_what_the_chief_sends_between_turns() {
    let context = Context::new();
    let tiers = working(&context);
    context
        .adapter
        .answer_leaving_agents("zeus", "Started the research");
    context.pass().unwrap();
    assert_eq!(given(&context, "zeus").len(), 1);

    // The same rule as for the chief's window: its turn is over, it takes input.
    context.tell(
        tiers.project.id,
        &tiers.assignee(1),
        1,
        "How is the research going?",
    );
    context.pass().unwrap();
    context.pass().unwrap();
    let said = given(&context, "zeus");
    assert_eq!(said.len(), 2, "{said:?}");
    assert!(said[1].contains("How is the research going?"), "{said:?}");
}

#[test]
fn a_turn_that_left_a_job_and_agents_is_held_for_the_job_and_then_for_the_agents() {
    let context = Context::new();
    let tiers = working(&context);
    context.background.run("zeus", &[LOCK]);
    context
        .adapter
        .answer_leaving_agents("zeus", "Started the run and the research");
    context.pass().unwrap();
    assert_eq!(
        doing(&context, &tiers).reason.as_deref(),
        Some("background work running (1 process)")
    );

    // The job ends, and the agents run on: more than the settle time later, the
    // task is still held, for them.
    context.background.end("zeus");
    context.pass().unwrap();
    context.advance(60_000);
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working");
    assert_eq!(
        doing(&context, &tiers).reason.as_deref(),
        Some("background agents running")
    );

    context.adapter.busy("zeus");
    context.pass().unwrap();
    context.adapter.answer("zeus", "Both are done");
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "done");
}

#[test]
fn a_turn_that_ends_with_agents_pending_is_not_a_failure_and_a_failed_turn_is_whatever_it_left() {
    let context = Context::new();
    let tiers = working(&context);
    context
        .adapter
        .answer_leaving_agents("zeus", "Started the research");
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working");

    // The harness reports the turn failed, with agents still running.
    context.adapter.with("zeus", |agent| agent.failed = true);
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "failed");
}

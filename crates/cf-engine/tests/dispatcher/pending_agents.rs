//! A turn that ends while background agents or workflows it started still run
//! (2026-10-11: a chief's four research agents, and a result that waited twelve
//! minutes for the last one). The harness's own record says so of the turn that
//! ended (`agents_pending` of the reading), and its window takes input between
//! turns: a message for it goes in then, whoever the window is for. What a worker's
//! turn like that is, since the owner's decision of 2026-10-13, is its task's result
//! at once, as for a job it left running: nothing holds it back for the agents, and
//! the requester is told in a note beside it that the turn left them (the host can
//! end no agent: Claude Code's own ways are keys typed into a window that may be the
//! human's, and the close that follows a done task ends them with the window). Node's
//! dispatcher had neither, so none of these is held to a Node trace.

use cf_engine::delivery_text::marker_of;
use cf_engine::testing::Context;

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

    assert_eq!(context.background.endings_asked("chief"), 0);
}

#[test]
fn a_workers_turn_that_ends_with_agents_pending_is_its_result_at_once_and_the_requester_is_told() {
    let context = Context::new();
    let tiers = working(&context);

    // zeus starts three research agents and ends its turn.
    context
        .adapter
        .answer_leaving_agents("zeus", "Started three research agents");
    context.pass().unwrap();

    assert_eq!(tiers.task(1).task.state, "done", "no hold for the agents");
    assert_eq!(results(&context, &tiers), ["Started three research agents"]);
    assert_eq!(
        tiers.notes("chief"),
        ["T-1's turn left background agents running."]
    );
    assert_eq!(
        context.background.ended("zeus"),
        Vec::<String>::new(),
        "the host was asked, as for any turn, and no process of its tree is among them"
    );
}

#[test]
fn a_reviewer_or_an_advisor_whose_turn_ends_with_agents_pending_gives_its_findings_at_once_as_a_worker_does(
) {
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

    assert_eq!(tiers.task(1).task.state, "done");
    assert_eq!(results(&context, &tiers), ["Started two readers"]);
    assert_eq!(
        tiers.notes("chief"),
        ["T-1's turn left background agents running."]
    );
}

#[test]
fn a_turn_that_left_a_job_and_agents_has_its_job_ended_and_says_the_agents_were_left() {
    let context = Context::new();
    let tiers = working(&context);
    context.background.run("zeus", &[LOCK]);
    context
        .adapter
        .answer_leaving_agents("zeus", "Started the run and the research");
    context.pass().unwrap();

    assert_eq!(tiers.task(1).task.state, "done");
    assert_eq!(context.background.ended("zeus"), [LOCK]);
    assert_eq!(
        tiers.notes("chief"),
        ["T-1's turn left background work running. Ended: python3 lock.py record --run 7. Its background agents were left running too."]
    );
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
fn an_agent_that_wakes_in_a_window_kept_open_after_the_task_is_done_runs_a_turn_that_is_no_result()
{
    // Today's rule, said: the window is the human's and stays, the agents go on, and
    // what one's end wakes is a turn of a worker whose task is done. Nothing is
    // recorded, nothing is failed, nothing more is said.
    let context = Context::new();
    let tiers = working(&context);
    context
        .open_window(tiers.project.id, &tiers.assignee(1))
        .unwrap();
    context
        .adapter
        .answer_leaving_agents("zeus", "Started the research");
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "done");
    let told = tiers.notes("chief");

    context.adapter.busy("zeus");
    context.pass().unwrap();
    context
        .adapter
        .answer("zeus", "All three are in: the parser is done");
    context.pass().unwrap();
    context.pass().unwrap();

    assert_eq!(tiers.task(1).task.state, "done");
    assert_eq!(results(&context, &tiers), ["Started the research"]);
    assert_eq!(tiers.notes("chief"), told);
}

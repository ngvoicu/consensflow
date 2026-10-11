//! A test is a task like any other: for a tester of its tier, in a session of
//! its own, opened with the tester's text and the brief, its results the
//! task's result for the chief. Node's dispatcher had no testers, so no trace
//! holds these.

use cf_engine::testing::Context;
use cf_ledger::{NewTask, ProjectView};
use serde_json::{json, Value};

use crate::fixtures::{assert_match, placed};

/// A project at /work/app: apollo its chief, and `staff` as `(agent, roles, tier)`.
fn project_of(context: &Context, staff: &[(&str, &[&str], &str)]) -> ProjectView {
    let members: Vec<Value> = staff
        .iter()
        .map(|(agent, roles, tier)| {
            json!({ "agent": agent, "harness": "claude-code", "roles": roles, "tier": tier })
        })
        .collect();
    context
        .open_project(json!({
            "directory": "/work/app",
            "name": "app",
            "chief": { "harness": "claude-code", "agent": "apollo" },
            "staff": members,
        }))
        .unwrap()
        .expect("the project")
}

/// The chief opens a task for `tier` of `pool`.
fn open(context: &Context, project: &ProjectView, pool: &str, tier: &str, body: &str) {
    context.create_task(
        project.id,
        NewTask {
            from: "chief".to_owned(),
            pool: Some(pool.to_owned()),
            tier: Some(tier.to_owned()),
            body: body.to_owned(),
            ..NewTask::default()
        },
    );
}

#[test]
fn gives_a_test_to_a_tester_of_its_tier_in_a_session_of_its_own_and_brings_its_results_back_as_the_result(
) {
    let context = Context::new();
    let project = project_of(
        &context,
        &[
            ("zeus", &["worker"], "standard"),
            ("tess", &["tester"], "standard"),
            ("hera", &["tester"], "light"),
        ],
    );
    let task = |number: i64| context.task(project.id, number);
    open(&context, &project, "worker", "standard", "Write the parser");
    context.pass().unwrap();
    context.pass().unwrap();
    context.adapter.answer("zeus", "Parser done");
    context.pass().unwrap();
    assert_eq!(task(1).task.state, "done", "nothing is tested on its own");

    open(
        &context,
        &project,
        "tester",
        "standard",
        "Test T-1: run the parser's tests and its command",
    );
    context.pass().unwrap();
    let test = task(2).task;
    assert_eq!(
        (
            test.pool.as_deref(),
            test.assignee.as_deref(),
            test.state.as_str()
        ),
        (Some("tester"), Some("tess-brisk-birch"), "queued"),
        "the tester of its tier, not the light one"
    );
    let launch = context.adapter.prepared().last().cloned().unwrap();
    assert_eq!(launch["role"], "tester");
    assert_eq!(launch["instructions"], "instructions for tester");
    assert_match(
        launch["message"].as_str().unwrap(),
        r"Test T-1: run the parser's tests and its command",
    );
    context.pass().unwrap();
    assert_eq!(placed(&task(2).task).0, "working");
    context
        .adapter
        .answer("tess", "cargo test: 12 passed, 0 failed. Ready.");
    context.pass().unwrap();
    assert_eq!(
        [task(1).task.state, task(2).task.state],
        ["done", "done"],
        "the chief decides both"
    );
    let thread = task(2);
    let result = thread.messages.last().unwrap();
    assert_eq!(
        (
            result.kind.as_str(),
            result.recipient.as_str(),
            result.body.as_str()
        ),
        ("result", "chief", "cargo test: 12 passed, 0 failed. Ready.")
    );
}

#[test]
fn opens_a_test_of_a_member_who_is_a_worker_too_with_the_testers_text_in_a_session_apart_from_its_work(
) {
    let context = Context::new();
    let project = project_of(&context, &[("hera", &["worker", "tester"], "standard")]);
    let assignee = |number: i64| {
        context
            .task(project.id, number)
            .task
            .assignee
            .unwrap_or_default()
    };
    open(&context, &project, "worker", "standard", "Write the docs");
    context.pass().unwrap();
    open(&context, &project, "tester", "standard", "Test the docs");
    context.pass().unwrap();
    assert_ne!(
        assignee(1),
        assignee(2),
        "hera works in one session and tests in another"
    );
    let roles: Vec<Value> = context
        .adapter
        .prepared()
        .iter()
        .map(|launch| launch["role"].clone())
        .collect();
    assert_eq!(roles.len(), 3, "the chief's window, then one for each task");
    assert_eq!(
        roles[1..],
        [json!("worker"), json!("tester")],
        "each session opens with the text of the role its task needs"
    );
}

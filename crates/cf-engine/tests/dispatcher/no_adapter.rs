//! A harness ConsensFlow has no adapter for (Pi stands for one in these
//! tests): no work for a member on it, no window opened, no project, member or
//! chief on it.

use cf_engine::testing::Context;
use cf_engine::{SwitchTo, SwitchWhen};
use cf_ledger::{NewMember, NewTask, ProjectView};
use serde_json::json;

use crate::fixtures::{assert_match, notes};
use crate::traces::held_to;

const SUITES: &[&str] = &["a harness ConsensFlow has no adapter for"];

/// A worker of the standard tier on Pi joins the staff (`onPi`).
fn on_pi(context: &Context, project: &ProjectView) {
    context
        .ledger
        .borrow_mut()
        .add_member(
            project.id,
            &NewMember {
                agent: "hera".to_owned(),
                harness: "pi".to_owned(),
                designer: false,
                roles: vec!["worker".to_owned()],
                tier: "standard".to_owned(),
            },
        )
        .unwrap();
}

#[test]
fn gives_a_member_on_it_no_work_says_why_and_every_pass_goes_on() {
    let context = Context::new();
    let project = context.with_staff(&[]);
    on_pi(&context, &project);
    context.create_task(
        project.id,
        NewTask {
            from: "chief".to_owned(),
            pool: Some("worker".to_owned()),
            tier: Some("standard".to_owned()),
            body: "Write the parser".to_owned(),
            ..NewTask::default()
        },
    );
    context.pass().unwrap();
    context.pass().unwrap();
    let task = context.task(project.id, 1).task;
    assert_eq!((task.state.as_str(), task.assignee), ("open", None));
    assert_eq!(
        notes(&context, context.id(project.id, "chief")),
        ["T-1 waits for a free standard worker: @hera runs on pi, whose windows ConsensFlow cannot open."]
    );
    assert_eq!(context.host.opened().len(), 1, "only the chief window");
    held_to(
        context.close(),
        SUITES,
        "gives a member on it no work, says why, and every pass goes on",
    );
}

#[test]
fn fails_what_was_given_to_a_member_on_it_by_name_and_its_requester_hears_why() {
    let context = Context::new();
    let project = context.with_staff(&[]);
    on_pi(&context, &project);
    context.give(project.id, "hera", "Parser");
    context.pass().unwrap();
    let task = context.task(project.id, 1);
    assert_eq!(task.task.state, "failed");
    let note = task.messages.iter().find(|m| m.kind == "note").unwrap();
    assert_match(
        &note.body,
        r"^T-1 failed: the launch failed: ConsensFlow cannot open pi windows\.",
    );
    held_to(
        context.close(),
        SUITES,
        "fails what was given to a member on it by name, and its requester hears why",
    );
}

#[test]
fn opens_no_project_member_or_chief_on_it() {
    let context = Context::new();
    let refused = context
        .open_project(json!({
            "directory": "/work/app",
            "name": "app",
            "chief": { "harness": "pi", "agent": "leto" },
        }))
        .unwrap_err()
        .to_string();
    assert_match(&refused, "ConsensFlow cannot open pi windows");
    let refused = context
        .open_project(json!({
            "directory": "/work/app",
            "name": "app",
            "chief": { "harness": "claude-code", "agent": "apollo" },
            "staff": [{ "agent": "hera", "harness": "pi", "role": "worker", "tier": "standard" }],
        }))
        .unwrap_err()
        .to_string();
    assert_match(&refused, "ConsensFlow cannot open pi windows");
    assert!(context.ledger.borrow().projects().unwrap().is_empty());
    let project = context.with_staff(&["zeus"]);
    let refused = context
        .switch_chief(
            project.id,
            SwitchTo {
                harness: "pi".to_owned(),
                agent: "leto".to_owned(),
            },
            SwitchWhen::Now,
            false,
        )
        .unwrap_err()
        .to_string();
    assert_match(&refused, "ConsensFlow cannot open pi windows");
    held_to(
        context.close(),
        SUITES,
        "opens no project, member or chief on it",
    );
}

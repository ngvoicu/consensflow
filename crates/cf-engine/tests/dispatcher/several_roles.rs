//! A member with several roles opens each task's session with the text of
//! the role the task needs.

use cf_engine::testing::Context;
use cf_ledger::NewTask;
use serde_json::{json, Value};

use crate::fixtures::assert_match;
use crate::traces::held_to;

const SUITES: &[&str] = &["a member with several roles"];

#[test]
fn opens_with_the_text_of_the_role_its_task_needs_in_a_session_per_task() {
    let context = Context::new();
    let project = context
        .open_project(json!({
            "directory": "/work/app",
            "name": "app",
            "chief": { "harness": "claude-code", "agent": "apollo" },
            "staff": [
                { "agent": "zeus", "harness": "claude-code", "role": "worker", "tier": "standard" },
                {
                    "agent": "hera",
                    "harness": "claude-code",
                    "roles": ["worker", "reviewer", "advisor"],
                    "tier": "complex",
                },
            ],
        }))
        .unwrap();
    let task = |number: i64| context.task(project.id, number);
    let assignee = |number: i64| task(number).task.assignee.unwrap_or_default();
    // What each of a member's sessions was opened with: the role and its text.
    let launches = |handle: &str| -> Vec<(Value, Value)> {
        context
            .adapter
            .prepared()
            .into_iter()
            .filter(|request| {
                request["participant"]["handle"]
                    .as_str()
                    .is_some_and(|name| name.starts_with(&format!("{handle}-")))
            })
            .map(|request| (request["role"].clone(), request["instructions"].clone()))
            .collect()
    };
    let open = |body: &str, tier: &str, pool: &str| {
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
    };
    let text = |role: &str| (json!(role), json!(format!("instructions for {role}")));
    open("Write the parser", "standard", "worker");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(assignee(1), "zeus-amber-pine");
    context.adapter.answer("zeus", "Parser done");
    context.pass().unwrap();
    assert_eq!(task(1).task.state, "done");
    open("Review T-1: the parser", "complex", "reviewer");
    context.pass().unwrap();
    assert_eq!(assignee(2), "hera-brisk-birch");
    assert_eq!(
        launches("hera"),
        [text("reviewer")],
        "a review opens the reviewer text"
    );
    open("Write the docs", "complex", "worker");
    context.pass().unwrap();
    assert_match(&assignee(3), "^hera-");
    assert_ne!(
        assignee(3),
        assignee(2),
        "hera reviews in one session and works in another"
    );
    context.pass().unwrap();
    context.adapter.answer(&assignee(2), "Fine.");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(task(2).task.state, "done");
    context.pass().unwrap();
    assert_eq!(
        launches("hera").last(),
        Some(&text("worker")),
        "and opens with the worker text"
    );
    context.adapter.answer(&assignee(3), "Docs done");
    context.pass().unwrap();
    context.pass().unwrap();
    open("Which parser design?", "complex", "advisor");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(
        launches("hera").last(),
        Some(&text("advisor")),
        "advice opens with the advisor text, whatever role the member was saved with first"
    );
    assert_match(&assignee(4), "^hera-");
    context.adapter.answer(&assignee(4), "The second.");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(task(4).task.state, "done");
    assert_eq!(task(4).messages.last().unwrap().recipient, "chief");
    held_to(
        context.close(),
        SUITES,
        "opens with the text of the role its task needs, in a session per task",
    );
}

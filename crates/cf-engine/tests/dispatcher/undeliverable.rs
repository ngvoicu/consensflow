//! A message that cannot be delivered: the human hears once what it was, for
//! whom and why, whatever its kind, and a task they gave that fails tells
//! them only that (`describe('a message that cannot be delivered')`).

use cf_engine::testing::Context;
use cf_ledger::NewTask;

use crate::fixtures::Tiers;
use crate::traces::held_to;

const SUITES: &[&str] = &["a message that cannot be delivered"];

/// What the human was told, newest first.
fn told(context: &Context, human: i64) -> Vec<String> {
    context
        .inbox(human)
        .into_iter()
        .map(|message| message.body)
        .collect()
}

#[test]
fn tells_the_human_once_what_it_was_for_whom_and_why_whatever_its_kind() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus", "diana"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working");
    context.adapter.with("chief", |agent| agent.admit = false);
    let question = context.ask(
        tiers.project.id,
        "zeus-amber-pine",
        "chief",
        1,
        "Which dialect?",
    );
    context.adapter.answer("zeus", "I asked the chief.");
    for _ in 0..5 {
        context.pass().unwrap();
    }
    assert_eq!(context.message(question.id).state, "failed");
    let human = tiers.id("human");
    assert_eq!(
        told(&context, human),
        [format!(
            "m-{}, a question from @zeus-amber-pine on T-1, did not reach @chief: refused by the test.",
            question.id
        )]
    );
    context.pass().unwrap();
    assert_eq!(told(&context, human).len(), 1, "once");
    held_to(
        context.close(),
        SUITES,
        "tells the human once what it was, for whom and why, whatever its kind",
    );
}

#[test]
fn tells_the_human_once_when_a_task_they_gave_fails() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.adapter.with("chief", |agent| agent.admit = false);
    context.create_task(
        project.id,
        NewTask {
            from: "human".to_owned(),
            to: Some("chief".to_owned()),
            body: "Plan the week".to_owned(),
            ..NewTask::default()
        },
    );
    for _ in 0..5 {
        context.pass().unwrap();
    }
    assert_eq!(context.task(project.id, 1).task.state, "failed");
    assert_eq!(
        told(&context, context.id(project.id, "human")),
        [r#"T-1 failed: refused by the test. Reopen it with: cf task reopen T-1 "…""#]
    );
    held_to(
        context.close(),
        SUITES,
        "tells the human once when a task they gave fails",
    );
}

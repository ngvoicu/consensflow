//! The dispatcher runs a review like any task: a task for a reviewer of its
//! tier, in a session of its own.

use cf_engine::testing::Context;

use crate::fixtures::{assert_match, placed, Tiers};
use crate::traces::held_to;

const SUITES: &[&str] = &["the dispatcher runs a review like any task"];

#[test]
fn gives_a_review_to_a_reviewer_of_its_tier_in_a_session_of_its_own_and_brings_its_findings_back_as_the_result(
) {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus", "diana"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    context.adapter.answer("zeus", "Parser done");
    context.pass().unwrap();
    assert_eq!(
        tiers.task(1).task.state,
        "done",
        "nothing is reviewed on its own"
    );
    tiers.open_for(
        "reviewer",
        "standard",
        "Review T-1: the parser in src/parse.js",
    );
    context.pass().unwrap();
    let review = tiers.task(2).task;
    assert_eq!(
        (
            review.pool.as_deref(),
            review.assignee.as_deref(),
            review.state.as_str()
        ),
        (Some("reviewer"), Some("calliope-brisk-birch"), "queued"),
        "the earliest joined reviewer of the tier"
    );
    let launch = context.adapter.prepared().last().cloned().unwrap();
    assert_eq!(launch["role"], "reviewer");
    assert_eq!(launch["instructions"], "instructions for reviewer");
    assert_match(
        launch["message"].as_str().unwrap(),
        r"Review T-1: the parser in src/parse\.js",
    );
    context.pass().unwrap();
    assert_eq!(placed(&tiers.task(2).task).0, "working");
    context
        .adapter
        .answer("calliope", "No test for empty input.");
    context.pass().unwrap();
    assert_eq!(
        [tiers.task(1).task.state, tiers.task(2).task.state],
        ["done", "done"],
        "the chief decides both"
    );
    let thread = tiers.task(2);
    let result = thread.messages.last().unwrap();
    assert_eq!(
        (
            result.kind.as_str(),
            result.recipient.as_str(),
            result.body.as_str()
        ),
        ("result", "chief", "No test for empty input.")
    );
    held_to(
        context.close(),
        SUITES,
        "gives a review to a reviewer of its tier in a session of its own, and brings its findings back as the result",
    );
}

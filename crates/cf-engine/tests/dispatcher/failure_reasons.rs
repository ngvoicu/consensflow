//! A member's turn that failed fails its task, and the note names why (field
//! report dip-agentos #4, 3.0.4, 2026-10-19: every failure said only "the harness
//! reported a failure", and neither the chief nor the human could tell a crash
//! from a refusal from a bad model name). The reason is the harness's own words
//! where its record has them, and otherwise the last lines its window shows,
//! each held to a quote's bounds: the note goes to the board, the chief's window
//! and the log. The words here are made up for the tests.

use serde_json::json;

use crate::background::working;
use crate::fixtures::Tiers;
use cf_engine::testing::Context;

/// What the chief is told when T-1 failed `because`, the handle of zeus's
/// session in place of `@zeus`.
fn failed(tiers: &Tiers<'_>, because: &str) -> String {
    let handle = tiers.assignee(1);
    let because = because.replace("@zeus", &format!("@{handle}"));
    format!("T-1 failed: {because}. Reopen it with: cf task reopen T-1 \"…\"")
}

/// zeus's turn on T-1 fails, its record saying `words` of why.
fn fails(context: &Context, words: Option<&str>) {
    context.adapter.with("zeus", |agent| {
        agent.failed = true;
        agent.failure = words.map(str::to_owned);
    });
    context.adapter.answer("zeus", "Starting on the parser");
    context.pass().unwrap();
}

fn told(tiers: &Tiers<'_>) -> Vec<String> {
    tiers.notes("chief")
}

#[test]
fn a_failed_turn_says_what_its_harness_said_of_it() {
    let context = Context::new();
    let tiers = working(&context);
    fails(
        &context,
        Some("Selected model is at capacity. Please try a different model."),
    );

    assert_eq!(tiers.task(1).task.state, "failed");
    assert_eq!(
        told(&tiers),
        [failed(
            &tiers,
            "@zeus's harness reported a failure; it said: \"Selected model is at capacity. Please try a different model.\""
        )]
    );
}

#[test]
fn a_failed_turn_whose_record_says_nothing_says_what_its_window_shows_last() {
    let context = Context::new();
    let tiers = working(&context);
    context.host.set_snapshot(json!({
        "tail": ["Working on the parser", "", "Error: model 'gpt-0-nonesuch' not found"]
    }));
    fails(&context, None);

    assert_eq!(tiers.task(1).task.state, "failed");
    assert_eq!(
        told(&tiers),
        [failed(
            &tiers,
            "@zeus's harness reported a failure; its screen ended with: \"Working on the parser / Error: model 'gpt-0-nonesuch' not found\""
        )]
    );
}

#[test]
fn the_harness_words_are_held_to_a_quotes_bounds() {
    let context = Context::new();
    let tiers = working(&context);
    let long = format!("Internal error: {}", "x".repeat(600));
    fails(&context, Some(&format!("first line\n{long}")));

    // The last line, cut to the quote's 400 characters, and nothing before it.
    let quoted = format!("{}…", &long[..399]);
    assert_eq!(
        told(&tiers),
        [failed(
            &tiers,
            &format!("@zeus's harness reported a failure; it said: \"{quoted}\"")
        )]
    );
}

#[test]
fn harness_words_that_name_a_secret_are_not_quoted() {
    let context = Context::new();
    let tiers = working(&context);
    fails(&context, Some("invalid password for user ci-bot: hunter2"));

    assert_eq!(
        told(&tiers),
        [failed(
            &tiers,
            "@zeus's harness reported a failure; what it said named a secret, so it is left out"
        )]
    );
}

#[test]
fn a_token_in_the_harness_words_is_masked() {
    let context = Context::new();
    let tiers = working(&context);
    fails(
        &context,
        Some("request rejected for sk-ant-api03-AbCdEfGhIjKlMnOpQrStUvWxYz0123456789"),
    );

    let note = &told(&tiers)[0];
    assert!(!note.contains("AbCdEfGhIjKlMnOpQrStUvWxYz"), "{note}");
    assert!(
        note.contains("(1 key- or token-like string masked)"),
        "{note}"
    );
}

#[test]
fn a_task_cancelled_while_its_window_is_looked_at_is_not_failed_after() {
    let context = Context::new();
    let tiers = working(&context);
    context
        .host
        .set_snapshot(json!({ "tail": ["Error: the agent stopped"] }));
    context.adapter.with("zeus", |agent| agent.failed = true);
    context.adapter.answer("zeus", "Starting on the parser");
    // The pass asks the host what the window shows; the human cancels T-1 meanwhile.
    let answer = context
        .host
        .request_holds
        .hold(|args| args[0] == "pane.snapshot" && args[1].get("tail").is_some());
    let passing = context.begin_pass();
    context.settle();
    context
        .ledger
        .borrow_mut()
        .cancel_task(tiers.project.id, 1, "human")
        .unwrap();
    answer.open();
    context.settle();
    passing.take().unwrap().unwrap();

    assert_eq!(tiers.task(1).task.state, "cancelled");
    let told = told(&tiers);
    assert!(told.iter().all(|note| !note.contains("failed")), "{told:?}");
}

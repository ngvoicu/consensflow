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
    // The next look finds the failure told: it is not told again.
    context.pass().unwrap();

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
fn the_harness_words_are_held_to_a_quotes_bounds_from_their_first_line() {
    // An error's headline is its first line: the first six lines are kept.
    let context = Context::new();
    let tiers = working(&context);
    let frames: Vec<String> = (1..=9)
        .map(|n| format!("    at frame{n} (agent.js:{n})"))
        .collect();
    fails(
        &context,
        Some(&format!("Error: the agent crashed\n{}", frames.join("\n"))),
    );
    assert_eq!(
        told(&tiers),
        [failed(
            &tiers,
            "@zeus's harness reported a failure; it said: \"Error: the agent crashed / at frame1 (agent.js:1) / at frame2 (agent.js:2) / at frame3 (agent.js:3) / at frame4 (agent.js:4) / at frame5 (agent.js:5)\""
        )]
    );

    // A first line too long for the quote is cut, and nothing after it is kept.
    let context = Context::new();
    let tiers = working(&context);
    let long = format!("Internal error: {}", "x".repeat(600));
    fails(&context, Some(&format!("{long}\nsecond line")));
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
fn a_secret_value_in_the_harness_words_is_masked_and_the_rest_quoted() {
    let context = Context::new();
    let tiers = working(&context);
    fails(
        &context,
        Some("login refused for ci-bot, password: hunter2"),
    );

    assert_eq!(
        told(&tiers),
        [failed(
            &tiers,
            "@zeus's harness reported a failure; it said: \"login refused for ci-bot, password: [masked]\" (1 key- or token-like string masked)"
        )]
    );
}

#[test]
fn a_harness_without_a_login_says_so_in_the_note() {
    // Pi's words when it has no key: a key named, none held. The fix is the human's /login.
    let context = Context::new();
    let tiers = working(&context);
    let words = "No API key found for the selected model. Use /login to log into a provider.";
    fails(&context, Some(words));

    assert_eq!(
        told(&tiers),
        [failed(
            &tiers,
            &format!("@zeus's harness reported a failure; it said: \"{words}\"")
        )]
    );
}

#[test]
fn an_email_address_in_the_harness_words_is_masked() {
    let context = Context::new();
    let tiers = working(&context);
    fails(
        &context,
        Some("Your account ci-bot@example.com has no seats left."),
    );

    assert_eq!(
        told(&tiers),
        [failed(
            &tiers,
            "@zeus's harness reported a failure; it said: \"Your account [masked] has no seats left.\" (1 key- or token-like string masked)"
        )]
    );
}

#[test]
fn a_screen_the_host_would_not_show_is_said_to_be_unread() {
    let context = Context::new();
    let tiers = working(&context);
    context.host.set_snapshot(json!({ "ok": false }));
    fails(&context, None);

    assert_eq!(
        told(&tiers),
        [failed(
            &tiers,
            "@zeus's harness reported a failure; its screen could not be read"
        )]
    );
}

#[test]
fn the_screens_chrome_is_no_line_of_its_quote() {
    // A composer's box and a footer's rule under the error: none has a letter or digit.
    let context = Context::new();
    let tiers = working(&context);
    context.host.set_snapshot(json!({
        "tail": [
            "Working on the parser",
            "Error: model 'gpt-0-nonesuch' not found",
            "",
            "╭──────────────────╮",
            "│ >                │",
            "│                  │",
            "│                  │",
            "╰──────────────────╯",
            "────────────────────",
        ]
    }));
    fails(&context, None);

    assert_eq!(
        told(&tiers),
        [failed(
            &tiers,
            "@zeus's harness reported a failure; its screen ended with: \"Working on the parser / Error: model 'gpt-0-nonesuch' not found\""
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

#[test]
fn a_task_released_to_the_board_while_its_window_is_looked_at_is_not_failed_after() {
    let context = Context::new();
    let tiers = working(&context);
    context
        .host
        .set_snapshot(json!({ "tail": ["Error: the agent stopped"] }));
    context.adapter.with("zeus", |agent| agent.failed = true);
    context.adapter.answer("zeus", "Starting on the parser");
    // The pass asks the host what the window shows; the human takes T-1 back to the board meanwhile.
    let answer = context
        .host
        .request_holds
        .hold(|args| args[0] == "pane.snapshot" && args[1].get("tail").is_some());
    let passing = context.begin_pass();
    context.settle();
    context
        .ledger
        .borrow_mut()
        .release_task(tiers.project.id, 1, "the human took it back")
        .unwrap();
    answer.open();
    context.settle();
    passing.take().unwrap().unwrap();

    let task = tiers.task(1).task;
    assert_eq!((task.state.as_str(), task.assignee), ("open", None));
    let told = told(&tiers);
    assert!(told.iter().all(|note| !note.contains("failed")), "{told:?}");
}

#[test]
fn a_turn_that_fails_while_its_task_waits_tells_whoever_gave_it_once_and_the_task_still_waits() {
    let context = Context::new();
    let tiers = working(&context);
    let zeus = tiers.assignee(1);
    context.ask(tiers.project.id, &zeus, "chief", 1, "Which format?");
    assert_eq!(tiers.task(1).task.state, "waiting");
    fails(&context, Some("The model is busy, try another."));
    context.pass().unwrap();
    context.pass().unwrap();

    assert_eq!(tiers.task(1).task.state, "waiting");
    let failures: Vec<String> = told(&tiers)
        .into_iter()
        .filter(|note| note.contains("failed"))
        .collect();
    assert_eq!(
        failures,
        [format!(
            "A turn of T-1's window failed while T-1 was waiting, as it stays: @{zeus}'s harness reported a failure; it said: \"The model is busy, try another.\"."
        )]
    );
}

#[test]
fn a_chiefs_failed_turn_tells_the_human_once() {
    let context = Context::new();
    let tiers = working(&context);
    context.adapter.with("chief", |agent| {
        agent.failed = true;
        agent.failure = Some("API Error: 529 Overloaded".to_owned());
    });
    context.adapter.answer("chief", "Looking at the board");
    context.pass().unwrap();
    context.pass().unwrap();
    context.pass().unwrap();

    let chief = tiers.participant("chief").unwrap().handle;
    let failures: Vec<String> = tiers
        .notes("human")
        .into_iter()
        .filter(|note| note.contains("failed"))
        .collect();
    assert_eq!(
        failures,
        [format!(
            "The chief's turn failed: @{chief}'s harness reported a failure; it said: \"API Error: 529 Overloaded\"."
        )]
    );
}

#[test]
fn harness_words_with_nothing_of_a_word_leave_the_reason_to_the_screen() {
    let context = Context::new();
    let tiers = working(&context);
    context
        .host
        .set_snapshot(json!({ "tail": ["Error: the agent stopped"] }));
    fails(&context, Some("…"));

    assert_eq!(
        told(&tiers),
        [failed(
            &tiers,
            "@zeus's harness reported a failure; its screen ended with: \"Error: the agent stopped\""
        )]
    );
}

#[test]
fn a_tasks_failure_is_its_failed_turn_told_in_a_window_the_human_keeps_open() {
    // The window stays after its task failed: its next looks find the failure told
    // by the task's own note, and say nothing more.
    let context = Context::new();
    let tiers = working(&context);
    context
        .open_window(tiers.project.id, &tiers.assignee(1))
        .unwrap();
    fails(&context, Some("The model is busy, try another."));
    context.pass().unwrap();
    context.pass().unwrap();

    assert_eq!(tiers.task(1).task.state, "failed");
    assert_eq!(
        told(&tiers),
        [failed(
            &tiers,
            "@zeus's harness reported a failure; it said: \"The model is busy, try another.\""
        )]
    );
}

#[test]
fn the_lines_consensflow_typed_into_the_window_are_no_reason_and_are_not_quoted() {
    let context = Context::new();
    let tiers = working(&context);
    let brief = context.task(tiers.project.id, 1).messages[0].clone();
    let header = format!("[ConsensFlow m-{} · T-1 · task from @chief]", brief.id);
    context.host.set_snapshot(json!({
        "tail": [header, brief.body, "", "Error: model 'gpt-0-nonesuch' not found"]
    }));
    fails(&context, None);

    assert_eq!(
        told(&tiers),
        [failed(
            &tiers,
            "@zeus's harness reported a failure; its screen ended with: \"Error: model 'gpt-0-nonesuch' not found\""
        )]
    );
}

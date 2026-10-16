//! 3 (A3): an ended notice after its result; 4 (A4): a result that is not its
//! turn's final answer.

use cf_evals::long_run::invariants;
use cf_evals::long_run::look::Look;
use serde_json::json;

use crate::scene::{found, ms, t, Scene, CHIEF};

/// T-14 of the worker (3), done.
fn task(scene: &Scene) -> i64 {
    scene.task(14, 3, "done", &t(2, 0))
}

const ENDED: &str =
    "T-14's turn left background work running. Ended: npx stryker run --concurrency 4.";

/// What the look could not judge of the third: the keys of the notices the log ties
/// to no result.
fn unbound(look: &Look) -> Vec<String> {
    invariants::unjudged(look)
        .into_iter()
        .filter(|open| open.invariant == 3)
        .map(|open| open.key)
        .collect()
}

#[test]
fn an_ended_notice_after_its_result_breaks_the_third() {
    let scene = Scene::new(&[("long-worker", "claude-code")]);
    let task = task(&scene);
    scene.message(
        131,
        (CHIEF, Some(3)),
        "result",
        Some(task),
        "Done.",
        &t(2, 0),
    );
    scene.message(132, (CHIEF, None), "note", Some(task), ENDED, &t(2, 1));
    // The task began no attempt since the result: the notice is its, at once.
    let breaks = found(&scene.look(ms(2, 2)), 3);
    assert_eq!(breaks.len(), 1, "{breaks:?}");
    assert_eq!(breaks[0].key, "3:m-132");
    assert_eq!(
        breaks[0].said,
        "T-14's ended notice m-132 came after its result m-131"
    );
    assert_eq!(breaks[0].evidence.rows.len(), 2);
    assert!(breaks[0].evidence.rows[0].starts_with("m-131 result"));
    assert!(unbound(&scene.look(ms(2, 2))).is_empty());
    // Accepted since: no other attempt either.
    scene.event(
        &t(2, 1),
        "task.state",
        &json!({ "task": 14, "from": "done", "to": "accepted" }),
    );
    assert_eq!(found(&scene.look(ms(4, 0)), 3).len(), 1);
}

/// T-14 sent back for more work at `at`: it leaves done, and an attempt begins.
fn sent_back(scene: &Scene, at: &str) {
    scene.event(
        at,
        "task.state",
        &json!({ "task": 14, "from": "done", "to": "queued" }),
    );
}

#[test]
fn an_ended_notice_before_any_result_or_a_close_note_after_one_holds_the_third() {
    // The turn-end work's step: the notice, then its result, the first attempt's.
    let scene = Scene::new(&[("long-worker", "claude-code")]);
    let task = task(&scene);
    scene.message(131, (CHIEF, None), "note", Some(task), ENDED, &t(2, 0));
    scene.message(
        132,
        (CHIEF, Some(3)),
        "result",
        Some(task),
        "Done.",
        &t(2, 0),
    );
    // What a window's close ended is said after the result, as it happens after it.
    scene.message(
        140,
        (CHIEF, None),
        "note",
        Some(task),
        "T-14's window closed with background work still running: node serve.mjs",
        &t(3, 0),
    );
    let look = scene.look(ms(4, 0));
    assert!(found(&look, 3).is_empty());
    assert!(unbound(&look).is_empty());
}

#[test]
fn a_notice_after_a_result_and_a_send_back_is_unresolved_however_near_the_next_result() {
    // The reviewers' sequence: the old result at 0:30, T-14 sent back at 0:40 and at
    // work at 0:45, an ended notice at 1:00, the next attempt's result at 1:01. The
    // notice may be the old attempt's, late, or the new one's own: no break, and not
    // taken to hold, at every look after.
    let scene = Scene::new(&[("long-worker", "claude-code")]);
    let task = task(&scene);
    scene.message(
        100,
        (CHIEF, Some(3)),
        "result",
        Some(task),
        "First round.",
        &t(0, 30),
    );
    sent_back(&scene, &t(0, 40));
    scene.event(
        &t(0, 45),
        "task.state",
        &json!({ "task": 14, "from": "queued", "to": "working" }),
    );
    scene.message(101, (CHIEF, None), "note", Some(task), ENDED, &t(1, 0));
    assert!(found(&scene.look(ms(1, 0)), 3).is_empty());
    assert_eq!(unbound(&scene.look(ms(1, 0))), ["3:m-101"]);
    scene.message(
        150,
        (CHIEF, Some(3)),
        "result",
        Some(task),
        "Done.",
        &t(1, 1),
    );
    for at in [ms(1, 2), ms(6, 0)] {
        let look = scene.look(at);
        assert!(found(&look, 3).is_empty());
        assert_eq!(
            unbound(&look),
            ["3:m-101"],
            "not cleared by the result after"
        );
    }
    let said = &invariants::unjudged(&scene.look(ms(6, 0)))[0].said;
    assert_eq!(
        said,
        "T-14's ended notice m-101 came after its result m-100 and after T-14 was sent back: whether it is that result's, late, or the next attempt's own, the log does not say"
    );
}

#[test]
fn a_late_notice_written_before_the_send_back_is_its_results_whatever_comes_after() {
    // The late notice m-131 of the result m-100, written before T-14 was sent back,
    // and the next attempt's result m-132 after: a break, of m-100.
    let scene = Scene::new(&[("long-worker", "claude-code")]);
    let task = self::task(&scene);
    scene.message(
        100,
        (CHIEF, Some(3)),
        "result",
        Some(task),
        "First round.",
        &t(0, 30),
    );
    scene.message(131, (CHIEF, None), "note", Some(task), ENDED, &t(0, 31));
    sent_back(&scene, &t(1, 0));
    scene.message(
        132,
        (CHIEF, Some(3)),
        "result",
        Some(task),
        "Done.",
        &t(2, 0),
    );
    let look = scene.look(ms(4, 0));
    let breaks = found(&look, 3);
    assert_eq!(breaks.len(), 1, "{breaks:?}");
    assert_eq!(
        breaks[0].said,
        "T-14's ended notice m-131 came after its result m-100"
    );
    assert!(unbound(&look).is_empty());
    // A send-back the log puts at the notice's own moment may be before it.
    let scene = Scene::new(&[("long-worker", "claude-code")]);
    let task = self::task(&scene);
    scene.message(
        100,
        (CHIEF, Some(3)),
        "result",
        Some(task),
        "First round.",
        &t(0, 30),
    );
    sent_back(&scene, &t(0, 31));
    scene.message(131, (CHIEF, None), "note", Some(task), ENDED, &t(0, 31));
    let look = scene.look(ms(4, 0));
    assert!(found(&look, 3).is_empty());
    assert_eq!(unbound(&look), ["3:m-131"]);
}

#[test]
fn a_notices_row_is_masked_whole_before_it_is_cut() {
    // The secret's closing quote is past the 200 characters a row keeps of a body.
    let scene = Scene::new(&[("long-worker", "claude-code")]);
    let task = task(&scene);
    let body = format!(
        "{}password=\"correct horse battery staple\" in the log.",
        "Done; ".repeat(27)
    );
    scene.message(131, (CHIEF, Some(3)), "result", Some(task), &body, &t(2, 0));
    scene.message(132, (CHIEF, None), "note", Some(task), ENDED, &t(2, 1));
    let look = scene.look(ms(2, 2));
    // As the run keeps it: the keeper masks what it records again.
    let kept = invariants::Keeper::default().keep(&look, found(&look, 3));
    let row = &kept[0].evidence.rows[0];
    assert!(!row.contains("horse"), "{row}");
    assert!(row.contains("password=\"[masked]\""), "{row}");
}

/// The worker's conversation for T-14: its brief, two lines of progress each
/// before a tool call, and its report after the last call.
fn worked(scene: &Scene) -> (i64, i64) {
    let task = task(scene);
    let conversation = scene.conversation(3, "claude-code", "s-worker");
    scene.item(
        conversation,
        0,
        "user",
        "[ConsensFlow m-120 · T-14 · task from @chief]\nFix the rounding",
        true,
        &t(0, 0),
    );
    scene.item(
        conversation,
        1,
        "assistant",
        "Reading src/money.mjs to find the rounding.",
        false,
        &t(0, 10),
    );
    scene.item(
        conversation,
        2,
        "tool",
        "src/money.mjs: 120 lines",
        true,
        &t(0, 11),
    );
    scene.item(
        conversation,
        3,
        "assistant",
        "Found it: half-up instead of half-even. Fixing.",
        false,
        &t(0, 30),
    );
    scene.item(
        conversation,
        4,
        "tool",
        "The file src/money.mjs has been updated.",
        true,
        &t(0, 31),
    );
    scene.item(
        conversation,
        5,
        "assistant",
        "Rounding is half-even now, with a test; 61 tests pass.",
        true,
        &t(1, 0),
    );
    (task, conversation)
}

#[test]
fn a_result_that_holds_the_progress_of_its_turn_or_not_its_final_answer_breaks_the_fourth() {
    let scene = Scene::new(&[("long-worker", "claude-code")]);
    let (task, _) = worked(&scene);
    scene.message(
        131,
        (CHIEF, Some(3)),
        "result",
        Some(task),
        "Reading src/money.mjs to find the rounding.\n\nFound it: half-up instead of half-even. Fixing.\n\nRounding is half-even now, with a test; 61 tests pass.",
        &t(1, 5),
    );
    let breaks = found(&scene.look(ms(5, 0)), 4);
    assert_eq!(breaks.len(), 1, "{breaks:?}");
    assert_eq!(breaks[0].said, "m-131 holds 2 lines of its turn's progress");
    assert!(breaks[0]
        .evidence
        .record
        .iter()
        .any(|line| line.contains("half-even now")));

    let scene = Scene::new(&[("long-worker", "claude-code")]);
    let (task, _) = worked(&scene);
    scene.message(
        131,
        (CHIEF, Some(3)),
        "result",
        Some(task),
        "Working on it.",
        &t(1, 5),
    );
    let breaks = found(&scene.look(ms(5, 0)), 4);
    assert_eq!(
        breaks[0].said,
        "m-131 is not its turn's final answer, whole"
    );
}

#[test]
fn a_result_cut_after_the_start_of_its_final_answer_breaks_the_fourth() {
    // The findings are in the part left out: the start alone is no final answer.
    let scene = Scene::new(&[("long-worker", "claude-code")]);
    let task = task(&scene);
    let conversation = scene.conversation(3, "claude-code", "s-worker");
    scene.item(
        conversation,
        0,
        "user",
        "[ConsensFlow m-120 · T-14 · task from @chief]\nReview it",
        true,
        &t(0, 0),
    );
    scene.item(
        conversation,
        1,
        "tool",
        "src/money.mjs: 120 lines",
        true,
        &t(0, 10),
    );
    let report = "I read src/money.mjs, src/invoice.mjs and their tests from top to bottom, \
                  and ran the whole suite twice. Findings: the rounding is half-up where the brief asks for half-even, \
                  and the invoice total skips the discount line.";
    scene.item(conversation, 2, "assistant", report, true, &t(1, 0));
    scene.message(
        131,
        (CHIEF, Some(3)),
        "result",
        Some(task),
        &report[..140],
        &t(1, 5),
    );
    let breaks = found(&scene.look(ms(5, 0)), 4);
    assert_eq!(breaks.len(), 1, "{breaks:?}");
    assert_eq!(
        breaks[0].said,
        "m-131 is not its turn's final answer, whole"
    );
}

#[test]
fn a_final_answer_that_says_again_what_its_progress_said_holds_the_fourth() {
    // "The rounding bug is now fixed." said before the tests ran, and again in the
    // report: the report is the final answer, whole, and nothing else.
    let scene = Scene::new(&[("long-worker", "claude-code")]);
    let task = task(&scene);
    let conversation = scene.conversation(3, "claude-code", "s-worker");
    scene.item(
        conversation,
        0,
        "user",
        "[ConsensFlow m-120 · T-14 · task from @chief]\nFix the rounding",
        true,
        &t(0, 0),
    );
    scene.item(
        conversation,
        1,
        "assistant",
        "The rounding bug is now fixed.",
        false,
        &t(0, 20),
    );
    scene.item(conversation, 2, "tool", "61 tests pass", true, &t(0, 40));
    let report = "The rounding bug is now fixed.\n\nAll 61 tests pass, one of them new.";
    scene.item(conversation, 3, "assistant", report, true, &t(1, 0));
    scene.message(
        131,
        (CHIEF, Some(3)),
        "result",
        Some(task),
        report,
        &t(1, 5),
    );
    assert!(found(&scene.look(ms(5, 0)), 4).is_empty());
}

#[test]
fn codex_commentary_after_the_last_tool_call_is_no_part_of_its_final_answer() {
    // Codex writes a progress note after its last command and then its final answer;
    // its record marks only the final answer complete, and the product leaves the
    // commentary out of the result.
    let scene = Scene::new(&[("long-worker", "codex")]);
    let task = task(&scene);
    let conversation = scene.conversation(3, "codex", "s-codex");
    scene.item(
        conversation,
        0,
        "user",
        "[ConsensFlow m-120 · T-14 · task from @chief]\nFix the rounding",
        true,
        &t(0, 0),
    );
    scene.item(conversation, 1, "tool", "61 tests pass", true, &t(0, 40));
    scene.item(
        conversation,
        2,
        "assistant",
        "I'll summarise what changed for the chief now.",
        false,
        &t(0, 50),
    );
    let report = "Rounding is half-even now, with a test; 61 tests pass.";
    scene.item(conversation, 3, "assistant", report, true, &t(1, 0));
    scene.message(
        131,
        (CHIEF, Some(3)),
        "result",
        Some(task),
        report,
        &t(1, 5),
    );
    assert!(found(&scene.look(ms(5, 0)), 4).is_empty());
    // A result that holds the commentary holds progress.
    let scene = Scene::new(&[("long-worker", "codex")]);
    let task = self::task(&scene);
    let conversation = scene.conversation(3, "codex", "s-codex");
    scene.item(
        conversation,
        0,
        "user",
        "[ConsensFlow m-120 · T-14 · task from @chief]\nFix the rounding",
        true,
        &t(0, 0),
    );
    scene.item(conversation, 1, "tool", "61 tests pass", true, &t(0, 40));
    scene.item(
        conversation,
        2,
        "assistant",
        "I'll summarise what changed for the chief now.",
        false,
        &t(0, 50),
    );
    scene.item(conversation, 3, "assistant", report, true, &t(1, 0));
    scene.message(
        131,
        (CHIEF, Some(3)),
        "result",
        Some(task),
        &format!("I'll summarise what changed for the chief now.\n\n{report}"),
        &t(1, 5),
    );
    let breaks = found(&scene.look(ms(5, 0)), 4);
    assert_eq!(breaks.len(), 1, "{breaks:?}");
    assert_eq!(breaks[0].said, "m-131 holds 1 line of its turn's progress");
}

#[test]
fn a_result_that_is_its_turns_final_answer_holds_the_fourth() {
    let scene = Scene::new(&[("long-worker", "claude-code")]);
    let (task, conversation) = worked(&scene);
    scene.message(
        131,
        (CHIEF, Some(3)),
        "result",
        Some(task),
        "Rounding is half-even now, with a test; 61 tests pass.",
        &t(1, 5),
    );
    assert!(found(&scene.look(ms(5, 0)), 4).is_empty());
    // A turn its job's end woke is the last turn: the turn before went as progress.
    scene.item(
        conversation,
        6,
        "user",
        "<task-notification>\n<task-id>b1</task-id>\n</task-notification>",
        true,
        &t(2, 0),
    );
    scene.item(
        conversation,
        7,
        "assistant",
        "The mutation run finished: 33 of 33 killed.",
        true,
        &t(2, 30),
    );
    scene.message(
        140,
        (CHIEF, Some(3)),
        "result",
        Some(task),
        "The mutation run finished: 33 of 33 killed.",
        &t(2, 35),
    );
    assert!(found(&scene.look(ms(5, 0)), 4).is_empty());
    // A worker that called no tool: its last words are the answer.
    let scene = Scene::new(&[("long-worker", "pi")]);
    let task = self::task(&scene);
    let conversation = scene.conversation(3, "pi", "s-pi");
    scene.item(
        conversation,
        0,
        "user",
        "[ConsensFlow m-120 · T-14 · task from @chief]\nSay it",
        true,
        &t(0, 0),
    );
    scene.item(
        conversation,
        1,
        "assistant",
        "It is said, with every word the brief asked for.",
        true,
        &t(0, 20),
    );
    scene.message(
        131,
        (CHIEF, Some(3)),
        "result",
        Some(task),
        "It is said, with every word the brief asked for.",
        &t(0, 25),
    );
    assert!(found(&scene.look(ms(5, 0)), 4).is_empty());
}

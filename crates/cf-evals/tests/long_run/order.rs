//! 3 (A3): an ended notice after its result; 4 (A4): a result that is not its
//! turn's final answer.

use cf_evals::long_run::invariants::{self, Break};
use cf_evals::long_run::known;
use cf_evals::long_run::look::Look;
use serde_json::json;

use crate::scene::{found, ms, t, Scene, CHIEF};

/// T-14 of the worker (3), done; its brief, m-120, delivered to it at 10:00.
fn task(scene: &Scene) -> i64 {
    let task = scene.task(14, 3, "done", &t(2, 0));
    scene.briefed(120, 3, task, &t(0, 0));
    task
}

const ENDED: &str =
    "T-14's turn left background work running. Ended: npx stryker run --concurrency 4.";

/// What the look could not judge of the third: the keys of the notices the log ties
/// to no result.
fn unbound(look: &Look) -> Vec<String> {
    invariants::unjudged(look, &Default::default())
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

#[test]
fn a_late_notice_says_which_notice_it_is_the_background_ended_note_or_a_let_go() {
    // What a list of known breaks matches a break of the third by.
    let late = |body: &str| {
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
        scene.message(132, (CHIEF, None), "note", Some(task), body, &t(2, 1));
        found(&scene.look(ms(2, 2)), 3)
    };
    assert_eq!(late(ENDED)[0].shape, Some(invariants::ENDED_NOTE));
    assert_eq!(
        late("T-14 is let go: its job is gone.")[0].shape,
        Some(invariants::LET_GO_NOTE)
    );
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
    let said = invariants::unjudged(&scene.look(ms(6, 0)), &Default::default())
        .into_iter()
        .find(|open| open.invariant == 3)
        .unwrap()
        .said;
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

/// What the look could not judge of the fourth, by key and words.
fn unanswered(look: &Look) -> Vec<(String, String)> {
    invariants::unjudged(look, &Default::default())
        .into_iter()
        .filter(|open| open.invariant == 4)
        .map(|open| (open.key, open.said))
        .collect()
}

#[test]
fn a_result_whose_turn_the_copy_does_not_hold_is_unresolved_never_skipped() {
    // No copy of the worker's conversation at all.
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
    let look = scene.look(ms(4, 0));
    assert!(found(&look, 4).is_empty());
    let open = unanswered(&look);
    assert_eq!(open.len(), 1, "{open:?}");
    assert_eq!(open[0].0, "4:m-131");
    assert_eq!(
        open[0].1,
        "m-131's turn is not in the copy of @long-worker's conversation, which holds no prompt of m-120 after its delivery attempt 1 began: whether it is the turn's final answer cannot be judged"
    );
    // Too young to judge: the copy is made a moment after.
    assert!(unanswered(&scene.look(ms(2, 30))).is_empty());
    // A copy that holds the prompt and a tool's item alone.
    let conversation = scene.conversation(3, "claude-code", "s-worker");
    scene.item(
        conversation,
        0,
        "user",
        "[ConsensFlow m-120 · T-14 · task from @chief]\nRun them",
        true,
        &t(0, 0),
    );
    scene.item(conversation, 1, "tool", "61 tests pass", true, &t(0, 30));
    let open = unanswered(&scene.look(ms(4, 0)));
    assert_eq!(
        open[0].1,
        "m-131's turn is not in the copy of @long-worker's conversation, which holds no answer of the agent's after its last prompt: whether it is the turn's final answer cannot be judged"
    );
    // A result no delivery of its task to the worker began, as the log tells it.
    let scene = Scene::new(&[("long-worker", "claude-code")]);
    let task = scene.task(14, 3, "done", &t(2, 0));
    scene.message(
        131,
        (CHIEF, Some(3)),
        "result",
        Some(task),
        "Done.",
        &t(2, 0),
    );
    let open = unanswered(&scene.look(ms(4, 0)));
    assert_eq!(
        open[0].1,
        "m-131's turn was begun by no delivery to @long-worker of T-14 that the log shows before it: whether it is the turn's final answer cannot be judged"
    );
    // One whose turn says nothing after its last tool call.
    let scene = Scene::new(&[("long-worker", "claude-code")]);
    let task = self::task(&scene);
    scene.message(
        131,
        (CHIEF, Some(3)),
        "result",
        Some(task),
        "Done.",
        &t(2, 0),
    );
    let conversation = scene.conversation(3, "claude-code", "s-worker");
    scene.item(
        conversation,
        0,
        "user",
        "[ConsensFlow m-120 · T-14 · task from @chief]\nRun them",
        true,
        &t(0, 0),
    );
    scene.item(
        conversation,
        1,
        "assistant",
        "Running the tests.",
        false,
        &t(0, 10),
    );
    scene.item(conversation, 2, "tool", "61 tests pass", true, &t(0, 30));
    let open = unanswered(&scene.look(ms(4, 0)));
    assert!(
        open[0].1.contains("says nothing after its last tool call"),
        "{open:?}"
    );
    // A turn with its final answer is judged, and unresolved in nothing.
    let scene = Scene::new(&[("long-worker", "claude-code")]);
    let (task, _) = worked(&scene);
    scene.message(
        131,
        (CHIEF, Some(3)),
        "result",
        Some(task),
        "Rounding is half-even now, with a test; 61 tests pass.",
        &t(1, 30),
    );
    assert!(unanswered(&scene.look(ms(4, 0))).is_empty());
}

#[test]
fn a_result_whose_own_turn_is_missing_is_never_held_to_an_earlier_turn_of_its_worker() {
    // Sol's: T-14 answered at 10:01, its result m-131; sent back with m-140 at 10:03,
    // the retry's turn never copied, and its result m-141 the old answer again.
    let scene = Scene::new(&[("long-worker", "claude-code")]);
    let (task, conversation) = worked(&scene);
    let answer = "Rounding is half-even now, with a test; 61 tests pass.";
    scene.message(
        131,
        (CHIEF, Some(3)),
        "result",
        Some(task),
        answer,
        &t(1, 30),
    );
    scene.briefed(140, 3, task, &t(3, 0));
    // A delivery of T-14 to another, and one of another task to the worker, began none
    // of its turns.
    scene.briefed(145, CHIEF, task, &t(3, 20));
    let other = scene.task(15, 3, "queued", &t(3, 40));
    scene.briefed(146, 3, other, &t(3, 40));
    scene.message(
        141,
        (CHIEF, Some(3)),
        "result",
        Some(task),
        answer,
        &t(4, 0),
    );
    // A note of T-14's, its delivery begun after the result, began no turn of it.
    scene.message(
        139,
        (3, Some(CHIEF)),
        "note",
        Some(task),
        "Also the totals.",
        &t(2, 50),
    );
    scene.event(
        &t(4, 30),
        "delivery.begun",
        &json!({ "message": 139, "attempt": 1 }),
    );
    let look = scene.look(ms(8, 0));
    assert!(found(&look, 4).is_empty(), "{:?}", found(&look, 4));
    let open = unanswered(&look);
    assert_eq!(open.len(), 1, "{open:?}");
    assert_eq!(open[0].0, "4:m-141");
    assert_eq!(
        open[0].1,
        "m-141's turn is not in the copy of @long-worker's conversation, which holds no prompt of m-140 after its delivery attempt 1 began: whether it is the turn's final answer cannot be judged"
    );
    // Its turn copied: the old answer again is not its final answer.
    scene.item(
        conversation,
        6,
        "user",
        "[ConsensFlow m-140 · T-14 · task from @chief]\nRound the totals too",
        true,
        &t(3, 0),
    );
    scene.item(
        conversation,
        7,
        "assistant",
        "The totals are half-even too; 62 tests pass.",
        true,
        &t(3, 30),
    );
    let look = scene.look(ms(8, 0));
    let breaks = found(&look, 4);
    assert_eq!(breaks.len(), 1, "{breaks:?}");
    assert_eq!(
        breaks[0].said,
        "m-141 is not its turn's final answer, whole"
    );
    assert!(unanswered(&look).is_empty());
}

/// The final answer of [`worked`]'s turn.
const ANSWER: &str = "Rounding is half-even now, with a test; 61 tests pass.";

#[test]
fn a_retried_delivery_whose_turn_was_lost_is_never_held_to_the_first_attempts_turn() {
    // Astra's: m-120 delivered and answered at 10:01; delivered again at 10:03 (its
    // second attempt), the retry's turn never copied, and the result the old answer.
    let scene = Scene::new(&[("long-worker", "claude-code")]);
    let (task, conversation) = worked(&scene);
    scene.event(
        &t(3, 0),
        "delivery.begun",
        &json!({ "message": 120, "attempt": 2 }),
    );
    scene.message(
        131,
        (CHIEF, Some(3)),
        "result",
        Some(task),
        ANSWER,
        &t(4, 0),
    );
    let look = scene.look(ms(8, 0));
    assert!(found(&look, 4).is_empty(), "{:?}", found(&look, 4));
    let open = unanswered(&look);
    assert_eq!(open.len(), 1, "{open:?}");
    assert_eq!(
        open[0].1,
        "m-131's turn is not in the copy of @long-worker's conversation, which holds no prompt of m-120 after its delivery attempt 2 began: whether it is the turn's final answer cannot be judged"
    );
    // The retry's turn copied: the old answer again is not its final answer.
    scene.item(
        conversation,
        6,
        "user",
        "[ConsensFlow m-120 · T-14 · task from @chief]\nFix the rounding",
        true,
        &t(3, 0),
    );
    scene.item(
        conversation,
        7,
        "assistant",
        "Rounding is half-even now in the totals too; 62 tests pass.",
        true,
        &t(3, 30),
    );
    let breaks = found(&scene.look(ms(8, 0)), 4);
    assert_eq!(breaks.len(), 1, "{breaks:?}");
    assert_eq!(
        breaks[0].said,
        "m-131 is not its turn's final answer, whole"
    );
}

#[test]
fn a_late_result_is_held_to_its_own_tasks_turn_and_never_to_the_next_tasks() {
    // Sol's: T-14 answered at 10:01; T-15 delivered (m-140) and answered at 10:02:30;
    // then T-14's late result carries T-15's answer.
    let scene = Scene::new(&[("long-worker", "claude-code")]);
    let (task, conversation) = worked(&scene);
    let next = scene.task(15, 3, "done", &t(2, 0));
    scene.briefed(140, 3, next, &t(2, 0));
    scene.item(
        conversation,
        6,
        "user",
        "[ConsensFlow m-140 · T-15 · task from @chief]\nRound the totals",
        true,
        &t(2, 0),
    );
    let other = "The totals are rounded half-even too; 62 tests pass.";
    scene.item(conversation, 7, "assistant", other, true, &t(2, 30));
    scene.message(150, (CHIEF, Some(3)), "result", Some(task), other, &t(3, 0));
    let look = scene.look(ms(8, 0));
    let breaks: Vec<_> = found(&look, 4)
        .into_iter()
        .filter(|found| found.key == "4:m-150")
        .collect();
    assert_eq!(breaks.len(), 1, "{breaks:?}");
    assert_eq!(
        breaks[0].said,
        "m-150 is not its turn's final answer, whole"
    );
    // Its own answer as its result holds.
    scene.message(
        151,
        (CHIEF, Some(3)),
        "result",
        Some(task),
        ANSWER,
        &t(3, 0),
    );
    let look = scene.look(ms(8, 0));
    assert!(found(&look, 4).iter().all(|found| found.key != "4:m-151"));
}

#[test]
fn a_header_a_prompt_quotes_is_never_the_delivery_that_began_a_turn() {
    // Sol's: m-120 answered at 10:01 and delivered again at 10:03, the retry's turn
    // never copied; then T-15's brief, m-121, quotes m-120's header; and T-14's result
    // carries T-15's answer.
    let scene = Scene::new(&[("long-worker", "claude-code")]);
    let (task, conversation) = worked(&scene);
    scene.event(
        &t(3, 0),
        "delivery.begun",
        &json!({ "message": 120, "attempt": 2 }),
    );
    let next = scene.task(15, 3, "done", &t(3, 30));
    let brief = "Do for the totals what [ConsensFlow m-120 · T-14 · task from @chief] did.";
    scene.message(121, (3, Some(CHIEF)), "task", Some(next), brief, &t(3, 30));
    scene.event(
        &t(3, 30),
        "delivery.begun",
        &json!({ "message": 121, "attempt": 1 }),
    );
    scene.item(
        conversation,
        6,
        "user",
        &format!("[ConsensFlow m-121 · T-15 · task from @chief]\n{brief}"),
        true,
        &t(3, 30),
    );
    let other = "The totals are rounded half-even too; 62 tests pass.";
    scene.item(conversation, 7, "assistant", other, true, &t(3, 50));
    scene.message(131, (CHIEF, Some(3)), "result", Some(task), other, &t(4, 0));
    let look = scene.look(ms(8, 0));
    assert!(found(&look, 4).is_empty(), "{:?}", found(&look, 4));
    assert_eq!(
        unanswered(&look),
        [(
            "4:m-131".to_owned(),
            "m-131's turn is not in the copy of @long-worker's conversation, which holds no prompt of m-120 after its delivery attempt 2 began: whether it is the turn's final answer cannot be judged".to_owned()
        )]
    );
}

#[test]
fn a_header_a_later_prompt_quotes_never_ends_a_turn() {
    // Astra's: T-14 answered at 10:01; then a prompt that delivers nothing to the worker
    // quotes T-15's header (m-140, delivered to another worker), and the worker answers
    // again; T-14's result is the first answer. Its turn ran on to the second.
    let revised = "Rounding is half-even everywhere now, the totals too; 62 tests pass.";
    for prompt in [
        // A notification of a job, whose output holds the header.
        "<task-notification>\n<output>queued: [ConsensFlow m-140 · T-15 · task from @chief]</output>\n</task-notification>",
        // One that opens with it: m-140 is none of this worker's.
        "[ConsensFlow m-140 · T-15 · task from @chief]\nthe brief",
        // A notification that quotes the header of the worker's own next task, m-141: a
        // quote, which delivers nothing, whoever the message is to.
        "<task-notification>\n<output>next: [ConsensFlow m-141 · T-16 · task from @chief]</output>\n</task-notification>",
    ] {
        let scene = Scene::new(&[("long-worker", "claude-code"), ("long-devin-2", "devin")]);
        let (task, conversation) = worked(&scene);
        let next = scene.task(15, 4, "working", &t(1, 20));
        scene.briefed(140, 4, next, &t(1, 20));
        let own = scene.task(16, 3, "queued", &t(1, 25));
        scene.briefed(141, 3, own, &t(1, 25));
        scene.item(conversation, 6, "user", prompt, true, &t(1, 30));
        scene.item(conversation, 7, "assistant", revised, true, &t(2, 0));
        scene.message(
            131,
            (CHIEF, Some(3)),
            "result",
            Some(task),
            ANSWER,
            &t(2, 30),
        );
        let look = scene.look(ms(8, 0));
        let breaks = found(&look, 4);
        assert_eq!(breaks.len(), 1, "{prompt}: {breaks:?}");
        assert_eq!(
            breaks[0].said,
            "m-131 is not its turn's final answer, whole"
        );
    }
}

#[test]
fn a_header_a_person_quotes_in_a_fence_is_never_the_lost_delivery_that_began_a_turn() {
    // GPT-6-Astra's review of round b2: m-120's delivery began and never reached the window;
    // a later message quotes its header on a line of its own, in a fence, and its answer is
    // taken for T-14's result. Its turn is in no copy: unresolved, never held.
    let scene = Scene::new(&[("long-worker", "claude-code")]);
    let task = task(&scene);
    let conversation = scene.conversation(3, "claude-code", "s-worker");
    scene.item(
        conversation,
        0,
        "user",
        "Why is this one stuck?\n```\n[ConsensFlow m-120 · T-14 · task from @chief]\nFix the rounding\n```",
        true,
        &t(0, 30),
    );
    let answer = "It never reached me; I changed nothing.";
    scene.item(conversation, 1, "assistant", answer, true, &t(1, 0));
    scene.message(
        131,
        (CHIEF, Some(3)),
        "result",
        Some(task),
        answer,
        &t(1, 5),
    );
    let look = scene.look(ms(5, 0));
    assert!(found(&look, 4).is_empty(), "{:?}", found(&look, 4));
    assert_eq!(
        unanswered(&look),
        [(
            "4:m-131".to_owned(),
            "m-131's turn is not in the copy of @long-worker's conversation, which holds no prompt of m-120 after its delivery attempt 1 began: whether it is the turn's final answer cannot be judged".to_owned()
        )]
    );
}

/// T-14's worker's turn: the brief, then each of `progress` said before a tool's call, then
/// `answer`; and its result m-131 saying `result`. What the fourth finds of it.
fn progress_then(progress: &[&str], answer: &str, result: &str) -> Vec<Break> {
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
    let mut seq = 1;
    for (at, line) in (10..).zip(progress) {
        scene.item(conversation, seq, "assistant", line, false, &t(0, at));
        scene.item(conversation, seq + 1, "tool", "ok", true, &t(0, at));
        seq += 2;
    }
    scene.item(conversation, seq, "assistant", answer, true, &t(1, 0));
    scene.message(
        131,
        (CHIEF, Some(3)),
        "result",
        Some(task),
        result,
        &t(1, 5),
    );
    found(&scene.look(ms(5, 0)), 4)
}

#[test]
fn a_result_whose_final_answer_its_progress_holds_too_is_never_taken_for_turn_ends_a4() {
    // GPT-6.1-Sol's review of round b3: the final answer `Done.` opens two progress lines, and
    // the result holds those two alone, no final answer of its own. Taken out last, `Done.`
    // came out of the second line and left the result read as turn-end's A4: known, a false
    // PASS. Taken out where it first stands, words are left: a break that fails a run.
    let listed = known::KNOWN.as_deref().unwrap();
    let breaks = progress_then(
        &[
            "Done. Checking the diff carefully.",
            "Done. Reviewing the final changes.",
            "Reviewing the final changes.",
        ],
        "Done.",
        "Done. Checking the diff carefully.\n\nDone. Reviewing the final changes.",
    );
    assert_eq!(breaks.len(), 1, "{breaks:?}");
    assert_eq!(breaks[0].shape, None, "{breaks:?}");
    assert_eq!(known::of(&breaks[0], listed), None);
    // The cost, accepted (the lead's line of round b4): a true A4 whose progress opens with
    // the answer's words leaves words too, and fails a run the lead reads, never the other
    // way.
    let progress = "All 61 tests pass. Checking the diff now.";
    let breaks = progress_then(
        &[progress],
        "All 61 tests pass.",
        &format!("{progress}\n\nAll 61 tests pass."),
    );
    assert_eq!(breaks.len(), 1, "{breaks:?}");
    assert_eq!(
        breaks[0].said,
        "m-131 holds 1 line of its turn's progress, and words that are none of its turn's"
    );
    assert_eq!(breaks[0].shape, None);
}

#[test]
fn a_delivery_claude_code_kept_wrapped_as_a_paste_begins_its_turn() {
    // Claude Code 2.1.303 keeps a long paste wrapped in its record, and the copy with it
    // (the Mac's run of 2026-10-19): the task's delivery is the prompt that opens with its
    // header inside the wrapper, and the result is judged against the turn it began.
    let scene = Scene::new(&[("long-worker", "claude-code")]);
    let task = task(&scene);
    let conversation = scene.conversation(3, "claude-code", "s-worker");
    scene.item(
        conversation,
        0,
        "user",
        "\n\n<pasted_content id=\"a1b2\">\n[ConsensFlow m-120 · T-14 · task from @chief]\nFix the rounding\n</pasted_content id=\"a1b2\">\n",
        true,
        &t(0, 0),
    );
    let progress = "Found it: half-up instead of half-even. Fixing.";
    scene.item(conversation, 1, "assistant", progress, false, &t(0, 30));
    scene.item(
        conversation,
        2,
        "tool",
        "The file src/money.mjs has been updated.",
        true,
        &t(0, 31),
    );
    scene.item(conversation, 3, "assistant", ANSWER, true, &t(1, 0));
    scene.message(
        131,
        (CHIEF, Some(3)),
        "result",
        Some(task),
        &format!("{progress}\n\n{ANSWER}"),
        &t(1, 5),
    );
    let look = scene.look(ms(5, 0));
    assert!(unanswered(&look).is_empty(), "{:?}", unanswered(&look));
    let breaks = found(&look, 4);
    assert_eq!(breaks.len(), 1, "{breaks:?}");
    assert_eq!(breaks[0].said, "m-131 holds 1 line of its turn's progress");
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
    // Turn-end's A4 (field report dip-agentos #5), deferred to 3.0.6: the list of known
    // breaks names this shape.
    assert_eq!(breaks[0].shape, Some("progress-lines"));
    let listed = known::KNOWN.as_deref().unwrap();
    assert!(known::of(&breaks[0], listed).is_some_and(|entry| entry.issue.contains("3.0.6")));

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
    // No part of the known bug: it fails a run.
    assert_eq!(breaks[0].shape, None);
    assert_eq!(known::of(&breaks[0], listed), None);
}

#[test]
fn a_result_of_progress_without_its_whole_final_answer_is_no_known_a4() {
    // GPT-6.1-Sol's review of round b1: the known shape is turn-end's A4 alone, the whole
    // final answer there and nothing besides it but the turn's progress. Progress alone,
    // progress and the answer cut, progress and another task's answer, and progress and
    // the answer with words of no part of the turn each fail a run.
    let listed = known::KNOWN.as_deref().unwrap();
    let progress = "Reading src/money.mjs to find the rounding.";
    for (result, said) in [
        (
            progress.to_owned(),
            "m-131 is not its turn's final answer, whole, and holds 1 line of its turn's progress",
        ),
        (
            format!("{progress}\n\nRounding is half-even now"),
            "m-131 is not its turn's final answer, whole, and holds 1 line of its turn's progress",
        ),
        (
            format!("{progress}\n\nThe totals are rounded half-even too; 62 tests pass."),
            "m-131 is not its turn's final answer, whole, and holds 1 line of its turn's progress",
        ),
        (
            format!("{progress}\n\nThe totals are rounded half-even too.\n\n{ANSWER}"),
            "m-131 holds 1 line of its turn's progress, and words that are none of its turn's",
        ),
    ] {
        let scene = Scene::new(&[("long-worker", "claude-code")]);
        let (task, _) = worked(&scene);
        scene.message(
            131,
            (CHIEF, Some(3)),
            "result",
            Some(task),
            &result,
            &t(1, 5),
        );
        let breaks = found(&scene.look(ms(5, 0)), 4);
        assert_eq!(breaks.len(), 1, "{result}: {breaks:?}");
        assert_eq!(breaks[0].said, said, "{result}");
        assert_eq!(breaks[0].shape, None, "{result}");
        assert_eq!(known::of(&breaks[0], listed), None, "{result}");
    }
    // The shape, where the result is the whole final answer and progress besides, short
    // lines among it too.
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
    scene.item(conversation, 1, "assistant", progress, false, &t(0, 10));
    scene.item(
        conversation,
        2,
        "tool",
        "src/money.mjs: 120 lines",
        true,
        &t(0, 11),
    );
    scene.item(conversation, 3, "assistant", "Fixing.", false, &t(0, 30));
    scene.item(conversation, 4, "tool", "ok", true, &t(0, 31));
    scene.item(conversation, 5, "assistant", ANSWER, true, &t(1, 0));
    scene.message(
        131,
        (CHIEF, Some(3)),
        "result",
        Some(task),
        &format!("{progress}\n\nFixing.\n\n{ANSWER}"),
        &t(1, 5),
    );
    let breaks = found(&scene.look(ms(5, 0)), 4);
    assert_eq!(breaks.len(), 1, "{breaks:?}");
    assert_eq!(breaks[0].shape, Some("progress-lines"));
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
    // Both reviewers': no final answer came, and the result is the commentary. It is
    // never the expected answer: unresolved, and no PASS.
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
    let commentary = "I'll summarise what changed for the chief now.";
    scene.item(conversation, 2, "assistant", commentary, false, &t(0, 50));
    scene.message(
        131,
        (CHIEF, Some(3)),
        "result",
        Some(task),
        commentary,
        &t(1, 5),
    );
    let look = scene.look(ms(5, 0));
    assert!(found(&look, 4).is_empty());
    let open = unanswered(&look);
    assert_eq!(open.len(), 1, "{open:?}");
    assert_eq!(
        open[0].1,
        "m-131's turn has no final answer after its last tool call in the copy of @long-worker's conversation, only Codex's commentary, which it never marks complete: whether it is the turn's final answer cannot be judged"
    );
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

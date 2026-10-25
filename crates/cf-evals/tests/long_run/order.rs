//! 3 (A3): no work is ended under a task at work, as the note of a window's close says
//! it under 3.0.6 (turn-end); 4 (A4): a result that is not its turn's final answer.

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

/// What turn-end's note of a window's close says (`tell_left_running`): written once the
/// task has ended, to the one who gave it.
const CLOSED: &str = "T-14's window closed with background work running, which its close ended: npx stryker run --concurrency 4";

/// The pause note of a window that closed under a working task: a pause, no ending.
const PAUSED: &str = "T-14 is paused: @long-worker's window closed with background work running, which its close ended: node serve.mjs. Resume it with: cf task resume T-14 \"…\"";

/// What 3.0.5 said when a turn's result was taken, which 3.0.6 never writes.
const ENDED: &str =
    "T-14's turn left background work running. Ended: npx stryker run --concurrency 4.";

/// T-14 moved from `from` to `to` at `at`, as the ledger logs a move.
fn moved(scene: &Scene, at: &str, from: &str, to: &str) {
    scene.event(
        at,
        "task.state",
        &json!({ "task": 14, "from": from, "to": to }),
    );
}

/// The close note m-132, written to the chief for T-14 at `at`.
fn closed(scene: &Scene, task: i64, at: &str) {
    scene.message(132, (CHIEF, None), "note", Some(task), CLOSED, at);
}

/// What the look could not judge of the third: the keys of the notes the log ties
/// to neither attempt.
fn unbound(look: &Look) -> Vec<String> {
    invariants::unjudged(look, &Default::default())
        .into_iter()
        .filter(|open| open.invariant == 3)
        .map(|open| open.key)
        .collect()
}

/// The breaks of the third at `at`.
fn broke(scene: &Scene, at: i64) -> Vec<Break> {
    found(&scene.look(at), 3)
}

#[test]
fn a_close_note_after_its_tasks_end_holds_the_third_whichever_way_the_task_ended() {
    // A result (done, then accepted), a Finish (done with no result of the worker's), a
    // cancel and a failure each end a task; a cancelled or failed one has a close note and
    // no result, so the note must not be taken for one with none before it.
    for (end, result) in [
        ("done", true),
        ("accepted", true),
        ("done", false),
        ("cancelled", false),
        ("failed", false),
    ] {
        let scene = Scene::new(&[("long-worker", "claude-code")]);
        let task = task(&scene);
        moved(&scene, &t(0, 10), "queued", "working");
        if result {
            scene.message(
                131,
                (CHIEF, Some(3)),
                "result",
                Some(task),
                "Done.",
                &t(2, 0),
            );
        }
        moved(&scene, &t(2, 0), "working", end);
        closed(&scene, task, &t(3, 0));
        let look = scene.look(ms(4, 0));
        assert!(found(&look, 3).is_empty(), "{end} {result}");
        assert!(unbound(&look).is_empty(), "{end} {result}");
    }
}

#[test]
fn a_close_note_while_its_task_is_still_at_work_breaks_the_third() {
    // Work ended under a task that was not done: the mutation run ended mid-edit (T-14's
    // m-132) A3 was written for, as 3.0.6 can still do it, by a window's close.
    for state in ["queued", "working", "waiting"] {
        let scene = Scene::new(&[("long-worker", "claude-code")]);
        let task = scene.task(14, 3, state, &t(0, 10));
        scene.briefed(120, 3, task, &t(0, 0));
        moved(&scene, &t(0, 10), "queued", state);
        closed(&scene, task, &t(1, 0));
        let breaks = broke(&scene, ms(1, 5));
        assert_eq!(breaks.len(), 1, "{state}: {breaks:?}");
        assert_eq!(breaks[0].key, "3:m-132");
        assert_eq!(
            breaks[0].said,
            format!("T-14's close note m-132 came while T-14 was still {state}: the close ended work under a task at work")
        );
        assert_eq!(breaks[0].shape, Some(invariants::CLOSE_NOTE));
        assert_eq!(breaks[0].evidence.rows.len(), 2);
        assert!(breaks[0].evidence.rows[0].starts_with("m-132 note"));
        assert_eq!(
            breaks[0].evidence.rows[1],
            format!("T-14 {state} since {}", t(0, 10))
        );
        assert!(unbound(&scene.look(ms(1, 5))).is_empty());
        // And at every look after, the task done since: the note is judged as it was written.
        moved(&scene, &t(2, 0), state, "done");
        assert_eq!(broke(&scene, ms(6, 0)).len(), 1, "{state}");
    }
}

#[test]
fn what_is_no_close_note_is_never_one_the_pause_note_and_the_3_0_5_notes_of_a_turns_end() {
    // A pause, a 3.0.5 "ended" note and a note of no task's close, each while T-14 works.
    let scene = Scene::new(&[("long-worker", "claude-code")]);
    let task = scene.task(14, 3, "working", &t(0, 10));
    scene.briefed(120, 3, task, &t(0, 0));
    moved(&scene, &t(0, 10), "queued", "working");
    scene.message(130, (CHIEF, None), "note", Some(task), PAUSED, &t(1, 0));
    scene.message(131, (CHIEF, None), "note", Some(task), ENDED, &t(1, 1));
    scene.message(
        133,
        (CHIEF, None),
        "note",
        Some(task),
        "T-14 is let go: its job is gone.",
        &t(1, 2),
    );
    // A worker's own words that say the same are no ConsensFlow's note.
    scene.message(134, (CHIEF, Some(3)), "note", Some(task), CLOSED, &t(1, 3));
    // And a ConsensFlow message of another kind that says it is none.
    scene.message(135, (CHIEF, None), "question", Some(task), CLOSED, &t(1, 4));
    let look = scene.look(ms(2, 0));
    assert!(found(&look, 3).is_empty(), "{:?}", found(&look, 3));
    assert!(unbound(&look).is_empty());
}

#[test]
fn a_close_note_of_a_task_that_is_paused_or_open_is_neither_a_break_nor_an_ending() {
    for state in ["paused", "open"] {
        let scene = Scene::new(&[("long-worker", "claude-code")]);
        let task = scene.task(14, 3, state, &t(0, 20));
        scene.briefed(120, 3, task, &t(0, 0));
        moved(&scene, &t(0, 10), "queued", "working");
        moved(&scene, &t(0, 20), "working", state);
        closed(&scene, task, &t(1, 0));
        let look = scene.look(ms(2, 0));
        assert!(found(&look, 3).is_empty(), "{state}");
        assert!(unbound(&look).is_empty(), "{state}");
    }
}

#[test]
fn a_close_note_of_a_task_that_was_given_to_another_worker_since_is_neither() {
    // Whose window a close note is the log does not say: the earlier worker's closes under
    // the new worker's work, which is none of its ending. The same task, given to one
    // worker only, is the break.
    let given = |reassigned: bool| {
        let scene = Scene::new(&[("long-worker", "claude-code")]);
        let task = scene.task(14, 3, "working", &t(1, 0));
        scene.briefed(120, 3, task, &t(0, 0));
        scene.event(
            &t(0, 5),
            "task.created",
            &json!({ "task": 14, "from": "chief", "to": "long-worker", "message": 120 }),
        );
        moved(&scene, &t(0, 10), "queued", "working");
        if reassigned {
            scene.event(
                &t(0, 40),
                "task.assigned",
                &json!({ "task": 14, "from": "working", "to": "queued", "assignee": "long-worker-2", "member": "long-worker-2", "message": 121 }),
            );
            moved(&scene, &t(1, 0), "queued", "working");
        }
        closed(&scene, task, &t(1, 30));
        (broke(&scene, ms(2, 0)), unbound(&scene.look(ms(2, 0))))
    };
    let (breaks, open) = given(true);
    assert!(breaks.is_empty() && open.is_empty(), "{breaks:?}");
    let (breaks, open) = given(false);
    assert_eq!(breaks.len(), 1, "{breaks:?}");
    assert!(open.is_empty());
}

/// T-14 sent back for more work at `at`: it leaves done, and an attempt begins.
fn sent_back(scene: &Scene, at: &str) {
    moved(scene, at, "done", "queued");
}

#[test]
fn a_close_note_after_a_send_back_is_unresolved_however_near_the_next_result() {
    // The reviewers' sequence: the old result at 0:30, T-14 sent back at 0:40 and at
    // work at 0:45, a close note at 1:00, the next attempt's result at 1:01. The note
    // may be the old attempt's window, closed late, or the new one's own: no break, and
    // not taken to hold, at every look after.
    let scene = Scene::new(&[("long-worker", "claude-code")]);
    let task = task(&scene);
    moved(&scene, &t(0, 10), "queued", "working");
    scene.message(
        100,
        (CHIEF, Some(3)),
        "result",
        Some(task),
        "First round.",
        &t(0, 30),
    );
    moved(&scene, &t(0, 30), "working", "done");
    sent_back(&scene, &t(0, 40));
    moved(&scene, &t(0, 45), "queued", "working");
    scene.message(101, (CHIEF, None), "note", Some(task), CLOSED, &t(1, 0));
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
    moved(&scene, &t(1, 1), "working", "done");
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
        "T-14's close note m-101 came while T-14 was working again, after it had ended and been sent back: whether it is the earlier attempt's window's, closed late, or the new attempt's own, the log does not say"
    );
}

#[test]
fn a_close_note_written_before_the_send_back_holds_whatever_comes_after() {
    // The close note m-131 of the first attempt's window, written before T-14 was sent back,
    // and the next attempt's result m-132 after: it holds, as the task had ended.
    let scene = Scene::new(&[("long-worker", "claude-code")]);
    let task = self::task(&scene);
    moved(&scene, &t(0, 10), "queued", "working");
    scene.message(
        100,
        (CHIEF, Some(3)),
        "result",
        Some(task),
        "First round.",
        &t(0, 30),
    );
    moved(&scene, &t(0, 30), "working", "done");
    scene.message(131, (CHIEF, None), "note", Some(task), CLOSED, &t(0, 31));
    sent_back(&scene, &t(1, 0));
    moved(&scene, &t(1, 5), "queued", "working");
    scene.message(
        132,
        (CHIEF, Some(3)),
        "result",
        Some(task),
        "Done.",
        &t(2, 0),
    );
    moved(&scene, &t(2, 0), "working", "done");
    let look = scene.look(ms(4, 0));
    assert!(found(&look, 3).is_empty());
    assert!(unbound(&look).is_empty());
    // A send-back the log puts at the note's own moment may be before it.
    let scene = Scene::new(&[("long-worker", "claude-code")]);
    let task = self::task(&scene);
    moved(&scene, &t(0, 10), "queued", "working");
    scene.message(
        100,
        (CHIEF, Some(3)),
        "result",
        Some(task),
        "First round.",
        &t(0, 30),
    );
    moved(&scene, &t(0, 30), "working", "done");
    sent_back(&scene, &t(0, 31));
    moved(&scene, &t(0, 31), "queued", "working");
    scene.message(131, (CHIEF, None), "note", Some(task), CLOSED, &t(0, 31));
    let look = scene.look(ms(4, 0));
    assert!(found(&look, 3).is_empty());
    assert_eq!(unbound(&look), ["3:m-131"]);
}

#[test]
fn a_close_notes_row_is_masked_whole_before_it_is_cut() {
    // The secret's closing quote is past the 200 characters a row keeps of a body.
    let scene = Scene::new(&[("long-worker", "claude-code")]);
    let task = scene.task(14, 3, "working", &t(0, 10));
    scene.briefed(120, 3, task, &t(0, 0));
    moved(&scene, &t(0, 10), "queued", "working");
    let body = format!(
        "{CLOSED}; {}password=\"correct horse battery staple\" in the log.",
        "node serve.mjs; ".repeat(4)
    );
    scene.message(132, (CHIEF, None), "note", Some(task), &body, &t(1, 0));
    let look = scene.look(ms(1, 5));
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

#[test]
fn a_header_inside_an_attribute_of_a_paste_wrapper_is_never_the_lost_delivery_that_began_a_turn() {
    // GPT-6-Astra's review of round b4: m-120's delivery began and never reached the window;
    // a later row opens with a `<pasted_content` tag whose attribute holds a `>` and m-120's
    // header. The tag is not Claude Code's shape (a `>` ends it only after its attributes, and
    // none holds a `>`): no wrapper, and the row opens with no header. Unresolved, never held.
    let scene = Scene::new(&[("long-worker", "claude-code")]);
    let task = task(&scene);
    let conversation = scene.conversation(3, "claude-code", "s-worker");
    scene.item(
        conversation,
        0,
        "user",
        "<pasted_content note=\">[ConsensFlow m-120 · T-14 · task from @chief]\">quoted metadata</pasted_content>",
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
    // PASS. Since round b5 a result whose progress lines hold its final answer is no
    // turn-end's A4 at all: a break that fails a run, whichever occurrence is taken out.
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
    // The cost, accepted (the lead's line of round b4): a true A4 whose progress line holds
    // the answer's words is read the same, and fails a run the lead reads, never the other
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
        "m-131 holds 1 line of its turn's progress; its turn's final answer stands inside a progress line it holds"
    );
    assert_eq!(breaks[0].shape, None);
}

#[test]
fn a_result_whose_progress_lines_hold_its_final_answer_is_never_taken_for_turn_ends_a4() {
    // Both reviewers of round b4, as given: the final answer `Done.` stands inside the two
    // progress lines the result holds, and a third line is the rest of the first. Taken out
    // where it first stands, then the progress lines, nothing was left: known, a false PASS.
    // A result whose progress lines hold the answer is no turn-end's A4: a break that fails a
    // run.
    let listed = known::KNOWN.as_deref().unwrap();
    let breaks = progress_then(
        &[
            "Done. Checking the diff carefully.",
            "Done. Reviewing the final changes.",
            "Checking the diff carefully.",
        ],
        "Done.",
        "Done. Checking the diff carefully.\n\nDone. Reviewing the final changes.",
    );
    assert_eq!(breaks.len(), 1, "{breaks:?}");
    assert_eq!(
        breaks[0].said,
        "m-131 holds 3 lines of its turn's progress; its turn's final answer stands inside a progress line it holds"
    );
    assert_eq!(breaks[0].shape, None, "{breaks:?}");
    assert_eq!(known::of(&breaks[0], listed), None);
    // A line of progress too short to be told as held is a line the result holds all the
    // same.
    let breaks = progress_then(
        &[
            "Done. Ok now.",
            "Ok now.",
            "Reviewing the final changes now.",
        ],
        "Done.",
        "Done. Ok now.\n\nReviewing the final changes now.",
    );
    assert_eq!(breaks.len(), 1, "{breaks:?}");
    assert_eq!(breaks[0].shape, None, "{breaks:?}");
    assert_eq!(known::of(&breaks[0], listed), None);
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

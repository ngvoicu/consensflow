//! A turn its provider refused for a moment (T-1112, Gabriel's field report: a designer's
//! Codex window ended on "Selected model is at capacity", and its task failed with its work
//! saved): a model overloaded or at capacity, a server error, is no failure. The task is held
//! with its window and tried again after a backoff, a minute, then two, then four; once those
//! tries are refused too it goes to another free member of its tier, or is held on and tried
//! every five minutes while none is free. Its requester is told once, plainly. A real
//! failure still fails, and a chief's busy turn is still the human's to hear.
//!
//! Each harness's own shape of the refusal is read by its reader (`cf-harness`'s tests of
//! each, from the recorded lines); `every_harness_s_busy_refusal_holds_its_task` gives the
//! engine what each reader reads of it.

use cf_engine::testing::Context;
use cf_engine::ActivityState;
use cf_harness::records::Quota;

use crate::background::{results, working};
use crate::fixtures::{after, assert_match, now, Tiers};

/// Codex's words, as its rollout recorded them (`errored-task-complete.jsonl`).
const AT_CAPACITY: &str = "Selected model is at capacity. Please try a different model.";

/// What the requester is told, once, of T-1's busy model, given by tier.
const TOLD: &str = r"^T-1 waits with @zeus-amber-pine: its model is busy, a refusal of its provider for a moment and no failure \(Selected model is at capacity\. Please try a different model\.\)\. ConsensFlow tries it again in 1, 2 and 4 minutes, then gives it to another free member of its tier, or tries every 5 minutes while none is free; you are not told of each try\.$";

/// A refusal of the provider for a moment, now, in `words`.
fn busy(context: &Context, words: &str) -> Quota {
    Quota::Busy {
        at: Some(now(context)),
        because: words.to_owned(),
    }
}

/// `handle`'s window is refused for a moment, and looked at twice: the second look reads
/// the same refusal, which is no second try.
fn refused(context: &Context, handle: &str) {
    context.adapter.refuse(handle, busy(context, AT_CAPACITY));
    context.pass().unwrap();
    context.pass().unwrap();
}

/// The notes the chief was given of T-1's busy model.
fn told_busy(tiers: &Tiers<'_>) -> Vec<String> {
    tiers
        .notes("chief")
        .into_iter()
        .filter(|note| note.contains("its model is busy"))
        .collect()
}

#[test]
fn a_busy_model_holds_the_task_with_its_window_and_tries_it_again_after_a_minute() {
    let context = Context::new();
    let tiers = working(&context);
    let tried_at = after(&context, 60_000);
    refused(&context, "zeus");

    let held = tiers.task(1).task;
    assert_eq!(
        (
            held.state.as_str(),
            held.assignee.as_deref(),
            held.held_until.as_deref()
        ),
        ("paused", Some("zeus-amber-pine"), Some(tried_at.as_str())),
        "held a minute with its window, not failed"
    );
    assert_eq!(
        tiers.out_until("zeus").as_deref(),
        Some(tried_at.as_str()),
        "its member takes no other work meanwhile"
    );
    let told = told_busy(&tiers);
    assert_eq!(told.len(), 1, "{:?}", tiers.notes("chief"));
    assert_match(&told[0], TOLD);
    assert!(
        tiers.notes("human").is_empty(),
        "nothing for the human to do: {:?}",
        tiers.notes("human")
    );

    // A minute on, it is tried again in its window, with the words of a Resume, and its
    // answer is its result.
    context.advance(61_000);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working");
    let zeus = tiers.id("zeus-amber-pine");
    let resumed = &context.ledger.borrow().inbox(zeus, 100).unwrap()[0];
    assert_match(&resumed.body, r"^Resumed: Go on where you stopped\.$");
    context.adapter.answer("zeus", "Parser done");
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "done");
    assert_eq!(results(&context, &tiers).len(), 1);
    assert_eq!(
        told_busy(&tiers).len(),
        1,
        "told once: {:?}",
        tiers.notes("chief")
    );
    assert!(
        !tiers
            .notes("chief")
            .iter()
            .any(|note| note.contains("failed")),
        "{:?}",
        tiers.notes("chief")
    );
}

#[test]
fn each_try_waits_longer_and_once_three_are_refused_the_task_goes_to_another_free_member() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus", "diana"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.assignee(1), "zeus-amber-pine");
    for wait in [60_000, 120_000, 240_000] {
        let tried_at = after(&context, wait);
        refused(&context, "zeus");
        let held = tiers.task(1).task;
        assert_eq!(
            (
                held.state.as_str(),
                held.assignee.as_deref(),
                held.held_until.as_deref()
            ),
            ("paused", Some("zeus-amber-pine"), Some(tried_at.as_str())),
            "held {wait} ms with its window, though diana is free"
        );
        context.advance(wait + 1_000);
        context.pass().unwrap();
        context.pass().unwrap();
        assert_eq!(tiers.task(1).task.state, "working", "tried again");
    }
    // The fourth refusal: three tries were refused too, and diana is free.
    refused(&context, "zeus");
    context.pass().unwrap();
    let task = tiers.task(1).task;
    assert!(
        task.assignee
            .as_deref()
            .is_some_and(|who| who.starts_with("diana")),
        "{task:?}"
    );
    assert_ne!(task.state, "failed");
    assert_eq!(
        told_busy(&tiers).len(),
        1,
        "told once of the wait, whatever the tries: {:?}",
        tiers.notes("chief")
    );
}

#[test]
fn with_nobody_else_free_the_task_is_held_on_and_tried_every_five_minutes() {
    let context = Context::new();
    let tiers = working(&context);
    for wait in [60_000, 120_000, 240_000] {
        refused(&context, "zeus");
        context.advance(wait + 1_000);
        context.pass().unwrap();
        context.pass().unwrap();
    }
    for _ in 0..2 {
        let tried_at = after(&context, 5 * 60_000);
        refused(&context, "zeus");
        let held = tiers.task(1).task;
        assert_eq!(
            (
                held.state.as_str(),
                held.assignee.as_deref(),
                held.held_until.as_deref()
            ),
            ("paused", Some("zeus-amber-pine"), Some(tried_at.as_str())),
        );
        context.advance(5 * 60_000 + 1_000);
        context.pass().unwrap();
        context.pass().unwrap();
    }
    assert_eq!(told_busy(&tiers).len(), 1, "{:?}", tiers.notes("chief"));
}

#[test]
fn the_lane_and_the_board_say_the_model_is_busy_not_out_of_quota() {
    let context = Context::new();
    let tiers = working(&context);
    let tried_at = after(&context, 60_000);
    refused(&context, "zeus");
    let activity = context.dispatcher.activity(tiers.id("zeus-amber-pine"));
    assert_eq!(activity.state, ActivityState::Out);
    assert_eq!(
        activity.reason.as_deref(),
        Some(format!("model busy, tried again at {tried_at}: {AT_CAPACITY}").as_str())
    );
    // Another task of its tier waits while zeus, the only one, is busy.
    tiers.open_body("Write the lexer");
    context.pass().unwrap();
    let waits: Vec<String> = tiers
        .notes("chief")
        .into_iter()
        .filter(|note| note.starts_with("T-2 waits for a free"))
        .collect();
    assert_eq!(waits.len(), 1, "{:?}", tiers.notes("chief"));
    assert!(
        waits[0].contains(&format!(
            "@zeus's model is busy (ConsensFlow tries it again at {tried_at})"
        )),
        "{waits:?}"
    );
}

#[test]
fn a_note_withdrawn_before_it_was_given_is_told_again_at_the_next_try() {
    // The chief is at work, so the note waits in its queue; the resume a minute on withdraws
    // it, as every note of the wait, and the next refusal tells it again.
    let context = Context::new();
    let tiers = working(&context);
    context.adapter.busy("chief");
    refused(&context, "zeus");
    let chief = tiers.id("chief");
    let queued = |state: &str| {
        context
            .ledger
            .borrow()
            .inbox(chief, 100)
            .unwrap()
            .into_iter()
            .filter(|message| message.body.contains("its model is busy") && message.state == state)
            .count()
    };
    assert_eq!(queued("queued"), 1);
    context.advance(61_000);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(
        (queued("queued"), queued("cancelled")),
        (0, 1),
        "withdrawn by the resume"
    );
    refused(&context, "zeus");
    assert_eq!(queued("queued"), 1, "told again: nobody was told");
}

#[test]
fn a_task_given_by_name_stays_with_its_member_whoever_else_is_free() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus", "diana"]);
    context.give(tiers.project.id, "zeus", "Write the parser");
    context.pass().unwrap();
    context.pass().unwrap();
    let zeus = tiers.assignee(1);
    for wait in [60_000, 120_000, 240_000, 5 * 60_000] {
        refused(&context, "zeus");
        assert_eq!(tiers.task(1).task.state, "paused");
        context.advance(wait + 1_000);
        context.pass().unwrap();
        context.pass().unwrap();
    }
    refused(&context, "zeus");
    let task = tiers.task(1).task;
    assert_eq!(
        (task.state.as_str(), task.assignee.as_deref()),
        ("paused", Some(zeus.as_str()))
    );
    let told = told_busy(&tiers);
    assert_eq!(told.len(), 1, "{told:?}");
    assert!(told[0].ends_with(
        "ConsensFlow tries it again in 1, 2 and 4 minutes, then every 5 minutes; you are not told of each try."
    ));
}

#[test]
fn a_turn_through_after_a_try_begins_the_tries_again() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus", "diana"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    for wait in [60_000, 120_000] {
        refused(&context, "zeus");
        context.advance(wait + 1_000);
        context.pass().unwrap();
        context.pass().unwrap();
    }
    // The second try gets a turn through: the model answers again.
    context
        .adapter
        .writes("zeus", "Parsing the grammar", &now(&context));
    context.adapter.with("zeus", |agent| {
        agent.items.last_mut().unwrap().complete = true
    });
    context.pass().unwrap();
    // Refused again later: a minute, as at the first.
    context.advance(10 * 60_000);
    let tried_at = after(&context, 60_000);
    refused(&context, "zeus");
    let held = tiers.task(1).task;
    assert_eq!(
        (held.state.as_str(), held.held_until.as_deref()),
        ("paused", Some(tried_at.as_str()))
    );
}

#[test]
fn a_real_failure_still_fails_its_task() {
    let context = Context::new();
    let tiers = working(&context);
    context.adapter.with("zeus", |agent| {
        agent.failed = true;
        agent.failure = Some("model 'gpt-0-nonesuch' not found".to_owned());
    });
    context.adapter.answer("zeus", "Starting on the parser");
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "failed");
    assert!(told_busy(&tiers).is_empty());
}

#[test]
fn a_chiefs_busy_turn_is_the_humans_to_hear_and_holds_nothing() {
    let context = Context::new();
    let tiers = working(&context);
    context.adapter.refuse("chief", busy(&context, AT_CAPACITY));
    context.pass().unwrap();
    context.pass().unwrap();
    let chief = tiers.participant("chief").unwrap();
    assert_eq!(chief.out_until, None, "the chief is not marked out");
    let human = tiers.notes("human");
    assert_eq!(human.len(), 1, "{human:?}");
    assert_match(
        &human[0],
        r"^The chief's turn failed: @.*'s harness reported a failure",
    );
    assert_eq!(tiers.task(1).task.state, "working", "zeus's task goes on");
}

/// The words each harness's reader says of its busy refusal, as its own reader test reads
/// them, and where the shape comes from: a refusal for a moment, `Quota::Busy`, for each.
const SHAPES: [(&str, &str); 5] = [
    // Claude Code: an API error message whose `error` is `server_error` (repeated 529s), its
    // words as Claude Code 2.1.316's binary writes them.
    (
        "claude",
        "API Error: Repeated 529 Overloaded errors. The API is at capacity \u{2014} this is usually temporary. Try again in a moment.",
    ),
    // Codex: `task_complete`'s error, `codex_error_info` `server_overloaded`: recorded
    // (`errored-task-complete.jsonl`, codex-cli 0.153.4).
    ("codex", AT_CAPACITY),
    // Pi: an assistant message stopped on `error`, the Anthropic SDK's status then body, after
    // Pi's own retries (pi-ai's `utils/retry.js` takes it for retryable).
    (
        "pi",
        r#"529 {"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#,
    ),
    // Devin: the turn's end, cause `error`, in `errorMessage`. Provisional: no Devin record of
    // a server's error is on file; the words are the shared ones.
    ("devin", "Model provider error: 529 overloaded_error: Overloaded"),
    // Muse: a run that failed, in the shape of its recorded 402 (`API error <status>
    // [request_id=…]: <message> (<kind>)`, T-705) with a server's status. Provisional.
    (
        "muse",
        "API error 503 [request_id=req_1]: The service is temporarily unavailable (server_error)",
    ),
];

#[test]
fn every_harness_s_busy_refusal_holds_its_task() {
    for (harness, words) in SHAPES {
        let context = Context::new();
        let tiers = working(&context);
        context.adapter.refuse("zeus", busy(&context, words));
        context.pass().unwrap();
        context.pass().unwrap();
        let task = tiers.task(1).task;
        assert_eq!(task.state, "paused", "{harness}");
        let told = told_busy(&tiers);
        assert_eq!(told.len(), 1, "{harness}: {:?}", tiers.notes("chief"));
        assert!(
            told[0].contains(&format!("({words})")),
            "{harness}: {told:?}"
        );
    }
}

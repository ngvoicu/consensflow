//! A worker's turn that ends on a question to the chief is a question, not a result
//! (T-1119, from Gabriel's field report of 2026-11-02).
//!
//! Since 3.0.7-alpha.1 a turn's end is the task's result. Two workers on another board
//! ended theirs with a question in their words, not with `cf ask`: "Should I continue with
//! the live A/B runs and the packet/socket capture, or stop here so you can review the
//! findings first?", and "Question for you: has the Build Tools 2022 install … finished? …
//! I'm waiting for your answer." Both reached the chief as results, "Decide with: cf task
//! accept | cf task reopen", which `cf answer` cannot reach; the second task showed done
//! with none of its work, and accepting it would have closed that work and released the
//! tasks that wait for it.
//!
//! Now such a final answer is put to the chief as the question it is, from the worker, as
//! `cf ask` puts one: the task waits, the window stays open, and the chief's `cf answer`
//! reaches the window. The rule that tells one (`turn_end::asks`) reads the last paragraph
//! of the worker's own words only: a first-person question about what to do next, or a word
//! that the worker waits for the chief. A report that ends with an open question about its
//! product is a result, and so is a wait for something else. The rule is on the words of
//! the final answer, whatever harness wrote them, and for every role that finishes with one.

use cf_engine::testing::Context;
use cf_harness::records::Role;
use serde_json::json;

use crate::background::{results, working};
use crate::fixtures::{closed, given, Tiers};

/// The field report's m-383, as the ledger it came from can no longer be shown.
const SHOULD_I_CONTINUE: &str = "Should I continue with the live A/B runs and the packet/socket capture, or stop here so you can review the findings first?";

/// The field report's m-387.
const WAITING_FOR_THE_INSTALL: &str =
    "Question for you: has the Build Tools 2022 install finished? I'm waiting for your answer.";

/// What the chief was put as questions: who asked, and the words.
fn questions(context: &Context, tiers: &Tiers<'_>) -> Vec<(String, String)> {
    let mut asked: Vec<(String, String)> = context
        .inbox(tiers.id("chief"))
        .into_iter()
        .filter(|message| message.kind == "question")
        .map(|message| (message.sender.unwrap_or_default(), message.body))
        .collect();
    asked.reverse();
    asked
}

/// T-1 at zeus, whose agent ends its turn with `words` for its answer: the pass that
/// looks at it is made.
fn ended_on<'a>(context: &'a Context, words: &str) -> Tiers<'a> {
    let tiers = working(context);
    context.adapter.answer("zeus", words);
    context.pass().unwrap();
    tiers
}

/// The field's first report: the work is half done, and the worker offers a choice.
#[test]
fn a_turn_that_ends_asking_whether_to_go_on_is_a_question_to_the_chief_and_its_task_waits() {
    let context = Context::new();
    let words =
        format!("Both hosts are measured; the A/B runs are half done.\n\n{SHOULD_I_CONTINUE}");
    let tiers = ended_on(&context, &words);

    assert_eq!(tiers.task(1).task.state, "waiting", "not done");
    assert_eq!(results(&context, &tiers), Vec::<String>::new());
    assert_eq!(
        questions(&context, &tiers),
        [(tiers.assignee(1), words)],
        "the worker's words, whole, from the worker"
    );
}

/// The field's second report: nothing of the work was done, and the worker waits.
#[test]
fn a_turn_that_ends_saying_it_waits_for_the_chiefs_answer_is_a_question_and_its_task_waits() {
    let context = Context::new();
    let tiers = ended_on(&context, WAITING_FOR_THE_INSTALL);

    assert_eq!(tiers.task(1).task.state, "waiting", "not done");
    assert_eq!(results(&context, &tiers), Vec::<String>::new());
    assert_eq!(
        questions(&context, &tiers),
        [(tiers.assignee(1), WAITING_FOR_THE_INSTALL.to_owned())]
    );
}

/// What the chief does with it: it answers with `cf answer`, which reaches the window.
#[test]
fn the_chief_answers_it_the_window_stayed_open_and_the_next_answer_is_the_result() {
    let context = Context::new();
    let tiers = ended_on(&context, SHOULD_I_CONTINUE);
    let zeus_pane = context.host.last("zeus").expect("zeus's window").pane;
    assert!(
        !closed(&context, &zeus_pane),
        "a task that waits keeps its window"
    );
    let asked = context
        .inbox(tiers.id("chief"))
        .into_iter()
        .find(|message| message.kind == "question")
        .expect("the question");

    let chief = tiers.id("chief");
    context
        .ledger
        .borrow_mut()
        .answer(asked.id, chief, Some(&json!("Stop here.")), None)
        .expect("the chief's answer");
    context.pass().unwrap();
    context.pass().unwrap();
    assert!(
        given(&context, "zeus")
            .last()
            .is_some_and(|text| text.contains("Stop here.")),
        "the answer went into the window"
    );
    assert_eq!(tiers.task(1).task.state, "working");

    context
        .adapter
        .answer("zeus", "Stopped; the findings are in the report.");
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "done");
    assert_eq!(
        results(&context, &tiers),
        ["Stopped; the findings are in the report."],
        "the answer to the answer is the result"
    );
}

/// The turn is looked at again and again while the chief has not answered: one question.
#[test]
fn a_question_raised_from_a_turns_end_is_raised_once() {
    let context = Context::new();
    let tiers = ended_on(&context, WAITING_FOR_THE_INSTALL);
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(questions(&context, &tiers).len(), 1);
    assert_eq!(tiers.task(1).task.state, "waiting");
}

/// The words that tell are those of the final answer, however the harness wrote it.
#[test]
fn a_question_in_the_final_answer_counts_whatever_the_harness_wrote_before_it() {
    let context = Context::new();
    let tiers = working(&context);
    // Progress that is no answer, in the shape each harness has it, and a note of Codex's
    // that is marked as its commentary: none of them is the final answer.
    let items: Vec<_> = [
        (Role::Assistant, "Reading the capture.", false, false),
        (Role::Tool, "output of a tool", true, false),
        (
            Role::Assistant,
            "Checking the install; nothing to ask yet.",
            false,
            true,
        ),
    ]
    .into_iter()
    .map(|(role, text, complete, commentary)| {
        let mut item = context.adapter.item(role, text);
        item.complete = complete;
        item.commentary = commentary;
        item
    })
    .collect();
    context
        .adapter
        .with("zeus", |agent| agent.items.extend(items));
    context.adapter.answer("zeus", SHOULD_I_CONTINUE);
    context.pass().unwrap();

    assert_eq!(tiers.task(1).task.state, "waiting");
    assert_eq!(
        questions(&context, &tiers),
        [(tiers.assignee(1), SHOULD_I_CONTINUE.to_owned())]
    );
}

/// A question or a wait in the progress, with a plain report to end the turn: a result.
#[test]
fn a_question_that_is_not_in_the_final_answer_leaves_it_a_result() {
    let context = Context::new();
    let tiers = working(&context);
    let mut progress = context.adapter.item(
        Role::Assistant,
        "Should I continue? I'm waiting for your answer.",
    );
    progress.complete = false;
    let mut commentary = context.adapter.item(
        Role::Assistant,
        "Waiting for your go would be silly; going on.",
    );
    commentary.complete = false;
    commentary.commentary = true;
    let tool = || context.adapter.item(Role::Tool, "output of a tool");
    context.adapter.with("zeus", |agent| {
        agent.items.extend([progress, tool(), commentary, tool()])
    });
    context
        .adapter
        .answer("zeus", "Both hosts are measured; the A/B runs are done.");
    context.pass().unwrap();

    assert_eq!(tiers.task(1).task.state, "done");
    assert_eq!(questions(&context, &tiers), Vec::<(String, String)>::new());
    assert_eq!(
        results(&context, &tiers),
        ["Both hosts are measured; the A/B runs are done."]
    );
}

/// Every role finishes a task with its last answer, and the rule is not the worker's alone.
#[test]
fn a_reviewers_turn_that_ends_waiting_for_the_chief_is_a_question_too() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    tiers.open_for("reviewer", "standard", "Review the parser");
    context.pass().unwrap();
    context.pass().unwrap();
    let reviewer = tiers.assignee(1);
    assert_eq!(tiers.task(1).task.state, "working");

    let words = "The diff is read. I'm waiting for your go before I run the plants.";
    context.adapter.answer(&reviewer, words);
    context.pass().unwrap();

    assert_eq!(tiers.task(1).task.state, "waiting");
    assert_eq!(questions(&context, &tiers), [(reviewer, words.to_owned())]);
}

/// The reports the rule was counted against, whose last paragraph asks about their product
/// or waits for something else: results, each.
#[test]
fn a_report_that_ends_with_an_open_question_about_its_product_or_a_wait_for_something_else_is_a_result(
) {
    for words in [
        "The parser is done.\n\n## Open question\n\nShould the lane start folded when nothing is at work?",
        "The hook change is in. A hook that is slow or noisy on stderr will show that in the log. Fine as it is?",
        "Both runs passed.\n\nWaiting for the lock; the monitor will report each stage.",
        "Done. Nothing is waiting for your answer, and no question is open for you.",
        "The scratch folders can be deleted by exact path on your word.",
    ] {
        let context = Context::new();
        let tiers = ended_on(&context, words);
        assert_eq!(tiers.task(1).task.state, "done", "{words:?}");
        assert_eq!(results(&context, &tiers), [words]);
        assert_eq!(questions(&context, &tiers), Vec::<(String, String)>::new());
    }
}

/// The rule reads the worker's own words, not the line ConsensFlow puts after them when a
/// line of the window's record was lost in the turn: the question is still one, and the
/// chief is told of the hole as a result's reader is.
#[test]
fn a_question_taken_past_a_line_lost_in_its_turn_is_still_a_question_and_carries_the_line() {
    let context = Context::new();
    context.host.exits_showing(Some(0), None, None);
    let tiers = working(&context);
    let worker = tiers.assignee(1);
    let place = context.adapter.loses_a_line("zeus");
    context.pass().unwrap();
    context.adapter.answer("zeus", SHOULD_I_CONTINUE);
    context.pass().unwrap();

    assert_eq!(tiers.task(1).task.state, "waiting");
    assert_eq!(
        questions(&context, &tiers),
        [(
            worker.clone(),
            format!(
                "{SHOULD_I_CONTINUE}\n\nA line of @{worker}'s record was lost (record {place}); this result may miss part of the turn."
            )
        )]
    );
}

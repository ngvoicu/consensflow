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
use cf_ledger::NewTask;
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

/// The questions the chief has that are still open: not withdrawn, and with no answer.
fn open_questions(context: &Context, tiers: &Tiers<'_>) -> Vec<i64> {
    let answered: Vec<i64> = tiers
        .task(1)
        .messages
        .iter()
        .filter(|message| message.kind == "answer")
        .filter_map(|message| message.reply_to)
        .collect();
    context
        .inbox(tiers.id("chief"))
        .into_iter()
        .filter(|message| {
            message.kind == "question"
                && message.state != "cancelled"
                && !answered.contains(&message.id)
        })
        .map(|message| message.id)
        .collect()
}

/// A question raised from the last words of `handle`'s turn on T-1, as the chief reads it:
/// the words, and the line that says how to take them as the result.
fn raised(words: &str, handle: &str) -> String {
    format!(
        "{words}\n\n(ConsensFlow: @{handle}'s turn ended on these words. If they are its result: `cf task finish T-1`.)"
    )
}

/// What a harness writes when a job it left running ends and wakes its agent.
const NOTICE: &str =
    "<task-notification>\n<task-id>a1</task-id>\n<status>completed</status>\n</task-notification>";

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
        [(tiers.assignee(1), raised(&words, &tiers.assignee(1)))],
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
        [(
            tiers.assignee(1),
            raised(WAITING_FOR_THE_INSTALL, &tiers.assignee(1))
        )]
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
        [(
            tiers.assignee(1),
            raised(SHOULD_I_CONTINUE, &tiers.assignee(1))
        )]
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
    assert_eq!(
        questions(&context, &tiers),
        [(reviewer.clone(), raised(words, &reviewer))]
    );
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
        "Done. Awaiting your review.",
        "The parser is done. Want me to also add the Windows case?",
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
            raised(
                &format!(
                    "{SHOULD_I_CONTINUE}\n\nA line of @{worker}'s record was lost (record {place}); this result may miss part of the turn."
                ),
                &worker
            )
        )]
    );
}

// --- The way out of a misfire: Finish takes a waiting task (T-1125's review, finding 1).

/// A question raised from a turn's end that were the result after all: the chief finishes the
/// task, its words are the result, and no question is left open for `cf answer` to reach.
#[test]
fn the_chief_takes_a_raised_questions_words_as_the_result_and_no_question_is_left_open() {
    let context = Context::new();
    let tiers = ended_on(&context, WAITING_FOR_THE_INSTALL);
    let chief = tiers.id("chief");
    let question = open_questions(&context, &tiers)[0];
    assert_eq!(tiers.task(1).task.state, "waiting");

    let finished = context
        .finish_task(tiers.project.id, 1, "chief")
        .expect("Finish takes a waiting task");
    assert_eq!(finished.task.state, "done");
    let results = results(&context, &tiers);
    assert_eq!(results.len(), 1, "{results:?}");
    assert!(
        results[0].starts_with(WAITING_FOR_THE_INSTALL),
        "{results:?}"
    );
    assert!(
        results[0].contains("Finished at @chief's word"),
        "{results:?}"
    );

    assert_eq!(open_questions(&context, &tiers), Vec::<i64>::new());
    assert_eq!(context.message(question).state, "cancelled");
    let refused = context
        .ledger
        .borrow_mut()
        .answer(question, chief, Some(&json!("Go")), None)
        .unwrap_err();
    assert_eq!(refused.code(), Some("question-withdrawn"), "{refused}");
    // Nothing the passes do raises it again or moves the task.
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "done");
    assert_eq!(open_questions(&context, &tiers), Vec::<i64>::new());
}

/// The question says it was raised, and how to finish.
#[test]
fn a_raised_question_tells_the_chief_it_was_the_last_words_of_a_turn_and_how_to_finish() {
    let context = Context::new();
    let tiers = ended_on(&context, WAITING_FOR_THE_INSTALL);
    let worker = tiers.assignee(1);
    let body = questions(&context, &tiers).remove(0).1;
    assert!(
        body.ends_with(&format!(
            "\n\n(ConsensFlow: @{worker}'s turn ended on these words. If they are its result: `cf task finish T-1`.)"
        )),
        "{body}"
    );
    // A question the window asked itself with cf ask has no such line.
    let context = Context::new();
    let tiers = working(&context);
    let asked = context.ask(tiers.project.id, "zeus", "chief", 1, "Which grammar?");
    assert_eq!(asked.body, "Which grammar?");
}

/// "Awaiting your review" is no wait, so the first words below are a result; the wait that
/// does fire, answered with words that wait again, ends with one Finish.
#[test]
fn a_wait_that_fires_again_after_its_answer_ends_with_one_finish() {
    let context = Context::new();
    let tiers = ended_on(&context, "Everything is in. I'm waiting for your go.");
    let chief = tiers.id("chief");
    let first = open_questions(&context, &tiers)[0];
    context.answer(tiers.project.id, first, "Nothing more is needed.");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working");

    // The worker's reply waits again, and is raised again.
    context
        .adapter
        .answer("zeus", "Noted, nothing more. Still waiting for your go.");
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "waiting");
    let second = open_questions(&context, &tiers);
    assert_eq!(second.len(), 1);
    assert_ne!(second[0], first);

    // One Finish ends it: its words are the result and nothing is open.
    context
        .finish_task(tiers.project.id, 1, "chief")
        .expect("finished");
    assert_eq!(tiers.task(1).task.state, "done");
    let results = results(&context, &tiers);
    assert_eq!(results.len(), 1, "{results:?}");
    assert!(results[0].starts_with("Noted, nothing more. Still waiting for your go."));
    assert_eq!(open_questions(&context, &tiers), Vec::<i64>::new());
    let _ = chief;
}

/// A pause leaves the question open, and a Finish of the paused task takes the last words
/// as the result as it always did: the question goes with the move.
#[test]
fn a_raised_question_whose_task_is_paused_is_finished_as_a_paused_task_is() {
    let context = Context::new();
    let tiers = ended_on(&context, WAITING_FOR_THE_INSTALL);
    context.pause_task(tiers.project.id, 1);
    assert_eq!(tiers.task(1).task.state, "paused");

    context
        .finish_task(tiers.project.id, 1, "chief")
        .expect("Finish takes a paused task");
    assert_eq!(tiers.task(1).task.state, "done");
    let results = results(&context, &tiers);
    assert!(
        results[0].starts_with(WAITING_FOR_THE_INSTALL),
        "{results:?}"
    );
    assert_eq!(open_questions(&context, &tiers), Vec::<i64>::new());
}

// --- Where the guard is, and where it is not (the review's findings 6 and 7).

/// A turn that ends asking while it left a job running is held, as a turn that left one
/// always was; Finish takes its words as the result, and is never turned into a question.
#[test]
fn a_held_turns_words_that_ask_are_no_question_and_finish_takes_them_as_the_result() {
    let context = Context::new();
    let tiers = working(&context);
    context
        .adapter
        .answer_leaving_a_job("zeus", SHOULD_I_CONTINUE);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working", "held");
    assert_eq!(open_questions(&context, &tiers), Vec::<i64>::new());

    context
        .finish_task(tiers.project.id, 1, "chief")
        .expect("finished");
    assert_eq!(tiers.task(1).task.state, "done");
    let results = results(&context, &tiers);
    assert!(results[0].starts_with(SHOULD_I_CONTINUE), "{results:?}");
    assert_eq!(open_questions(&context, &tiers), Vec::<i64>::new());
}

/// The question is delayed, not lost: the job ends, the harness wakes the agent, and the
/// turn that ends on the question is raised then, not before.
#[test]
fn a_held_turn_whose_next_turn_ends_asking_raises_the_question_then() {
    let context = Context::new();
    let tiers = working(&context);
    context
        .adapter
        .answer_leaving_a_job("zeus", "The run started");
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working");

    context.adapter.wake("zeus", NOTICE);
    context.pass().unwrap();
    context.adapter.answer("zeus", SHOULD_I_CONTINUE);
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "waiting");
    assert_eq!(open_questions(&context, &tiers).len(), 1);
}

/// A follow-up that goes into the same window after T-1, a reopened task and a resumed one
/// all end their turns through the same place.
#[test]
fn a_follow_ups_turn_that_ends_asking_is_a_question_on_the_follow_up() {
    let context = Context::new();
    context.adapter.resumes_records.set(true);
    context.host.exits_showing(Some(0), None, None);
    let tiers = working(&context);
    context.adapter.answer("zeus", "Parser done");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "done");
    // A follow-up of a done task waits for its acceptance (T-1118).
    context.accept_if_done(tiers.project.id, 1);

    context.create_task(
        tiers.project.id,
        NewTask {
            from: "chief".to_owned(),
            after: Some(1),
            body: "Now the lexer".to_owned(),
            ..NewTask::default()
        },
    );
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(2).task.state, "working");
    context.adapter.answer("zeus", SHOULD_I_CONTINUE);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(2).task.state, "waiting");
    assert_eq!(tiers.task(1).task.state, "accepted", "T-1 is as it was");
    assert_eq!(open_questions(&context, &tiers).len(), 1);
}

#[test]
fn a_reopened_tasks_turn_that_ends_asking_is_a_question() {
    let context = Context::new();
    let tiers = ended_on(&context, "Parser done");
    assert_eq!(tiers.task(1).task.state, "done");
    context
        .ledger
        .borrow_mut()
        .reopen_task(tiers.project.id, 1, "chief", "Also the printer")
        .expect("reopened");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working");
    context.adapter.answer("zeus", SHOULD_I_CONTINUE);
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "waiting");
    assert_eq!(open_questions(&context, &tiers).len(), 1);
}

#[test]
fn a_resumed_tasks_turn_that_ends_asking_is_a_question() {
    let context = Context::new();
    let tiers = working(&context);
    context.pause_task(tiers.project.id, 1);
    // The turn the pause cut ends, and the resume's words go into the window at rest.
    context.adapter.answer("zeus", "Half a parser");
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "paused");
    context.resume_task(tiers.project.id, 1, "Go on with it");
    for _ in 0..4 {
        context.pass().unwrap();
    }
    assert_eq!(tiers.task(1).task.state, "working");
    context.adapter.answer("zeus", SHOULD_I_CONTINUE);
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "waiting");
    assert_eq!(open_questions(&context, &tiers).len(), 1);
}

/// What a window answers to a tell while its task is paused is the chief's to take (a note
/// and Finish), not a question: the guard does not look at it.
#[test]
fn the_answer_to_a_tell_that_asks_is_left_to_the_pause_and_raises_no_question() {
    let context = Context::new();
    let tiers = working(&context);
    let zeus = tiers.assignee(1);
    // The chief tells the window; the turn the stop cut ends, and the window begins a turn on
    // the tell, which it ends on words that ask.
    context.adapter.busy("zeus");
    let told = context.tell(tiers.project.id, &zeus, 1, "Stop, and say where you are");
    context.pass().unwrap();
    context.adapter.answer("zeus", "Half a parser");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.message(told.id).state, "delivered");
    context.adapter.busy("zeus");
    context.adapter.answer("zeus", SHOULD_I_CONTINUE);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "paused");
    assert_eq!(open_questions(&context, &tiers), Vec::<i64>::new());
    assert!(
        tiers
            .notes("chief")
            .iter()
            .any(|note| note.contains(SHOULD_I_CONTINUE)),
        "{:?}",
        tiers.notes("chief")
    );
}

/// A raised question is cancelled as any waiting task is, unless a task waits for it.
#[test]
fn a_task_with_a_raised_question_is_cancelled_unless_another_waits_for_it() {
    let context = Context::new();
    let tiers = ended_on(&context, WAITING_FOR_THE_INSTALL);
    context.create_task(
        tiers.project.id,
        NewTask {
            from: "chief".to_owned(),
            pool: Some("worker".to_owned()),
            tier: Some("standard".to_owned()),
            body: "Then the lexer".to_owned(),
            needs: vec![1],
            ..NewTask::default()
        },
    );
    let refused = context
        .ledger
        .borrow_mut()
        .cancel_task(tiers.project.id, 1, "chief")
        .unwrap_err();
    assert_eq!(refused.code(), Some("waited-for"), "{refused}");
    assert_eq!(tiers.task(1).task.state, "waiting");

    // Finished, it releases the one that waits for it once accepted: the chief's way on.
    context
        .finish_task(tiers.project.id, 1, "chief")
        .expect("finished");
    context
        .ledger
        .borrow_mut()
        .accept_task(tiers.project.id, 1, "chief")
        .expect("accepted");
    assert_eq!(tiers.task(1).task.state, "accepted");
}

#[test]
fn a_task_with_a_raised_question_and_no_waiter_is_cancelled() {
    let context = Context::new();
    let tiers = ended_on(&context, WAITING_FOR_THE_INSTALL);
    context
        .ledger
        .borrow_mut()
        .cancel_task(tiers.project.id, 1, "chief")
        .expect("cancelled");
    assert_eq!(tiers.task(1).task.state, "cancelled");
    assert_eq!(open_questions(&context, &tiers), Vec::<i64>::new());
}

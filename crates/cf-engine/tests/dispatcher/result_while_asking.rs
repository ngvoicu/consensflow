//! A worker's result is not lost because a question of its was still out when its turn
//! ended (T-1204; the field board's T-837 of 2026-10-28, found again as T-846).
//!
//! T-837's worker asked the chief a question that needed no answer (`cf ask`: "disk notice,
//! no answer needed") and went on in the same turn. Its task read `waiting` from then until
//! the answer was *delivered*, and an answer goes into a window only when it is at rest.
//! The chief answered at 00:43; the turn ended at about 01:01 with the 5.8 KB write-up;
//! then the answer went in, and the turn it began ended with five lines ("I'd already
//! finished… My result above still stands"). A turn's end on a task that is not working
//! takes no result, so the write-up was in the window's record and nowhere on the board,
//! and the five lines became the result.
//!
//! Now nothing completes at the turn that ends with the question out (a worker that asks
//! and stops ends on a line about it, and that is no result), and the chief is told nothing
//! new; but the result that is recorded when the turn the answer began ends, or when the
//! chief finishes the task, begins with the turns that ended with a question of the worker's
//! out, in order, each labelled. The record holds them, so nothing is written to the ledger
//! for them (`turn_end::while_asking`).

use cf_engine::testing::Context;
use cf_harness::records::{Item, Role};
use cf_proto::ledger::MessageView;

use crate::background::{results, working};
use crate::fixtures::{given, Tiers};

/// The write-up T-837's turn ended with, shortened.
const WRITE_UP: &str = "## T-837: the disk notice\n\nCauses: the builds disk filled while two trees built.\nFixes: `builds-disk.sh` frees a tree's folder.\nReds: none left.\nOpen points: the notice should name the tree.";

/// What the turn the answer began ended with.
const REPLY: &str =
    "I'd already finished; the answer changes nothing. My result above still stands.";

/// The worker's question, as `cf ask` puts it, asked in the middle of its turn: its task
/// waits, and its record has the output `cf ask` printed.
fn asks(context: &Context, tiers: &Tiers<'_>, body: &str) -> MessageView {
    let asked = context.ask(tiers.project.id, &tiers.assignee(1), "chief", 1, body);
    assert_eq!(tiers.task(1).task.state, "waiting");
    let printed = context.adapter.item(
        Role::Tool,
        &format!(
            "m-{} asked @chief. The answer arrives as a message; end your turn now.",
            asked.id
        ),
    );
    context
        .adapter
        .with("zeus", |agent| agent.items.push(printed));
    asked
}

/// The worker goes on after its question, as T-837's did for twenty minutes: it writes what it
/// is doing and runs tools.
fn works_on(context: &Context) {
    let mut doing = context
        .adapter
        .item(Role::Assistant, "Counting what the builds left");
    doing.complete = false;
    let output = context.adapter.item(Role::Tool, "412 GiB in 9 trees");
    context.adapter.with("zeus", |agent| {
        agent.items.extend([doing, output]);
    });
}

/// The chief's `cf answer`.
fn answers(context: &Context, tiers: &Tiers<'_>, question: &MessageView, body: &str) {
    context.answer(tiers.project.id, question.id, body);
}

/// What a result says of a write-up that ended a turn before the answer to `question` went in.
fn before(question: &MessageView, write_up: &str) -> String {
    format!(
        "Written before the answer to its question m-{} went in:\n\n{write_up}\n\n",
        question.id
    )
}

/// The result of a write-up, then a reply, with the labels.
fn with_write_up(question: &MessageView, write_up: &str, last: &str) -> String {
    format!("{}What it said last:\n\n{last}", before(question, write_up))
}

/// The notes and questions the chief was given, which a turn that ended with a question out
/// is to add nothing to: its own question is all there is.
fn chief_has(context: &Context, tiers: &Tiers<'_>) -> Vec<String> {
    let mut kinds: Vec<String> = context
        .inbox(tiers.id("chief"))
        .into_iter()
        .map(|message| message.kind)
        .collect();
    kinds.reverse();
    kinds
}

/// T-837's order: the question asked, the chief's answer waiting while the window works on
/// (it is typed in at rest), the turn ended with the write-up, then the answer, then the
/// short reply. The write-up is in the result, before the reply.
#[test]
fn a_write_up_that_ended_the_turn_with_the_workers_question_out_is_in_the_result_before_the_reply()
{
    let context = Context::new();
    let tiers = working(&context);
    let question = asks(&context, &tiers, "disk notice, no answer needed");
    works_on(&context);
    answers(&context, &tiers, &question, "Noted.");
    context.pass().unwrap();
    assert_eq!(
        tiers.task(1).task.state,
        "waiting",
        "the answer waits for the window's rest"
    );

    context.adapter.answer("zeus", WRITE_UP);
    context.pass().unwrap();
    context.pass().unwrap();
    assert!(
        given(&context, "zeus")
            .last()
            .is_some_and(|text| text.contains("Noted.")),
        "the answer went in once the window was at rest"
    );
    assert_eq!(tiers.task(1).task.state, "working");
    assert_eq!(
        results(&context, &tiers),
        Vec::<String>::new(),
        "the turn that ended with a question out took no result"
    );

    context.adapter.answer("zeus", REPLY);
    context.pass().unwrap();

    assert_eq!(tiers.task(1).task.state, "done");
    assert_eq!(
        results(&context, &tiers),
        [with_write_up(&question, WRITE_UP, REPLY)],
        "the write-up, then the reply"
    );
}

/// The other order: the chief answers after the turn ended. The task waits, the write-up is
/// told to nobody and recorded as nothing, and the result is the same once the reply is.
#[test]
fn a_write_up_with_the_question_unanswered_waits_and_is_in_the_result_once_it_is_answered() {
    let context = Context::new();
    let tiers = working(&context);
    let question = asks(&context, &tiers, "disk notice, no answer needed");
    works_on(&context);
    let before_the_turn_ended = chief_has(&context, &tiers);

    context.adapter.answer("zeus", WRITE_UP);
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(tiers.task(1).task.state, "waiting");
    assert_eq!(results(&context, &tiers), Vec::<String>::new());
    assert_eq!(
        chief_has(&context, &tiers),
        before_the_turn_ended,
        "the chief is told nothing of a turn that ended with its own question out"
    );
    assert_eq!(tiers.notes("chief"), Vec::<String>::new());

    answers(&context, &tiers, &question, "Noted.");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working");
    context.adapter.answer("zeus", REPLY);
    context.pass().unwrap();

    assert_eq!(tiers.task(1).task.state, "done");
    assert_eq!(
        results(&context, &tiers),
        [with_write_up(&question, WRITE_UP, REPLY)]
    );
}

/// What each harness wrote on the way to its write-up and its reply, in the shape its
/// record has it (`final_answer`'s): none of it is the turn's words but the last.
fn on_the_way(context: &Context, harness: &str) -> Vec<Item> {
    let progress = |said: &str| {
        let mut item = context.adapter.item(Role::Assistant, said);
        item.complete = false;
        item
    };
    let commentary = |said: &str| {
        let mut item = progress(said);
        item.commentary = true;
        item
    };
    let tool = || context.adapter.item(Role::Tool, "output of a tool");
    let its = || {
        context
            .adapter
            .item(Role::Custom, "context the harness added")
    };
    match harness {
        // Hook context among the steps, each step that stopped for a tool incomplete.
        "claude" => vec![
            its(),
            progress("Still reading the capture"),
            tool(),
            progress("Now the counts"),
            tool(),
        ],
        // Notes marked as commentary, a tool's output after each.
        "codex" => vec![
            commentary("I'll read the commit, then check it on disk."),
            tool(),
            commentary("The diff matches; checking the counts now."),
            tool(),
        ],
        // The extension's own messages, the tool's output after the reply that called it.
        "pi" => vec![
            progress("Reading"),
            its(),
            tool(),
            its(),
            progress("Now"),
            tool(),
        ],
        // System messages among the steps.
        "devin" => vec![progress("Reading the capture"), its(), tool(), its()],
        // Steps that stopped for a tool, one after the other.
        "muse" => vec![
            progress("Reading the capture"),
            tool(),
            progress("Counting"),
            tool(),
        ],
        other => panic!("no harness {other}"),
    }
}

/// The result is the same whatever the record's shape: the harnesses write progress, tools
/// and their own messages differently, and the engine reads the turn's final answer out of
/// each one.
#[test]
fn every_harnesss_write_up_is_in_the_result_before_the_reply() {
    for harness in ["claude", "codex", "pi", "devin", "muse"] {
        let context = Context::new();
        let tiers = working(&context);
        let question = asks(&context, &tiers, "disk notice, no answer needed");
        works_on(&context);
        answers(&context, &tiers, &question, "Noted.");
        let written = on_the_way(&context, harness);
        context
            .adapter
            .with("zeus", |agent| agent.items.extend(written));
        context.adapter.answer("zeus", WRITE_UP);
        context.pass().unwrap();
        context.pass().unwrap();
        assert_eq!(tiers.task(1).task.state, "working", "{harness}");

        let written = on_the_way(&context, harness);
        context
            .adapter
            .with("zeus", |agent| agent.items.extend(written));
        context.adapter.answer("zeus", REPLY);
        context.pass().unwrap();

        assert_eq!(tiers.task(1).task.state, "done", "{harness}");
        assert_eq!(
            results(&context, &tiers),
            [with_write_up(&question, WRITE_UP, REPLY)],
            "{harness}"
        );
    }
}

/// The common case: a worker asks and ends its turn on a line that says it waits. Nothing
/// completes, the chief has its question and no more, and the reply to the answer is the
/// result as it always was: the line is the question's own and is not in front of it.
#[test]
fn a_worker_that_asks_and_ends_its_turn_waiting_has_the_reply_alone_for_its_result() {
    let context = Context::new();
    let tiers = working(&context);
    let question = asks(&context, &tiers, "Which grammar?");
    let questions_only = chief_has(&context, &tiers);

    context.adapter.answer(
        "zeus",
        "Asked the chief which grammar. I'm waiting for your answer.",
    );
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(tiers.task(1).task.state, "waiting");
    assert_eq!(results(&context, &tiers), Vec::<String>::new());
    assert_eq!(chief_has(&context, &tiers), questions_only);

    answers(&context, &tiers, &question, "The second.");
    context.pass().unwrap();
    context.pass().unwrap();
    context
        .adapter
        .answer("zeus", "Parsed with the second grammar.");
    context.pass().unwrap();

    assert_eq!(tiers.task(1).task.state, "done");
    assert_eq!(
        results(&context, &tiers),
        ["Parsed with the second grammar."]
    );
}

/// What the worker says after `cf ask` is about the question, in whatever words: none of
/// the ones below say it waits for the chief, and none is in front of the result.
#[test]
fn a_line_about_the_question_in_any_words_is_not_in_front_of_the_result() {
    for line in [
        "Asked.",
        "I still need the format",
        "Question sent; ending my turn.",
        "Mulțumesc! Aștept să văd ce spui.",
    ] {
        let context = Context::new();
        let tiers = working(&context);
        let question = asks(&context, &tiers, "Which grammar?");
        context.adapter.answer("zeus", line);
        context.pass().unwrap();
        assert_eq!(tiers.task(1).task.state, "waiting", "{line}");

        answers(&context, &tiers, &question, "The second.");
        context.pass().unwrap();
        context.pass().unwrap();
        context
            .adapter
            .answer("zeus", "Parsed with the second grammar.");
        context.pass().unwrap();

        assert_eq!(
            results(&context, &tiers),
            ["Parsed with the second grammar."],
            "{line}"
        );
    }
}

/// A record that has no tool output at all (a test's, or a harness's that lost it) is a turn
/// that asked nothing, and a short line in it is about the question all the same.
#[test]
fn a_turn_that_went_on_after_asking_keeps_its_words_though_they_are_short() {
    let context = Context::new();
    let tiers = working(&context);
    let question = asks(&context, &tiers, "disk notice, no answer needed");
    works_on(&context);
    context.adapter.answer("zeus", "Done: 3 files changed.");
    context.pass().unwrap();
    answers(&context, &tiers, &question, "Noted.");
    context.pass().unwrap();
    context.pass().unwrap();
    context.adapter.answer("zeus", REPLY);
    context.pass().unwrap();

    assert_eq!(
        results(&context, &tiers),
        [with_write_up(&question, "Done: 3 files changed.", REPLY)]
    );
}

/// The notice was the worker's last act and the write-up came after it: the words are long,
/// and a long turn is a report, whatever it ended on.
#[test]
fn a_long_write_up_after_a_notice_that_was_the_last_tool_is_kept() {
    let context = Context::new();
    let tiers = working(&context);
    works_on(&context);
    let question = asks(&context, &tiers, "disk notice, no answer needed");
    let long = format!(
        "{WRITE_UP}\n\n{}",
        "The builds disk filled twice. ".repeat(30)
    )
    .trim_end()
    .to_owned();
    assert!(long.chars().count() > 500);
    context.adapter.answer("zeus", &long);
    context.pass().unwrap();
    answers(&context, &tiers, &question, "Noted.");
    context.pass().unwrap();
    context.pass().unwrap();
    context.adapter.answer("zeus", REPLY);
    context.pass().unwrap();

    assert_eq!(
        results(&context, &tiers),
        [with_write_up(&question, &long, REPLY)]
    );
}

/// A report that ends asking the chief what to do is a report when the turn did its work
/// after asking; it is kept as it is, whatever the words ask.
#[test]
fn a_report_that_ends_asking_whether_to_go_on_is_kept_when_the_turn_worked_on() {
    let context = Context::new();
    let tiers = working(&context);
    let question = asks(&context, &tiers, "disk notice, no answer needed");
    works_on(&context);
    let report = format!("{WRITE_UP}\n\nShould I continue with the live runs, or stop here?");
    context.adapter.answer("zeus", &report);
    context.pass().unwrap();
    answers(&context, &tiers, &question, "Stop here.");
    context.pass().unwrap();
    context.pass().unwrap();
    context.adapter.answer("zeus", "Stopped.");
    context.pass().unwrap();

    assert_eq!(
        results(&context, &tiers),
        [with_write_up(&question, &report, "Stopped.")]
    );
}

/// A question ConsensFlow raised from the turn's last words (T-1119) is on the board with
/// those words: the result does not give them again.
#[test]
fn a_question_raised_from_a_turns_last_words_is_not_given_again_in_the_result() {
    let context = Context::new();
    let tiers = working(&context);
    let words = "Both hosts are measured; the A/B runs are half done. Should I continue with the live runs, or stop here?";
    context.adapter.answer("zeus", words);
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "waiting");
    let raised = context
        .inbox(tiers.id("chief"))
        .into_iter()
        .find(|message| message.kind == "question")
        .expect("the question ConsensFlow raised");

    context.answer(tiers.project.id, raised.id, "Stop here.");
    context.pass().unwrap();
    context.pass().unwrap();
    context
        .adapter
        .answer("zeus", "Stopped; the findings are in the report.");
    context.pass().unwrap();

    assert_eq!(tiers.task(1).task.state, "done");
    assert_eq!(
        results(&context, &tiers),
        ["Stopped; the findings are in the report."]
    );
}

/// Two questions, each out when a turn ended: both write-ups, in order, then the last words.
#[test]
fn the_write_ups_of_two_turns_that_ended_with_a_question_out_come_in_order() {
    let context = Context::new();
    let tiers = working(&context);
    let first = asks(&context, &tiers, "Which grammar?");
    works_on(&context);
    context
        .adapter
        .answer("zeus", "The parser reads both grammars.");
    context.pass().unwrap();
    answers(&context, &tiers, &first, "The second.");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working");

    let second = asks(&context, &tiers, "Which folder?");
    works_on(&context);
    context
        .adapter
        .answer("zeus", "The second grammar is parsed.");
    context.pass().unwrap();
    answers(&context, &tiers, &second, "The first.");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working");
    context.adapter.answer("zeus", REPLY);
    context.pass().unwrap();

    assert_eq!(tiers.task(1).task.state, "done");
    assert_eq!(
        results(&context, &tiers),
        [format!(
            "{}{}What it said last:\n\n{REPLY}",
            before(&first, "The parser reads both grammars."),
            before(&second, "The second grammar is parsed."),
        )]
    );
}

/// Finish before the answer went in takes the write-up, as it always took a waiting task's
/// last words; and the question is withdrawn.
#[test]
fn the_chief_finishing_a_waiting_task_takes_the_write_up_alone() {
    let context = Context::new();
    let tiers = working(&context);
    let question = asks(&context, &tiers, "disk notice, no answer needed");
    works_on(&context);
    context.adapter.answer("zeus", WRITE_UP);
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "waiting");

    context
        .finish_task(tiers.project.id, 1, "chief")
        .expect("finished");
    let results = results(&context, &tiers);
    assert_eq!(results.len(), 1, "{results:?}");
    assert!(results[0].starts_with(WRITE_UP), "{results:?}");
    assert!(!results[0].contains("Written before"), "{results:?}");
    assert_eq!(context.message(question.id).state, "cancelled");
}

/// Finish after the answer went in, where the reply turn is held by a job: the result is the
/// reply with the write-up in front, as the turn's own end would have recorded it.
#[test]
fn the_chief_finishing_the_held_reply_to_an_answer_gives_the_write_up_too() {
    let context = Context::new();
    let tiers = working(&context);
    let question = asks(&context, &tiers, "disk notice, no answer needed");
    works_on(&context);
    answers(&context, &tiers, &question, "Noted.");
    context.adapter.answer("zeus", WRITE_UP);
    context.pass().unwrap();
    context.pass().unwrap();
    context.adapter.answer_leaving_a_job("zeus", REPLY);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working", "held by its job");

    context
        .finish_task(tiers.project.id, 1, "chief")
        .expect("finished");
    let results = results(&context, &tiers);
    assert_eq!(results.len(), 1, "{results:?}");
    assert!(
        results[0].starts_with(&with_write_up(&question, WRITE_UP, REPLY)),
        "{results:?}"
    );
}

/// A reply that repeats the write-up word for word is the same words: they are given once.
#[test]
fn a_reply_that_repeats_the_write_up_is_not_given_twice() {
    let context = Context::new();
    let tiers = working(&context);
    let question = asks(&context, &tiers, "disk notice, no answer needed");
    works_on(&context);
    answers(&context, &tiers, &question, "Noted.");
    context.adapter.answer("zeus", WRITE_UP);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working");
    context.adapter.answer("zeus", WRITE_UP);
    context.pass().unwrap();

    assert_eq!(tiers.task(1).task.state, "done");
    assert_eq!(results(&context, &tiers), [WRITE_UP]);
}

/// What a turn wrote before the words that sent the task on again was written for the round
/// before them: the resume's turn has its own words for its result.
#[test]
fn a_write_up_from_before_the_words_that_resumed_the_task_is_not_in_its_result() {
    let context = Context::new();
    let tiers = working(&context);
    let question = asks(&context, &tiers, "disk notice, no answer needed");
    works_on(&context);
    answers(&context, &tiers, &question, "Noted.");
    context.adapter.answer("zeus", WRITE_UP);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working");
    context.adapter.answer("zeus", REPLY);
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "done");

    // A round more: the chief sends the task back with new words.
    context
        .ledger
        .borrow_mut()
        .reopen_task(tiers.project.id, 1, "chief", "Add the Windows case.")
        .expect("reopened");
    context.pass().unwrap();
    context.pass().unwrap();
    context.adapter.answer("zeus", "The Windows case is in.");
    context.pass().unwrap();

    // The chief's inbox lists the newest first.
    assert_eq!(
        results(&context, &tiers).first().map(String::as_str),
        Some("The Windows case is in.")
    );
}

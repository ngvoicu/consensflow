//! A worker's result is its final answer: what it wrote after its last tool call or
//! message in its last turn, and not every line of progress before it (the owner's
//! decision of 2026-10-15, after the results of T-14 and T-15 began with ten to twenty
//! lines of it). Where it wrote nothing after its last tool call, the last words it wrote
//! before it in that turn (the chief's decision on T-262). What it wrote before stays in
//! the transcript. Each harness's reader
//! gives the turn's items in the shape its own record has (`cf-harness`'s tests of each
//! reader hold those shapes), and the engine takes the final answer out of every one of
//! them; Codex's notes are marked as its commentary and never complete, the others'
//! steps that stopped for a tool are never complete either. Node's dispatcher gave the
//! result everything the member wrote, so none of these is held to a Node trace (the
//! one that was is `the_dispatcher`'s, which departs).

use cf_engine::testing::Context;
use cf_harness::records::{Item, Role};

use crate::background::{results, working};

const REPORT: &str = "PASS: no findings in the parser.";

/// What a harness wrote on the way, in the shape its record has it.
#[derive(Clone, Copy)]
enum Wrote {
    /// A step that said something and stopped for a tool: never complete.
    Progress(&'static str),
    /// Codex's note, marked as its commentary.
    Commentary(&'static str),
    /// A tool's output.
    Tool,
    /// What the harness put in the conversation itself: a hook's context, an extension's
    /// message, a system message.
    Its,
}

/// The items `wrote` made, from the harness whose window `context` has.
fn items(context: &Context, wrote: &[Wrote]) -> Vec<Item> {
    wrote
        .iter()
        .map(|wrote| match *wrote {
            Wrote::Progress(said) => {
                let mut item = context.adapter.item(Role::Assistant, said);
                item.complete = false;
                item
            }
            Wrote::Commentary(said) => {
                let mut item = context.adapter.item(Role::Assistant, said);
                item.complete = false;
                item.commentary = true;
                item
            }
            Wrote::Tool => context.adapter.item(Role::Tool, "output of a tool"),
            Wrote::Its => context
                .adapter
                .item(Role::Custom, "context the harness added"),
        })
        .collect()
}

/// A worker whose turn wrote `wrote` and then ended with `last` for its answer: the result.
fn result_of(wrote: &[Wrote], last: &str) -> String {
    let context = Context::new();
    let tiers = working(&context);
    let written = items(&context, wrote);
    context
        .adapter
        .with("zeus", |agent| agent.items.extend(written));
    context.adapter.answer("zeus", last);
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "done");
    results(&context, &tiers).remove(0)
}

/// The progress of a review that read a diff and then ran the tests.
const REVIEWING: [Wrote; 4] = [
    Wrote::Progress("Still reviewing the diff"),
    Wrote::Tool,
    Wrote::Progress("Now the tests"),
    Wrote::Tool,
];

#[test]
fn claudes_steps_that_stopped_for_a_tool_are_progress_and_the_message_that_ended_its_turn_is_the_result(
) {
    // Its hook's context comes in the middle of a turn, as an item of its own.
    let wrote = [
        Wrote::Its,
        Wrote::Progress("Still reviewing the diff"),
        Wrote::Tool,
        Wrote::Progress("Now the tests"),
        Wrote::Tool,
    ];
    assert_eq!(result_of(&wrote, REPORT), REPORT);
}

#[test]
fn codexs_notes_are_commentary_and_its_final_answer_is_the_result() {
    let wrote = [
        Wrote::Commentary("I'll read the commit, then check it on disk."),
        Wrote::Tool,
        Wrote::Commentary("The diff matches; checking the counts now."),
        Wrote::Tool,
    ];
    assert_eq!(result_of(&wrote, REPORT), REPORT);
    // A note after its last command, said just before the answer, is no part of it either.
    let last_word = [
        Wrote::Commentary("Reading the diff"),
        Wrote::Tool,
        Wrote::Commentary("Nothing else to check; writing it up."),
    ];
    assert_eq!(result_of(&last_word, REPORT), REPORT);
}

#[test]
fn pis_steps_and_devins_messages_before_the_one_that_stopped_are_progress() {
    // Pi's extension adds messages of its own to the conversation, a tool's output comes
    // after the reply that called it, and Devin's system messages sit among the steps.
    assert_eq!(result_of(&REVIEWING, REPORT), REPORT);
    let with_its_own = [
        Wrote::Progress("Still reviewing the diff"),
        Wrote::Its,
        Wrote::Tool,
        Wrote::Its,
        Wrote::Progress("Now the tests"),
        Wrote::Tool,
    ];
    assert_eq!(result_of(&with_its_own, REPORT), REPORT);
}

#[test]
fn what_the_member_wrote_after_its_last_tool_call_is_all_of_the_result_in_the_items_it_was_written_in(
) {
    // A final answer written in two messages after the last tool call.
    let context = Context::new();
    let tiers = working(&context);
    let written = items(&context, &REVIEWING);
    let first = context.adapter.item(Role::Assistant, "The report:");
    context.adapter.with("zeus", |agent| {
        agent.items.extend(written.into_iter().chain([first]))
    });
    context.adapter.answer("zeus", "PASS: no findings.");
    context.pass().unwrap();

    assert_eq!(
        results(&context, &tiers),
        ["The report:\n\nPASS: no findings."]
    );
}

#[test]
fn a_turn_that_wrote_nothing_after_its_last_tool_call_gives_the_last_words_it_wrote_before_it() {
    // The member's last step is empty (it ended its turn on a tool's output): the result is
    // the last words it wrote on the way, so that a result is never empty (the chief's
    // decision on T-262). The same for every harness's shape of it.
    assert_eq!(result_of(&REVIEWING, ""), "Now the tests");
    assert_eq!(result_of(&REVIEWING, "  \n "), "Now the tests");
    // The words of a step that said nothing do not count: the last that said something does.
    let silent_step = [
        Wrote::Progress("Still reviewing the diff"),
        Wrote::Tool,
        Wrote::Progress(" "),
        Wrote::Tool,
    ];
    assert_eq!(result_of(&silent_step, ""), "Still reviewing the diff");
    // Codex's notes are its own progress, never its answer.
    let noted = [
        Wrote::Progress("Read the diff"),
        Wrote::Tool,
        Wrote::Commentary("Checking the counts now."),
        Wrote::Tool,
    ];
    assert_eq!(result_of(&noted, ""), "Read the diff");
    // A turn that wrote no words at all says so.
    assert_eq!(
        result_of(&[Wrote::Tool, Wrote::Its, Wrote::Tool], ""),
        "(the agent ended its turn without a written answer)"
    );
}

#[test]
fn the_words_before_a_woken_turns_last_tool_call_are_its_own_and_none_of_the_turn_before() {
    // A job's end woke a turn that called a tool and wrote nothing after it: the words of
    // the turn before the notification are no part of this turn's answer, which has none.
    let context = Context::new();
    let tiers = working(&context);
    let first = context
        .adapter
        .item(Role::Assistant, "Still going; I'll report when it ends");
    let woken = context.adapter.item(
        Role::User,
        "<task-notification>b1 completed</task-notification>",
    );
    let output = context.adapter.item(Role::Tool, "output of a tool");
    context.adapter.with("zeus", |agent| {
        agent.items.extend([first, woken, output]);
    });
    context.adapter.answer("zeus", "");
    context.pass().unwrap();

    assert_eq!(
        results(&context, &tiers),
        ["(the agent ended its turn without a written answer)"]
    );
}

#[test]
fn a_turn_whose_last_word_is_a_step_that_stopped_for_a_tool_is_no_result_though_its_window_is_at_rest(
) {
    // The window says it is at rest and the last thing its record has is a step that said
    // something and stopped for a tool: the record is behind, the turn is not over, and
    // what it said is no result.
    let context = Context::new();
    let tiers = working(&context);
    context
        .adapter
        .writes("zeus", "Running the tests now", "2026-10-15T10:00:00Z");
    context.adapter.with("zeus", |agent| agent.settled = true);
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working");
    assert!(results(&context, &tiers).is_empty());

    // The tool's output and the report come: the report is the result.
    let output = context.adapter.item(Role::Tool, "output of a tool");
    context
        .adapter
        .with("zeus", |agent| agent.items.push(output));
    context.adapter.answer("zeus", REPORT);
    context.pass().unwrap();
    assert_eq!(results(&context, &tiers), [REPORT]);
}

#[test]
fn a_turn_that_was_woken_has_the_final_answer_of_its_last_turn_and_none_of_the_turn_before() {
    // The first turn ended with its words and the harness's own message began the next
    // (Claude Code's task notification): the words before that message are not the answer,
    // whether the turn it began read something first or went straight to its report.
    for woken_turn in [&REVIEWING[..], &[]] {
        let context = Context::new();
        let tiers = working(&context);
        let first = context
            .adapter
            .item(Role::Assistant, "Still going; I'll report when it ends");
        let woken = context.adapter.item(
            Role::User,
            "<task-notification>b1 completed</task-notification>",
        );
        let reading = items(&context, woken_turn);
        context.adapter.with("zeus", |agent| {
            agent.items.push(first);
            agent.items.push(woken);
            agent.items.extend(reading);
        });
        context.adapter.answer("zeus", REPORT);
        context.pass().unwrap();

        assert_eq!(results(&context, &tiers), [REPORT]);
    }
}

#[test]
fn finish_takes_the_last_words_before_the_last_tool_call_of_a_held_turn_that_wrote_none_after_it() {
    // A turn that left a job running is held, and Finish is the way out of the wait: it
    // takes the same answer a result does, the last words before the last tool call where
    // there are none after it (T-262), and not that there is none.
    let context = Context::new();
    let tiers = working(&context);
    let written = items(&context, &REVIEWING);
    context
        .adapter
        .with("zeus", |agent| agent.items.extend(written));
    context.adapter.answer_leaving_a_job("zeus", "");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working", "held for its job");
    context
        .finish_task(tiers.project.id, 1, "chief")
        .expect("finished");
    assert_eq!(tiers.task(1).task.state, "done");
    let result = results(&context, &tiers).remove(0);
    assert!(result.starts_with("Now the tests"), "{result}");
}

#[test]
fn a_question_written_before_the_last_tool_call_with_no_words_after_it_is_a_question_to_the_chief()
{
    // A turn that wrote no words after its last tool call ends on the last words it wrote
    // before it (T-262), and those words are what says whether it ended asking the chief
    // (T-1119): a question there is put to the chief, and the task waits for the answer.
    let context = Context::new();
    let tiers = working(&context);
    let asked = "Should I continue with the live runs, or stop here?";
    let written = items(&context, &[Wrote::Progress(asked), Wrote::Tool]);
    context
        .adapter
        .with("zeus", |agent| agent.items.extend(written));
    context.adapter.answer("zeus", "");
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "waiting");
    assert_eq!(results(&context, &tiers), Vec::<String>::new());
}

//! A member's turn that ends to wait for its own work is no result: it is sent on once
//! (T-1923, from the advice T-1919).
//!
//! On 2026-11-21 two Claude Code workers ended their turns on a line about waiting ("I'll check
//! them at the next wakeup.", "I'm ending this turn until it runs."), with nothing pending that
//! the board could see, and the lines were taken as the tasks' results. Now the member is typed
//! one note, as it rests, to go on and wait inside its turn; the requester is told nothing; the
//! member's next turn ends with the result; and a second such end is the result as it was.
//! The rule (`turn_end::waits`) reads the member's words, so it is the same on every harness.

use cf_engine::testing::Context;

use crate::background::{results, working};
use crate::fixtures::{given, Tiers};

/// T-1910's m-12911.
const ENDING_THE_TURN: &str = "Windows dispatcher passes: 1791 tests. The rig is still waiting for its build turn, so I'm ending this turn until it runs.";

/// T-1887's m-12797.
const AT_THE_NEXT_WAKEUP: &str = "Final-head workspace, whole rig and Windows harness runs are going (head `c61c13a60`); I'll check them at the next wakeup.";

/// The notes of ConsensFlow's that went to the member about the go-on: ledger, in order.
fn go_on_notes(_context: &Context, tiers: &Tiers<'_>) -> Vec<String> {
    tiers
        .task(1)
        .messages
        .iter()
        .filter(|message| message.kind == "note" && message.sender.is_none())
        .filter(|message| {
            message
                .body
                .starts_with("ConsensFlow: your last turn ended to wait")
        })
        .map(|message| message.body.clone())
        .collect()
}

/// T-1 at zeus, whose agent ends its turn with `words`: the passes that look at it and send
/// the note are made.
fn ended_on<'a>(context: &'a Context, words: &str) -> Tiers<'a> {
    let tiers = working(context);
    context.adapter.answer("zeus", words);
    context.pass().unwrap();
    context.pass().unwrap();
    tiers
}

/// Notes that came to the requester about the task: none may.
fn told_the_chief(context: &Context, tiers: &Tiers<'_>) -> Vec<String> {
    context
        .inbox(tiers.id("chief"))
        .into_iter()
        .filter(|message| message.kind != "result")
        .map(|message| message.body)
        .collect()
}

#[test]
fn a_turn_that_ends_to_wait_is_no_result_and_the_member_is_sent_on_once() {
    for words in [ENDING_THE_TURN, AT_THE_NEXT_WAKEUP] {
        let context = Context::new();
        let tiers = ended_on(&context, words);

        assert_eq!(tiers.task(1).task.state, "working", "{words:?}");
        assert_eq!(results(&context, &tiers), Vec::<String>::new(), "{words:?}");
        assert_eq!(
            go_on_notes(&context, &tiers).len(),
            1,
            "one note: {words:?}"
        );
        assert!(
            given(&context, "zeus")
                .last()
                .is_some_and(|text| text.contains("ended to wait for your own work")),
            "the note went into the window: {:?}",
            given(&context, "zeus")
        );
        assert_eq!(
            told_the_chief(&context, &tiers),
            Vec::<String>::new(),
            "the requester is told nothing"
        );
    }
}

#[test]
fn the_note_is_sent_once_however_often_the_turn_is_looked_at() {
    let context = Context::new();
    let tiers = ended_on(&context, ENDING_THE_TURN);
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(go_on_notes(&context, &tiers).len(), 1);
    assert_eq!(results(&context, &tiers), Vec::<String>::new());
}

#[test]
fn the_turn_the_note_begins_ends_with_the_result() {
    let context = Context::new();
    let tiers = ended_on(&context, AT_THE_NEXT_WAKEUP);
    context
        .adapter
        .answer("zeus", "Both runs passed. Head abc123, pushed.");
    context.pass().unwrap();

    assert_eq!(tiers.task(1).task.state, "done");
    assert_eq!(
        results(&context, &tiers),
        ["Both runs passed. Head abc123, pushed."],
        "the interim line is a line about the wait: no part of the result"
    );
}

#[test]
fn a_long_report_before_the_note_is_kept_with_the_result() {
    let context = Context::new();
    let report = format!(
        "{}\n\n{AT_THE_NEXT_WAKEUP}",
        "The reading is done and every file is written. ".repeat(20)
    );
    let tiers = ended_on(&context, &report);
    assert_eq!(tiers.task(1).task.state, "working");
    context.adapter.answer("zeus", "All runs passed.");
    context.pass().unwrap();

    let [result] = results(&context, &tiers).try_into().expect("one result");
    assert!(result.starts_with("Written before the note m-"), "{result}");
    assert!(result.contains("every file is written"), "{result}");
    assert!(result.ends_with("All runs passed."), "{result}");
}

#[test]
fn a_second_such_end_after_the_note_is_the_result() {
    let context = Context::new();
    let tiers = ended_on(&context, AT_THE_NEXT_WAKEUP);
    context.adapter.answer("zeus", ENDING_THE_TURN);
    context.pass().unwrap();

    assert_eq!(tiers.task(1).task.state, "done");
    assert_eq!(results(&context, &tiers), [ENDING_THE_TURN]);
    assert_eq!(go_on_notes(&context, &tiers).len(), 1, "no second note");
}

#[test]
fn a_restart_sends_no_second_note() {
    let context = Context::new();
    let tiers = ended_on(&context, ENDING_THE_TURN);
    let after = context.make();
    after.pass().unwrap();
    after.pass().unwrap();
    // The engine made again pauses the task, as a restart does; no note is sent for it.
    assert_eq!(go_on_notes(&context, &tiers).len(), 1);
    assert_eq!(results(&context, &tiers), Vec::<String>::new());
}

#[test]
fn a_new_delivery_of_the_task_may_be_sent_on_again() {
    let context = Context::new();
    let tiers = ended_on(&context, ENDING_THE_TURN);
    context.adapter.answer("zeus", "Done; head abc123.");
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "done");

    // The chief reopens it with words; the window's next turn ends to wait once more.
    context
        .ledger
        .borrow_mut()
        .reopen_task(tiers.project.id, 1, "chief", "Run the rig again.")
        .expect("reopened");
    context.pass().unwrap();
    context.pass().unwrap();
    context.adapter.answer("zeus", AT_THE_NEXT_WAKEUP);
    context.pass().unwrap();
    context.pass().unwrap();

    assert_eq!(
        go_on_notes(&context, &tiers).len(),
        2,
        "one for each delivery"
    );
    assert_eq!(tiers.task(1).task.state, "working");
}

#[test]
fn work_its_harness_reports_holds_the_task_as_it_did_and_no_note_goes() {
    let context = Context::new();
    let tiers = working(&context);
    context
        .adapter
        .answer_leaving_a_job("zeus", AT_THE_NEXT_WAKEUP);
    context.pass().unwrap();
    context.pass().unwrap();

    assert_eq!(tiers.task(1).task.state, "working");
    assert_eq!(results(&context, &tiers), Vec::<String>::new());
    assert_eq!(go_on_notes(&context, &tiers), Vec::<String>::new());
}

#[test]
fn a_paused_task_gets_no_note() {
    let context = Context::new();
    let tiers = working(&context);
    context.pause_task(tiers.project.id, 1);
    context.adapter.answer("zeus", ENDING_THE_TURN);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(go_on_notes(&context, &tiers), Vec::<String>::new());
}

#[test]
fn a_wait_for_the_chief_is_still_a_question_and_a_finished_report_is_a_result() {
    let context = Context::new();
    let tiers = ended_on(
        &context,
        "I'm waiting for your go; I'll resume when you reply.",
    );
    assert_eq!(tiers.task(1).task.state, "waiting");
    assert_eq!(go_on_notes(&context, &tiers), Vec::<String>::new());

    for words in [
        "Done: head abc123, pushed. Nothing of mine is still running.",
        "The rig ran inside the turn and passed; the reader checks them at the next wakeup.",
    ] {
        let context = Context::new();
        let tiers = ended_on(&context, words);
        assert_eq!(tiers.task(1).task.state, "done", "{words:?}");
        assert_eq!(results(&context, &tiers), [words]);
        assert_eq!(go_on_notes(&context, &tiers), Vec::<String>::new());
    }
}

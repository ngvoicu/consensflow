//! A window that exits right after its harness wrote the task's final answer, before the
//! engine's next pass, keeps that answer: the exit first collects what an ordinary pass
//! would collect in the task's state, through the same collector, and then settles as it
//! always did. Review T-1883 saw the host deliver such an exit 0.119 s after the last
//! byte, 1.13 s before the pass, and the task paused with no result though the record
//! held the answer. Everything else an exit settles is as it was: a window that ends
//! mid-turn (a crash among them), a paused task, a question out, background work.

use cf_engine::testing::Context;

use crate::background::{results, working};

const ANSWER: &str = "VERIFIED FINAL ANSWER";
const QUESTION: &str =
    "Should I continue with the live runs, or stop here so you can review the findings first?";

/// What the task and the chief's inbox read once the window exited.
fn after_exit(context: &Context, tiers: &crate::fixtures::Tiers<'_>) -> (String, Vec<String>) {
    (tiers.task(1).task.state, results(context, tiers))
}

#[test]
fn an_exit_after_the_final_answer_and_before_the_pass_makes_the_task_done_with_that_answer_once() {
    let context = Context::new();
    let tiers = working(&context);
    context.adapter.busy("zeus");
    context.pass().unwrap();
    context.adapter.answer("zeus", ANSWER);
    context.host.exits_showing(Some(0), None, Some(&["done"]));
    context.exit("zeus");
    assert_eq!(
        after_exit(&context, &tiers),
        ("done".to_owned(), vec![ANSWER.to_owned()])
    );
    context.pass().unwrap();
    assert_eq!(
        after_exit(&context, &tiers),
        ("done".to_owned(), vec![ANSWER.to_owned()]),
        "once, and the next pass adds nothing"
    );
}

#[test]
fn a_window_that_exits_mid_turn_with_no_settled_answer_still_pauses_its_task_with_no_result() {
    let context = Context::new();
    let tiers = working(&context);
    context.adapter.busy("zeus");
    context.pass().unwrap();
    context.host.exits_showing(Some(0), None, Some(&["done"]));
    context.exit("zeus");
    assert_eq!(after_exit(&context, &tiers), ("paused".to_owned(), vec![]));
    context.pass().unwrap();
    assert_eq!(after_exit(&context, &tiers), ("paused".to_owned(), vec![]));
}

#[test]
fn a_crash_mid_turn_still_pauses_its_task_with_no_result() {
    let context = Context::new();
    let tiers = working(&context);
    context.adapter.busy("zeus");
    context.pass().unwrap();
    context
        .host
        .exits_showing(Some(0xC000_0409), None, Some(&["fatal runtime error"]));
    context.exit("zeus");
    assert_eq!(after_exit(&context, &tiers), ("paused".to_owned(), vec![]));
}

#[test]
fn a_paused_task_whose_window_exits_with_an_answer_in_its_record_stays_paused_with_no_result() {
    let context = Context::new();
    let tiers = working(&context);
    context.adapter.busy("zeus");
    context.pass().unwrap();
    context.pause_task(tiers.project.id, 1);
    context.adapter.answer("zeus", ANSWER);
    context.host.exits_showing(Some(0), None, None);
    context.exit("zeus");
    assert_eq!(after_exit(&context, &tiers), ("paused".to_owned(), vec![]));
}

#[test]
fn a_task_waiting_on_its_question_whose_window_exits_is_settled_as_before() {
    let context = Context::new();
    let tiers = working(&context);
    context.adapter.busy("zeus");
    context.pass().unwrap();
    context.adapter.answer("zeus", QUESTION);
    context.pass().unwrap();
    let before = after_exit(&context, &tiers);
    assert_eq!(before.0, "waiting");
    context.host.exits_showing(Some(0), None, None);
    context.exit("zeus");
    let at_exit = after_exit(&context, &tiers);
    context.pass().unwrap();
    assert_eq!(after_exit(&context, &tiers), at_exit);
    assert_eq!(at_exit, ("paused".to_owned(), vec![]), "as main settles it");
}

#[test]
fn a_turn_that_left_background_work_running_is_no_result_at_an_exit_either() {
    let context = Context::new();
    let tiers = working(&context);
    context.adapter.busy("zeus");
    context.pass().unwrap();
    context
        .adapter
        .answer_leaving_a_job("zeus", "Started the recording run");
    context.host.exits_showing(Some(0), None, None);
    context.exit("zeus");
    assert_eq!(after_exit(&context, &tiers), ("paused".to_owned(), vec![]));
}

/// The thread's messages, in order: what was said of the task, and to whom.
fn said(tiers: &crate::fixtures::Tiers<'_>) -> Vec<(String, String)> {
    tiers
        .task(1)
        .messages
        .into_iter()
        .map(|message| (message.kind, message.body))
        .collect()
}

#[test]
fn a_paused_or_background_task_is_told_nothing_more_for_the_answer_its_window_left_at_its_exit() {
    // What an exit says of a task is the same with the answer written and without it.
    let twin = |written: Option<&dyn Fn(&Context)>, pause: bool| {
        let context = Context::new();
        let tiers = working(&context);
        context.adapter.busy("zeus");
        context.pass().unwrap();
        if pause {
            context.pause_task(tiers.project.id, 1);
        }
        match written {
            Some(write) => write(&context),
            None => {}
        }
        context.host.exits_showing(Some(0), None, None);
        context.exit("zeus");
        said(&tiers)
    };
    let paused_with = twin(Some(&|c: &Context| c.adapter.answer("zeus", ANSWER)), true);
    assert_eq!(paused_with, twin(None, true));
    let job_with = twin(
        Some(&|c: &Context| c.adapter.answer_leaving_a_job("zeus", "Started the run")),
        false,
    );
    assert_eq!(job_with, twin(None, false));
}

#[test]
fn a_window_the_engine_closed_itself_keeps_the_pause_it_gave_with_an_answer_written() {
    let context = Context::new();
    let tiers = working(&context);
    context.adapter.busy("zeus");
    context.pass().unwrap();
    context.adapter.answer("zeus", ANSWER);
    context.close_project(tiers.project.id).unwrap();
    assert_eq!(after_exit(&context, &tiers), ("paused".to_owned(), vec![]));
}

#[test]
fn a_turn_that_failed_is_not_collected_at_its_exit() {
    let context = Context::new();
    let tiers = working(&context);
    context.adapter.busy("zeus");
    context.pass().unwrap();
    context
        .adapter
        .refuse("zeus", crate::fixtures::exhausted(None, None));
    context.host.exits_showing(Some(1), None, None);
    context.exit("zeus");
    let (state, results) = after_exit(&context, &tiers);
    assert_eq!((state.as_str(), results.len()), ("paused", 0));
}

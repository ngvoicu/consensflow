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
        if let Some(write) = written {
            write(&context);
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

/// A Devin-shaped end: its quota refusal with complete words in the record. An ordinary pass
/// takes the refusal as the quota's, and so does the exit that is first to see it (T-1904).
#[test]
fn a_refusal_first_seen_at_the_exit_is_the_quotas_as_after_a_pass_and_not_a_result() {
    use cf_engine::testing::Made;
    use cf_harness::records::Quota;
    use serde_json::json;
    for (refused, words, pass_first, expected) in [
        (false, true, false, ("done", 1)),
        (true, true, false, ("paused", 0)),
        (true, true, true, ("paused", 0)),
        (true, false, false, ("paused", 0)),
    ] {
        let context = Context::made(Made {
            harnesses: vec!["claude-code", "devin"],
            ..Made::default()
        });
        let project = context
            .open_project(json!({
                "directory": "/work/app", "name": "exit review",
                "chief": {"harness": "claude-code", "model": "claude-opus-5"},
                "staff": [{"agent": "zeus", "harness": "devin", "role": "worker", "tier": "standard"}]
            }))
            .unwrap()
            .unwrap();
        let tiers = crate::fixtures::Tiers {
            context: &context,
            project,
        };
        tiers.open();
        for _ in 0..4 {
            context.pass().unwrap();
        }
        let window = tiers.assignee(1);
        context.adapter.busy(&window);
        context.pass().unwrap();
        assert_eq!(tiers.task(1).task.state, "working");
        if words {
            context
                .adapter
                .answer(&window, "I recorded the first two findings.");
        } else {
            context.adapter.with(&window, |agent| {
                agent.settled = true;
                agent.failed = false;
            });
        }
        if refused {
            context.adapter.quota(
                &window,
                Some(Quota::Exhausted {
                    at: None,
                    resets_at: None,
                }),
            );
        }
        if pass_first {
            context.pass().unwrap();
        }
        context.host.exits_showing(Some(0), None, Some(&["done"]));
        context.exit(&window);
        let (state, results) = after_exit(&context, &tiers);
        assert_eq!(
            (state.as_str(), results.len()),
            expected,
            "refused {refused}, words {words}, pass first {pass_first}"
        );
    }
}

/// A member held by another window's refusal: its window B, whose message was received, exits
/// with a refusal dated before the hold. A pass leaves B's turn to the hold, and so does the
/// exit that sees it first (T-1920).
#[test]
fn a_window_of_a_member_held_by_its_sibling_is_no_result_at_its_exit_as_after_a_pass() {
    use cf_engine::testing::Made;
    use cf_harness::records::Quota;
    use serde_json::json;
    for exit_first in [false, true] {
        let context = Context::made(Made {
            harnesses: vec!["claude-code", "devin"],
            ..Made::default()
        });
        let project = context
            .open_project(json!({
                "directory": "/work/app", "name": "sibling hold",
                "chief": {"harness": "claude-code", "model": "claude-opus-5"},
                "staff": [{"agent": "zeus", "harness": "devin", "role": "worker", "tier": "standard"}]
            }))
            .unwrap()
            .unwrap();
        let tiers = crate::fixtures::Tiers {
            context: &context,
            project,
        };
        let passes = |n: usize| {
            for _ in 0..n {
                context.pass().unwrap();
            }
        };
        tiers.open();
        passes(4);
        let b = tiers.assignee(1);
        context.adapter.with(&b, |agent| {
            agent.settled = true;
            agent.at_prompt = true;
        });
        let until = crate::fixtures::after(&context, 30_000);
        context
            .ledger
            .borrow_mut()
            .hold_task(tiers.project.id, 1, &until, "one retry")
            .unwrap();
        let retry = context
            .ledger
            .borrow_mut()
            .resume_task(
                tiers.project.id,
                1,
                None,
                "Your last turn was refused by its provider: trying again. Go on where you stopped.",
            )
            .unwrap()
            .message
            .unwrap();
        passes(5);
        assert_eq!(context.message(retry.id).state, "delivered");
        context.give(tiers.project.id, "zeus", "A second window after receipt");
        passes(5);
        let a = tiers.assignee(2);
        assert_ne!(a, b);
        context.advance(1000);
        let dated = crate::fixtures::now(&context);
        context.advance(1000);
        context.adapter.refuse(
            &a,
            Quota::Exhausted {
                at: Some(crate::fixtures::now(&context)),
                resets_at: Some(crate::fixtures::soon(&context, 1)),
            },
        );
        passes(3);
        assert!(tiers.out_until("zeus").is_some(), "the member is held");
        context
            .adapter
            .answer(&b, "Partial words beside the refused turn");
        context.adapter.quota(
            &b,
            Some(Quota::Exhausted {
                at: Some(dated),
                resets_at: Some(crate::fixtures::soon(&context, 1)),
            }),
        );
        if !exit_first {
            context.pass().unwrap();
        }
        context.host.exits_showing(Some(0), None, Some(&["done"]));
        context.exit(&b);
        let given = results(&context, &tiers);
        assert!(given.is_empty(), "exit first {exit_first}: {given:?}");
    }
}

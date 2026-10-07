//! A window that stopped a turn before a word of its answer and took the
//! turn's message back out of its conversation (Claude: it is in its input
//! box again) was not given that message: the stop is paid at rest, and in
//! the same step the ledger keeps the message for the window again, so the
//! words that resume the task carry it. A stop that leaves the message in the
//! conversation (an interrupt record, words written) changes nothing. None of
//! these tests is held to a Node recording: the rule is Node's no more.

use cf_engine::testing::Context;
use cf_harness::records::Role;

use crate::fixtures::{assert_match, fail_next_take_back};

/// A project with zeus at work on T-1, its brief (m-1, "Parser") received.
fn working(context: &Context) -> i64 {
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.task(project.id, 1).task.state, "working");
    project.id
}

/// What was last pasted into zeus's window: the text of its newest user's item.
fn pasted(context: &Context) -> String {
    let items = context.adapter.agent("zeus").items;
    let last = items.iter().rev().find(|item| item.role == Role::User);
    last.expect("something was pasted").text.to_string()
}

#[test]
fn a_stop_paid_at_rest_by_a_window_that_took_its_brief_back_keeps_the_brief_for_the_words_that_resume_the_task(
) {
    let context = Context::new();
    let project = working(&context);
    // The turn is in the hooks of its prompt: stopped there, Claude writes no
    // record of it and has the brief in its input box again.
    context.adapter.busy_until_taken_back("zeus");
    context.pause_task(project, 1);
    context.pass().unwrap();
    assert_eq!(context.host.inputs().len(), 1, "one Escape");
    assert_eq!(
        context.message(1).state,
        "delivered",
        "at work, nothing is told of it"
    );

    // The next look finds the window at rest: the stop is paid, and the brief
    // is kept for it again, with why.
    context.pass().unwrap();
    let kept = context.message(1);
    assert_eq!(kept.state, "queued");
    assert_eq!(
        kept.reason.as_deref(),
        Some("@zeus stopped before a word of its answer and took it back out of its conversation")
    );
    assert_eq!(context.task(project, 1).task.state, "paused");
    assert_eq!(context.host.inputs().len(), 1, "paid with no second key");

    // The words that resume the task carry it.
    context.resume_task(project, 1, "Carry on");
    context.pass().unwrap();
    assert_match(
        &pasted(&context),
        r"^\[ConsensFlow m-\d+ · T-1 · task from @chief\]\n\(kept for you: task m-1 from @chief\)\nParser\n\nResumed: Carry on$",
    );
    assert_eq!(context.host.inputs().len(), 1, "no key with them");
    context.pass().unwrap();
    assert_eq!(context.task(project, 1).task.state, "working");
    assert_eq!(
        context.message(1).state,
        "delivered",
        "received with the words that carried it"
    );

    // The result is the turn after them.
    context.adapter.answer("zeus", "Parser, finished");
    context.pass().unwrap();
    let thread = context.task(project, 1);
    assert_eq!(thread.task.state, "done");
    let result = thread.messages.iter().find(|m| m.kind == "result").unwrap();
    assert_eq!(result.body, "Parser, finished");
}

#[test]
fn words_that_resumed_the_task_before_the_stop_was_paid_carry_the_brief_the_window_took_back() {
    let context = Context::new();
    let project = working(&context);
    context.adapter.busy_until_taken_back("zeus");
    context.pause_task(project, 1);
    // Resumed while the window was still being stopped: the brief was in its
    // conversation as far as the ledger knew, so the words did not carry it.
    context.resume_task(project, 1, "Carry on");
    context.pass().unwrap();
    assert_eq!(context.host.inputs().len(), 1, "one Escape");
    assert_eq!(context.task(project, 1).task.state, "queued");

    context.pass().unwrap();
    context.pass().unwrap();
    assert_match(
        &pasted(&context),
        r"^\[ConsensFlow m-\d+ · T-1 · task from @chief\]\n\(kept for you: task m-1 from @chief\)\nParser\n\nResumed: Carry on$",
    );
    assert_eq!(context.host.inputs().len(), 1, "no second key");
    context.pass().unwrap();
    assert_eq!(context.task(project, 1).task.state, "working");
    assert_eq!(context.message(1).state, "delivered");
}

/// How a window may stop and leave the brief in its conversation: it wrote
/// words, it stopped with a record of it, or the turn it stopped was another
/// message's than the task's last (the human typed in the window).
#[test]
fn a_stop_that_leaves_the_message_in_the_conversation_leaves_it_received_and_the_resume_carries_no_brief(
) {
    for how in [
        "words",
        "a record of the interrupt",
        "words of the human's own",
    ] {
        let context = Context::new();
        let project = working(&context);
        match how {
            "words" => context.adapter.busy("zeus"),
            "a record of the interrupt" => context.adapter.busy_until_interrupted("zeus"),
            _ => {
                // The human typed in the window since: the stopped turn began with their words.
                let typed = context.adapter.item(Role::User, "Wait, use JSON");
                context
                    .adapter
                    .with("zeus", |agent| agent.items.push(typed));
                context.adapter.busy_until_taken_back("zeus");
            }
        }
        context.pause_task(project, 1);
        context.pass().unwrap();
        if how == "words" {
            context.adapter.answer("zeus", "Half a parser");
        }
        context.pass().unwrap();
        assert_eq!(context.message(1).state, "delivered", "{how}");
        assert_eq!(context.host.inputs().len(), 1, "{how}");

        context.resume_task(project, 1, "Carry on");
        context.pass().unwrap();
        assert_match(
            &pasted(&context),
            r"^\[ConsensFlow m-\d+ · T-1 · task from @chief\]\nResumed: Carry on$",
        );
        context.pass().unwrap();
        assert_eq!(context.task(project, 1).task.state, "working", "{how}");
    }
}

#[test]
fn a_ledger_that_could_not_be_written_leaves_the_stop_owed_and_the_next_look_at_rest_takes_it_back()
{
    let context = Context::new();
    let project = working(&context);
    context.adapter.busy_until_taken_back("zeus");
    context.pause_task(project, 1);
    context.pass().unwrap();
    assert_eq!(context.host.inputs().len(), 1);

    fail_next_take_back(&context);
    assert!(context.pass().is_err(), "the pass says the write failed");
    assert_eq!(
        context.message(1).state,
        "delivered",
        "the write failed, and the stop was not paid before it"
    );
    context.advance(3_100);
    context.pass().unwrap();
    assert_eq!(
        context.message(1).state,
        "queued",
        "the look that found the window at rest again told it"
    );
    assert_eq!(
        context.host.inputs().len(),
        1,
        "and a window at rest was given no key, owed or not"
    );
    context.resume_task(project, 1, "Carry on");
    context.pass().unwrap();
    assert_match(
        &pasted(&context),
        r"\(kept for you: task m-1 from @chief\)\nParser\n\nResumed: Carry on$",
    );
}

//! Approval required, through the dispatcher: what a window is given, and when,
//! while words wait at the gate for the human. The words that resume a paused
//! task, a result the chief decided on before it was given it, and a result the
//! chief was given once; the brief of a task that was paused, cancelled or
//! declined at the gate; approval switched off, and the daemon restarted, while
//! words wait. None of these is held to a Node recording: the live app of the
//! reports behind them (alpha.81) ran Node's engine, whose reader never read a
//! window stopped by an id-less interrupt at rest, and which pasted a result
//! after the decision that withdraws it.

use cf_engine::testing::Context;
use cf_engine::SwitchWhen;
use cf_harness::contract::Pane;
use cf_harness::records::Role;
use cf_ledger::{MessageView, NewNote};
use serde_json::{json, Value};

use crate::chiefs::{handoffs_of, to, with_codex};
use crate::fixtures::assert_match;

/// What pressing Escape into `pane` is, as the host is asked it.
fn escape(pane: &Pane) -> Value {
    json!({ "id": pane.id, "generation": pane.generation, "bytes": [27] })
}

/// The pane of the last window of `handle`.
fn pane_of(context: &Context, handle: &str) -> Pane {
    context.host.last(handle).expect("its window").pane
}

/// The human passes a message on.
fn approve(context: &Context, id: i64) {
    context
        .ledger
        .borrow_mut()
        .approve_message(id, "human")
        .unwrap();
}

/// A project whose human approves every hand-off, with zeus at work on T-1:
/// its brief passed on, and received.
fn working(context: &Context) -> i64 {
    let project = context.with_staff(&["zeus"]);
    context
        .ledger
        .borrow_mut()
        .set_gate(project.id, true)
        .unwrap();
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    let brief = context.task(project.id, 1).messages[0].clone();
    assert_eq!(brief.state, "gated", "held for the human");
    approve(context, brief.id);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.task(project.id, 1).task.state, "working");
    project.id
}

/// What the window of `handle` was given as messages, oldest first.
fn given(context: &Context, handle: &str) -> Vec<String> {
    context
        .adapter
        .agent(handle)
        .items
        .iter()
        .filter(|item| item.role == Role::User)
        .map(|item| item.text.to_string())
        .collect()
}

/// The chief pauses T-1 and resumes it a moment later with `words`: the
/// message that carries them, held for the human.
fn paused_and_resumed(context: &Context, project: i64, words: &str) -> i64 {
    context.pause_task(project, 1);
    context.resume_task(project, 1, words);
    let held = context.task(project, 1).messages.last().unwrap().clone();
    assert_eq!(
        (held.kind.as_str(), held.state.as_str()),
        ("task", "gated"),
        "the words wait for the human"
    );
    held.id
}

#[test]
fn words_that_resume_a_paused_task_wait_for_the_human_and_the_window_is_stopped_all_the_same() {
    let context = Context::new();
    let project = working(&context);
    context.adapter.busy_until_interrupted("zeus");
    let words = paused_and_resumed(&context, project, "Use JSON");
    context.pass().unwrap();
    let pane = pane_of(&context, "zeus");
    assert_eq!(
        context.host.inputs(),
        [escape(&pane)],
        "the turn the pause stopped is interrupted whether or not the words are passed on"
    );
    context.pass().unwrap();
    assert_eq!(
        given(&context, "zeus").len(),
        1,
        "the window at rest is given nothing while the words wait"
    );
    assert_eq!(context.task(project, 1).task.state, "queued");
    assert_eq!(context.message(words).state, "gated");
}

#[test]
fn words_the_human_passes_on_after_the_window_stopped_go_in_at_once_and_the_task_is_working_again()
{
    let context = Context::new();
    let project = working(&context);
    context.adapter.busy_until_interrupted("zeus");
    let words = paused_and_resumed(&context, project, "Use JSON");
    context.pass().unwrap();
    context.pass().unwrap();
    // The human approves them twelve seconds later.
    context.advance(12_000);
    approve(&context, words);
    context.pass().unwrap();
    let seen = given(&context, "zeus");
    assert_match(
        seen.last().unwrap(),
        r"T-1 · task from @chief\]\nResumed: Use JSON$",
    );
    assert_eq!(context.host.inputs().len(), 1, "no second key");
    context.pass().unwrap();
    assert_eq!(context.task(project, 1).task.state, "working");
    assert_eq!(context.message(words).state, "delivered");
}

#[test]
fn words_the_human_passes_on_before_the_window_has_stopped_wait_for_it_and_go_in_once_it_has() {
    let context = Context::new();
    let project = working(&context);
    // The window ignores every round of Escape and ends its turn by itself,
    // after the third.
    context.adapter.busy("zeus");
    let words = paused_and_resumed(&context, project, "Use JSON");
    approve(&context, words);
    context.pass().unwrap();
    assert_eq!(context.message(words).state, "queued");
    assert_eq!(
        given(&context, "zeus").len(),
        1,
        "nothing is pasted into a turn at work"
    );
    for _ in 0..2 {
        context.advance(3_100);
        context.pass().unwrap();
    }
    assert_eq!(
        context.host.inputs().len(),
        3,
        "three rounds, as for any stop"
    );
    context.adapter.answer("zeus", "Half a parser");
    context.pass().unwrap();
    let seen = given(&context, "zeus");
    assert_match(
        seen.last().unwrap(),
        r"T-1 · task from @chief\]\nResumed: Use JSON$",
    );
    context.pass().unwrap();
    assert_eq!(context.task(project, 1).task.state, "working");
    assert_eq!(
        context.host.inputs().len(),
        3,
        "paid at rest: no key goes in with the words"
    );
}

#[test]
fn a_second_resume_before_the_first_was_passed_on_takes_it_back_and_only_the_last_words_are_typed()
{
    let context = Context::new();
    let project = working(&context);
    context.adapter.busy_until_interrupted("zeus");
    let first = paused_and_resumed(&context, project, "Use YAML");
    context.pass().unwrap();
    // The chief thinks again before the human has seen the first words.
    let second = paused_and_resumed(&context, project, "Use JSON");
    assert_eq!(
        context.message(first).reason.as_deref(),
        Some("withdrawn by @chief's pause")
    );
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(given(&context, "zeus").len(), 1, "nothing is typed yet");

    approve(&context, second);
    context.pass().unwrap();
    let seen = given(&context, "zeus");
    assert_eq!(seen.len(), 2);
    assert_match(
        seen.last().unwrap(),
        r"T-1 · task from @chief\]\nResumed: Use JSON$",
    );
    context.pass().unwrap();
    assert_eq!(context.task(project, 1).task.state, "working");
    assert!(
        given(&context, "zeus")
            .iter()
            .all(|text| !text.contains("Use YAML")),
        "the words the chief took back are never typed"
    );
}

#[test]
fn a_turn_stopped_before_its_first_word_is_taken_back_and_the_brief_rides_in_the_words_the_human_passes_on(
) {
    let context = Context::new();
    let project = working(&context);
    // The brief just reached the window, which stops in the hooks of its prompt
    // and puts the message back in its input box.
    context.adapter.busy_until_taken_back("zeus");
    let words = paused_and_resumed(&context, project, "Use JSON");
    context.pass().unwrap();
    context.pass().unwrap();
    let thread = context.task(project, 1);
    let brief = thread.messages[0].clone();
    assert_eq!(
        (brief.state.as_str(), brief.id != words),
        ("queued", true),
        "the brief was not received after all: it is kept for the window again"
    );
    assert_eq!(given(&context, "zeus").len(), 1, "nothing goes in yet");
    approve(&context, words);
    context.pass().unwrap();
    let seen = given(&context, "zeus");
    assert_eq!(seen.len(), 2, "one paste");
    assert_match(
        seen.last().unwrap(),
        r"(?s)T-1 · task from @chief\].*Parser.*Resumed: Use JSON",
    );
    context.pass().unwrap();
    let thread = context.task(project, 1);
    assert_eq!(thread.task.state, "working");
    assert_eq!(
        thread
            .messages
            .iter()
            .map(|message| message.state.as_str())
            .collect::<Vec<_>>(),
        ["delivered", "delivered"]
    );
}

/// How many messages the window of `handle` was given that open with the
/// header of message `id`.
fn headed(context: &Context, handle: &str, id: i64) -> usize {
    given(context, handle)
        .iter()
        .filter(|text| text.starts_with(&format!("[ConsensFlow m-{id} ")))
        .count()
}

/// The result of T-1, as the thread of the task has it.
fn result_of(context: &Context, project: i64) -> MessageView {
    context
        .task(project, 1)
        .messages
        .into_iter()
        .find(|message| message.kind == "result")
        .expect("a result")
}

/// The chief is in a turn of its own when zeus finishes T-1: the result waits
/// at the gate, the human passes it on, and it waits behind the turn.
fn passed_on_in_the_chiefs_turn(context: &Context, project: i64) -> MessageView {
    context.adapter.busy("chief");
    context.adapter.answer("zeus", "Parser done");
    context.pass().unwrap();
    context.pass().unwrap();
    let result = result_of(context, project);
    assert_eq!(result.state, "gated", "the human has not passed it on");
    assert!(
        context.inbox(context.id(project, "chief")).is_empty(),
        "the chief's inbox does not show it"
    );
    approve(context, result.id);
    context.pass().unwrap();
    context.pass().unwrap();
    let result = context.message(result.id);
    assert_eq!(result.state, "queued", "a busy chief is not interrupted");
    result
}

/// The chief's turn ends, and the dispatcher looks at its window.
fn the_chiefs_turn_ends(context: &Context) {
    context.adapter.with("chief", |agent| agent.settled = true);
    context.pass().unwrap();
    context.pass().unwrap();
}

#[test]
fn a_result_passed_on_in_the_chiefs_turn_and_decided_on_there_is_not_pasted_when_the_turn_ends() {
    for sends_back in [false, true] {
        let context = Context::new();
        let project = working(&context);
        let result = passed_on_in_the_chiefs_turn(&context, project);

        // In that turn the chief reads it, whole, which receives nothing, and decides.
        let decided = if sends_back {
            context
                .ledger
                .borrow_mut()
                .reopen_task(project, 1, "chief", "Handle empty input too")
                .map(|_| ())
        } else {
            context
                .ledger
                .borrow_mut()
                .accept_task(project, 1, "chief")
                .map(|_| ())
        };
        decided.unwrap();
        let withdrawn = context.message(result.id);
        assert_eq!(
            (withdrawn.state.as_str(), withdrawn.reason.as_deref()),
            (
                "cancelled",
                Some(if sends_back {
                    "T-1 was sent back"
                } else {
                    "T-1 was accepted"
                })
            ),
            "sent back: {sends_back}"
        );

        the_chiefs_turn_ends(&context);
        assert_eq!(
            headed(&context, "chief", result.id),
            0,
            "the chief is never given it, sent back: {sends_back}"
        );
        assert!(
            given(&context, "chief")
                .iter()
                .all(|text| !text.contains("Decide with")),
            "{:?}",
            given(&context, "chief")
        );
        if sends_back {
            // What it sent back with waits at the gate like any words, and goes
            // on to the window once the human passes it on.
            let words = context.task(project, 1).messages.last().unwrap().clone();
            assert_eq!(
                (words.kind.as_str(), words.state.as_str()),
                ("task", "gated")
            );
            assert!(given(&context, "zeus")
                .iter()
                .all(|text| !text.contains("Handle empty input too")));
            approve(&context, words.id);
            context.pass().unwrap();
            context.pass().unwrap();
            assert!(given(&context, "zeus")
                .iter()
                .any(|text| text.contains("Handle empty input too")));
        }
    }
}

#[test]
fn a_result_passed_on_in_the_chiefs_turn_is_pasted_once_when_the_turn_ends_and_not_before() {
    let context = Context::new();
    let project = working(&context);
    let result = passed_on_in_the_chiefs_turn(&context, project);
    for _ in 0..4 {
        context.advance(3_100);
        context.pass().unwrap();
        assert_eq!(headed(&context, "chief", result.id), 0, "behind the turn");
    }
    the_chiefs_turn_ends(&context);
    assert_eq!(headed(&context, "chief", result.id), 1);
    assert_eq!(context.message(result.id).state, "delivered");
    // The window takes the next turn and rests again: nothing is pasted twice.
    context.adapter.answer("chief", "Read it.");
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(headed(&context, "chief", result.id), 1, "once");
    assert_eq!(
        context.task(project, 1).task.state,
        "done",
        "still undecided"
    );
}

#[test]
fn a_result_that_waits_at_the_gate_is_not_the_chiefs_to_decide_and_is_pasted_once_the_human_passes_it_on(
) {
    let context = Context::new();
    let project = working(&context);
    context.adapter.answer("zeus", "Parser done");
    context.pass().unwrap();
    context.pass().unwrap();
    let result = result_of(&context, project);
    assert_eq!(result.state, "gated");
    // The chief tries to decide it before it was given it: refused, and nothing changes.
    let accepted = context
        .ledger
        .borrow_mut()
        .accept_task(project, 1, "chief")
        .unwrap_err();
    assert_eq!(accepted.code(), Some("result-gated"));
    assert_eq!(context.task(project, 1).task.state, "done");
    context.pass().unwrap();
    assert_eq!(headed(&context, "chief", result.id), 0);

    approve(&context, result.id);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(headed(&context, "chief", result.id), 1);
    assert_eq!(context.message(result.id).state, "delivered");
}

#[test]
fn a_task_the_human_decided_before_passing_its_result_on_leaves_the_chief_no_result_to_be_given() {
    let context = Context::new();
    let project = working(&context);
    context.adapter.answer("zeus", "Parser done");
    context.pass().unwrap();
    context.pass().unwrap();
    let result = result_of(&context, project);
    context
        .ledger
        .borrow_mut()
        .accept_task(project, 1, "human")
        .unwrap();
    context.pass().unwrap();
    context.pass().unwrap();
    let withdrawn = context.message(result.id);
    assert_eq!(
        (withdrawn.state.as_str(), withdrawn.reason.as_deref()),
        ("cancelled", Some("accepted by @human"))
    );
    assert_eq!(headed(&context, "chief", result.id), 0);
}

#[test]
fn a_brief_the_human_declines_opens_no_window_and_the_chief_is_told_it_is_cancelled() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context
        .ledger
        .borrow_mut()
        .set_gate(project.id, true)
        .unwrap();
    context.pool_task(project.id, "worker", "Write the parser");
    context.pass().unwrap();
    context.pass().unwrap();
    let brief = context.task(project.id, 1).messages[0].clone();
    assert_eq!(brief.state, "gated");
    context
        .ledger
        .borrow_mut()
        .decline_message(brief.id, "human")
        .unwrap();
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.task(project.id, 1).task.state, "cancelled");
    assert!(context.host.last("zeus").is_none(), "no window opened");
    let told = given(&context, "chief");
    assert_eq!(told.len(), 1, "{told:?}");
    assert_match(
        &told[0],
        r"note from @human\]\n@human declined T-1 \(Write the parser\)\. It is cancelled\.$",
    );
}

#[test]
fn words_the_human_declines_cancel_the_task_and_the_window_stopped_for_them_is_given_nothing_and_closes(
) {
    let context = Context::new();
    let project = working(&context);
    context.adapter.busy_until_interrupted("zeus");
    let words = paused_and_resumed(&context, project, "Use JSON");
    context.pass().unwrap();
    context.pass().unwrap();
    let pane = pane_of(&context, "zeus");
    context
        .ledger
        .borrow_mut()
        .decline_message(words, "human")
        .unwrap();
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.task(project, 1).task.state, "cancelled");
    assert_eq!(given(&context, "zeus").len(), 1, "never given the words");
    assert!(
        context.host.killed().contains(&pane),
        "the window of a task that is over closes"
    );
}

#[test]
fn a_task_cancelled_while_its_words_wait_at_the_gate_is_given_nothing_after_and_its_window_closes()
{
    let context = Context::new();
    let project = working(&context);
    context.adapter.busy_until_interrupted("zeus");
    let words = paused_and_resumed(&context, project, "Use JSON");
    context.pass().unwrap();
    context.pass().unwrap();
    let pane = pane_of(&context, "zeus");
    context
        .ledger
        .borrow_mut()
        .cancel_task(project, 1, "chief")
        .unwrap();
    assert_eq!(context.message(words).state, "cancelled");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(given(&context, "zeus").len(), 1, "never given the words");
    assert!(context.host.killed().contains(&pane));
    let refused = context
        .ledger
        .borrow_mut()
        .approve_message(words, "human")
        .unwrap_err();
    assert_eq!(refused.code(), Some("invalid-transition"));
}

#[test]
fn a_pause_of_a_task_whose_brief_waits_at_the_gate_leaves_no_window_and_the_words_that_resume_open_it_with_the_brief(
) {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context
        .ledger
        .borrow_mut()
        .set_gate(project.id, true)
        .unwrap();
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    let brief = context.task(project.id, 1).messages[0].clone();
    assert_eq!(brief.state, "gated");
    context.pause_task(project.id, 1);
    assert_eq!(context.message(brief.id).state, "cancelled");
    context.resume_task(project.id, 1, "Go on");
    let words = context.task(project.id, 1).messages.last().unwrap().clone();
    assert_eq!(words.state, "gated");
    context.pass().unwrap();
    context.pass().unwrap();
    assert!(context.host.last("zeus").is_none(), "no window yet");
    approve(&context, words.id);
    context.pass().unwrap();
    let launch = context.adapter.prepared().last().cloned().unwrap();
    assert_match(
        launch["message"].as_str().unwrap(),
        r"T-1 · task from @chief\]\nParser\n\nResumed: Go on$",
    );
    context.pass().unwrap();
    assert_eq!(context.task(project.id, 1).task.state, "working");
}

#[test]
fn a_brief_passed_on_while_the_human_has_the_task_paused_is_given_to_no_window_until_the_task_goes_on(
) {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context
        .ledger
        .borrow_mut()
        .set_gate(project.id, true)
        .unwrap();
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    let brief = context.task(project.id, 1).messages[0].clone();
    context.human_pause(project.id, 1);
    assert_eq!(
        context.message(brief.id).state,
        "gated",
        "the chief's words stay"
    );
    approve(&context, brief.id);
    context.pass().unwrap();
    context.pass().unwrap();
    assert!(
        context.host.last("zeus").is_none(),
        "a paused task opens no window"
    );

    // The chief resumes it: its words wait, and the brief goes in with them.
    context.resume_task(project.id, 1, "Go on");
    let words = context.task(project.id, 1).messages.last().unwrap().clone();
    context.pass().unwrap();
    assert!(context.host.last("zeus").is_none());
    approve(&context, words.id);
    context.pass().unwrap();
    context.pass().unwrap();
    let launch = context.adapter.prepared().last().cloned().unwrap();
    assert_match(
        launch["message"].as_str().unwrap(),
        r"(?s)T-1 · task from @chief\].*Parser.*Resumed: Go on",
    );
    assert_eq!(context.task(project.id, 1).task.state, "working");
}

#[test]
fn words_that_send_a_finished_task_back_wait_at_the_gate_and_its_window_is_given_them_once_they_are_passed_on(
) {
    let context = Context::new();
    let project = working(&context);
    context.adapter.answer("zeus", "Parser done");
    context.pass().unwrap();
    context.pass().unwrap();
    let result = result_of(&context, project);
    approve(&context, result.id);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.message(result.id).state, "delivered");

    context
        .ledger
        .borrow_mut()
        .reopen_task(project, 1, "chief", "Handle empty input too")
        .unwrap();
    let words = context.task(project, 1).messages.last().unwrap().clone();
    assert_eq!(
        (words.kind.as_str(), words.state.as_str()),
        ("task", "gated")
    );
    context.pass().unwrap();
    context.pass().unwrap();
    assert!(
        given(&context, "zeus")
            .iter()
            .all(|text| !text.contains("Handle empty input too")),
        "nothing typed while the words wait"
    );
    approve(&context, words.id);
    context.pass().unwrap();
    context.pass().unwrap();
    assert!(given(&context, "zeus")
        .iter()
        .any(|text| text.contains("Handle empty input too")));
    assert_eq!(context.task(project, 1).task.state, "working");
}

#[test]
fn what_waits_at_the_gate_when_approval_is_switched_off_stays_there_and_what_comes_after_is_not_held(
) {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context
        .ledger
        .borrow_mut()
        .set_gate(project.id, true)
        .unwrap();
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    let brief = context.task(project.id, 1).messages[0].clone();
    context
        .ledger
        .borrow_mut()
        .set_gate(project.id, false)
        .unwrap();
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.message(brief.id).state, "gated");
    assert!(
        context.host.last("zeus").is_none(),
        "no window while it waits"
    );

    approve(&context, brief.id);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.task(project.id, 1).task.state, "working");
    context.adapter.answer("zeus", "Parser done");
    for _ in 0..3 {
        context.pass().unwrap();
    }
    // The result is not held, as nothing is now: the chief is given it.
    let result = result_of(&context, project.id);
    assert_eq!(result.state, "delivered");
    assert_eq!(headed(&context, "chief", result.id), 1);
}

#[test]
fn a_restart_leaves_the_words_at_the_gate_where_they_were_and_they_go_in_once_they_are_passed_on() {
    let context = Context::new();
    let project = working(&context);
    context.adapter.busy_until_interrupted("zeus");
    let words = paused_and_resumed(&context, project, "Use JSON");
    context.pass().unwrap();
    context.pass().unwrap();
    let launches_of_zeus = || {
        context
            .adapter
            .prepared()
            .into_iter()
            .filter(|launch| launch["participant"]["handle"] == "zeus")
            .collect::<Vec<_>>()
    };
    assert_eq!(launches_of_zeus().len(), 1);

    // The app quits and comes back, and its windows are gone: the human resumes
    // the project, and the words wait for them still: no window opens for them.
    context.ledger.borrow_mut().suspend_for_restart().unwrap();
    let after = context.make();
    after.settle_after_restart().unwrap();
    after.resume_project(project).unwrap();
    assert_eq!(context.message(words).state, "gated");
    after.pass().unwrap();
    after.pass().unwrap();
    assert_eq!(
        launches_of_zeus().len(),
        1,
        "no window opens for the words while they wait"
    );
    assert_eq!(context.task(project, 1).task.state, "queued");

    // Passed on, they open the window again, on the conversation it had.
    approve(&context, words);
    after.pass().unwrap();
    let launches = launches_of_zeus();
    assert_eq!(launches.len(), 2);
    assert_match(
        launches[1]["message"].as_str().unwrap(),
        r"T-1 · task from @chief\]\nResumed: Use JSON$",
    );
    assert_eq!(
        launches[1]["resume"],
        json!(format!(
            "native-{}",
            launches[0]["launchId"].as_str().unwrap()
        )),
        "the conversation it had"
    );
    after.pass().unwrap();
    assert_eq!(context.task(project, 1).task.state, "working");
}

#[test]
fn a_tell_the_human_has_not_passed_on_waits_while_the_window_it_stopped_rests_and_goes_in_once_passed_on(
) {
    let context = Context::new();
    let project = working(&context);
    context.adapter.busy_until_interrupted("zeus");
    let told = context.tell(project, "zeus", 1, "Stop: use grammar v2");
    assert_eq!(told.state, "gated");
    context.pass().unwrap();
    let pane = pane_of(&context, "zeus");
    assert_eq!(
        context.host.inputs(),
        [escape(&pane)],
        "the task is paused for the tell, which stops its window all the same"
    );
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(
        given(&context, "zeus").len(),
        1,
        "the tell waits for the human"
    );
    assert_eq!(context.task(project, 1).task.state, "paused");

    approve(&context, told.id);
    context.pass().unwrap();
    let seen = given(&context, "zeus");
    assert_eq!(seen.len(), 2);
    assert_match(
        seen.last().unwrap(),
        r#"question from @chief\]\nStop: use grammar v2\n\nT-1 is paused for this\. Run in your shell: cf answer m-\d+ "…"; the chief resumes the task\.$"#,
    );
    assert_eq!(context.host.inputs().len(), 1, "no second key");
}

#[test]
fn the_handoff_a_switched_chief_is_given_is_consensflows_own_word_and_the_gate_does_not_hold_it() {
    let context = with_codex();
    let project = context.with_staff(&["zeus"]);
    context
        .ledger
        .borrow_mut()
        .set_gate(project.id, true)
        .unwrap();
    context.pass().unwrap();
    context.adapter.answer("chief", "Hello");
    context.pass().unwrap();
    context
        .switch_chief(project.id, to("codex", "astraeus"), SwitchWhen::Now, false)
        .unwrap();
    let handoffs = handoffs_of(&context, project.id);
    assert_eq!(
        handoffs.len(),
        1,
        "it is in the chief's inbox, not at the gate"
    );
    assert_ne!(handoffs[0].state, "gated");
    let launched = context.codex.prepared().last().cloned().unwrap();
    assert_match(
        launched["message"].as_str().unwrap(),
        r"You are the chief now\. The human switched this project's chief from Claude Code \(apollo\) to you, Codex \(astraeus\)\.",
    );
}

#[test]
fn a_note_and_a_question_the_human_passes_on_while_the_chief_is_in_a_turn_reach_it_oldest_first_once(
) {
    let context = Context::new();
    let project = working(&context);
    context.adapter.busy("chief");
    let note = context.note(project, "zeus", "chief", "Mind the tests");
    let question = context.ask(project, "zeus", "chief", 1, "Which format?");
    assert_eq!(
        [
            context.message(note.id).state,
            context.message(question.id).state
        ],
        ["gated", "gated"]
    );
    // The human passes the later one on first.
    approve(&context, question.id);
    approve(&context, note.id);
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(
        [
            headed(&context, "chief", note.id),
            headed(&context, "chief", question.id)
        ],
        [0, 0],
        "a busy chief is given nothing"
    );

    // Its turn ends: one message at a time, the oldest first.
    the_chiefs_turn_ends(&context);
    assert_eq!(
        [
            headed(&context, "chief", note.id),
            headed(&context, "chief", question.id)
        ],
        [1, 0]
    );
    context.adapter.answer("chief", "Read the note.");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(
        [
            headed(&context, "chief", note.id),
            headed(&context, "chief", question.id)
        ],
        [1, 1]
    );
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(
        [
            headed(&context, "chief", note.id),
            headed(&context, "chief", question.id)
        ],
        [1, 1],
        "each once"
    );
}

#[test]
fn a_result_waiting_at_the_gate_is_there_after_a_restart_and_reaches_the_chief_once_passed_on() {
    let context = Context::new();
    let project = working(&context);
    context.adapter.answer("zeus", "Parser done");
    context.pass().unwrap();
    context.pass().unwrap();
    let result = result_of(&context, project);
    assert_eq!(result.state, "gated");

    context.ledger.borrow_mut().suspend_for_restart().unwrap();
    let after = context.make();
    after.settle_after_restart().unwrap();
    after.resume_project(project).unwrap();
    after.pass().unwrap();
    assert_eq!(context.message(result.id).state, "gated");
    assert_eq!(headed(&context, "chief", result.id), 0);
    approve(&context, result.id);
    after.pass().unwrap();
    after.pass().unwrap();
    assert_eq!(headed(&context, "chief", result.id), 1);
    assert_eq!(context.message(result.id).state, "delivered");
}

/// A note the chief tells zeus's window about T-1, held for the human, when the window
/// closes by itself: the task is paused, and the note waits for the human's word and then
/// for the resume, named so in the chief's note of the pause, and the human hears of it
/// once, as waiting for their approval first (T-414 P3-3: a held note went unnamed).
#[test]
fn a_note_held_for_the_human_when_the_window_closes_is_named_as_waiting_for_the_human_then_the_resume(
) {
    let context = Context::new();
    let project = working(&context);
    let note = context
        .ledger
        .borrow_mut()
        .note(
            project,
            &NewNote {
                from: Some("chief".to_owned()),
                to: "zeus".to_owned(),
                task: Some(1),
                body: "Use the grammar in the docs".to_owned(),
            },
        )
        .unwrap();
    assert_eq!(
        context.message(note.id).state,
        "gated",
        "held for the human"
    );

    context.host.exits_showing(Some(3), None, None);
    context.exit("zeus");
    context.pass().unwrap();

    assert_eq!(context.task(project, 1).task.state, "paused");
    let held = context.message(note.id);
    assert_eq!(
        (held.state.as_str(), held.reason.as_deref()),
        ("gated", None),
        "still the human's, the gate's own reason kept"
    );
    let why = "@zeus's window closed (exit code 3)";
    let chief = context.id(project, "chief");
    let paused: Vec<String> = context
        .inbox(chief)
        .into_iter()
        .map(|message| message.body)
        .filter(|said| said.starts_with("T-1 is paused: "))
        .collect();
    assert_eq!(
        paused,
        [format!(
            "T-1 is paused: {why}; the words told its window wait for the human, then the resume: m-{}. Resume it with: cf task resume T-1 \"…\"; its window comes back on its own conversation.",
            note.id
        )]
    );
    let human = context.id(project, "human");
    assert_eq!(
        context
            .inbox(human)
            .into_iter()
            .map(|message| message.body)
            .filter(|said| said.starts_with(&format!("m-{},", note.id)))
            .collect::<Vec<_>>(),
        [format!(
            "m-{}, a note from @chief on T-1, waits for your approval, then T-1's resume, to reach @zeus: {why}.",
            note.id
        )]
    );
}

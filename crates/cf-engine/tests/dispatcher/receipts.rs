//! What makes an answer received, and what a task does until it is: a door's
//! claim that nobody acknowledged is voided when its window rests, shows a
//! dialog of its own, exits or the daemon starts; an answer `cf` says it
//! printed whole is received; a turn that ends while the task waits makes no
//! result. None of these tests is held to a Node recording: Node's answer was
//! read when it was written.

use cf_engine::delivery_text::marker_of;
use cf_engine::testing::Context;
use cf_harness::contract::Waiting;
use cf_ledger::{Claim, NewQuestion, Read};
use serde_json::json;

use crate::fixtures::assert_match;

/// zeus at work on T-1 with a question of its harness's tool asked through a
/// door, and the chief's choice for it: the project, the question, the answer.
fn answered_through_a_door(context: &Context) -> (i64, i64, i64) {
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    let question = context
        .ledger
        .borrow_mut()
        .ask(
            project.id,
            &NewQuestion {
                from: Some("zeus".to_owned()),
                to: "chief".to_owned(),
                task: Some(1),
                questions: Some(json!([{
                    "question": "Which colour?", "header": "Colour",
                    "options": [{ "label": "red" }, { "label": "blue" }], "multiple": false,
                }])),
                ..NewQuestion::default()
            },
        )
        .unwrap();
    let chief = context.id(project.id, "chief");
    let answer = context
        .ledger
        .borrow_mut()
        .answer(question.id, chief, None, Some(&json!([["blue"]])))
        .unwrap();
    (project.id, question.id, answer.id)
}

/// The door of zeus claims the answer, as its poll does.
fn claim(context: &Context, project: i64, question: i64) {
    let zeus = context.id(project, "zeus");
    let claimed = context
        .ledger
        .borrow_mut()
        .claim_answer(question, zeus)
        .unwrap();
    assert!(matches!(claimed, Claim::Answered(_)));
}

/// How many items of zeus's window hold the marker of message `id`.
fn pasted(context: &Context, id: i64) -> usize {
    context
        .adapter
        .agent("zeus")
        .items
        .iter()
        .filter(|item| item.text.contains(&marker_of(id)))
        .count()
}

/// What voided each claim the log says was given back, in order.
fn unclaimed(context: &Context, project: i64) -> Vec<String> {
    context
        .ledger
        .borrow()
        .events(project, 0, 500)
        .unwrap()
        .iter()
        .filter(|event| event.kind == "delivery.unclaimed")
        .map(|event| event.data["because"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn an_unacknowledged_claim_is_voided_at_the_look_that_finds_the_window_at_rest_and_the_answer_is_pasted_once(
) {
    let context = Context::new();
    let (project, question, answer) = answered_through_a_door(&context);
    claim(&context, project, question);
    // The window is at work: the door may yet hand the answer over, and nothing is pasted.
    context.pass().unwrap();
    assert_eq!(context.message(answer).state, "queued");
    assert_eq!(pasted(&context, answer), 0);
    // Its turn is over, and nobody said the answer was handed over.
    context.adapter.answer("zeus", "Waiting for the answer");
    context.pass().unwrap();
    assert_eq!(unclaimed(&context, project), ["its window is at rest"]);
    assert_eq!(pasted(&context, answer), 1, "pasted at that look, once");
    context.pass().unwrap();
    assert_eq!(context.message(answer).state, "delivered");
    context.pass().unwrap();
    assert_eq!(pasted(&context, answer), 1);
}

/// The door's transport failed after its poll claimed the answer, and the
/// harness's own dialog took over: the window waits on it, which is neither
/// at work nor at rest. Nobody that holds the claim is left, so the look gives
/// it up; a dialog is no place to paste into, so the answer waits for the
/// window to leave it, and is then pasted once and received: the task leaves
/// waiting.
#[test]
fn a_claim_whose_door_gave_up_for_the_windows_own_dialog_is_voided_and_the_answer_is_pasted_once_the_dialog_is_gone(
) {
    let context = Context::new();
    let (project, question, answer) = answered_through_a_door(&context);
    claim(&context, project, question);
    let zeus = context.id(project, "zeus");
    assert_eq!(context.task(project, 1).task.state, "waiting");
    context.adapter.with("zeus", |agent| {
        agent.waiting = Some(Waiting {
            reason: Some("a question".to_owned()),
        });
    });
    context.pass().unwrap();
    assert_eq!(
        unclaimed(&context, project),
        ["its window shows a dialog of its own"]
    );
    assert_eq!(
        pasted(&context, answer),
        0,
        "nothing is pasted into a dialog"
    );
    assert_eq!(context.message(answer).state, "queued");
    assert_eq!(
        context
            .ledger
            .borrow()
            .next_delivery(zeus)
            .unwrap()
            .map(|message| message.id),
        Some(answer),
        "the ledger's again, to deliver when the window can take it"
    );
    // Stays on the dialog: nothing is pasted at any look, and nothing is given up twice.
    context.pass().unwrap();
    assert_eq!(pasted(&context, answer), 0);
    // The dialog is left, the turn over: the window is at rest, and takes the answer.
    context.adapter.with("zeus", |agent| {
        agent.waiting = None;
        agent.settled = true;
    });
    context.pass().unwrap();
    assert_eq!(pasted(&context, answer), 1, "pasted once, at that look");
    context.pass().unwrap();
    assert_eq!(context.message(answer).state, "delivered");
    assert_eq!(context.task(project, 1).task.state, "working");
    assert_eq!(pasted(&context, answer), 1);
}

#[test]
fn a_dialog_of_its_own_gives_up_no_claim_that_was_not_there() {
    let context = Context::new();
    let (project, _, _) = answered_through_a_door(&context);
    context.adapter.with("zeus", |agent| {
        agent.waiting = Some(Waiting { reason: None })
    });
    context.pass().unwrap();
    context.pass().unwrap();
    assert!(unclaimed(&context, project).is_empty());
}

#[test]
fn an_unacknowledged_claim_is_voided_when_the_window_exits_and_the_answer_comes_with_the_words() {
    let context = Context::new();
    let (project, question, answer) = answered_through_a_door(&context);
    claim(&context, project, question);
    context.exit("zeus");
    // The exit gives the claim back before it pauses the task, which would too.
    assert_eq!(unclaimed(&context, project), ["its window exited"]);
    assert_eq!(context.message(answer).state, "queued");
    context.resume_task(project, 1, "Carry on");
    context.pass().unwrap();
    let launch = context.adapter.prepared().pop().unwrap();
    assert_match(
        launch["message"].as_str().unwrap(),
        r"\(kept for you: answer m-\d+ from @chief, to your m-\d+\)\nColour: blue\n\nResumed: Carry on$",
    );
}

#[test]
fn an_unacknowledged_claim_is_voided_when_the_daemon_starts() {
    let context = Context::new();
    let (project, question, answer) = answered_through_a_door(&context);
    claim(&context, project, question);
    let zeus = context.id(project, "zeus");
    assert!(context
        .ledger
        .borrow()
        .next_delivery(zeus)
        .unwrap()
        .is_none());
    context.make().resume_after_restart().unwrap();
    assert_eq!(unclaimed(&context, project), ["the daemon started again"]);
    assert_eq!(
        context
            .ledger
            .borrow()
            .next_delivery(zeus)
            .unwrap()
            .map(|message| message.id),
        Some(answer),
        "the answer is the ledger's again"
    );
}

#[test]
fn an_answer_the_asking_window_read_by_cf_while_still_at_work_makes_the_task_work_and_its_turn_the_result(
) {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    // It asks with cf ask, and goes on working.
    let question = context.ask(project.id, "zeus", "chief", 1, "Which format?");
    assert_eq!(context.task(project.id, 1).task.state, "waiting");
    let answer = context.answer(project.id, question.id, "JSON");
    // It reads the answer in its shell: `cf inbox read m-N`.
    let zeus = context.id(project.id, "zeus");
    context
        .ledger
        .borrow_mut()
        .receive_read(zeus, &[answer.id], Read::Inbox)
        .unwrap();
    assert_eq!(
        context.task(project.id, 1).task.state,
        "working",
        "the board says working, with no question pending"
    );
    context.pass().unwrap();
    assert_eq!(context.message(answer.id).state, "read");
    assert_eq!(pasted(&context, answer.id), 0, "nothing is pasted later");

    context.adapter.answer("zeus", "Parser in JSON");
    context.pass().unwrap();
    let thread = context.task(project.id, 1);
    assert_eq!(thread.task.state, "done", "the turn's end is the result");
    let result = thread.messages.iter().find(|m| m.kind == "result").unwrap();
    assert_eq!(result.body, "Parser in JSON");
    context.pass().unwrap();
    assert_eq!(pasted(&context, answer.id), 0);
}

#[test]
fn a_turn_that_ends_while_the_task_waits_makes_no_result_and_the_answers_paste_does() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    let question = context.ask(project.id, "zeus", "chief", 1, "Which format?");
    context.adapter.answer("zeus", "I still need the format");
    context.pass().unwrap();
    assert_eq!(
        context.task(project.id, 1).task.state,
        "waiting",
        "the turn ended with the question open: it is no result"
    );
    assert!(context
        .task(project.id, 1)
        .messages
        .iter()
        .all(|message| message.kind != "result"));

    let answer = context.answer(project.id, question.id, "JSON");
    context.pass().unwrap();
    assert_eq!(pasted(&context, answer.id), 1);
    context.pass().unwrap();
    assert_eq!(context.task(project.id, 1).task.state, "working");
    context.adapter.answer("zeus", "Parser in JSON");
    context.pass().unwrap();
    let thread = context.task(project.id, 1);
    assert_eq!(thread.task.state, "done");
    let result = thread.messages.iter().find(|m| m.kind == "result").unwrap();
    assert_eq!(result.body, "Parser in JSON", "the turn after the answer");
}

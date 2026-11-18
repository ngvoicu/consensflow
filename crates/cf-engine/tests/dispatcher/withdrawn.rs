//! A message withdrawn on its way into a window: what its window shows of it
//! after all confirms nothing, and a window that was getting ready is handed
//! nothing (`describe('a message withdrawn on its way into a window')`).

use cf_proto::cf_word::CfWord;
use std::rc::Rc;

use cf_engine::delivery_text::{delivery_text, marker_of};
use cf_engine::testing::Context;
use cf_harness::contract::Readiness;
use cf_harness::records::Role;
use cf_ledger::MessageView;
use serde_json::json;

use crate::fixtures::{assert_match, Tiers};
use crate::traces::held_to;

const SUITES: &[&str] = &["a message withdrawn on its way into a window"];

/// T-1 waits on its window's question; its answer goes to that window next
/// (`asked`).
fn asked(context: &Context) -> (Tiers<'_>, MessageView) {
    let tiers = Tiers::new(context, &["zeus", "diana"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    let question = context.ask(
        tiers.project.id,
        &tiers.assignee(1),
        "chief",
        1,
        "Which grammar?",
    );
    context.adapter.answer("zeus", "I asked.");
    context.pass().unwrap();
    let chief = tiers.id("chief");
    let answer = context
        .ledger
        .borrow_mut()
        .answer(question.id, chief, Some(&json!("The small one")), None)
        .unwrap();
    (tiers, answer)
}

#[test]
fn is_not_confirmed_when_it_shows_after_all_and_its_window_goes_with_its_work() {
    let context = Context::new();
    let (tiers, answer) = asked(&context);
    context.adapter.with("zeus", |agent| agent.arrive = false);
    context.pass().unwrap();
    assert_eq!(
        context.message(answer.id).state,
        "delivering",
        "handed over"
    );
    let pane = context.host.last("zeus").unwrap().pane;
    // The human takes the task back: the answer on its way is withdrawn,
    // and the window's harness shows it anyway.
    context
        .ledger
        .borrow_mut()
        .release_task(tiers.project.id, 1, "by @human")
        .unwrap();
    let shown = context.adapter.item(
        Role::User,
        &delivery_text(&context.message(answer.id), &[], &CfWord::bare()),
    );
    context
        .adapter
        .with("zeus", |agent| agent.items.push(shown));
    context.pass().unwrap();
    assert_eq!(
        context.message(answer.id).state,
        "cancelled",
        "it stays withdrawn"
    );
    assert!(
        context
            .host
            .killed()
            .iter()
            .any(|killed| killed.generation == pane.generation),
        "the window went with its work"
    );
    // The task goes on elsewhere without the old window's answer, which is
    // returned once to the chief with its full words.
    assert_match(&tiers.assignee(1), "^diana-");
    let first = context.adapter.prepared().last().unwrap()["message"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(!first.contains("The small one"), "{first}");
    assert_eq!(
        context
            .inbox(tiers.id("chief"))
            .iter()
            .filter(|m| m.kind == "note" && m.body.contains("The small one"))
            .count(),
        1
    );
}

#[test]
fn is_handed_nothing_when_it_was_withdrawn_while_its_window_got_ready_and_fails_nothing() {
    let context = Context::new();
    let (tiers, answer) = asked(&context);
    *context.adapter.ready.borrow_mut() = Some(Rc::new(|| Ok(Readiness::Ready)));
    let readying = context.adapter.ready_holds.hold(|_| true);
    context.pass().unwrap();
    context
        .ledger
        .borrow_mut()
        .cancel_task(tiers.project.id, 1, "chief")
        .unwrap();
    readying.open();
    context.settle();
    assert_eq!(context.message(answer.id).state, "cancelled");
    let chief = tiers.id("chief");
    assert!(
        context
            .ledger
            .borrow()
            .inbox(chief, 100)
            .unwrap()
            .iter()
            .any(|message| { message.kind == "note" && message.body.contains("The small one") }),
        "the chief gets the answer's words back"
    );
    let marker = marker_of(answer.id);
    assert!(
        !context
            .adapter
            .agent("zeus")
            .items
            .iter()
            .any(|item| item.text.contains(&marker)),
        "nothing pasted"
    );
    held_to(
        context.close(),
        SUITES,
        "is handed nothing when it was withdrawn while its window got ready, and fails nothing",
    );
}

#[test]
fn t1813_an_idle_window_exit_returns_its_taskless_answer_to_the_chief() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    let session = tiers.assignee(1);
    let window = tiers.id(&session);
    let chief = tiers.id("chief");
    {
        let mut ledger = context.ledger.borrow_mut();
        ledger.record_result(tiers.project.id, 1, "Done").unwrap();
        ledger.accept_task(tiers.project.id, 1, "human").unwrap();
        let q = ledger
            .ask(
                tiers.project.id,
                &cf_ledger::NewQuestion {
                    from: Some(session),
                    to: "chief".into(),
                    body: Some("Idle question".into()),
                    ..Default::default()
                },
            )
            .unwrap();
        ledger
            .answer(q.id, chief, Some(&json!("Idle full reply")), None)
            .unwrap();
    }
    context.exit("zeus");
    let ledger = context.ledger.borrow();
    assert_eq!(
        ledger
            .inbox(chief, 100)
            .unwrap()
            .iter()
            .filter(|m| m.kind == "note" && m.body.contains("Idle full reply"))
            .count(),
        1
    );
    assert!(ledger
        .inbox(window, 100)
        .unwrap()
        .iter()
        .filter(|m| m.kind == "answer")
        .all(|m| m.state == "cancelled"));
}

#[test]
fn t1813_project_resume_reopens_the_same_conversation_for_new_questions() {
    let context = Context::new();
    context.adapter.resumes_records.set(true);
    let (tiers, old_answer) = asked(&context);
    let project = tiers.project.id;
    let session = tiers.assignee(1);
    let window = tiers.id(&session);
    let task = tiers.task(1).task.id;
    let old_pane = context.host.last(&session).unwrap().pane;
    let old_question = old_answer.reply_to.unwrap();
    let chief = tiers.id("chief");
    let briefs_before = tiers
        .task(1)
        .messages
        .iter()
        .filter(|m| m.kind == "task")
        .count();

    context.ledger.borrow_mut().suspend_for_restart().unwrap();
    context.advance(1_000);
    let restarted = context.make();
    restarted.settle_after_restart().unwrap();
    assert_eq!(context.message(old_question).state, "cancelled");
    assert_eq!(context.message(old_answer.id).state, "cancelled");
    assert_eq!(
        context
            .inbox(chief)
            .iter()
            .filter(|m| m.kind == "note" && m.body.contains("The small one"))
            .count(),
        1
    );

    restarted.resume_project(project).unwrap();
    context.settle();
    assert_ne!(
        context.host.last(&session).unwrap().pane,
        old_pane,
        "task={}, windows={:?}",
        tiers.task(1).task.state,
        context
            .host
            .opened()
            .iter()
            .map(|o| &o.pane)
            .collect::<Vec<_>>()
    );
    let ledger = context.ledger.borrow();
    let holding = ledger
        .question_holding(window)
        .unwrap()
        .expect("the resumed task holding");
    assert!(
        holding.active,
        "the reopened physical window holds its old task"
    );
    assert_eq!(holding.task_id, task);
    assert!(ledger
        .question_holding_matches(window, Some(task), Some(holding.first_brief))
        .unwrap());
    assert_eq!(
        tiers
            .task(1)
            .messages
            .iter()
            .filter(|m| m.kind == "task")
            .count(),
        briefs_before,
        "reopening the same conversation needs no new brief"
    );
    drop(ledger);

    let question = context.ask(project, &session, "chief", 1, "New window asks");
    let answer = context.answer(project, question.id, "New window answer");
    context
        .ledger
        .borrow_mut()
        .close_question_window(window, None)
        .unwrap();
    assert_eq!(context.message(question.id).state, "cancelled");
    assert_eq!(context.message(answer.id).state, "cancelled");
    assert_eq!(
        context
            .inbox(chief)
            .iter()
            .filter(|m| m.kind == "note" && m.body.contains("New window answer"))
            .count(),
        1
    );
}

#[test]
fn t1813_project_resume_refuses_a_question_when_the_task_moves_during_open() {
    let context = Context::new();
    context.adapter.resumes_records.set(true);
    let (tiers, _) = asked(&context);
    let project = tiers.project.id;
    let session = tiers.assignee(1);
    let window = tiers.id(&session);
    let diana = tiers.id("diana");
    let task = tiers.task(1).task.id;
    let pane = format!("p{project}-{session}");
    context.ledger.borrow_mut().suspend_for_restart().unwrap();
    let restarted = context.make();
    restarted.settle_after_restart().unwrap();
    let opening = context
        .host
        .open_holds
        .hold(move |args| args[0]["id"] == pane);
    restarted.resume_project(project).unwrap();
    context.settle();
    let start = context
        .credentials
        .question_starts
        .borrow()
        .iter()
        .rev()
        .find(|(id, _)| *id == window)
        .unwrap()
        .1;
    assert_eq!(
        start.task_in_hand,
        Some(task),
        "the reopen credential remembers which task it meant to resume"
    );
    {
        let mut ledger = context.ledger.borrow_mut();
        ledger.release_task(project, 1, "by @human").unwrap();
        ledger.assign_task(project, 1, diana).unwrap();
    }
    opening.open();
    context.settle();
    let holding = context
        .ledger
        .borrow()
        .question_holding(window)
        .unwrap()
        .unwrap();
    assert!(
        !holding.active,
        "the reopened window cannot reclaim a moved task"
    );
    let refusal = context
        .ledger
        .borrow_mut()
        .ask(
            project,
            &cf_ledger::NewQuestion {
                from: Some(session),
                to: "chief".into(),
                task: Some(1),
                body: Some("Where?".into()),
                ..Default::default()
            },
        )
        .unwrap_err()
        .to_string();
    assert!(
        refusal.contains("diana"),
        "the refusal names the current member: {refusal}"
    );
}

#[test]
fn t1813_a_brief_queued_in_the_harness_does_not_replace_the_delivered_holding() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    let project = tiers.project.id;
    let session = tiers.assignee(1);
    let window = tiers.id(&session);
    let previous = context
        .ledger
        .borrow()
        .question_holding(window)
        .unwrap()
        .unwrap();
    context.open_window(project, &session).unwrap();
    context.adapter.answer("zeus", "Done.");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "done");
    let brief = context
        .ledger
        .borrow_mut()
        .reopen_task(project, 1, "chief", "Continue")
        .unwrap()
        .message
        .unwrap()
        .id;
    context.adapter.with("zeus", |agent| {
        agent.settled = true;
        agent.at_work = false;
        agent.queued = true;
        agent.arrive = false;
    });
    for _ in 0..4 {
        context.pass().unwrap();
        if context.message(brief).state == "delivering" {
            break;
        }
    }
    assert_eq!(context.message(brief).state, "delivering");
    let holding = context
        .ledger
        .borrow()
        .question_holding(window)
        .unwrap()
        .unwrap();
    assert_eq!(
        holding.last_brief, previous.last_brief,
        "a harness queue is not a delivered brief"
    );
    assert!(holding.active);
    context
        .ledger
        .borrow_mut()
        .confirm_delivery(brief, Some(&json!({"item":"received"})))
        .unwrap();
    let holding = context
        .ledger
        .borrow()
        .question_holding(window)
        .unwrap()
        .unwrap();
    assert_eq!(
        holding.last_brief, brief,
        "only proof of receipt changes the last brief"
    );
}

#[test]
fn t1813_a_delivered_brief_is_the_holding_before_the_adapter_replies() {
    use cf_engine::testing::Gate;
    use cf_harness::contract::FirstMessage;
    use std::cell::Cell;

    let context = Context::new();
    context.adapter.resumes_records.set(true);
    context.host.exits_showing(Some(0), None, None);
    let tiers = Tiers::new(&context, &["zeus"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    let session = tiers.assignee(1);
    let window = tiers.id(&session);
    context.adapter.answer("zeus", "Done.");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "done");
    let next = context.create_task(
        tiers.project.id,
        cf_ledger::NewTask {
            from: "chief".into(),
            after: Some(1),
            body: "Next task".into(),
            ..Default::default()
        },
    );
    context
        .ledger
        .borrow_mut()
        .accept_task(tiers.project.id, 1, "human")
        .unwrap();
    let brief = tiers
        .task(next.task.number)
        .messages
        .iter()
        .find(|m| m.kind == "task")
        .unwrap()
        .id;
    context.adapter.first.set(FirstMessage::Delivered);
    let handed = Rc::new(Cell::new(false));
    let saw = Rc::clone(&handed);
    let gate = Gate::default();
    let waiting = gate.clone();
    let fake = Rc::downgrade(&context.adapter);
    let session_for_delivery = session.clone();
    let marker = marker_of(brief);
    *context.adapter.deliver.borrow_mut() = Some(Rc::new(move |taking| {
        let outcome = taking();
        let delivered = fake.upgrade().is_some_and(|fake| {
            fake.agent(&session_for_delivery)
                .items
                .iter()
                .any(|item| item.text.contains(&marker))
        });
        if delivered {
            saw.set(true);
        }
        let waiting = waiting.clone();
        Box::pin(async move {
            if delivered {
                waiting.wait().await;
            }
            Ok(outcome)
        })
    }));
    for _ in 0..8 {
        context.advance(6_000);
        context.pass().unwrap();
        if handed.get() {
            break;
        }
    }
    assert!(
        handed.get(),
        "the adapter took the brief; state={}, task={:?}, next={:?}, events={:?}",
        context.message(brief).state,
        tiers.task(next.task.number).task,
        context
            .ledger
            .borrow()
            .next_delivery_of(window, brief)
            .unwrap()
            .map(|m| m.id),
        context
            .ledger
            .borrow()
            .events(tiers.project.id, 0, 200)
            .unwrap()
            .iter()
            .map(|e| &e.kind)
            .collect::<Vec<_>>()
    );
    assert!(
        context
            .adapter
            .agent(&session)
            .items
            .iter()
            .any(|item| item.text.contains(&marker_of(brief))),
        "the new brief was delivered to zeus before the adapter returned"
    );
    let holding = context
        .ledger
        .borrow()
        .question_holding(window)
        .unwrap()
        .unwrap();
    let start = context
        .credentials
        .question_starts
        .borrow()
        .iter()
        .rev()
        .find(|(id, _)| *id == window)
        .unwrap()
        .1;
    assert_eq!(
        start.launch_brief, None,
        "the first brief is delivered after launch"
    );
    assert_eq!(
        start.task_in_hand, None,
        "the closed old task is no new holding"
    );
    let chief = tiers.id("chief");
    let (question, answer, answer_task) = {
        let mut ledger = context.ledger.borrow_mut();
        let question = ledger
            .ask(
                tiers.project.id,
                &cf_ledger::NewQuestion {
                    from: Some(session.clone()),
                    to: "chief".into(),
                    body: Some("Question after the new brief".into()),
                    ..Default::default()
                },
            )
            .unwrap()
            .id;
        let answer = ledger
            .answer(question, chief, Some(&json!("Full new-task answer")), None)
            .unwrap();
        (question, answer.id, answer.task_number)
    };
    gate.open();
    context.settle();
    if context.message(brief).state == "delivering" {
        context
            .ledger
            .borrow_mut()
            .confirm_delivery(brief, Some(&json!({ "item": "received" })))
            .unwrap();
    }
    context
        .ledger
        .borrow_mut()
        .cancel_task(tiers.project.id, next.task.number, "human")
        .unwrap();
    let notices = context
        .inbox(chief)
        .iter()
        .filter(|m| m.kind == "note" && m.body.contains("Full new-task answer"))
        .count();
    let eligible = context
        .ledger
        .borrow()
        .next_delivery_of(window, answer)
        .unwrap()
        .map(|m| m.id);
    let answer_state = context.message(answer).state;
    let admitted = if eligible == Some(answer) {
        let mut ledger = context.ledger.borrow_mut();
        ledger.begin_delivery(answer).unwrap();
        Some(ledger.admit(answer, &[]).unwrap())
    } else {
        None
    };
    assert_eq!(
        (
            context.message(question).state.as_str(),
            answer_state.as_str(),
            notices,
            eligible,
            admitted
        ),
        ("cancelled", "cancelled", 1, None, None),
        "the delivered task's question must close and its full answer must return"
    );
    assert_eq!(answer_task, Some(next.task.number));
    assert_eq!(
        (holding.task_number, holding.active),
        (next.task.number, true),
        "the current holding must be the different task whose brief was already delivered"
    );
}

#[test]
fn t1813_restart_returns_a_queued_answer_without_resuming_its_project() {
    let context = Context::new();
    let (tiers, answer) = asked(&context);
    let chief = tiers.id("chief");
    context.ledger.borrow_mut().suspend_for_restart().unwrap();
    let restarted = context.make();
    restarted.settle_after_restart().unwrap();
    assert_eq!(context.message(answer.id).state, "cancelled");
    assert_eq!(
        context
            .inbox(chief)
            .iter()
            .filter(|m| m.kind == "note" && m.body.contains("The small one"))
            .count(),
        1,
        "the chief hears before the project is resumed"
    );
}

#[test]
fn t1813_restart_does_not_send_the_old_windows_answer_into_a_new_window() {
    let context = Context::new();
    let (tiers, answer) = asked(&context);
    let chief = tiers.id("chief");
    context.ledger.borrow_mut().suspend_for_restart().unwrap();
    let restarted = context.make();
    restarted.settle_after_restart().unwrap();
    assert_eq!(
        context.message(answer.id).state,
        "cancelled",
        "the startup window-end path returns the old window's answer"
    );
    restarted.resume_project(tiers.project.id).unwrap();
    for _ in 0..5 {
        restarted.pass().unwrap();
    }
    let notices = context
        .ledger
        .borrow()
        .inbox(chief, 100)
        .unwrap()
        .into_iter()
        .filter(|m| m.kind == "note" && m.body.contains("The small one"))
        .count();
    assert_eq!(
        notices, 1,
        "the ended window's answer is returned to the chief"
    );
    assert_eq!(context.message(answer.id).state, "cancelled");
    let question = context.message(answer.id).reply_to.unwrap();
    assert_eq!(
        context.message(question).state,
        "cancelled",
        "restart retry closes the old question"
    );
}

#[test]
fn t1813_a_gated_answer_approved_after_restart_returns_to_the_chief() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    let question = context.ask(tiers.project.id, &tiers.assignee(1), "chief", 1, "Which?");
    context.adapter.answer("zeus", "I asked.");
    context.pass().unwrap();
    let chief = tiers.id("chief");
    let answer = {
        let mut ledger = context.ledger.borrow_mut();
        ledger.set_gate(tiers.project.id, true).unwrap();
        ledger
            .answer(
                question.id,
                chief,
                Some(&json!("Full gated restart answer")),
                None,
            )
            .unwrap()
            .id
    };
    context.ledger.borrow_mut().suspend_for_restart().unwrap();
    let restarted = context.make();
    restarted.settle_after_restart().unwrap();
    assert_eq!(context.message(answer).state, "gated");
    context
        .ledger
        .borrow_mut()
        .approve_message(answer, "human")
        .unwrap();
    assert_eq!(context.message(answer).state, "cancelled");
    assert_eq!(
        context
            .inbox(chief)
            .iter()
            .filter(|m| m.kind == "note" && m.body.contains("Full gated restart answer"))
            .count(),
        1
    );
}

#[test]
fn t1813_a_done_task_does_not_strand_its_answer_after_restart() {
    let context = Context::new();
    let (tiers, answer) = asked(&context);
    let chief = tiers.id("chief");
    context
        .ledger
        .borrow_mut()
        .record_result(tiers.project.id, 1, "Done")
        .unwrap();
    context.ledger.borrow_mut().suspend_for_restart().unwrap();
    let restarted = context.make();
    restarted.settle_after_restart().unwrap();
    assert_eq!(context.message(answer.id).state, "cancelled");
    restarted.resume_project(tiers.project.id).unwrap();
    for _ in 0..5 {
        restarted.pass().unwrap();
    }
    assert_eq!(
        context
            .inbox(chief)
            .iter()
            .filter(|m| m.kind == "note" && m.body.contains("The small one"))
            .count(),
        1
    );
    assert_eq!(context.message(answer.id).state, "cancelled");
}

#[test]
fn t1813_taskless_gated_answer_after_restart_is_returned() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    let session = tiers.assignee(1);
    let chief = tiers.id("chief");
    let answer = {
        let mut ledger = context.ledger.borrow_mut();
        ledger.record_result(tiers.project.id, 1, "Done").unwrap();
        ledger.accept_task(tiers.project.id, 1, "human").unwrap();
        let q = ledger
            .ask(
                tiers.project.id,
                &cf_ledger::NewQuestion {
                    from: Some(session),
                    to: "chief".into(),
                    body: Some("Idle question".into()),
                    ..Default::default()
                },
            )
            .unwrap();
        ledger.set_gate(tiers.project.id, true).unwrap();
        ledger
            .answer(q.id, chief, Some(&json!("Idle gated reply")), None)
            .unwrap()
            .id
    };
    context.ledger.borrow_mut().suspend_for_restart().unwrap();
    let restarted = context.make();
    restarted.settle_after_restart().unwrap();
    assert_eq!(context.message(answer).state, "gated");
    context
        .ledger
        .borrow_mut()
        .approve_message(answer, "human")
        .unwrap();
    assert_eq!(context.message(answer).state, "cancelled");
    assert_eq!(
        context
            .inbox(chief)
            .iter()
            .filter(|m| m.kind == "note" && m.body.contains("Idle gated reply"))
            .count(),
        1
    );
}

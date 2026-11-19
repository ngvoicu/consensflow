use std::cell::RefCell;
use std::rc::Rc;

use serde_json::json;

use super::*;
use crate::testing::Context;

#[test]
fn t1813_the_pane_holding_changes_when_the_daemon_issues_the_brief_write() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    let task = context.give(project.id, "zeus", "New task");
    let brief = task.message.unwrap().id;
    let window = context.message(brief).recipient_id;
    context.ledger.borrow_mut().begin_delivery(brief).unwrap();
    assert!(context
        .ledger
        .borrow()
        .question_holding(window)
        .unwrap()
        .is_none());
    let seen = Rc::new(RefCell::new(Vec::new()));
    let recorded = Rc::clone(&seen);
    *context.host.on_request.borrow_mut() = Some(Rc::new(move |op, body| {
        if op == "pane.write_paste" {
            recorded.borrow_mut().push(body.clone());
        }
    }));
    let admitting = Admitting::handing(&context.dispatcher, brief, Vec::new());
    let write = admitting.request(
        "pane.write_paste",
        json!({"id":"pane","generation":1,"body":"brief"}),
    );
    let holding = context.ledger.borrow().question_holding(window).unwrap();
    assert_eq!(
        holding.map(|h| (h.task_number, h.active)),
        Some((task.task.number, true)),
        "the new task is held before the pane host responds or reports a write"
    );
    assert!(
        seen.borrow()[0].get("brief").is_none(),
        "the pane needs no brief-report protocol"
    );
    drop(write);
}

#[tokio::test(flavor = "current_thread")]
async fn t1813_a_pane_refusal_rebinds_the_question_asked_during_the_write() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    let task = context.give(project.id, "zeus", "New task");
    let brief = task.message.unwrap().id;
    let recipient = context.message(brief);
    context.ledger.borrow_mut().begin_delivery(brief).unwrap();
    context.host.refuse_pastes.set(true);
    let admitting = Admitting::handing(&context.dispatcher, brief, Vec::new());
    let write = admitting.request(
        "pane.write_paste",
        json!({"id":"pane","generation":1,"body":"brief"}),
    );
    let question = context
        .ledger
        .borrow_mut()
        .ask(
            project.id,
            &cf_ledger::NewQuestion {
                from: Some(recipient.recipient),
                to: "chief".into(),
                body: Some("During the write".into()),
                task: Some(task.task.number),
                ..Default::default()
            },
        )
        .unwrap()
        .id;
    let chief = context.id(project.id, "chief");
    let answer = context
        .ledger
        .borrow_mut()
        .answer(question, chief, Some(&json!("Answer retained")), None)
        .unwrap()
        .id;
    assert_eq!(write.await.unwrap()["ok"], false);
    assert!(context
        .ledger
        .borrow()
        .question_holding(recipient.recipient_id)
        .unwrap()
        .is_none());
    assert_eq!(context.message(answer).task_number, None);
    assert_ne!(context.message(answer).state, "cancelled");
}

#[test]
fn t1813_codex_broker_handover_binds_and_a_refusal_rebinds() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    let task = context.give(project.id, "zeus", "New task");
    let brief = task.message.unwrap().id;
    let recipient = context.message(brief);
    context.ledger.borrow_mut().begin_delivery(brief).unwrap();
    let admitting = Admitting::handing(&context.dispatcher, brief, Vec::new());
    assert!(admitting.hand_over_to_broker());
    assert_eq!(
        context
            .ledger
            .borrow()
            .question_holding(recipient.recipient_id)
            .unwrap()
            .unwrap()
            .task_number,
        task.task.number
    );
    let question = context
        .ledger
        .borrow_mut()
        .ask(
            project.id,
            &cf_ledger::NewQuestion {
                from: Some(recipient.recipient),
                to: "chief".into(),
                body: Some("During broker send".into()),
                task: Some(task.task.number),
                ..Default::default()
            },
        )
        .unwrap()
        .id;
    let chief = context.id(project.id, "chief");
    let answer = context
        .ledger
        .borrow_mut()
        .answer(question, chief, Some(&json!("Broker answer")), None)
        .unwrap()
        .id;
    admitting.broker_send_failed();
    assert!(context
        .ledger
        .borrow()
        .question_holding(recipient.recipient_id)
        .unwrap()
        .is_none());
    assert_eq!(context.message(answer).task_number, None);
    assert_ne!(context.message(answer).state, "cancelled");
}

#[tokio::test(flavor = "current_thread")]
async fn t1813_an_uncertain_pane_reply_keeps_the_started_handover_until_settled() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    let task = context.give(project.id, "zeus", "New task");
    let brief = task.message.unwrap().id;
    let recipient = context.message(brief);
    context.ledger.borrow_mut().begin_delivery(brief).unwrap();
    context.host.uncertain_pastes.set(true);
    let admitting = Admitting::handing(&context.dispatcher, brief, Vec::new());
    let write = admitting.request(
        "pane.write_paste",
        json!({"id":"pane","generation":1,"body":"brief"}),
    );
    let question = context
        .ledger
        .borrow_mut()
        .ask(
            project.id,
            &cf_ledger::NewQuestion {
                from: Some(recipient.recipient),
                to: "chief".into(),
                body: Some("While the send is uncertain".into()),
                task: Some(task.task.number),
                ..Default::default()
            },
        )
        .unwrap()
        .id;
    let chief = context.id(project.id, "chief");
    let answer = context
        .ledger
        .borrow_mut()
        .answer(question, chief, Some(&json!("Uncertain answer")), None)
        .unwrap()
        .id;
    assert!(write.await.unwrap()["admitted"].is_null());
    assert_eq!(
        context
            .ledger
            .borrow()
            .question_holding(recipient.recipient_id)
            .unwrap()
            .unwrap()
            .task_number,
        task.task.number
    );
    assert_eq!(context.message(answer).task_number, Some(task.task.number));
    context
        .ledger
        .borrow_mut()
        .retry_delivery(brief, "its record proves the send failed", true)
        .unwrap();
    assert!(context
        .ledger
        .borrow()
        .question_holding(recipient.recipient_id)
        .unwrap()
        .is_none());
    assert_eq!(context.message(answer).task_number, None);
}

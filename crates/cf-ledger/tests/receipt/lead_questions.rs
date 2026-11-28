//! A member's question on a lead's task, as the ledger places it (the leads design, section 3;
//! step 4 of the build): the recipient is the decider of the task its window holds, resolved
//! before the question is written; the answer is the recipient's and goes back by the same
//! binding; a question's reminder waits on that recipient.

use super::fixture::{world, World};
use cf_ledger::{NewLead, NewQuestion, NewTask, QuestionTo};
use serde_json::json;

fn with_a_lead(w: &mut World) -> String {
    w.ledger
        .add_lead(
            w.project,
            &NewLead {
                harness: "devin".into(),
                model: None,
                effort: None,
                model_name: None,
                scope: "the site".into(),
                own_hands: false,
            },
        )
        .expect("a lead")
        .handle
}

/// A task `from` gave the standard workers, at work in a session of zeus, its brief delivered.
fn at_work(w: &mut World, from: &str) -> (i64, String) {
    let created = w
        .ledger
        .create_task(
            w.project,
            &NewTask {
                from: from.into(),
                pool: Some("worker".into()),
                tier: Some("standard".into()),
                body: "Parser".into(),
                ..NewTask::default()
            },
        )
        .expect("a task for the tier");
    let task = created.task.number;
    let moved = w.assign(task, "zeus");
    let session = moved.task.assignee.clone().expect("a session");
    w.deliver(moved.message.expect("its brief").id);
    (task, session)
}

/// The member's own question, as the engine puts last words that ask.
fn asks_its_giver(w: &mut World, session: &str, task: i64, words: &str) -> cf_ledger::MessageView {
    w.ledger
        .ask_to(
            w.project,
            &NewQuestion {
                from: Some(session.into()),
                body: Some(words.into()),
                task: Some(task),
                ..NewQuestion::default()
            },
            &QuestionTo::TaskGiver,
        )
        .expect("a question")
}

#[test]
fn a_members_question_on_a_leads_task_is_the_leads_and_only_the_lead_answers_it() {
    let mut w = world();
    let lead = with_a_lead(&mut w);
    let (task, session) = at_work(&mut w, &lead);
    let question = asks_its_giver(&mut w, &session, task, "Which grammar?");
    assert_eq!(question.recipient, lead);
    assert_eq!(question.task_number, Some(task));
    w.deliver(question.id);
    let refused = w
        .ledger
        .answer(question.id, w.id("chief"), Some(&json!("PEG")), None)
        .expect_err("the chief was not asked");
    assert_eq!(refused.code(), Some("not-your-question"));
    let answer = w
        .ledger
        .answer(question.id, w.id(&lead), Some(&json!("PEG")), None)
        .expect("the lead answers");
    assert_eq!(answer.recipient, session);
    assert_eq!(answer.task_number, Some(task));
    assert_eq!(
        w.ledger.task(w.project, task).unwrap().unwrap().task.state,
        "working"
    );
}

#[test]
fn a_question_on_a_leads_task_waits_on_the_lead_once_the_lead_has_it() {
    let mut w = world();
    let lead = with_a_lead(&mut w);
    let (task, session) = at_work(&mut w, &lead);
    let question = asks_its_giver(&mut w, &session, task, "Which grammar?");
    assert!(
        w.ledger.waiting_questions(w.project).unwrap().is_empty(),
        "not received yet"
    );
    w.deliver(question.id);
    let waits = w.ledger.waiting_questions(w.project).unwrap();
    assert_eq!(waits.len(), 1);
    assert_eq!(
        (
            waits[0].id,
            waits[0].recipient.as_str(),
            waits[0].recipient_id
        ),
        (question.id, lead.as_str(), w.id(&lead))
    );
}

#[test]
fn a_members_question_with_no_task_and_on_the_humans_task_is_the_chiefs_with_a_lead_there() {
    let mut w = world();
    with_a_lead(&mut w);
    let (task, session) = at_work(&mut w, "human");
    let question = asks_its_giver(&mut w, &session, task, "Which grammar?");
    assert_eq!(question.recipient, "chief");
}

#[test]
fn a_named_question_keeps_its_name_whoever_gave_the_task() {
    // `Ledger::ask` names its recipient, as every test and tool of the ledger asks today.
    let mut w = world();
    let lead = with_a_lead(&mut w);
    let (task, session) = at_work(&mut w, &lead);
    let question = w.ask(&session, task, "Which grammar?");
    assert_eq!(question.recipient, "chief");
}

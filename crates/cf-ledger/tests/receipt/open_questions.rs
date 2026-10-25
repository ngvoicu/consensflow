//! What the chief's handoff lists as waiting for its answer (`chief_open_work`),
//! held to the rule that refuses a second answer (`answer`): a question is open
//! when it has no answer that stands, and the two agree whatever state the answer
//! is in. Found by the review of T-157's first two rounds (T-193): an answer that
//! failed to arrive read as an answer in the list, so a new chief was told nothing
//! waited while the task waited for a second one, and an answer held at the gate
//! read as none, so the list sent the chief to answer a question the ledger
//! refuses to take a second answer to.

use serde_json::json;

use crate::fixture::{gated_world, world, World};

/// The questions the handoff lists as waiting on the chief.
fn listed(w: &World) -> Vec<i64> {
    w.ledger
        .chief_open_work(w.project)
        .expect("the open work")
        .questions
        .iter()
        .map(|question| question.id)
        .collect()
}

/// Whether the chief is refused a second answer to `question`, as one that has
/// its answer: the code of the refusal, or none when the answer is taken.
fn second_answer(w: &mut World, question: i64) -> Option<&'static str> {
    let chief = w.id("chief");
    w.ledger
        .answer(question, chief, Some(&json!("Again")), None)
        .err()
        .and_then(|refused| refused.code())
}

/// A task given to zeus, its brief received, and a question of its asked and
/// given to the chief: the task, the question. In a world with a gate the human
/// passes each on.
fn asked(w: &mut World) -> (i64, i64) {
    let gated = w.ledger.project(w.project).unwrap().unwrap().gate;
    let created = w.give("zeus", "Parser");
    let (task, brief) = (created.task.number, created.message.expect("a brief").id);
    if gated {
        w.ledger.approve_message(brief, "human").expect("passed on");
    }
    w.deliver(brief);
    let question = w.ask("zeus", task, "Which format?").id;
    if gated {
        w.ledger
            .approve_message(question, "human")
            .expect("passed on");
    }
    w.deliver(question);
    (task, question)
}

/// A world whose one question has an answer in `state`, and the question.
fn answered_in(state: &str) -> (World, i64) {
    let mut w = if state == "gated" {
        gated_world()
    } else {
        world()
    };
    let (task, question) = asked(&mut w);
    let answer = if state == "read" {
        // The harness's own tool asked, and its door took the answer.
        let door = w.ask_with_options("zeus", task).id;
        let answer = w.choose(door, "blue").id;
        let zeus = w.id("zeus");
        w.ledger.claim_answer(door, zeus).expect("claimed");
        w.ledger
            .settle_claim(answer, zeus, true)
            .expect("handed over");
        return (w, door);
    } else {
        w.answer(question, "JSON").id
    };
    match state {
        "queued" | "gated" => {}
        "delivering" => {
            w.begin(answer);
        }
        "delivered" => {
            w.deliver(answer);
        }
        "cancelled" => {
            w.ledger
                .cancel_message(answer, "withdrawn by the chief")
                .expect("withdrawn");
        }
        "failed" => {
            w.begin(answer);
            w.ledger
                .fail_delivery(answer, "the window closed")
                .expect("given up");
        }
        other => panic!("no answer in {other}"),
    }
    assert_eq!(w.message(answer).state, state, "the setup");
    (w, question)
}

#[test]
fn a_question_is_listed_exactly_when_the_ledger_takes_another_answer_to_it_whatever_state_its_answer_is_in(
) {
    let mut disagree = Vec::new();
    for (state, open) in [
        ("queued", false),
        ("delivering", false),
        ("delivered", false),
        ("read", false),
        ("gated", false),
        ("cancelled", true),
        ("failed", true),
    ] {
        let (mut w, question) = answered_in(state);
        let listed = listed(&w).contains(&question);
        let refused = second_answer(&mut w, question);
        if listed != open || (refused == Some("already-answered")) == listed {
            disagree.push(format!(
                "answer {state}: listed {listed} (should be {open}), second answer {refused:?}"
            ));
        }
    }
    assert!(disagree.is_empty(), "{disagree:#?}");
}

#[test]
fn a_question_whose_answer_failed_to_arrive_is_still_listed_as_its_task_waits_for_another() {
    let (mut w, question) = answered_in("failed");
    assert_eq!(
        listed(&w),
        [question],
        "the chief answers it again, or a new chief is told nothing waits"
    );
    assert_eq!(w.state(1), "waiting", "and the task does wait for it");
    assert_eq!(second_answer(&mut w, question), None, "which is taken");
}

#[test]
fn a_question_whose_answer_waits_at_the_gate_is_not_listed_as_it_takes_no_second_answer() {
    let (mut w, question) = answered_in("gated");
    assert_eq!(listed(&w), [] as [i64; 0], "the chief answered it");
    assert_eq!(second_answer(&mut w, question), Some("already-answered"));
}

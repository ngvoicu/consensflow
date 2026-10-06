//! A door claims an answer for its harness, and says it handed it over: until
//! it does, the answer is the ledger's to deliver as text, and a claim that
//! no one acknowledged is voided at the first sign that its door is gone.

use cf_ledger::Claim;
use serde_json::json;

use crate::fixture::{ids, world, World};

/// A task given to zeus, its brief received, with a question of the harness's
/// own tool asked and answered by a choice: the door's question, its answer.
fn asked_through_a_door(w: &mut World) -> (i64, i64, i64) {
    let created = w.give("zeus", "Parser");
    let (task, brief) = (created.task.number, created.message.expect("a brief").id);
    w.deliver(brief);
    let question = w.ask_with_options("zeus", task);
    let answer = w.choose(question.id, "blue");
    (task, question.id, answer.id)
}

/// Each claim the log says was voided: the answer, and why.
fn unclaimed(w: &World) -> Vec<(i64, String)> {
    w.ledger
        .events(w.project, 0, 500)
        .unwrap()
        .iter()
        .filter(|event| event.kind == "delivery.unclaimed")
        .map(|event| {
            (
                event.data["message"].as_i64().unwrap(),
                event.data["because"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

#[test]
fn a_poll_claims_the_answer_and_the_same_answer_comes_again_to_a_second_poll() {
    let mut w = world();
    let created = w.give("zeus", "Parser");
    let (task, brief) = (created.task.number, created.message.expect("a brief").id);
    w.deliver(brief);
    let zeus = w.id("zeus");
    let question = w.ask_with_options("zeus", task);
    assert_eq!(
        w.ledger.claim_answer(question.id, zeus).unwrap(),
        Claim::Waiting,
        "no answer yet"
    );
    let answer = w.choose(question.id, "blue");
    assert_eq!(w.state(task), "waiting");

    let claimed = w.ledger.claim_answer(question.id, zeus).unwrap();
    assert!(matches!(&claimed, Claim::Answered(found) if found.id == answer.id));
    assert_eq!(
        w.states(&[answer.id]),
        ["queued"],
        "claimed is not received"
    );
    assert_eq!(w.state(task), "waiting");
    assert_eq!(w.next("zeus"), None, "a claimed answer is not pasted");
    // A reply that was lost: the door asks again, and is given the same one.
    let again = w.ledger.claim_answer(question.id, zeus).unwrap();
    assert_eq!(again, claimed);
    let logged = w
        .ledger
        .events(w.project, 0, 500)
        .unwrap()
        .iter()
        .filter(|event| event.kind == "delivery.claimed")
        .count();
    assert_eq!(logged, 1, "claimed once");
}

#[test]
fn what_a_door_handed_over_and_said_so_is_read_with_its_receipt_and_its_task_goes_on() {
    let mut w = world();
    let (task, question, answer) = asked_through_a_door(&mut w);
    let zeus = w.id("zeus");
    w.ledger.claim_answer(question, zeus).unwrap();
    w.ledger.settle_claim(answer, zeus, true).expect("received");
    let read = w.message(answer);
    assert_eq!(
        (
            read.state.as_str(),
            &read.receipt,
            read.delivered_at.is_some()
        ),
        ("read", &json!({ "door": true }), true)
    );
    assert_eq!(w.state(task), "working");
    // The same acknowledgement twice, and a poll after it, are harmless.
    w.ledger
        .settle_claim(answer, zeus, true)
        .expect("received again");
    assert!(matches!(
        w.ledger.claim_answer(question, zeus).unwrap(),
        Claim::Answered(found) if found.state == "read"
    ));
}

#[test]
fn a_door_that_did_not_hand_it_over_gives_the_claim_back() {
    let mut w = world();
    let (task, question, answer) = asked_through_a_door(&mut w);
    let zeus = w.id("zeus");
    w.ledger.claim_answer(question, zeus).unwrap();
    w.ledger
        .settle_claim(answer, zeus, false)
        .expect("given back");
    assert_eq!(
        w.next("zeus"),
        Some(answer),
        "it is the ledger's again, as text"
    );
    assert_eq!(w.state(task), "waiting");
}

#[test]
fn a_claim_and_a_paste_exclude_each_other_whichever_comes_first() {
    // The claim first: the paste is refused.
    let mut w = world();
    let (_, question, answer) = asked_through_a_door(&mut w);
    let zeus = w.id("zeus");
    w.ledger.claim_answer(question, zeus).unwrap();
    let refused = w.ledger.begin_delivery(answer).unwrap_err();
    assert_eq!(refused.code(), Some("invalid-transition"));
    assert_eq!(w.states(&[answer]), ["queued"]);

    // The paste first: the door is closed, and the answer comes as text.
    let mut w = world();
    let (_, question, answer) = asked_through_a_door(&mut w);
    let zeus = w.id("zeus");
    w.begin(answer);
    assert_eq!(
        w.ledger.claim_answer(question, zeus).unwrap(),
        Claim::Closed
    );
    w.confirm(answer);
    assert_eq!(
        w.ledger.claim_answer(question, zeus).unwrap(),
        Claim::Closed,
        "and stays so once it arrived"
    );
}

#[test]
fn a_pause_voids_the_claim_and_shuts_the_door_and_the_resume_carries_the_answer() {
    let mut w = world();
    let (task, question, answer) = asked_through_a_door(&mut w);
    let zeus = w.id("zeus");
    w.ledger.claim_answer(question, zeus).unwrap();

    w.pause(task);
    assert_eq!(w.states(&[answer]), ["queued"]);
    assert_eq!(
        unclaimed(&w),
        [(answer, format!("T-{task} was paused"))],
        "the pause gave the claim back, and said why"
    );
    assert_eq!(
        w.ledger.claim_answer(question, zeus).unwrap(),
        Claim::Closed,
        "the pause shut the door before any key was pressed"
    );
    let refused = w.ledger.settle_claim(answer, zeus, true).unwrap_err();
    assert_eq!(
        (refused.code(), refused.to_string()),
        (
            Some("door-closed"),
            format!("m-{answer} is not answered here: it comes to you as a message")
        ),
        "an acknowledgement at a closed door is refused"
    );
    assert_eq!(
        w.states(&[answer]),
        ["queued"],
        "and the answer is still to come"
    );

    let words = w.resume(task, "Go on").message.expect("its words");
    let begun = w.deliver(words.id);
    assert_eq!(
        ids(&begun.carried),
        [answer],
        "as text, once, with the words"
    );
    assert_eq!(w.state(task), "working");
}

#[test]
fn a_claim_nobody_acknowledged_is_voided_when_its_window_is_at_rest_and_when_the_daemon_starts() {
    let mut w = world();
    let (_, question, answer) = asked_through_a_door(&mut w);
    let zeus = w.id("zeus");
    w.ledger.claim_answer(question, zeus).unwrap();
    assert_eq!(w.next("zeus"), None);
    w.ledger
        .release_claims(zeus, "its window is at rest")
        .expect("its window is at rest");
    assert_eq!(w.next("zeus"), Some(answer), "the answer is text again");

    w.ledger.claim_answer(question, zeus).unwrap();
    assert_eq!(w.next("zeus"), None);
    w.ledger.release_all_claims().expect("the daemon starts");
    assert_eq!(w.next("zeus"), Some(answer));
    assert_eq!(
        unclaimed(&w),
        [
            (answer, "its window is at rest".to_owned()),
            (answer, "the daemon started again".to_owned())
        ],
        "each voided claim is in the log, with what voided it"
    );
}

#[test]
fn an_answer_that_rides_in_a_paste_is_closed_to_the_door_and_comes_with_it() {
    let mut w = world();
    let (task, question, answer) = asked_through_a_door(&mut w);
    let zeus = w.id("zeus");
    w.pause(task);
    let words = w.resume(task, "Go on").message.expect("its words");
    assert_eq!(
        w.ledger.claim_answer(question, zeus).unwrap(),
        Claim::Closed,
        "the door was shut, and the answer rides in the words' paste"
    );
    let begun = w.deliver(words.id);
    assert_eq!(ids(&begun.carried), [answer]);
}

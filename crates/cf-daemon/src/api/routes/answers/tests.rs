//! `POST /api/answers`: what it reads and in what order, whom it refuses,
//! and that the dispatcher is woken once the answer is written and never
//! for one that was refused.

use cf_ledger::MessageView;
use hyper::Method;
use serde_json::{json, Value};

use super::*;
use crate::api::callers::caller_of;
use crate::api::routes::tests::support::{note_for_zeus, question_on, state_of, working_question};
use crate::testing::{request, said, scene, Scene};

/// `POST /api/answers` from the window of `token`, with `body` as its text.
async fn post(scene: &Scene, token: &str, body: &str) -> (u16, Value) {
    let asked = request(Method::POST, "/api/answers", Some(token), body);
    let caller = caller_of(&scene.context, &asked).unwrap();
    said(handle(&scene.context, &caller, asked).await)
}

/// The answer to the scene's question, as zeus's inbox holds it, or none while it waits.
fn answer_to(scene: &Scene) -> Option<cf_ledger::MessageView> {
    let zeus = scene
        .project
        .participants
        .iter()
        .find(|participant| participant.handle == "zeus")
        .unwrap()
        .id;
    let inbox = scene.context.ledger.borrow().inbox(zeus, 100).unwrap();
    inbox
        .into_iter()
        .filter(|message| message.kind == "answer" && message.reply_to == Some(scene.question.id))
        .filter(|message| message.state != "cancelled")
        .min_by_key(|message| message.id)
}

#[tokio::test]
async fn the_chief_answers_in_words_and_the_answer_is_created_and_the_dispatcher_woken() {
    let scene = scene();
    let body = json!({ "question": scene.question.id, "body": "The first." }).to_string();
    let (status, said) = post(&scene, &scene.chief, &body).await;
    assert_eq!(status, 201);
    let message = &said["message"];
    assert_eq!(message["kind"], "answer");
    assert_eq!(message["sender"], "chief");
    assert_eq!(message["recipient"], "zeus");
    assert_eq!(message["preview"], "The first.");
    assert_eq!(scene.kicks.get(), 1, "woken once, after the write");
    assert_eq!(
        answer_to(&scene).map(|found| found.body),
        Some("The first.".to_owned())
    );
}

#[tokio::test]
async fn the_answer_may_be_by_choices() {
    let scene = scene();
    // The question has options to choose from.
    let asked = scene
        .context
        .ledger
        .borrow_mut()
        .ask(
            scene.project.id,
            &cf_ledger::NewQuestion {
                from: Some("zeus".to_owned()),
                to: "chief".to_owned(),
                questions: Some(json!([{
                    "question": "Which?",
                    "header": "Pick",
                    "options": [{ "label": "A" }, { "label": "B" }]
                }])),
                ..cf_ledger::NewQuestion::default()
            },
        )
        .unwrap();
    let body = json!({ "question": asked.id, "choices": [["A"]] }).to_string();
    let (status, said) = post(&scene, &scene.chief, &body).await;
    assert_eq!(status, 201, "{said}");
    assert_eq!(said["message"]["choices"], json!([["A"]]));
}

#[tokio::test]
async fn a_whole_number_in_any_spelling_names_the_question() {
    let scene = scene();
    let id = scene.question.id;
    let body = format!(r#"{{"question": {id}.0, "body": "Yes"}}"#);
    let (status, _) = post(&scene, &scene.chief, &body).await;
    assert_eq!(status, 201);
}

#[tokio::test]
async fn what_is_no_positive_whole_number_names_no_question_and_is_quoted_as_it_was_written() {
    let scene = scene();
    for (written, said) in [
        (r#"{"body":"x"}"#, "undefined"),
        (r#"{"question":null,"body":"x"}"#, "null"),
        (r#"{"question":"3","body":"x"}"#, "3"),
        (r#"{"question":0,"body":"x"}"#, "0"),
        (r#"{"question":-4,"body":"x"}"#, "-4"),
        (r#"{"question":2.5,"body":"x"}"#, "2.5"),
        (r#"{"question":true,"body":"x"}"#, "true"),
        (r#"{"question":[1,2],"body":"x"}"#, "1,2"),
        (r#"{"question":{"a":1},"body":"x"}"#, "[object Object]"),
        (
            r#"{"question":99999999999999999999,"body":"x"}"#,
            "100000000000000000000",
        ),
        (r#"{"question":1e21,"body":"x"}"#, "1e+21"),
    ] {
        let (status, body) = post(&scene, &scene.chief, written).await;
        assert_eq!(status, 404, "{written}");
        assert_eq!(
            body,
            json!({ "error": "unknown-message", "message": format!("no question m-{said} in this project") }),
            "{written}"
        );
    }
    assert_eq!(scene.kicks.get(), 0);
    assert!(answer_to(&scene).is_none());
}

#[tokio::test]
async fn a_message_that_is_no_question_or_is_of_another_project_is_not_answered() {
    let scene = scene();
    let note = scene
        .context
        .ledger
        .borrow_mut()
        .note(
            scene.project.id,
            &cf_ledger::NewNote {
                from: Some("chief".to_owned()),
                to: "zeus".to_owned(),
                body: "FYI".to_owned(),
                task: None,
            },
        )
        .unwrap();
    let body = json!({ "question": note.id, "body": "x" }).to_string();
    let (status, said) = post(&scene, &scene.zeus, &body).await;
    assert_eq!(
        (status, said["error"].as_str()),
        (404, Some("unknown-message"))
    );

    // The same question as a window of another project reads it: no such question there.
    let other = scene
        .context
        .ledger
        .borrow_mut()
        .create_project(&cf_ledger::NewProject {
            directory: "/work/other".to_owned(),
            name: "other".to_owned(),
            chief: cf_ledger::NewChief {
                harness: "claude-code".to_owned(),
                agent: Some("mybuilder".to_owned()),
            },
            staff: Vec::new(),
            gate: false,
        })
        .unwrap();
    let chief = other
        .participants
        .iter()
        .find(|p| p.handle == "chief")
        .unwrap();
    let token = scene.context.credentials.issue(other.id, chief.id);
    let body = json!({ "question": scene.question.id, "body": "x" }).to_string();
    let (status, said) = post(&scene, &token, &body).await;
    assert_eq!(status, 404);
    assert_eq!(
        said["message"],
        format!("no question m-{} in this project", scene.question.id)
    );
    assert_eq!(scene.kicks.get(), 0);
}

#[tokio::test]
async fn what_the_ledger_refuses_is_said_with_its_status_and_wakes_nothing() {
    let scene = scene();
    // An answer with no words and no choices.
    let body = json!({ "question": scene.question.id }).to_string();
    let (status, said) = post(&scene, &scene.chief, &body).await;
    assert!((400..500).contains(&status), "{status} {said}");
    assert!(said["error"].is_string() && said["message"].is_string());
    assert_eq!(scene.kicks.get(), 0);
    assert!(answer_to(&scene).is_none());
}

#[tokio::test]
async fn the_body_is_read_first_so_a_bad_one_is_refused_before_anything_is_asked_of_the_ledger() {
    let scene = scene();
    for (written, status, error) in [
        ("not json", 400, "invalid-json"),
        ("[1]", 400, "invalid-json"),
        (&"x".repeat(3 * 1024 * 1024), 413, "too-large"),
    ] {
        let (got, said) = post(&scene, &scene.chief, written).await;
        assert_eq!((got, said["error"].as_str()), (status, Some(error)));
    }
    assert_eq!(scene.kicks.get(), 0);
}

/// `POST /api/answers/<id>/receipt` from the window of `token`, with `body` as its text.
async fn receipt_from(scene: &Scene, token: &str, id: &str, body: &str) -> (u16, Value) {
    let target = format!("/api/answers/{id}/receipt");
    let asked = request(Method::POST, &target, Some(token), body);
    let caller = caller_of(&scene.context, &asked).unwrap();
    said(receipt(&scene.context, &caller, asked, id).await)
}

const HANDED_OVER: &str = r#"{"received":true}"#;
const NOT_HANDED_OVER: &str = r#"{"received":false}"#;

/// zeus's question on its working task, the chief's choice for it, and the
/// claim of zeus's door on that answer: the question's number and the answer.
fn claimed(scene: &Scene) -> (i64, MessageView) {
    let question = working_question(scene);
    let answer = scene.choose(question.id, "red");
    scene.claim(question.id);
    (question.id, answer)
}

#[tokio::test]
async fn a_door_that_handed_its_answer_over_makes_it_read_and_its_task_work_and_wakes_the_dispatcher(
) {
    let scene = scene();
    let (_, answer) = claimed(&scene);
    assert_eq!(state_of(&scene, 1), "waiting");
    assert_eq!(scene.kicks.get(), 0);
    let (status, said) =
        receipt_from(&scene, &scene.zeus, &answer.id.to_string(), HANDED_OVER).await;
    assert_eq!(status, 200, "{said}");
    assert_eq!(said["message"]["id"], answer.id);
    assert_eq!(said["message"]["state"], "read");
    let read = scene.message(answer.id);
    assert_eq!(read.receipt, json!({ "door": true }));
    assert!(read.delivered_at.is_some());
    assert_eq!(state_of(&scene, 1), "working", "what it asked is answered");
    assert_eq!(scene.logged("message.read"), 1);
    assert_eq!(scene.next_for_zeus(), None, "nothing is left to paste");
    assert_eq!(scene.kicks.get(), 1);
}

#[tokio::test]
async fn an_answer_read_already_is_ok_and_writes_nothing_more_by_whichever_way_it_was_read() {
    let scene = scene();
    let (_, answer) = claimed(&scene);
    let id = answer.id.to_string();
    // The door's own receipt, said twice: a reply that was lost.
    for _ in 0..2 {
        let (status, said) = receipt_from(&scene, &scene.zeus, &id, HANDED_OVER).await;
        assert_eq!(
            (status, said["message"]["state"].as_str()),
            (200, Some("read"))
        );
    }
    assert_eq!(scene.logged("message.read"), 1, "read once");
    assert_eq!(scene.message(answer.id).receipt, json!({ "door": true }));

    // An answer `cf inbox read` served while the door held it, and then the door says so.
    let question = question_on(&scene, 1);
    let other = scene.choose(question.id, "blue");
    scene.claim(question.id);
    let zeus = scene.id("zeus");
    scene
        .context
        .ledger
        .borrow_mut()
        .receive_read(zeus, &[other.id], cf_ledger::Read::Inbox)
        .unwrap();
    let (status, _) = receipt_from(&scene, &scene.zeus, &other.id.to_string(), HANDED_OVER).await;
    assert_eq!(status, 200);
    assert_eq!(scene.message(other.id).receipt, json!({ "read": "inbox" }));
    assert_eq!(scene.logged("message.read"), 2, "each was read once");
}

#[tokio::test]
async fn an_answer_to_a_question_on_no_task_is_read_and_moves_no_task() {
    let scene = scene();
    let chief = scene.id("chief");
    let answer = scene
        .context
        .ledger
        .borrow_mut()
        .answer(scene.question.id, chief, Some(&json!("Yes")), None)
        .unwrap();
    let (status, said) =
        receipt_from(&scene, &scene.zeus, &answer.id.to_string(), HANDED_OVER).await;
    assert_eq!(status, 200, "{said}");
    assert_eq!(scene.message(answer.id).state, "read");
    assert_eq!(scene.kicks.get(), 1);
}

#[tokio::test]
async fn a_receipt_at_a_door_a_pause_shut_is_refused_and_the_answer_stays_to_come_as_a_message() {
    let scene = scene();
    let (_, answer) = claimed(&scene);
    // The pause voids the claim and shuts the door; the answer waits for the words that resume the task.
    scene.pause();
    let (status, said) =
        receipt_from(&scene, &scene.zeus, &answer.id.to_string(), HANDED_OVER).await;
    assert_eq!(status, 409);
    assert_eq!(
        said,
        json!({
            "error": "door-closed",
            "message": format!("m-{} is not answered here: it comes to you as a message", answer.id),
        })
    );
    assert_eq!(scene.message(answer.id).state, "queued");
    assert_eq!(scene.logged("message.read"), 0);
    assert_eq!(scene.kicks.get(), 0, "nothing was written");
}

#[tokio::test]
async fn a_receipt_for_an_answer_pasted_as_text_is_refused_whether_it_is_being_pasted_or_was() {
    let scene = scene();
    let (_, answer) = claimed(&scene);
    // The window rested before the door said anything: its claim is voided and the answer is pasted.
    let zeus = scene.id("zeus");
    {
        let mut ledger = scene.context.ledger.borrow_mut();
        ledger
            .release_claims(zeus, "its window is at rest")
            .unwrap();
        ledger.begin_delivery(answer.id).unwrap();
    }
    let id = answer.id.to_string();
    let (status, said) = receipt_from(&scene, &scene.zeus, &id, HANDED_OVER).await;
    assert_eq!((status, said["error"].as_str()), (409, Some("door-closed")));
    assert_eq!(scene.message(answer.id).state, "delivering");
    scene
        .context
        .ledger
        .borrow_mut()
        .confirm_delivery(answer.id, Some(&json!({ "item": "pasted" })))
        .unwrap();
    let (status, said) = receipt_from(&scene, &scene.zeus, &id, HANDED_OVER).await;
    assert_eq!((status, said["error"].as_str()), (409, Some("door-closed")));
    let pasted = scene.message(answer.id);
    assert_eq!(pasted.state, "delivered");
    assert_eq!(
        pasted.receipt,
        json!({ "item": "pasted" }),
        "the paste's receipt stands"
    );
    assert_eq!(scene.kicks.get(), 0);
}

#[tokio::test]
async fn a_door_that_did_not_hand_its_answer_over_gives_the_claim_back_and_wakes_the_dispatcher() {
    let scene = scene();
    let (_, answer) = claimed(&scene);
    assert_eq!(scene.next_for_zeus(), None, "claimed: the paste skips it");
    let (status, said) =
        receipt_from(&scene, &scene.zeus, &answer.id.to_string(), NOT_HANDED_OVER).await;
    assert_eq!(status, 200, "{said}");
    assert_eq!(said["message"]["state"], "queued");
    assert_eq!(
        scene.next_for_zeus(),
        Some("H: red".to_owned()),
        "the ledger's to deliver again"
    );
    assert_eq!(scene.logged("message.read"), 0);
    assert_eq!(state_of(&scene, 1), "waiting", "nobody received it");
    assert_eq!(scene.kicks.get(), 1);
}

#[tokio::test]
async fn a_receipt_that_does_not_say_whether_it_was_handed_over_is_refused_and_changes_nothing() {
    let scene = scene();
    let (_, answer) = claimed(&scene);
    let id = answer.id.to_string();
    for written in [
        "{}",
        r#"{"received":"yes"}"#,
        r#"{"received":1}"#,
        r#"{"received":null}"#,
    ] {
        let (status, said) = receipt_from(&scene, &scene.zeus, &id, written).await;
        assert_eq!(status, 400, "{written}");
        assert_eq!(
            said,
            json!({ "error": "invalid-receipt", "message": "received is true (handed over) or false (not)" }),
            "{written}"
        );
    }
    for (written, status, error) in [
        ("not json", 400, "invalid-json"),
        ("[1]", 400, "invalid-json"),
        (&"x".repeat(3 * 1024 * 1024), 413, "too-large"),
    ] {
        let (got, said) = receipt_from(&scene, &scene.zeus, &id, written).await;
        assert_eq!((got, said["error"].as_str()), (status, Some(error)));
    }
    assert_eq!(scene.kicks.get(), 0);
    assert_eq!(scene.next_for_zeus(), None, "the claim stands");
}

#[tokio::test]
async fn only_the_one_an_answer_was_for_may_say_it_was_handed_over_and_only_of_an_answer() {
    let scene = scene();
    let (question, answer) = claimed(&scene);
    for (token, id) in [
        (&scene.chief, answer.id.to_string()),
        (&scene.zeus, question.to_string()),
        (&scene.zeus, "99999".to_owned()),
        (&scene.zeus, "99999999999999999999".to_owned()),
    ] {
        let (status, said) = receipt_from(&scene, token, &id, HANDED_OVER).await;
        assert_eq!(status, 404, "{id}");
        assert_eq!(said["error"], "unknown-message", "{id}");
    }
    let (_, said) = receipt_from(&scene, &scene.chief, &answer.id.to_string(), HANDED_OVER).await;
    assert_eq!(
        said["message"],
        format!("no answer m-{} for you", answer.id)
    );
    assert_eq!(scene.message(answer.id).state, "queued", "nothing was read");
    assert_eq!(scene.kicks.get(), 0);
}

/// `POST /api/answers/read` from the window of `token`, with `body` as its text.
async fn read_from(scene: &Scene, token: &str, body: &str) -> (u16, Value) {
    let asked = request(Method::POST, "/api/answers/read", Some(token), body);
    let caller = caller_of(&scene.context, &asked).unwrap();
    said(read(&scene.context, &caller, asked).await)
}

fn said_read(ids: &[i64], via: &str) -> String {
    json!({ "answers": ids, "via": via }).to_string()
}

#[tokio::test]
async fn answers_cf_wrote_whole_are_received_by_the_one_they_are_for_and_the_dispatcher_is_woken() {
    let scene = scene();
    let question = working_question(&scene);
    let first = scene.choose(question.id, "red");
    let other = question_on(&scene, 1);
    let second = scene.choose(other.id, "blue");
    assert_eq!(state_of(&scene, 1), "waiting");
    // One of the two, said to have been read with `cf task get`: the other still obliges.
    let (status, said) = read_from(&scene, &scene.zeus, &said_read(&[first.id], "task")).await;
    assert_eq!((status, said), (200, json!({})));
    let read = scene.message(first.id);
    assert_eq!(read.state, "read");
    assert_eq!(read.receipt, json!({ "read": "task" }));
    assert!(read.delivered_at.is_some());
    assert_eq!(
        state_of(&scene, 1),
        "waiting",
        "the other question has no answer yet"
    );
    assert_eq!(scene.logged("message.read"), 1);
    assert_eq!(scene.kicks.get(), 1, "the dispatcher is woken");
    // The other, by `cf inbox read`: the task goes on.
    let (status, _) = read_from(&scene, &scene.zeus, &said_read(&[second.id], "inbox")).await;
    assert_eq!(status, 200);
    assert_eq!(scene.message(second.id).receipt, json!({ "read": "inbox" }));
    assert_eq!(state_of(&scene, 1), "working");
    assert_eq!(scene.kicks.get(), 2);
}

#[tokio::test]
async fn an_answer_a_door_claimed_is_received_by_a_read_and_its_claim_is_given_up() {
    let scene = scene();
    let (_, answer) = claimed(&scene);
    assert_eq!(scene.next_for_zeus(), None, "claimed: the paste skips it");
    let (status, _) = read_from(&scene, &scene.zeus, &said_read(&[answer.id], "inbox")).await;
    assert_eq!(status, 200);
    assert_eq!(scene.message(answer.id).receipt, json!({ "read": "inbox" }));
    assert_eq!(state_of(&scene, 1), "working");
    assert_eq!(scene.next_for_zeus(), None, "nothing is left to paste");
}

#[tokio::test]
async fn what_is_not_a_queued_answer_for_the_caller_is_left_as_it_is_and_wakes_nothing() {
    let scene = scene();
    let (_, answer) = claimed(&scene);
    let note = note_for_zeus(&scene, 1, "Mind the tests");
    // Said by the chief, who is not the one it was for; a note; a message that is not there.
    let (status, _) = read_from(
        &scene,
        &scene.chief,
        &said_read(&[answer.id, scene.question.id], "task"),
    )
    .await;
    assert_eq!(status, 200);
    let (status, _) = read_from(
        &scene,
        &scene.zeus,
        &said_read(&[note.id, 99_999, -1, 0], "task"),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(scene.message(answer.id).state, "queued");
    assert_eq!(scene.message(note.id).state, "queued");
    assert_eq!(scene.logged("message.read"), 0);
    assert_eq!(scene.kicks.get(), 0);
    // An answer received is not received twice.
    read_from(&scene, &scene.zeus, &said_read(&[answer.id], "task")).await;
    read_from(&scene, &scene.zeus, &said_read(&[answer.id], "task")).await;
    assert_eq!(scene.logged("message.read"), 1);
    assert_eq!(scene.kicks.get(), 1);
}

#[tokio::test]
async fn an_answer_held_for_the_human_is_not_received_by_being_named() {
    let scene = scene();
    let question = working_question(&scene);
    scene
        .context
        .ledger
        .borrow_mut()
        .set_gate(scene.project.id, true)
        .unwrap();
    let answer = scene.choose(question.id, "red");
    assert_eq!(answer.state, "gated");
    read_from(&scene, &scene.zeus, &said_read(&[answer.id], "task")).await;
    assert_eq!(scene.message(answer.id).state, "gated");
    assert_eq!(scene.kicks.get(), 0);
}

#[tokio::test]
async fn a_read_that_names_no_answers_is_ok_and_one_that_is_no_list_of_numbers_or_names_no_way_is_refused(
) {
    let scene = scene();
    let (_, answer) = claimed(&scene);
    let (status, said) = read_from(&scene, &scene.zeus, &said_read(&[], "task")).await;
    assert_eq!((status, said), (200, json!({})));
    let id = answer.id;
    for written in [
        "{}".to_owned(),
        r#"{"answers":[1]}"#.to_owned(),
        json!({ "answers": [id], "via": "list" }).to_string(),
        json!({ "answers": [id], "via": 3 }).to_string(),
        json!({ "via": "task" }).to_string(),
        json!({ "answers": id, "via": "task" }).to_string(),
        json!({ "answers": [id, "7"], "via": "task" }).to_string(),
        json!({ "answers": [id, 2.5], "via": "task" }).to_string(),
        json!({ "answers": [null], "via": "task" }).to_string(),
    ] {
        let (status, said) = read_from(&scene, &scene.zeus, &written).await;
        assert_eq!(status, 400, "{written}");
        assert_eq!(said["error"], "invalid-read", "{written}");
    }
    for (written, status, error) in [
        ("not json", 400, "invalid-json"),
        ("[1]", 400, "invalid-json"),
        (&"x".repeat(3 * 1024 * 1024), 413, "too-large"),
    ] {
        let (got, said) = read_from(&scene, &scene.zeus, written).await;
        assert_eq!((got, said["error"].as_str()), (status, Some(error)));
    }
    assert_eq!(scene.next_for_zeus(), None, "the claim stands");
    assert_eq!(scene.kicks.get(), 0);
}

#[test]
fn a_message_number_is_a_whole_number_over_zero_and_within_what_a_message_holds() {
    let number = |text: &str| whole_and_positive(Some(&serde_json::from_str(text).unwrap()));
    assert_eq!(number("7"), Some(7));
    assert_eq!(number("7.0"), Some(7));
    assert_eq!(number("1e2"), Some(100));
    assert_eq!(number("9007199254740992"), Some(9_007_199_254_740_992));
    for text in [
        "0", "-1", "0.5", "7.5", "\"7\"", "null", "true", "[7]", "{}", "1e19", "-0",
    ] {
        assert_eq!(number(text), None, "{text}");
    }
    assert_eq!(whole_and_positive(None), None);
}

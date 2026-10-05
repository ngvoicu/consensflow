//! `POST /api/answers`: what it reads and in what order, whom it refuses,
//! and that the dispatcher is woken once the answer is written and never
//! for one that was refused.

use hyper::Method;
use serde_json::{json, Value};

use super::*;
use crate::api::callers::caller_of;
use crate::testing::{request, said, scene, Scene};

/// `POST /api/answers` from the window of `token`, with `body` as its text.
async fn post(scene: &Scene, token: &str, body: &str) -> (u16, Value) {
    let asked = request(Method::POST, "/api/answers", Some(token), body);
    let caller = caller_of(&scene.context, &asked).unwrap();
    said(handle(&scene.context, &caller, asked).await)
}

fn answer_to(scene: &Scene) -> Option<cf_ledger::MessageView> {
    scene
        .context
        .ledger
        .borrow()
        .answer_to(scene.question.id)
        .unwrap()
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

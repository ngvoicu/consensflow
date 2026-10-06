//! `GET /api/inbox/<id>`: one message, whole, to those it was between; what
//! still waits for the human is not yet the recipient's to read.

use hyper::Method;
use serde_json::json;

use crate::api::routes::tests::support::{api, gated_brief};
use crate::testing::scene;

#[tokio::test]
async fn the_recipient_and_the_sender_read_the_whole_message_and_no_one_else_does() {
    let scene = scene();
    let target = format!("/api/inbox/{}", scene.question.id);
    for token in [&scene.chief, &scene.zeus] {
        let (status, said) = api(&scene, Method::GET, &target, token, "").await;
        assert_eq!(status, 200);
        let message = said["message"].as_object().unwrap();
        assert_eq!(
            message.keys().map(String::as_str).collect::<Vec<_>>(),
            [
                "id",
                "projectId",
                "recipient",
                "recipientId",
                "recipientRole",
                "sender",
                "kind",
                "taskNumber",
                "replyTo",
                "body",
                "state",
                "attempts",
                "reason",
                "receipt",
                "questions",
                "choices",
                "urgent",
                "createdAt",
                "deliveredAt"
            ],
            "the ledger's own view, not the summary of a list"
        );
        assert_eq!(said["message"]["body"], "Which?");
    }
    // A third window of the project is neither.
    let hera = {
        let mut ledger = scene.context.ledger.borrow_mut();
        ledger
            .add_member(
                scene.project.id,
                &cf_ledger::NewMember {
                    agent: "hera".to_owned(),
                    harness: "claude-code".to_owned(),
                    designer: false,
                    roles: vec!["worker".to_owned()],
                    tier: "standard".to_owned(),
                },
            )
            .unwrap();
        let project = ledger.project(scene.project.id).unwrap().unwrap();
        let hera = project
            .participants
            .iter()
            .find(|p| p.handle == "hera")
            .unwrap();
        scene.context.credentials.issue(project.id, hera.id)
    };
    let (status, said) = api(&scene, Method::GET, &target, &hera, "").await;
    assert_eq!(status, 404);
    assert_eq!(
        said,
        json!({ "error": "unknown-message", "message": format!("no message m-{} for you", scene.question.id) })
    );
    assert_eq!(scene.kicks.get(), 0);
}

#[tokio::test]
async fn a_message_that_is_not_there_is_quoted_as_its_digits_were_typed() {
    let scene = scene();
    for digits in ["9", "00012", "99999999999999999999999"] {
        let (status, said) = api(
            &scene,
            Method::GET,
            &format!("/api/inbox/{digits}"),
            &scene.chief,
            "",
        )
        .await;
        assert_eq!(status, 404, "{digits}");
        assert_eq!(said["message"], format!("no message m-{digits} for you"));
    }
    // The same number, written with zeros before it, is the message.
    let digits = format!("000{}", scene.question.id);
    let (status, _) = api(
        &scene,
        Method::GET,
        &format!("/api/inbox/{digits}"),
        &scene.chief,
        "",
    )
    .await;
    assert_eq!(status, 200);
}

#[tokio::test]
async fn a_message_of_another_project_is_none_of_this_one_s_even_for_a_handle_that_repeats() {
    let scene = scene();
    // Every project's chief is `@chief`: the message of another project is
    // not the chief's here.
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
    let (status, _) = api(
        &scene,
        Method::GET,
        &format!("/api/inbox/{}", scene.question.id),
        &token,
        "",
    )
    .await;
    assert_eq!(status, 404);
}

#[tokio::test]
async fn what_waits_for_the_human_is_not_its_recipient_s_to_read_but_is_its_sender_s() {
    let scene = scene();
    let (session, brief) = gated_brief(&scene);
    let target = format!("/api/inbox/{brief}");
    let (status, said) = api(&scene, Method::GET, &target, &session, "").await;
    assert_eq!(status, 404, "{said}");
    assert_eq!(said["message"], format!("no message m-{brief} for you"));
    let (status, said) = api(&scene, Method::GET, &target, &scene.chief, "").await;
    assert_eq!(status, 200);
    assert_eq!(said["message"]["state"], "gated");
}

#[tokio::test]
async fn only_a_get_reads_a_message() {
    let scene = scene();
    let target = format!("/api/inbox/{}", scene.question.id);
    let (status, said) = api(&scene, Method::POST, &target, &scene.chief, "").await;
    assert_eq!(status, 404);
    assert_eq!(said["error"], "unknown-route");
}

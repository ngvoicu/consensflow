//! `GET /api/inbox/<id>`: one message, whole, to those it was between; what
//! still waits for the human is not yet the recipient's to read. A read
//! changes nothing.

use hyper::Method;
use serde_json::json;

use crate::api::routes::tests::support::{
    answer_in_words, answered_question, api, gated_brief, note_for_zeus, plain_question_on,
    state_of, working_question, working_task,
};
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
async fn an_answer_read_whole_by_the_one_it_is_for_is_served_and_nothing_is_received_by_serving_it()
{
    let scene = scene();
    let answer = answered_question(&scene, "JSON");
    assert_eq!(state_of(&scene, 1), "waiting");
    let target = format!("/api/inbox/{}", answer.id);
    // Read twice, as zeus, whose answer it is: the whole of it, as it was.
    for _ in 0..2 {
        let (status, said) = api(&scene, Method::GET, &target, &scene.zeus, "").await;
        assert_eq!(status, 200);
        assert_eq!(said["message"]["body"], "H: JSON");
        assert_eq!(said["message"]["state"], "queued");
    }
    // Whether `cf` printed it is for `cf` to say, once it has: a read is no receipt.
    let still = scene.message(answer.id);
    assert_eq!(still.state, "queued");
    assert_eq!(still.receipt, serde_json::Value::Null);
    assert_eq!(state_of(&scene, 1), "waiting");
    assert_eq!(scene.logged("message.read"), 0);
    assert_eq!(scene.kicks.get(), 0, "nothing was written");
    assert_eq!(scene.next_for_zeus(), Some("H: JSON".to_owned()));
}

#[tokio::test]
async fn what_the_list_cuts_this_route_serves_whole_and_still_receives_nothing() {
    let scene = scene();
    let number = working_task(&scene);
    let question = plain_question_on(&scene, number, "Which formats?");
    let answer = answer_in_words(&scene, question.id, "JSON\nand then YAML");
    let target = format!("/api/inbox/{}", answer.id);
    let (_, said) = api(&scene, Method::GET, &target, &scene.zeus, "").await;
    assert_eq!(said["message"]["body"], "JSON\nand then YAML");
    assert_eq!(scene.message(answer.id).state, "queued");
    assert_eq!(state_of(&scene, number), "waiting");
}

#[tokio::test]
async fn a_read_by_the_sender_or_of_a_note_changes_nothing_either() {
    let scene = scene();
    let answer = answered_question(&scene, "JSON");
    // The chief wrote the answer: reading it is no receipt of zeus's.
    let (status, _) = api(
        &scene,
        Method::GET,
        &format!("/api/inbox/{}", answer.id),
        &scene.chief,
        "",
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(scene.message(answer.id).state, "queued");
    // A note is pasted, and read here it stays to be.
    let note = note_for_zeus(&scene, 1, "Mind the tests");
    let (status, _) = api(
        &scene,
        Method::GET,
        &format!("/api/inbox/{}", note.id),
        &scene.zeus,
        "",
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(scene.message(note.id).state, "queued");
    assert_eq!(state_of(&scene, 1), "waiting");
    assert_eq!(scene.kicks.get(), 0, "nothing was written");
}

#[tokio::test]
async fn a_gated_answer_is_not_its_recipients_to_read_so_it_is_not_received() {
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
    let target = format!("/api/inbox/{}", answer.id);
    let (status, said) = api(&scene, Method::GET, &target, &scene.zeus, "").await;
    assert_eq!(status, 404, "{said}");
    assert_eq!(scene.message(answer.id).state, "gated");
    assert_eq!(scene.kicks.get(), 0);
}

#[tokio::test]
async fn only_a_get_reads_a_message() {
    let scene = scene();
    let target = format!("/api/inbox/{}", scene.question.id);
    let (status, said) = api(&scene, Method::POST, &target, &scene.chief, "").await;
    assert_eq!(status, 404);
    assert_eq!(said["error"], "unknown-route");
}

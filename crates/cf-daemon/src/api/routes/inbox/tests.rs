//! `GET /api/inbox`: what the ledger holds for the window, newest first, as
//! summaries, and not what still waits for the human.

use hyper::Method;
use serde_json::{json, Value};

use crate::api::routes::tests::support::{
    answer_in_words, answered_question, api, gated_brief, note_for_zeus, plain_question_on,
    state_of, working_question, working_task,
};
use crate::testing::scene;

#[tokio::test]
async fn a_window_is_given_its_messages_newest_first_as_summaries() {
    let scene = scene();
    // The question zeus asked the chief is the chief's.
    let (status, said) = api(&scene, Method::GET, "/api/inbox", &scene.chief, "").await;
    assert_eq!(status, 200);
    let messages = said["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(
        messages[0].to_string(),
        format!(
            r#"{{"id":{},"kind":"question","state":"queued","sender":"zeus","recipient":"chief","task":null,"preview":"Which?","questions":null,"choices":null,"createdAt":{}}}"#,
            scene.question.id,
            serde_json::Value::from(scene.question.created_at.clone())
        )
    );
    // A second one is first.
    api(
        &scene,
        Method::POST,
        "/api/notes",
        &scene.zeus,
        r#"{"body":"Second thing"}"#,
    )
    .await;
    let (_, said) = api(&scene, Method::GET, "/api/inbox", &scene.chief, "").await;
    let previews: Vec<&str> = said["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|message| message["preview"].as_str().unwrap())
        .collect();
    assert_eq!(previews, ["Second thing", "Which?"]);
}

#[tokio::test]
async fn a_window_with_nothing_is_given_an_empty_list() {
    let scene = scene();
    let (status, said) = api(&scene, Method::GET, "/api/inbox", &scene.zeus, "").await;
    assert_eq!(
        (status, said.to_string()),
        (200, r#"{"messages":[]}"#.to_owned())
    );
    assert_eq!(scene.kicks.get(), 0);
}

#[tokio::test]
async fn what_still_waits_for_the_human_is_not_in_the_recipient_s_inbox() {
    let scene = scene();
    let (session, _) = gated_brief(&scene);
    let (status, said) = api(&scene, Method::GET, "/api/inbox", &session, "").await;
    assert_eq!(
        (status, said.to_string()),
        (200, r#"{"messages":[]}"#.to_owned())
    );
}

/// The ids and states the list said, in its order.
fn listed(said: &Value) -> Vec<(i64, &str)> {
    said["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|message| {
            (
                message["id"].as_i64().unwrap(),
                message["state"].as_str().unwrap(),
            )
        })
        .collect()
}

#[tokio::test]
async fn an_answer_the_list_shows_whole_is_received_by_the_read_and_its_task_works_again() {
    let scene = scene();
    let answer = answered_question(&scene, "JSON");
    assert_eq!(state_of(&scene, 1), "waiting");
    let (status, said) = api(&scene, Method::GET, "/api/inbox", &scene.zeus, "").await;
    assert_eq!(status, 200);
    assert_eq!(
        listed(&said)[0],
        (answer.id, "queued"),
        "the list says it as it was served"
    );
    let read = scene.message(answer.id);
    assert_eq!(read.state, "read");
    assert_eq!(read.receipt, json!({ "read": "inbox" }));
    assert!(read.delivered_at.is_some());
    assert_eq!(state_of(&scene, 1), "working");
    assert_eq!(scene.logged("message.read"), 1);
    assert_eq!(
        scene.kicks.get(),
        1,
        "the dispatcher is woken: the task goes on"
    );
    // The next list says it read, and writes nothing more.
    let (_, said) = api(&scene, Method::GET, "/api/inbox", &scene.zeus, "").await;
    assert_eq!(listed(&said)[0], (answer.id, "read"));
    assert_eq!(scene.logged("message.read"), 1);
    assert_eq!(scene.kicks.get(), 1);
}

#[tokio::test]
async fn an_answer_the_door_claimed_is_received_by_the_read_and_its_claim_is_given_up() {
    let scene = scene();
    let question = working_question(&scene);
    let answer = scene.choose(question.id, "red");
    scene.claim(question.id);
    assert_eq!(scene.next_for_zeus(), None, "claimed: the paste skips it");
    api(&scene, Method::GET, "/api/inbox", &scene.zeus, "").await;
    assert_eq!(scene.message(answer.id).receipt, json!({ "read": "inbox" }));
    assert_eq!(state_of(&scene, 1), "working");
}

#[tokio::test]
async fn an_answer_the_list_cuts_and_a_note_are_not_received() {
    let scene = scene();
    // Two questions on the one task: one answered over two lines, one in a line too long for a preview.
    let number = working_task(&scene);
    let first = plain_question_on(&scene, number, "Which formats?");
    let second = plain_question_on(&scene, number, "Which grammar?");
    let two_lines = answer_in_words(&scene, first.id, "JSON\nand then YAML");
    let long = answer_in_words(&scene, second.id, &"x".repeat(300));
    let note = note_for_zeus(&scene, number, "Mind the tests");
    let (status, said) = api(&scene, Method::GET, "/api/inbox", &scene.zeus, "").await;
    assert_eq!(status, 200);
    for id in [two_lines.id, long.id, note.id] {
        assert!(listed(&said).contains(&(id, "queued")), "m-{id} is listed");
        assert_eq!(scene.message(id).state, "queued", "m-{id} was not received");
    }
    assert_eq!(state_of(&scene, 1), "waiting");
    assert_eq!(scene.kicks.get(), 0, "nothing was written");
}

#[tokio::test]
async fn a_gated_answer_is_not_listed_and_so_not_received() {
    let scene = scene();
    let question = working_question(&scene);
    scene
        .context
        .ledger
        .borrow_mut()
        .set_gate(scene.project.id, true)
        .unwrap();
    let answer = scene.choose(question.id, "red");
    assert_eq!(answer.state, "gated", "it waits for the human");
    let (status, said) = api(&scene, Method::GET, "/api/inbox", &scene.zeus, "").await;
    assert_eq!(status, 200);
    assert!(listed(&said).iter().all(|(id, _)| *id != answer.id));
    assert_eq!(scene.message(answer.id).state, "gated");
    assert_eq!(scene.kicks.get(), 0);
}

#[tokio::test]
async fn the_chiefs_list_holds_nothing_of_the_answer_it_wrote_and_receives_nothing() {
    let scene = scene();
    let answer = answered_question(&scene, "JSON");
    // The answer is zeus's: the chief's inbox is its own.
    let (_, said) = api(&scene, Method::GET, "/api/inbox", &scene.chief, "").await;
    assert!(listed(&said).iter().all(|(id, _)| *id != answer.id));
    assert_eq!(scene.message(answer.id).state, "queued");
    assert_eq!(scene.kicks.get(), 0);
}

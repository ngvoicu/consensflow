//! `GET /api/inbox`: what the ledger holds for the window, newest first, as
//! summaries, and not what still waits for the human. A list changes nothing.

use hyper::Method;
use serde_json::Value;

use crate::api::routes::tests::support::{
    answer_in_words, answered_question, api, gated_brief, note_for_zeus, plain_question_on,
    question_on, state_of, working_question, working_task,
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
async fn a_list_receives_nothing_whatever_it_shows_and_the_answers_are_still_to_be_delivered() {
    let scene = scene();
    // Four things on the one task: an answer the list shows whole, one it cuts
    // over two lines, one in a line too long for a preview, and a note; and one
    // a door claimed.
    let number = working_task(&scene);
    let ask = |words: &str| plain_question_on(&scene, number, words);
    let (first, second, third) = (ask("Which?"), ask("Which formats?"), ask("Which grammar?"));
    let whole = answer_in_words(&scene, first.id, "JSON");
    let two_lines = answer_in_words(&scene, second.id, "JSON\nand then YAML");
    let long = answer_in_words(&scene, third.id, &"x".repeat(300));
    let note = note_for_zeus(&scene, number, "Mind the tests");
    let claimed = question_on(&scene, number);
    let held = scene.choose(claimed.id, "red");
    scene.claim(claimed.id);
    let (status, said) = api(&scene, Method::GET, "/api/inbox", &scene.zeus, "").await;
    assert_eq!(status, 200);
    for id in [whole.id, two_lines.id, long.id, note.id, held.id] {
        assert!(listed(&said).contains(&(id, "queued")), "m-{id} is listed");
        assert_eq!(scene.message(id).state, "queued", "m-{id} was not received");
    }
    assert_eq!(scene.message(held.id).receipt, Value::Null);
    assert_eq!(state_of(&scene, number), "waiting");
    assert_eq!(scene.logged("message.read"), 0);
    assert_eq!(scene.kicks.get(), 0, "nothing was written");
    assert_eq!(scene.next_for_zeus(), Some("JSON".to_owned()));
}

#[tokio::test]
async fn an_answer_a_list_showed_is_still_there_to_be_pasted_when_the_next_one_is_read() {
    let scene = scene();
    let answer = answered_question(&scene, "JSON");
    assert_eq!(state_of(&scene, 1), "waiting");
    for _ in 0..2 {
        let (status, said) = api(&scene, Method::GET, "/api/inbox", &scene.zeus, "").await;
        assert_eq!(status, 200);
        assert_eq!(listed(&said)[0], (answer.id, "queued"));
    }
    assert_eq!(scene.next_for_zeus(), Some("H: JSON".to_owned()));
    assert_eq!(state_of(&scene, 1), "waiting");
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

//! `GET /api/inbox`: what the ledger holds for the window, newest first, as
//! summaries, and not what still waits for the human.

use hyper::Method;

use crate::api::routes::tests::support::{api, gated_brief};
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

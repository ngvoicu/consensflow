//! `POST /api/notes`: a note, from the chief to the human, from a member to
//! whoever gave it its task. Where a test says what Node answered, it is what
//! the real API answered the same request (the probe that printed those answers
//! went with Node's API).

use hyper::Method;
use serde_json::{json, Value};

use crate::api::routes::tests::support::{api, state_of, working_task};
use crate::testing::{scene, Scene};

async fn note(scene: &Scene, token: &str, body: &str) -> (u16, Value) {
    api(scene, Method::POST, "/api/notes", token, body).await
}

fn refused(answered: &(u16, Value)) -> (u16, &str, &str) {
    (
        answered.0,
        answered.1["error"].as_str().unwrap_or("-"),
        answered.1["message"].as_str().unwrap_or("-"),
    )
}

#[tokio::test]
async fn the_chief_notes_the_human_whatever_it_asks_and_whatever_task_it_is_on() {
    let scene = scene();
    for body in [
        r#"{"body":"Built."}"#,
        r#"{"body":"Built.","to":"chief"}"#,
        r#"{"body":"Built.","to":"human"}"#,
    ] {
        let (status, said) = note(&scene, &scene.chief, body).await;
        assert_eq!(status, 201, "{body}");
        assert_eq!(said["message"]["recipient"], "human", "{body}");
        assert_eq!(said["message"]["sender"], "chief");
        assert_eq!(said["message"]["kind"], "note");
        assert_eq!(said["message"]["preview"], "Built.");
    }
    assert_eq!(scene.kicks.get(), 3);
}

#[tokio::test]
async fn a_member_notes_whoever_gave_it_its_task_with_the_task_named() {
    let scene = scene();
    let number = working_task(&scene);
    let (status, said) = note(&scene, &scene.zeus, r#"{"body":"Half of it is done."}"#).await;
    assert_eq!(status, 201, "{said}");
    assert_eq!(said["message"]["recipient"], "chief");
    assert_eq!(said["message"]["task"], number);
    assert_eq!(
        said.as_object().unwrap().keys().collect::<Vec<_>>(),
        ["message"]
    );
    assert_eq!(scene.kicks.get(), 1);
}

#[tokio::test]
async fn a_member_whose_task_the_pause_has_not_stopped_yet_notes_about_that_task() {
    let scene = scene();
    let number = working_task(&scene);
    scene.pause();
    let (status, said) = note(&scene, &scene.zeus, r#"{"body":"Half of it is done."}"#).await;
    assert_eq!(status, 201, "{said}");
    assert_eq!(
        said["message"]["recipient"], "chief",
        "to whoever gave it the task"
    );
    assert_eq!(
        said["message"]["task"], number,
        "about the task it was stopped on"
    );
    assert_eq!(state_of(&scene, number), "paused", "a note moves nothing");
    assert_eq!(scene.kicks.get(), 1);
}

#[tokio::test]
async fn a_member_with_no_task_notes_the_chief() {
    let scene = scene();
    let (status, said) = note(&scene, &scene.zeus, r#"{"body":"hello"}"#).await;
    assert_eq!(status, 201);
    assert_eq!(said["message"]["recipient"], "chief");
    assert_eq!(said["message"]["task"], json!(null));
}

#[tokio::test]
async fn a_member_may_not_note_the_human_and_is_told_who_it_notes_instead() {
    let scene = scene();
    let said = note(&scene, &scene.zeus, r#"{"body":"hello","to":"human"}"#).await;
    assert_eq!(
        refused(&said),
        (
            403,
            "not-the-chief",
            "only the chief notes the human; without --human, your note goes to @chief"
        )
    );
    // `to` is the human or nobody: anything else is not read.
    let (status, _) = note(&scene, &scene.zeus, r#"{"body":"hello","to":"hera"}"#).await;
    assert_eq!(status, 201);
    assert_eq!(scene.kicks.get(), 1);
}

#[tokio::test]
async fn the_body_is_read_first_and_the_words_are_the_ledger_s_to_refuse() {
    let scene = scene();
    for token in [&scene.zeus, &scene.chief] {
        for (body, said) in [
            (
                "not json",
                (
                    400,
                    "invalid-json",
                    "the request body must be a JSON object",
                ),
            ),
            (
                "[]",
                (
                    400,
                    "invalid-json",
                    "the request body must be a JSON object",
                ),
            ),
            (
                "{}",
                (
                    400,
                    "invalid-text",
                    "body must be text, not empty, at most 1000000 characters",
                ),
            ),
            (
                r#"{"body":5}"#,
                (
                    400,
                    "invalid-text",
                    "body must be text, not empty, at most 1000000 characters",
                ),
            ),
        ] {
            let answered = note(&scene, token, body).await;
            assert_eq!(refused(&answered), said, "{body}");
        }
    }
    // Before who it may note is looked at.
    let said = note(&scene, &scene.zeus, r#"{"to":"human"}"#).await;
    assert_eq!(said.1["error"], "not-the-chief", "{said:?}");
    let said = note(&scene, &scene.zeus, "not json").await;
    assert_eq!(said.1["error"], "invalid-json");
    assert_eq!(scene.kicks.get(), 0);
}

#[tokio::test]
async fn a_member_whose_task_was_cancelled_is_told_so_and_the_chief_never_is() {
    let scene = scene();
    let number = working_task(&scene);
    scene
        .context
        .ledger
        .borrow_mut()
        .cancel_task(scene.project.id, number, "human")
        .unwrap();
    let said = note(&scene, &scene.zeus, r#"{"body":"Half of it is done."}"#).await;
    assert_eq!(
        refused(&said),
        (
            409,
            "task-cancelled",
            "T-1 is cancelled: nothing more of it goes to @chief"
        )
    );
    assert_eq!(scene.kicks.get(), 0);
    let (status, _) = note(&scene, &scene.chief, r#"{"body":"Noted."}"#).await;
    assert_eq!(status, 201);
}

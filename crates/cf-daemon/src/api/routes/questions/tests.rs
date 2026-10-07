//! `POST /api/questions`: a member's question for the chief. Where a test says
//! what Node answered, it is what the real API answered the same request:
//! `node tests/goldens/daemon/probes/api-corners.mjs` prints it again.

use hyper::Method;
use serde_json::{json, Value};

use crate::api::routes::tests::support::{api, state_of, working_task};
use crate::testing::{scene, Scene};

async fn ask(scene: &Scene, token: &str, body: &str) -> (u16, Value) {
    api(scene, Method::POST, "/api/questions", token, body).await
}

fn refused(answered: &(u16, Value)) -> (u16, &str, &str) {
    (
        answered.0,
        answered.1["error"].as_str().unwrap_or("-"),
        answered.1["message"].as_str().unwrap_or("-"),
    )
}

#[tokio::test]
async fn the_chief_is_sent_to_its_terminal_before_its_body_is_read() {
    let scene = scene();
    for body in [r#"{"body":"Ship?"}"#, "not json", "[]"] {
        let said = ask(&scene, &scene.chief, body).await;
        assert_eq!(
            refused(&said),
            (
                403,
                "ask-in-your-terminal",
                "ask the human here in your terminal: they read and answer you there"
            ),
            "{body}"
        );
    }
    let huge = "x".repeat(3 * 1024 * 1024);
    assert_eq!(ask(&scene, &scene.chief, &huge).await.0, 403);
    assert_eq!(scene.kicks.get(), 0);
}

#[tokio::test]
async fn a_member_s_question_is_about_the_task_it_has_in_progress_and_the_task_waits_for_the_answer(
) {
    let scene = scene();
    let number = working_task(&scene);
    let (status, said) = ask(&scene, &scene.zeus, r#"{"body":"Which format?"}"#).await;
    assert_eq!(status, 201, "{said}");
    let message = said["message"].to_string();
    assert!(
        message.starts_with(&format!(
            r#"{{"id":{},"kind":"question","state":"queued","sender":"zeus","recipient":"chief","task":{number},"preview":"Which format?","questions":null,"choices":null,"createdAt":""#,
            said["message"]["id"]
        )),
        "{message}"
    );
    assert_eq!(
        said.as_object().unwrap().keys().collect::<Vec<_>>(),
        ["message"]
    );
    assert_eq!(state_of(&scene, number), "waiting");
    assert_eq!(scene.kicks.get(), 1);
}

#[tokio::test]
async fn a_window_with_no_task_asks_about_none() {
    let scene = scene();
    let (status, said) = ask(&scene, &scene.zeus, r#"{"body":"Which?"}"#).await;
    assert_eq!(status, 201);
    assert_eq!(said["message"]["task"], json!(null));
    assert_eq!(said["message"]["preview"], "Which?");
}

#[tokio::test]
async fn a_window_whose_task_was_cancelled_is_told_so_and_nothing_is_put() {
    let scene = scene();
    let number = working_task(&scene);
    scene
        .context
        .ledger
        .borrow_mut()
        .cancel_task(scene.project.id, number, "human")
        .unwrap();
    let said = ask(&scene, &scene.zeus, r#"{"body":"Which format?"}"#).await;
    assert_eq!(
        refused(&said),
        (
            409,
            "task-cancelled",
            "T-1 is cancelled: nothing more of it goes to @chief"
        )
    );
    // The body is read first: a bad one is the answer, cancelled or not.
    assert_eq!(
        ask(&scene, &scene.zeus, "not json").await.1["error"],
        "invalid-json"
    );
    assert_eq!(scene.kicks.get(), 0);
}

/// The first poll of the door of question `id`, as zeus's harness makes it.
async fn poll(scene: &Scene, id: i64) -> (u16, Value) {
    let target = format!("/api/questions/{id}?wait=5000");
    api(scene, Method::GET, &target, &scene.zeus, "").await
}

#[tokio::test(start_paused = true)]
async fn a_window_the_pause_has_not_stopped_yet_asks_about_its_paused_task_and_its_door_is_born_shut(
) {
    let scene = scene();
    let number = working_task(&scene);
    scene.pause();
    let (status, said) = ask(&scene, &scene.zeus, r#"{"body":"Which format?"}"#).await;
    assert_eq!(status, 201, "{said}");
    assert_eq!(
        said["message"]["task"], number,
        "it is about the task it was stopped on"
    );
    assert_eq!(state_of(&scene, number), "paused", "asking moves nothing");
    assert_eq!(scene.kicks.get(), 1);
    // The turn that asks is an old one: its door answers at its first poll, without a wait.
    let started = tokio::time::Instant::now();
    let (status, said) = poll(&scene, said["message"]["id"].as_i64().unwrap()).await;
    assert_eq!((status, said["error"].as_str()), (409, Some("door-closed")));
    assert_eq!(started.elapsed(), std::time::Duration::ZERO);
}

#[tokio::test(start_paused = true)]
async fn a_window_whose_task_waits_for_the_words_that_resume_it_asks_about_it_with_its_door_born_shut(
) {
    let scene = scene();
    let number = working_task(&scene);
    scene.pause();
    scene
        .context
        .ledger
        .borrow_mut()
        .resume_task(scene.project.id, number, Some("chief"), "Carry on")
        .unwrap();
    assert_eq!(
        state_of(&scene, number),
        "queued",
        "the words are not pasted yet"
    );
    let (status, said) = ask(&scene, &scene.zeus, r#"{"body":"Which format?"}"#).await;
    assert_eq!(status, 201, "{said}");
    assert_eq!(said["message"]["task"], number);
    assert_eq!(
        state_of(&scene, number),
        "queued",
        "the words that resume it come first"
    );
    let (status, said) = poll(&scene, said["message"]["id"].as_i64().unwrap()).await;
    assert_eq!((status, said["error"].as_str()), (409, Some("door-closed")));
}

#[tokio::test(start_paused = true)]
async fn a_window_at_work_asks_with_its_door_open_and_the_task_waits() {
    let scene = scene();
    let number = working_task(&scene);
    let (_, said) = ask(&scene, &scene.zeus, r#"{"body":"Which format?"}"#).await;
    assert_eq!(state_of(&scene, number), "waiting");
    let started = tokio::time::Instant::now();
    let (status, said) = poll(&scene, said["message"]["id"].as_i64().unwrap()).await;
    assert_eq!((status, said["answer"].clone()), (200, json!(null)));
    assert_eq!(
        started.elapsed(),
        std::time::Duration::from_secs(5),
        "nothing shut it: it waited"
    );
}

#[tokio::test]
async fn words_that_are_no_words_are_refused_and_questions_that_are_none_are_refused_in_theirs() {
    let scene = scene();
    for (body, said) in [
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
        (
            r#"{"body":"x","questions":null}"#,
            (400, "bad-questions", "questions: one to 4 questions"),
        ),
        (
            r#"{"body":"x","questions":[]}"#,
            (400, "bad-questions", "questions: one to 4 questions"),
        ),
        (
            r#"{"body":"x","questions":"x"}"#,
            (400, "bad-questions", "questions: one to 4 questions"),
        ),
    ] {
        let answered = ask(&scene, &scene.zeus, body).await;
        assert_eq!(refused(&answered), said, "{body}");
    }
    assert_eq!(scene.kicks.get(), 0);
}

#[tokio::test]
async fn a_question_with_options_is_put_with_them_and_its_words_are_theirs() {
    let scene = scene();
    // What it says in `body` is not read when it asks by questions.
    let body = json!({
        "body": 5,
        "questions": [{
            "question": "Which parser?",
            "header": "Parser",
            "options": [{ "label": "A" }, { "label": "B", "description": "bee" }]
        }]
    })
    .to_string();
    let (status, said) = ask(&scene, &scene.zeus, &body).await;
    assert_eq!(status, 201, "{said}");
    assert_eq!(said["message"]["preview"], "Parser: Which parser?");
    assert_eq!(
        said["message"]["questions"].to_string(),
        r#"[{"question":"Which parser?","header":"Parser","options":[{"label":"A","description":null},{"label":"B","description":"bee"}],"multiple":false}]"#
    );
    assert_eq!(scene.kicks.get(), 1);
}

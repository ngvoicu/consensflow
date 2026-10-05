//! The door's wait, on the clock that moves only when nothing else can run:
//! how long it holds, what ends it early, and what it refuses.

use std::time::Duration;

use hyper::Method;
use serde_json::{json, Value};

use super::*;
use crate::api::callers::caller_of;
use crate::testing::{request, said, scene, Scene};

/// The door of `zeus` for message `id`, asked as `target` says, answered when it is.
async fn door(scene: &Scene, token: &str, target: &str, id: &str) -> (u16, Value) {
    let asked = request(Method::GET, target, Some(token), "");
    let caller = caller_of(&scene.context, &asked).unwrap();
    said(handle(&scene.context, &caller, asked, id).await)
}

/// The chief answers the question in `words`.
fn answer(scene: &Scene, words: &str) {
    let chief = scene
        .project
        .participants
        .iter()
        .find(|participant| participant.handle == "chief")
        .unwrap();
    scene
        .context
        .ledger
        .borrow_mut()
        .answer(
            scene.question.id,
            chief.id,
            Some(&Value::String(words.to_owned())),
            None,
        )
        .unwrap();
}

#[tokio::test(start_paused = true)]
async fn an_answer_that_is_there_is_given_without_waiting() {
    let scene = scene();
    answer(&scene, "The first.");
    let started = Instant::now();
    let target = format!("/api/questions/{}?wait=20000", scene.question.id);
    let (status, body) = door(&scene, &scene.zeus, &target, &scene.question.id.to_string()).await;
    assert_eq!(status, 200);
    assert_eq!(started.elapsed(), Duration::ZERO);
    assert_eq!(body["question"]["id"], scene.question.id);
    assert_eq!(body["question"]["preview"], "Which?");
    assert_eq!(body["answer"]["body"], "The first.");
    assert_eq!(body["answer"]["from"], "chief");
    let keys: Vec<&str> = body["answer"]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(keys, ["id", "from", "body", "choices"]);
}

#[tokio::test(start_paused = true)]
async fn with_no_wait_asked_it_answers_at_once_and_says_null() {
    let scene = scene();
    for target in [
        "",
        "?wait=0",
        "?wait=",
        "?wait=soon",
        "?wait=-5",
        "?wait=NaN",
    ] {
        let started = Instant::now();
        let target = format!("/api/questions/{}{target}", scene.question.id);
        let (status, body) =
            door(&scene, &scene.zeus, &target, &scene.question.id.to_string()).await;
        assert_eq!((status, &body["answer"]), (200, &Value::Null), "{target}");
        assert_eq!(started.elapsed(), Duration::ZERO, "{target}");
    }
}

#[tokio::test(start_paused = true)]
async fn it_holds_for_the_wait_asked_and_then_says_null() {
    let scene = scene();
    let started = Instant::now();
    let target = format!("/api/questions/{}?wait=3000", scene.question.id);
    let (status, body) = door(&scene, &scene.zeus, &target, &scene.question.id.to_string()).await;
    assert_eq!((status, &body["answer"]), (200, &Value::Null));
    assert_eq!(started.elapsed(), Duration::from_millis(3000));
}

#[tokio::test(start_paused = true)]
async fn it_holds_no_more_than_25_seconds_however_much_is_asked() {
    let scene = scene();
    for asked in ["99999", "Infinity", "1e9"] {
        let started = Instant::now();
        let target = format!("/api/questions/{}?wait={asked}", scene.question.id);
        let (status, _) = door(&scene, &scene.zeus, &target, &scene.question.id.to_string()).await;
        assert_eq!(status, 200);
        assert_eq!(started.elapsed(), Duration::from_millis(25_000), "{asked}");
    }
}

#[tokio::test(start_paused = true)]
async fn an_answer_that_comes_while_it_waits_is_found_at_the_next_poll() {
    let scene = scene();
    let started = Instant::now();
    let target = format!("/api/questions/{}?wait=20000", scene.question.id);
    let id = scene.question.id.to_string();
    let (waiting, ()) = tokio::join!(door(&scene, &scene.zeus, &target, &id), async {
        tokio::time::sleep(Duration::from_millis(1100)).await;
        answer(&scene, "Later.");
    });
    assert_eq!(waiting.1["answer"]["body"], "Later.");
    // The ledger is asked every 250 ms: 1100 ms is found at 1250.
    assert_eq!(started.elapsed(), Duration::from_millis(1250));
}

#[tokio::test(start_paused = true)]
async fn the_daemon_stopping_answers_a_waiting_door_at_once_with_what_there_is() {
    let scene = scene();
    let started = Instant::now();
    let target = format!("/api/questions/{}?wait=25000", scene.question.id);
    let id = scene.question.id.to_string();
    let (waiting, ()) = tokio::join!(door(&scene, &scene.zeus, &target, &id), async {
        tokio::time::sleep(Duration::from_millis(100)).await;
        scene.context.closing.set();
    });
    assert_eq!((waiting.0, &waiting.1["answer"]), (200, &Value::Null));
    assert_eq!(
        started.elapsed(),
        Duration::from_millis(100),
        "not at the next poll"
    );

    // And a door that comes once it is stopping does not wait at all.
    let again = Instant::now();
    let (status, _) = door(&scene, &scene.zeus, &target, &id).await;
    assert_eq!(status, 200);
    assert_eq!(again.elapsed(), Duration::ZERO);
}

#[tokio::test(start_paused = true)]
async fn an_answer_that_came_as_the_daemon_stops_is_given_not_dropped() {
    let scene = scene();
    let id = scene.question.id.to_string();
    let target = format!("/api/questions/{id}?wait=25000");
    let (waiting, ()) = tokio::join!(door(&scene, &scene.zeus, &target, &id), async {
        tokio::time::sleep(Duration::from_millis(100)).await;
        answer(&scene, "Just in time.");
        scene.context.closing.set();
    });
    assert_eq!(waiting.1["answer"]["body"], "Just in time.");
}

#[tokio::test(start_paused = true)]
async fn a_message_that_is_no_question_of_this_project_is_unknown_in_the_words_as_typed() {
    let scene = scene();
    // The chief's note to zeus is a message, and no question.
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
    for asked in [
        format!("{}", note.id),
        "999".to_owned(),
        "0042".to_owned(),
        "99999999999999999999999".to_owned(),
    ] {
        let (status, body) = door(
            &scene,
            &scene.zeus,
            &format!("/api/questions/{asked}"),
            &asked,
        )
        .await;
        assert_eq!(status, 404, "{asked}");
        assert_eq!(
            body,
            json!({ "error": "unknown-message", "message": format!("no question m-{asked}") })
        );
    }
}

#[tokio::test(start_paused = true)]
async fn only_the_one_who_asked_may_wait_for_the_answer() {
    let scene = scene();
    let id = scene.question.id.to_string();
    let (status, body) = door(&scene, &scene.chief, &format!("/api/questions/{id}"), &id).await;
    assert_eq!(status, 403);
    assert_eq!(
        body,
        json!({
            "error": "not-your-question",
            "message": format!("m-{id} was asked by @zeus")
        })
    );
}

#[test]
fn the_wait_is_read_as_number_reads_it_clamped_to_zero_and_25_seconds() {
    let seconds = |asked: Option<&str>| wait_of(asked).as_secs_f64();
    assert_eq!(seconds(None), 0.0);
    assert_eq!(seconds(Some("")), 0.0);
    assert_eq!(seconds(Some("abc")), 0.0);
    assert_eq!(seconds(Some("-1")), 0.0);
    assert_eq!(seconds(Some("2000")), 2.0);
    assert_eq!(seconds(Some(" 1500 ")), 1.5);
    assert_eq!(seconds(Some("1e3")), 1.0);
    assert_eq!(seconds(Some("0x10")), 0.016);
    assert_eq!(seconds(Some("0.5")), 0.0005);
    assert_eq!(seconds(Some("25000")), 25.0);
    assert_eq!(seconds(Some("25001")), 25.0);
    assert_eq!(seconds(Some("Infinity")), 25.0);
    assert_eq!(seconds(Some("-Infinity")), 0.0);
}

//! The door's wait, on the clock that moves only when nothing else can run:
//! how long it holds, what ends it early, what it refuses, and what a poll
//! writes: the answer it finds is claimed for the door, and nothing is
//! claimed for a daemon that is stopping or a window that is gone.

use std::time::Duration;

use hyper::Method;
use serde_json::{json, Value};

use super::*;
use crate::api::callers::caller_of;
use crate::api::routes::tests::support::working_question;
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
async fn an_answer_that_came_as_the_daemon_stops_is_not_claimed_and_stays_the_ledgers_to_deliver() {
    let scene = scene();
    let question = working_question(&scene);
    let id = question.id.to_string();
    let target = format!("/api/questions/{id}?wait=25000");
    let (waiting, ()) = tokio::join!(door(&scene, &scene.zeus, &target, &id), async {
        tokio::time::sleep(Duration::from_millis(100)).await;
        scene.choose(question.id, "red");
        scene.context.closing.set();
    });
    assert_eq!(
        (waiting.0, &waiting.1["answer"]),
        (200, &Value::Null),
        "the door ends with nothing, and a hook that gets none leaves it to the window's own dialog"
    );
    assert_eq!(
        scene.next_for_zeus(),
        Some("H: red".to_owned()),
        "the answer is still the ledger's: it goes in as a message"
    );
}

#[tokio::test(start_paused = true)]
async fn a_poll_claims_the_answer_it_finds_so_the_paste_skips_it_and_wakes_the_dispatcher() {
    let scene = scene();
    let question = working_question(&scene);
    scene.choose(question.id, "red");
    assert!(
        scene.next_for_zeus().is_some(),
        "before the poll it is the next to be pasted"
    );
    let id = question.id.to_string();
    let (status, body) = door(&scene, &scene.zeus, &format!("/api/questions/{id}"), &id).await;
    assert_eq!(
        (status, &body["answer"]["choices"]),
        (200, &json!([["red"]]))
    );
    assert_eq!(scene.next_for_zeus(), None, "claimed: not pasted");
    assert_eq!(scene.kicks.get(), 1, "the dispatcher is woken");
}

#[tokio::test(start_paused = true)]
async fn the_same_answer_comes_again_to_a_second_poll_which_writes_nothing() {
    let scene = scene();
    let question = working_question(&scene);
    scene.choose(question.id, "red");
    let id = question.id.to_string();
    let target = format!("/api/questions/{id}");
    let first = door(&scene, &scene.zeus, &target, &id).await;
    let again = door(&scene, &scene.zeus, &target, &id).await;
    assert_eq!(first, again, "a reply that was lost is given again");
    let claims = scene
        .context
        .ledger
        .borrow()
        .events(scene.project.id, 0, 500)
        .unwrap()
        .iter()
        .filter(|event| event.kind == "delivery.claimed")
        .count();
    assert_eq!(claims, 1, "claimed once");
}

#[tokio::test(start_paused = true)]
async fn a_door_a_pause_shut_is_refused_at_once_in_the_words_its_model_is_to_hear() {
    let scene = scene();
    let question = working_question(&scene);
    scene.pause();
    let id = question.id.to_string();
    let started = Instant::now();
    let target = format!("/api/questions/{id}?wait=20000");
    let (status, body) = door(&scene, &scene.zeus, &target, &id).await;
    assert_eq!(started.elapsed(), Duration::ZERO, "it does not wait");
    assert_eq!(
        (status, body),
        (
            409,
            json!({
                "error": "door-closed",
                "message": format!("T-1 was stopped, so m-{id} is not answered here: its answer comes to you as a message when the task goes on. Do not ask it again; end your turn now.")
            })
        )
    );
}

#[tokio::test(start_paused = true)]
async fn a_window_that_exited_ends_its_poll_at_the_next_look_and_nothing_is_claimed_for_it() {
    let scene = scene();
    let question = working_question(&scene);
    let id = question.id.to_string();
    let target = format!("/api/questions/{id}?wait=25000");
    let started = Instant::now();
    let (waiting, ()) = tokio::join!(door(&scene, &scene.zeus, &target, &id), async {
        tokio::time::sleep(Duration::from_millis(100)).await;
        scene.context.credentials.revoke(&scene.zeus);
        tokio::time::sleep(Duration::from_millis(50)).await;
        scene.choose(question.id, "red");
    });
    assert_eq!((waiting.0, &waiting.1["answer"]), (200, &Value::Null));
    assert_eq!(started.elapsed(), Duration::from_millis(250));
    assert!(
        scene.next_for_zeus().is_some(),
        "nothing was claimed for it"
    );
}

#[tokio::test(start_paused = true)]
async fn the_ledger_is_never_held_across_the_wait() {
    let scene = scene();
    let id = scene.question.id.to_string();
    let target = format!("/api/questions/{id}?wait=25000");
    let (waiting, ()) = tokio::join!(door(&scene, &scene.zeus, &target, &id), async {
        for _ in 0..3 {
            tokio::time::sleep(Duration::from_millis(100)).await;
            assert!(
                scene.context.ledger.try_borrow_mut().is_ok(),
                "a waiting door holds no borrow"
            );
        }
        scene.context.closing.set();
    });
    assert_eq!(waiting.0, 200);
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

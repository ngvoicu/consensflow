//! A meeting with the chief and the chief's reads (3.0.7 chief-meeting, CMT-14):
//! what the meeting holds is not the chief's to read until it ends. `cf inbox`
//! lists it with none of its text, `cf inbox read` is told it waits (409 `waits`),
//! and `cf task get` and `cf history` leave its text out, as they do of what waits
//! for a Resume. What the reader wrote itself, and what a meeting does not hold, is
//! read as ever, and everything is read once the meeting ends.

use hyper::Method;
use serde_json::{json, Value};

use super::support::{api, working_task};
use crate::api::waits::MEETING_WAITS;
use crate::testing::{scene, Scene};

fn meeting(scene: &Scene, on: bool) {
    scene
        .context
        .ledger
        .borrow_mut()
        .set_meeting(scene.project.id, on)
        .unwrap();
}

/// Task `number`'s result, done by zeus: queued for the chief.
fn result_of(scene: &Scene, number: i64) -> i64 {
    let mut ledger = scene.context.ledger.borrow_mut();
    ledger
        .record_result(scene.project.id, number, "The parser is written")
        .unwrap();
    ledger
        .task(scene.project.id, number)
        .unwrap()
        .unwrap()
        .messages
        .iter()
        .rfind(|message| message.kind == "result")
        .unwrap()
        .id
}

/// The preview `cf inbox` lists message `id` with, for the window of `token`.
async fn preview(scene: &Scene, token: &str, id: i64) -> Value {
    let (status, said) = api(scene, Method::GET, "/api/inbox", token, "").await;
    assert_eq!(status, 200, "{said}");
    said["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|message| message["id"] == id)
        .map(|message| message["preview"].clone())
        .unwrap_or(Value::Null)
}

#[tokio::test]
async fn what_a_meeting_holds_is_not_the_chiefs_to_read_until_it_ends() {
    let scene = scene();
    let number = working_task(&scene);
    let result = result_of(&scene, number);
    let question = scene.question.id;
    meeting(&scene, true);

    assert_eq!(
        preview(&scene, &scene.chief, question).await,
        json!(MEETING_WAITS)
    );
    assert_eq!(
        preview(&scene, &scene.chief, result).await,
        json!(MEETING_WAITS)
    );
    let (status, said) = api(
        &scene,
        Method::GET,
        &format!("/api/inbox/{question}"),
        &scene.chief,
        "",
    )
    .await;
    assert_eq!(
        (status, said),
        (
            409,
            json!({
                "error": "waits",
                "message": format!("m-{question} waits until your meeting with the human ends: it comes to you then. Do not ask for it again now.")
            })
        )
    );
    let (status, said) = api(
        &scene,
        Method::GET,
        &format!("/api/tasks/{number}"),
        &scene.chief,
        "",
    )
    .await;
    assert_eq!(status, 200, "{said}");
    let thread: Vec<i64> = said["task"]["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|message| message["id"].as_i64().unwrap())
        .collect();
    assert!(!thread.contains(&result), "{thread:?}");
    // zeus wrote both: its own words are its own to read.
    let (status, said) = api(
        &scene,
        Method::GET,
        &format!("/api/inbox/{question}"),
        &scene.zeus,
        "",
    )
    .await;
    assert_eq!((status, &said["message"]["body"]), (200, &json!("Which?")));

    meeting(&scene, false);
    assert_eq!(
        preview(&scene, &scene.chief, question).await,
        json!("Which?")
    );
    let (status, said) = api(
        &scene,
        Method::GET,
        &format!("/api/inbox/{question}"),
        &scene.chief,
        "",
    )
    .await;
    assert_eq!((status, &said["message"]["body"]), (200, &json!("Which?")));
    let (_, said) = api(
        &scene,
        Method::GET,
        &format!("/api/tasks/{number}"),
        &scene.chief,
        "",
    )
    .await;
    assert!(said["task"]["messages"]
        .as_array()
        .unwrap()
        .iter()
        .any(|message| message["id"] == result));
}

/// The handoff a chief that takes over is given is no message a meeting holds: the
/// chief reads it.
#[tokio::test]
async fn what_a_meeting_does_not_hold_the_chief_reads_during_it() {
    let scene = scene();
    meeting(&scene, true);
    let handoff = scene
        .context
        .ledger
        .borrow_mut()
        .note(
            scene.project.id,
            &cf_ledger::NewNote {
                from: None,
                to: "chief".to_owned(),
                task: None,
                body: format!(
                    "{}. The human switched this project's chief.",
                    cf_ledger::HANDOFF_TITLE
                ),
            },
        )
        .unwrap();
    let (status, said) = api(
        &scene,
        Method::GET,
        &format!("/api/inbox/{}", handoff.id),
        &scene.chief,
        "",
    )
    .await;
    assert_eq!(
        (status, &said["message"]["body"]),
        (200, &json!(handoff.body))
    );
}

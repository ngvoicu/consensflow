//! A meeting with the chief and the chief's reads (3.0.7 chief-meeting, CMT-14):
//! what the meeting holds is not the chief's to read until it ends. `cf inbox`
//! lists it with none of its text, `cf inbox read` is told it waits (409 `waits`),
//! and `cf task get` and `cf history` give none of its text, as they give none of what
//! waits for a Resume: each puts a note in the message's place that says it waits
//! (T-942; before it, a thread left the message out and said nothing). What the reader
//! wrote itself, and what a meeting does not hold, is read as ever, and everything is
//! read once the meeting ends.

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
    assert!(!said.to_string().contains("The parser is written"), "{said}");
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

/// What a thread says in the place of message `id`, which the meeting holds: the line
/// the chief gave for it (T-942), the same words as `cf inbox read`'s refusal without
/// its last sentence.
fn waits_note(id: i64) -> String {
    format!("(m-{id} waits until your meeting with the human ends: it comes to you then)")
}

/// Task `number`'s thread, read by the window of `token`.
async fn thread_of(scene: &Scene, token: &str, number: i64) -> Value {
    let (status, said) = api(
        scene,
        Method::GET,
        &format!("/api/tasks/{number}"),
        token,
        "",
    )
    .await;
    assert_eq!(status, 200, "{said}");
    said
}

/// The message `id` in a thread read as `said`.
fn in_thread(said: &Value, id: i64) -> Option<&Value> {
    said["task"]["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|message| message["id"] == id)
}

/// T-942: a chief who read only `cf task get` saw a task Done with no result in its
/// thread, and nothing said why. A message the meeting holds is in the thread, and
/// its body is the line that says it waits: never a byte of its text, whatever its
/// kind (a result, a question), and the message is read in full once the meeting ends.
#[tokio::test]
async fn a_thread_says_in_the_place_of_what_a_meeting_holds_that_it_waits_and_gives_none_of_its_text(
) {
    let scene = scene();
    let number = working_task(&scene);
    let result = result_of(&scene, number);
    let question = super::support::question_on(&scene, number).id;
    meeting(&scene, true);

    let said = thread_of(&scene, &scene.chief, number).await;
    for (id, kind) in [(result, "result"), (question, "question")] {
        let held = in_thread(&said, id)
            .unwrap_or_else(|| panic!("m-{id} is not in the thread, which says nothing: {said}"));
        assert_eq!(held["body"], waits_note(id), "{said}");
        assert_eq!(
            (held["kind"].as_str(), held["state"].as_str()),
            (Some(kind), Some("queued")),
            "{said}"
        );
        assert_eq!(held["questions"], Value::Null, "{said}");
        assert_eq!(held["choices"], Value::Null, "{said}");
    }
    let text = said.to_string();
    assert!(!text.contains("The parser is written"), "{said}");
    assert!(!text.contains("blue"), "its options: {said}");
    // The one who wrote them reads them: zeus asked the question.
    let own = thread_of(&scene, &scene.zeus, number).await;
    assert!(in_thread(&own, question).unwrap()["questions"].is_array());

    meeting(&scene, false);
    let said = thread_of(&scene, &scene.chief, number).await;
    assert_eq!(
        in_thread(&said, result).unwrap()["body"],
        "The parser is written"
    );
    assert!(in_thread(&said, question).unwrap()["questions"].is_array());
}

/// What `cf` does with a thread (`cf task get`): it takes a queued answer it printed for
/// received, and it prints no brief under the task's head once the thread has a task
/// message for the window that has it. A note in the place of either would do harm, so
/// a brief the meeting holds (a task the human gave the chief) stays out of the thread,
/// as before, and the chief reads it from the task's body.
#[tokio::test]
async fn a_brief_a_meeting_holds_is_not_in_the_thread_so_cf_prints_the_chief_its_body() {
    let scene = scene();
    let given = scene
        .context
        .ledger
        .borrow_mut()
        .create_task(
            scene.project.id,
            &cf_ledger::NewTask {
                from: "human".to_owned(),
                to: Some("chief".to_owned()),
                body: "Review the lexer".to_owned(),
                ..cf_ledger::NewTask::default()
            },
        )
        .unwrap();
    meeting(&scene, true);

    let said = thread_of(&scene, &scene.chief, given.task.number).await;
    assert_eq!(said["task"]["body"], "Review the lexer", "{said}");
    assert!(
        said["task"]["messages"]
            .as_array()
            .unwrap()
            .iter()
            .all(|message| message["kind"] != "task"),
        "{said}"
    );
}

/// The words of the chief's history, where it shows a message the meeting holds: the
/// same line (T-942).
#[tokio::test]
async fn the_history_says_of_what_a_meeting_holds_the_line_a_thread_does() {
    use cf_ledger::ChiefSwitch;
    use serde_json::json;
    let scene = scene();
    let question = scene.question.id;
    {
        let mut ledger = scene.context.ledger.borrow_mut();
        let project = ledger.project(scene.project.id).unwrap().unwrap();
        let chief = project
            .participants
            .iter()
            .find(|participant| participant.handle == "chief")
            .unwrap();
        let old = ledger.start_conversation(chief.id, "claude-code").unwrap();
        let named = format!("[ConsensFlow m-{question} · question from @zeus]");
        ledger
            .copy_transcript(
                old.id,
                &[json!({ "id": "u1", "role": "user", "text": named, "complete": true })],
                0,
            )
            .unwrap();
        ledger
            .switch_chief(
                scene.project.id,
                &ChiefSwitch {
                    harness: "claude-code".to_owned(),
                    agent: "other".to_owned(),
                    cut: false,
                },
            )
            .unwrap();
    }
    meeting(&scene, true);

    let (status, said) = api(&scene, Method::GET, "/api/history", &scene.chief, "").await;
    assert_eq!(status, 200, "{said}");
    let text = said["text"].as_str().unwrap();
    assert!(text.contains(&waits_note(question)), "{text}");
    assert!(!text.contains("Which?"), "{text}");
}

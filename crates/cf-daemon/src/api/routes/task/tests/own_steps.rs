//! The chief's own step (`cf task add --self`), finished by its word: `done` takes
//! it while its brief is still on its way back to the chief, and answers with it
//! accepted (the field report of 2026-10-15, B4). Its brief never reached the
//! chief's window then, so it has no transcript of its own, and the chief's
//! other work is not read as its (the second review, 2026-10-16).

use cf_ledger::{NewNote, NewTask};
use serde_json::{json, Value};

use super::{get, post};
use crate::testing::{scene, Scene};

/// The chief's own step, its brief queued for the chief's window: its number.
fn own_step(scene: &Scene) -> i64 {
    scene
        .context
        .ledger
        .borrow_mut()
        .create_task(
            scene.project.id,
            &NewTask {
                from: "chief".to_owned(),
                to: Some("chief".to_owned()),
                body: "Check the parser".to_owned(),
                ..NewTask::default()
            },
        )
        .unwrap()
        .task
        .number
}

#[tokio::test]
async fn the_chiefs_own_step_is_done_while_its_brief_is_on_its_way_and_answers_accepted() {
    let scene = scene();
    let number = own_step(&scene);
    let (status, said) = post(
        &scene,
        &scene.chief,
        &format!("{number}/done"),
        r#"{"body":"Checked: it holds"}"#,
    )
    .await;
    assert_eq!(status, 200, "{said}");
    assert_eq!(said["task"]["state"], "accepted");
    assert_eq!(scene.kicks.get(), 1);
}

#[tokio::test]
async fn a_step_done_before_its_brief_came_back_has_no_transcript_and_the_chiefs_other_work_is_not_its(
) {
    let scene = scene();
    {
        let mut ledger = scene.context.ledger.borrow_mut();
        let project = ledger.project(scene.project.id).unwrap().unwrap();
        let chief = project
            .participants
            .iter()
            .find(|p| p.handle == "chief")
            .unwrap()
            .id;
        let conversation = ledger.start_conversation(chief, "claude-code").unwrap();
        let items: Vec<Value> = ["Accepted T-1", "Reviewed the parser"]
            .iter()
            .enumerate()
            .map(|(at, text)| json!({ "id": format!("c{at}"), "role": "assistant", "text": text, "complete": true }))
            .collect();
        ledger.copy_transcript(conversation.id, &items, 0).unwrap();
    }
    let number = own_step(&scene);
    let (status, _) = post(
        &scene,
        &scene.chief,
        &format!("{number}/done"),
        r#"{"body":"Checked: it holds"}"#,
    )
    .await;
    assert_eq!(status, 200);
    for asked in ["last=10", "page=1", "find=parser"] {
        let route = format!("{number}/transcript?{asked}");
        let (status, said) = get(&scene, &scene.chief, &route).await;
        assert_eq!(status, 200, "{asked}: {said}");
        assert_eq!(said["reached"], false, "{asked}: it says so");
        assert_eq!(said["total"], 0, "{asked}: none of the chief's own is its");
        assert_eq!(said["items"], json!([]), "{asked}");
    }
}

/// The id of the brief of task `number`, queued for the chief.
fn brief_of(scene: &Scene, number: i64) -> i64 {
    scene
        .context
        .ledger
        .borrow()
        .task(scene.project.id, number)
        .unwrap()
        .unwrap()
        .messages
        .iter()
        .find(|message| message.kind == "task")
        .unwrap()
        .id
}

/// The ids and kinds the answer names under `key`.
fn named(said: &Value, key: &str) -> Vec<(Value, Value)> {
    said[key]
        .as_array()
        .unwrap_or(&Vec::new())
        .iter()
        .map(|message| (message["id"].clone(), message["kind"].clone()))
        .collect()
}

#[tokio::test]
async fn the_answer_names_what_was_withdrawn_with_the_brief_and_nothing_is_landing() {
    let scene = scene();
    let number = own_step(&scene);
    let brief = brief_of(&scene, number);
    let note = scene
        .context
        .ledger
        .borrow_mut()
        .note(
            scene.project.id,
            &NewNote {
                from: Some("zeus".to_owned()),
                to: "chief".to_owned(),
                body: "Mind the lexer".to_owned(),
                task: Some(number),
            },
        )
        .unwrap()
        .id;
    let (status, said) = post(
        &scene,
        &scene.chief,
        &format!("{number}/done"),
        r#"{"body":"Checked: it holds"}"#,
    )
    .await;
    assert_eq!(status, 200, "{said}");
    assert_eq!(
        named(&said, "withdrawn"),
        [(json!(brief), json!("task")), (json!(note), json!("note"))],
        "the brief, and the note that rode on it"
    );
    assert_eq!(said.get("landing"), None);
}

#[tokio::test]
async fn the_answer_names_a_brief_that_was_being_typed_in_as_landing() {
    let scene = scene();
    let number = own_step(&scene);
    let brief = brief_of(&scene, number);
    scene
        .context
        .ledger
        .borrow_mut()
        .begin_delivery(brief)
        .unwrap();
    let (status, said) = post(
        &scene,
        &scene.chief,
        &format!("{number}/done"),
        r#"{"body":"Checked: it holds"}"#,
    )
    .await;
    assert_eq!(status, 200, "{said}");
    assert_eq!(named(&said, "landing"), [(json!(brief), json!("task"))]);
    assert_eq!(
        said.get("withdrawn"),
        None,
        "nothing was queued to take back"
    );
}

#[tokio::test]
async fn an_own_step_with_nothing_on_its_way_answers_with_its_task_alone() {
    let scene = scene();
    let number = own_step(&scene);
    let brief = brief_of(&scene, number);
    {
        let mut ledger = scene.context.ledger.borrow_mut();
        ledger.begin_delivery(brief).unwrap();
        ledger
            .confirm_delivery(brief, Some(&json!({ "item": "test" })))
            .unwrap();
    }
    let (status, said) = post(
        &scene,
        &scene.chief,
        &format!("{number}/done"),
        r#"{"body":"Checked: it holds"}"#,
    )
    .await;
    assert_eq!(status, 200, "{said}");
    assert_eq!(
        said.as_object().unwrap().keys().collect::<Vec<_>>(),
        ["task"]
    );
}

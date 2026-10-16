//! The chief's own step (`cf task add --self`), finished by its word: `done` takes
//! it while its brief is still on its way back to the chief, and answers with it
//! accepted (the field report of 2026-10-15, B4). Its brief never reached the
//! chief's window then, so it has no transcript of its own, and the chief's
//! other work is not read as its (the second review, 2026-10-16).

use cf_ledger::NewTask;
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

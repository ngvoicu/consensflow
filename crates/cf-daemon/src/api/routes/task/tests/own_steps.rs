//! The chief's own step (`cf task add --self`), finished by its word: `done` takes
//! it while its brief is still on its way back to the chief, and answers with it
//! accepted (the field report of 2026-10-15, B4).

use cf_ledger::NewTask;

use super::post;
use crate::testing::scene;

#[tokio::test]
async fn the_chiefs_own_step_is_done_while_its_brief_is_on_its_way_and_answers_accepted() {
    let scene = scene();
    let number = scene
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
        .number;
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

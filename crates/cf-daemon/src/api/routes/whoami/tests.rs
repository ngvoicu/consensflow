//! `GET /api/whoami`: who the window is, and the task it has in progress.

use hyper::Method;
use serde_json::json;

use crate::api::routes::tests::support::{api, open_task, working_task};
use crate::testing::scene;

#[tokio::test]
async fn a_window_with_no_task_is_told_its_project_and_itself_and_a_null_task() {
    let scene = scene();
    let (status, said) = api(&scene, Method::GET, "/api/whoami", &scene.chief, "").await;
    assert_eq!(status, 200);
    assert_eq!(
        said.to_string(),
        r#"{"project":{"id":1,"name":"app","directory":"/work/app"},"participant":{"handle":"chief","role":"chief"},"task":null}"#,
        "the fields in the order Node said them"
    );
    let (_, said) = api(&scene, Method::GET, "/api/whoami", &scene.zeus, "").await;
    assert_eq!(
        said["participant"],
        json!({ "handle": "zeus", "role": "worker" })
    );
}

#[tokio::test]
async fn the_task_it_has_in_progress_is_summarised_without_its_brief() {
    let scene = scene();
    let number = working_task(&scene);
    let (status, said) = api(&scene, Method::GET, "/api/whoami", &scene.zeus, "").await;
    assert_eq!(status, 200);
    let task = said["task"].to_string();
    assert!(
        task.starts_with(&format!(
            r#"{{"number":{number},"title":"Parser","state":"working","requester":"chief","assignee":"zeus","pool":null,"tier":null,"needs":[],"blockedBy":[],"updatedAt":""#
        )),
        "{task}"
    );
    assert!(!task.contains("\"body\""), "{task}");
    assert!(
        !task.contains("\"kind\""),
        "Node's `kind` was never there: {task}"
    );
    // The one who gave it has none in progress.
    let (_, said) = api(&scene, Method::GET, "/api/whoami", &scene.chief, "").await;
    assert_eq!(said["task"], json!(null));
}

#[tokio::test]
async fn a_task_still_arriving_or_waiting_on_the_board_is_not_in_progress() {
    let scene = scene();
    // Given by name and not yet delivered: queued for the window.
    scene
        .context
        .ledger
        .borrow_mut()
        .create_task(
            scene.project.id,
            &cf_ledger::NewTask {
                from: "chief".to_owned(),
                to: Some("zeus".to_owned()),
                body: "Arriving".to_owned(),
                ..cf_ledger::NewTask::default()
            },
        )
        .unwrap();
    open_task(&scene);
    let (_, said) = api(&scene, Method::GET, "/api/whoami", &scene.zeus, "").await;
    assert_eq!(said["task"], json!(null));
    assert_eq!(scene.kicks.get(), 0, "a read wakes nothing");
}

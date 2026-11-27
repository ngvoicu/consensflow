//! Every view `cf` reads of a project, as the API says it, to the byte, on a project of one
//! chief, one member and the human (the leads design, step 0): who am I, the staff, the
//! board's tasks, one task with its thread, and each window's inbox. The migration that adds
//! leads must leave every one of them equal. The golden is `no_lead_views.json` beside this
//! file; set `CF_WRITE_GOLDENS=1` to write it again after a change that is meant to move one.

use hyper::Method;
use serde_json::{json, Map, Value};

use super::support::{api, working_task};
use crate::testing::scene;

const GOLDEN: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/src/api/routes/tests/no_lead_views.json"
);

#[tokio::test]
async fn every_view_cf_reads_of_a_project_with_no_lead_is_the_same_json_to_the_byte() {
    let scene = scene();
    let chiefs = working_task(&scene);
    let humans = human_task_working_in_another(&scene);
    let mut seen = Map::new();
    for (window, token) in [("chief", &scene.chief), ("zeus", &scene.zeus)] {
        for path in [
            "/api/whoami".to_owned(),
            "/api/staff".to_owned(),
            "/api/tasks".to_owned(),
            "/api/inbox".to_owned(),
            format!("/api/tasks/{chiefs}"),
            format!("/api/tasks/{humans}"),
        ] {
            let (status, said) = api(&scene, Method::GET, &path, token, "").await;
            seen.insert(
                format!("{window} {path}"),
                json!({ "status": status, "body": said }),
            );
        }
    }
    let mut seen = Value::Object(seen);
    untimed(&mut seen);
    let text = format!("{}\n", serde_json::to_string_pretty(&seen).unwrap());
    if writing_goldens() {
        std::fs::write(GOLDEN, &text).expect("the golden is written");
    }
    let golden = std::fs::read_to_string(GOLDEN).expect("the golden is there");
    assert!(
        golden == text,
        "a view of a project with no lead moved: if it is meant to, run again with CF_WRITE_GOLDENS=1 and read the diff of {GOLDEN}"
    );
}

/// The human's task for the chief by name, queued behind the member's.
fn human_task_working_in_another(scene: &crate::testing::Scene) -> i64 {
    scene
        .context
        .ledger
        .borrow_mut()
        .create_task(
            scene.project.id,
            &cf_ledger::NewTask {
                from: "human".to_owned(),
                to: Some("chief".to_owned()),
                body: "Plan the week".to_owned(),
                ..cf_ledger::NewTask::default()
            },
        )
        .unwrap()
        .task
        .number
}

/// The scene runs on the machine's clock: a moment is written as `<time>`, and nothing else
/// of a view is touched.
fn untimed(value: &mut Value) {
    match value {
        Value::String(text) if is_a_moment(text) => *text = "<time>".to_owned(),
        Value::Array(items) => items.iter_mut().for_each(untimed),
        Value::Object(fields) => fields.values_mut().for_each(untimed),
        _ => {}
    }
}

/// `2026-11-27T20:26:17.296Z`, as the ledger writes one.
fn is_a_moment(text: &str) -> bool {
    let shape = b"dddd-dd-ddTdd:dd:dd.dddZ";
    text.len() == shape.len()
        && text.bytes().zip(shape).all(|(found, wanted)| match wanted {
            b'd' => found.is_ascii_digit(),
            other => found == *other,
        })
}

/// Whether this run writes the golden again (a test's own switch, read where it is used).
#[allow(clippy::disallowed_methods)]
fn writing_goldens() -> bool {
    std::env::var_os("CF_WRITE_GOLDENS").is_some()
}

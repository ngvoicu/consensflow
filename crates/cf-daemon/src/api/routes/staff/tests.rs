//! `GET /api/staff`: the project's members, each with the harness, model and effort it
//! runs on itself (harness-models); not the chief, not a member's sessions.

use std::rc::Rc;

use hyper::Method;
use serde_json::json;

use crate::api::context::Context;
use crate::api::routes::tests::support::open_task;
use crate::testing::{scene, Scene};

async fn staff(scene: &Scene, context: &Rc<Context>) -> (u16, serde_json::Value) {
    let screens = crate::screens::testing::inert();
    crate::testing::said(
        crate::api::handle(
            context,
            &screens,
            crate::testing::request(Method::GET, "/api/staff", Some(&scene.chief), ""),
        )
        .await,
    )
}

#[tokio::test]
async fn a_member_is_said_with_its_roles_tier_harness_and_the_model_and_effort_it_runs_on() {
    let scene = scene();
    let (status, said) = staff(&scene, &scene.context).await;
    assert_eq!(status, 200);
    assert_eq!(
        said.to_string(),
        r#"{"members":[{"handle":"zeus","role":"worker","roles":["worker"],"tier":"standard","harness":"claude-code","model":"claude-sonnet-5","modelName":"zeus","effort":null}]}"#
    );
    assert_eq!(scene.kicks.get(), 0);
}

#[tokio::test]
async fn a_member_on_its_harness_s_default_says_so_and_a_tester_its_roles() {
    let scene = scene();
    scene
        .context
        .ledger
        .borrow_mut()
        .add_member(
            scene.project.id,
            &cf_ledger::NewMember {
                model: None,
                effort: None,
                model_name: None,
                harness: "codex".to_owned(),
                designer: false,
                roles: vec!["tester".to_owned(), "worker".to_owned()],
                tier: "light".to_owned(),
            },
        )
        .unwrap();
    let (_, said) = staff(&scene, &scene.context).await;
    let tess = &said["members"][1];
    assert_eq!(tess["handle"], "codex-default");
    assert_eq!(
        (&tess["model"], &tess["modelName"], &tess["effort"]),
        (&json!(null), &json!("Codex default"), &json!(null))
    );
    assert_eq!(tess["role"], "tester");
    assert_eq!(tess["roles"], json!(["tester", "worker"]));
}

#[tokio::test]
async fn the_staff_is_the_members_and_not_the_chief_nor_the_windows_the_daemon_opened() {
    let scene = scene();
    // The daemon gives an open task to a new window of zeus: a session of it.
    let number = open_task(&scene);
    {
        let mut ledger = scene.context.ledger.borrow_mut();
        let project = ledger.project(scene.project.id).unwrap().unwrap();
        let zeus = project
            .participants
            .iter()
            .find(|p| p.handle == "zeus")
            .unwrap();
        ledger
            .assign_task(scene.project.id, number, zeus.id)
            .unwrap();
        assert!(ledger
            .project(scene.project.id)
            .unwrap()
            .unwrap()
            .participants
            .iter()
            .any(|p| p.member_id.is_some()));
    }
    let (_, said) = staff(&scene, &scene.context).await;
    let handles: Vec<&str> = said["members"]
        .as_array()
        .unwrap()
        .iter()
        .map(|member| member["handle"].as_str().unwrap())
        .collect();
    assert_eq!(handles, ["zeus"]);
}

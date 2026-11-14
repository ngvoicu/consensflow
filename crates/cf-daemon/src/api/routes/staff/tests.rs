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
        r#"{"members":[{"handle":"zeus","role":"worker","roles":["worker"],"tier":"standard","harness":"claude-code","harnessName":"Claude Code","model":"claude-sonnet-5","nameApart":false,"modelName":"zeus","effort":null}]}"#
    );
    assert_eq!(scene.kicks.get(), 0);
}

#[tokio::test]
async fn staff_says_own_hands_only_while_the_person_has_it_on() {
    let scene = scene();
    assert!(staff(&scene, &scene.context)
        .await
        .1
        .get("ownHands")
        .is_none());
    scene
        .context
        .ledger
        .borrow_mut()
        .set_own_hands(scene.project.id, true)
        .unwrap();
    assert_eq!(staff(&scene, &scene.context).await.1["ownHands"], true);
    scene
        .context
        .ledger
        .borrow_mut()
        .set_own_hands(scene.project.id, false)
        .unwrap();
    assert!(staff(&scene, &scene.context)
        .await
        .1
        .get("ownHands")
        .is_none());
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
    // Its name holds no model: `cf staff` says what it runs on.
    assert_eq!(tess["nameApart"], true);
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

#[tokio::test]
async fn an_image_designer_is_said_with_no_tier_because_a_design_task_has_none() {
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
                designer: true,
                roles: vec!["designer".to_owned()],
                tier: "light".to_owned(),
            },
        )
        .unwrap();
    let (_, said) = staff(&scene, &scene.context).await;
    let designer = said["members"]
        .as_array()
        .unwrap()
        .iter()
        .find(|member| member["role"] == "designer")
        .expect("the designer");
    assert_eq!(designer["tier"], json!(null), "{designer}");
    assert_eq!(designer["modelName"], "Codex default");
    assert_eq!(
        said["members"][0]["tier"], "standard",
        "a worker's tier is said"
    );
}

#[tokio::test]
async fn a_member_whose_roles_have_no_tier_is_said_with_none_and_one_with_a_role_with_tiers_with_its_own(
) {
    // The scout of the table's seam: a role with no tier that is no image agent. The ledger's
    // CHECKs name the five roles of today alone, so they are off for its rows.
    let _scout = cf_ledger::roles::seam::put(&cf_ledger::roles::seam::WITH_A_SCOUT);
    let scene = scene();
    scene
        .context
        .ledger
        .borrow()
        .sqlite()
        .execute_batch("PRAGMA ignore_check_constraints = ON")
        .unwrap();
    for (name, roles, tier) in [
        ("argus", &["scout"][..], "light"),
        ("castor", &["worker", "scout"][..], "complex"),
    ] {
        scene
            .context
            .ledger
            .borrow_mut()
            .add_member(
                scene.project.id,
                &cf_ledger::NewMember {
                    model: None,
                    effort: None,
                    model_name: Some(name.to_owned()),
                    harness: "claude-code".to_owned(),
                    designer: false,
                    roles: roles.iter().map(|role| (*role).to_owned()).collect(),
                    tier: tier.to_owned(),
                },
            )
            .unwrap();
    }
    let (_, said) = staff(&scene, &scene.context).await;
    let tiers: Vec<(serde_json::Value, serde_json::Value)> = said["members"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|member| {
            member["roles"]
                .as_array()
                .is_some_and(|roles| roles.contains(&json!("scout")))
        })
        .map(|member| (member["roles"].clone(), member["tier"].clone()))
        .collect();
    assert_eq!(
        tiers,
        [
            (json!(["scout"]), json!(null)),
            (json!(["worker", "scout"]), json!("complex")),
        ]
    );
}

#[tokio::test]
async fn bkv05_staff_carries_the_backup_owner_and_each_vacation() {
    let scene = scene();
    let backup = scene
        .context
        .ledger
        .borrow_mut()
        .add_backup(
            scene.project.id,
            "zeus",
            &cf_ledger::NewMember {
                harness: "codex".into(),
                model: Some("gpt-6-astra".into()),
                model_name: Some("gpt-6-astra".into()),
                effort: None,
                designer: false,
                roles: vec!["worker".into()],
                tier: "standard".into(),
            },
        )
        .unwrap();
    scene
        .context
        .ledger
        .borrow_mut()
        .send_on_vacation(scene.project.id, "zeus", &[])
        .unwrap();
    scene
        .context
        .ledger
        .borrow_mut()
        .send_on_vacation(scene.project.id, &backup.handle, &[])
        .unwrap();
    let (_, said) = staff(&scene, &scene.context).await;
    let members = said["members"].as_array().unwrap();
    let owner = members
        .iter()
        .find(|member| member["handle"] == "zeus")
        .unwrap();
    let cover = members
        .iter()
        .find(|member| member["handle"] == backup.handle)
        .unwrap();
    assert!(owner["vacationSince"].is_string(), "{owner}");
    assert!(cover["vacationSince"].is_string(), "{cover}");
    assert_eq!(cover["backupOf"], "zeus", "{cover}");
}

//! What the human says of a project's specs on the page: all three parts at
//! once, each refused before the project is looked for, kept and shown on the
//! project, and the dispatcher woken. And that it is the human's alone: no
//! route of the agents' API says it, so a member's token reaches nothing of it.
//! Not held to a Node recording: this operation is this build's own.

use hyper::Method;

use super::reading::home_with;
use super::*;
use crate::testing::{request, said, scene};

/// The page over a project with a worker, on a bridge, and a home whose agents
/// file is empty (the board reads it).
struct Asked {
    _home: tempfile::TempDir,
    _daemon: Bridge,
    app: Bridge,
    rig: Rig,
    project: i64,
}

fn asked() -> Asked {
    let (home, env) = home_with("[]");
    let rig = rig_over(env, None);
    let (daemon, app) = bridge_pair_over(&rig.spawn);
    register(&daemon, &rig.page, &rig.spawn);
    let project = rig.kit.with_staff(&["zeus"]).id;
    Asked {
        _home: home,
        _daemon: daemon,
        app,
        rig,
        project,
    }
}

#[tokio::test]
async fn the_human_says_all_three_parts_and_the_project_keeps_them_and_shows_them() {
    LocalSet::new()
        .run_until(async {
            let held = asked();
            let answer = ask(
                &held.app,
                "project.specs",
                json!({
                    "project": held.project,
                    "on": true,
                    "folder": ".\\docs//specs/",
                    "variant": "tdd-html",
                }),
            )
            .await;
            assert_eq!(answer["ok"], true, "{answer}");
            assert_eq!(
                answer["project"]["specs"],
                json!({ "on": true, "folder": "docs/specs", "variant": "tdd-html" })
            );
            assert_eq!(held.rig.kicks.get(), 1, "the dispatcher is woken once");
            // The board shows it as the project's own word.
            let board = ask(&held.app, "board.get", json!({ "project": held.project })).await;
            assert_eq!(
                board["board"]["project"]["specs"],
                json!({ "on": true, "folder": "docs/specs", "variant": "tdd-html" }),
                "{board}"
            );
            let listed = ask(&held.app, "projects.list", json!({})).await;
            assert_eq!(
                listed["projects"][0]["specs"],
                json!({ "on": true, "folder": "docs/specs", "variant": "tdd-html" }),
                "{listed}"
            );
        })
        .await;
}

#[tokio::test]
async fn a_part_the_page_leaves_out_or_sends_null_is_not_said_and_a_project_that_says_nothing_has_no_specs_key(
) {
    LocalSet::new()
        .run_until(async {
            let held = asked();
            let project = held.project;
            let said_of = |answer: &Value| answer["project"].get("specs").cloned();
            let only_on = ask(
                &held.app,
                "project.specs",
                json!({ "project": project, "on": false }),
            )
            .await;
            assert_eq!(said_of(&only_on), Some(json!({ "on": false })));
            let cleared = ask(
                &held.app,
                "project.specs",
                json!({ "project": project, "on": null, "folder": null, "variant": null }),
            )
            .await;
            assert_eq!(cleared["ok"], true, "{cleared}");
            assert_eq!(said_of(&cleared), None, "it says nothing again: {cleared}");
            let blank = ask(
                &held.app,
                "project.specs",
                json!({ "project": project, "folder": "   " }),
            )
            .await;
            assert_eq!(said_of(&blank), None, "a blank folder is none: {blank}");
        })
        .await;
}

#[tokio::test]
async fn a_folder_that_leaves_the_project_a_variant_that_is_none_of_the_four_and_a_word_that_is_no_switch_are_refused_and_wake_nothing(
) {
    LocalSet::new()
        .run_until(async {
            let held = asked();
            let project = held.project;
            let inside = "the specs folder is a folder inside the project: ";
            for (body, error) in [
                (
                    json!({ "folder": "../elsewhere" }),
                    format!("{inside}it stays inside it, with no `..`"),
                ),
                (
                    json!({ "folder": "/etc/specs" }),
                    format!("{inside}it is relative to the project, not absolute"),
                ),
                (
                    json!({ "folder": "C:\\specs" }),
                    format!("{inside}it is relative to the project, not absolute"),
                ),
                (json!({ "folder": 4 }), format!("{inside}it is text")),
                (
                    json!({ "variant": "full" }),
                    "the specs variant is one of core, core-html, tdd, tdd-html, not \"full\""
                        .to_owned(),
                ),
                (
                    json!({ "on": "yes" }),
                    "specs are on (true), off (false) or follow the app's setting (null)"
                        .to_owned(),
                ),
            ] {
                let mut sent = body.clone();
                sent["project"] = json!(project);
                assert_eq!(
                    ask(&held.app, "project.specs", sent).await,
                    json!({ "ok": false, "error": error }),
                    "{body}"
                );
            }
            // The parts are checked before the project is looked for.
            assert_eq!(
                ask(
                    &held.app,
                    "project.specs",
                    json!({ "project": 99, "variant": "full" })
                )
                .await["error"],
                "the specs variant is one of core, core-html, tdd, tdd-html, not \"full\""
            );
            assert_eq!(
                held.rig.kicks.get(),
                0,
                "nothing succeeded, nothing is woken"
            );
            let board = ask(&held.app, "board.get", json!({ "project": project })).await;
            assert!(
                board["board"]["project"].get("specs").is_none(),
                "nothing was written"
            );
        })
        .await;
}

#[tokio::test]
async fn a_project_that_is_not_there_and_an_id_that_is_no_number_are_refused_in_the_usual_words() {
    LocalSet::new()
        .run_until(async {
            let held = asked();
            assert_eq!(
                ask(
                    &held.app,
                    "project.specs",
                    json!({ "project": 99, "on": true })
                )
                .await,
                json!({ "ok": false, "error": "no project 99" })
            );
            assert_eq!(
                ask(&held.app, "project.specs", json!({ "on": true })).await,
                json!({ "ok": false, "error": "project is a whole number, not undefined" })
            );
            assert_eq!(held.rig.kicks.get(), 0);
        })
        .await;
}

#[tokio::test]
async fn a_member_s_token_reaches_no_route_that_says_a_project_s_specs_so_only_the_human_can() {
    let scene = scene();
    let screens = crate::screens::testing::inert();
    let ask_with = |method: Method, target: &str, body: &str| {
        let request = request(method, target, Some(&scene.zeus), body);
        let (context, screens) = (&scene.context, &screens);
        async move { said(crate::api::handle(context, screens, request).await) }
    };
    let body = r#"{"project":1,"on":true,"folder":"specs","variant":"tdd"}"#;
    for target in [
        "/api/specs",
        "/api/project/specs",
        "/api/projects/1/specs",
        "/api/project.specs",
        "/api/settings",
        "/api/preferences/specs",
    ] {
        for method in [Method::POST, Method::PUT, Method::PATCH, Method::GET] {
            let (status, answered) = ask_with(method.clone(), target, body).await;
            // The agents' routes say no such command, whatever they are asked
            // (`/api/preferences` itself is the screens' own, under the UI
            // token a window has none of).
            assert_eq!(status, 404, "{method} {target}: {answered}");
            assert_eq!(answered["error"], "unknown-route", "{method} {target}");
        }
    }
    let after = scene
        .context
        .ledger
        .borrow()
        .project(scene.project.id)
        .unwrap()
        .unwrap();
    assert_eq!(after.specs, None, "nothing a window asked changed it");
}

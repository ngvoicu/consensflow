//! Whether the human leaves Claude Code's prompt suggestion on in the project's windows, on
//! the page: a flag, off for every project until it is ticked, refused in words when it is
//! anything else (before the project is looked for), kept, shown on every view of the project
//! the page reads only while it is on, the dispatcher woken. It is the human's to choose
//! alone: no route of the agents' API writes it, so no `cf` command does, whosever token it
//! holds. Not held to a Node recording: this operation is this build's own.

use hyper::Method;

use super::reading::home_with;
use super::*;
use crate::testing::{request, said, scene};

const WORDS: &str = "the prompt suggestion is on (true) or off (false)";

#[tokio::test]
async fn own_hands_is_a_page_only_project_switch() {
    LocalSet::new()
        .run_until(async {
            let held = asked();
            let project = held.project;
            let on = ask(
                &held.app,
                "project.own_hands",
                json!({"project": project, "ownHands": true}),
            )
            .await;
            assert_eq!(on["ok"], true, "{on}");
            assert_eq!(on["project"]["ownHands"], true);
            assert_eq!(held.rig.kicks.get(), 1);
            let board = ask(&held.app, "board.get", json!({"project": project})).await;
            assert_eq!(board["board"]["project"]["ownHands"], true);
            let invalid = ask(
                &held.app,
                "project.own_hands",
                json!({"project": project, "ownHands": "yes"}),
            )
            .await;
            assert_eq!(invalid["ok"], false);
            let off = ask(
                &held.app,
                "project.own_hands",
                json!({"project": project, "ownHands": false}),
            )
            .await;
            assert_eq!(off["ok"], true);
            assert!(off["project"].get("ownHands").is_none());
        })
        .await;
}

#[tokio::test]
async fn a_windows_token_cannot_switch_own_hands() {
    let scene = scene();
    let screens = crate::screens::testing::inert();
    for token in [&scene.zeus, &scene.chief] {
        for target in [
            "/api/own_hands",
            "/api/project.own_hands",
            "/api/projects/1/own_hands",
        ] {
            let request = request(
                Method::POST,
                target,
                Some(token),
                r#"{"project":1,"ownHands":true}"#,
            );
            let (status, _) = said(crate::api::handle(&scene.context, &screens, request).await);
            assert_eq!(status, 404, "{target}");
        }
    }
    assert!(
        !scene
            .context
            .ledger
            .borrow()
            .project(scene.project.id)
            .unwrap()
            .unwrap()
            .own_hands
    );
}

/// The page over a project with a worker, on a bridge.
struct Asked {
    _daemon: Bridge,
    app: Bridge,
    rig: Rig,
    project: i64,
    _home: tempfile::TempDir,
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

impl Asked {
    /// The project's key as the page reads it from `operation`: the list or the board. None
    /// where the project has no key for it.
    async fn read(&self, operation: &str) -> Option<Value> {
        let body = json!({ "project": self.project });
        let answer = ask(&self.app, operation, body).await;
        let project = match operation {
            "projects.list" => answer["projects"][0].clone(),
            _ => answer["board"]["project"].clone(),
        };
        project.get("promptSuggestion").cloned()
    }

    async fn set(&self, body: Value) -> Value {
        ask(&self.app, "project.prompt_suggestion", body).await
    }
}

#[tokio::test]
async fn the_human_switches_the_suggestion_and_every_view_of_the_project_carries_it_only_while_it_is_on(
) {
    LocalSet::new()
        .run_until(async {
            let held = asked();
            let project = held.project;
            for operation in ["board.get", "projects.list"] {
                assert_eq!(held.read(operation).await, None, "{operation}: off, no key");
            }
            let answer = held
                .set(json!({ "project": project, "promptSuggestion": true }))
                .await;
            assert_eq!(answer["ok"], true, "{answer}");
            assert_eq!(answer["project"]["promptSuggestion"], true);
            assert_eq!(held.rig.kicks.get(), 1, "the dispatcher is woken once");
            for operation in ["board.get", "projects.list"] {
                assert_eq!(held.read(operation).await, Some(json!(true)), "{operation}");
            }
            let answer = held
                .set(json!({ "project": project, "promptSuggestion": false }))
                .await;
            assert_eq!(answer["ok"], true, "{answer}");
            assert!(answer["project"].get("promptSuggestion").is_none());
            for operation in ["board.get", "projects.list"] {
                assert_eq!(held.read(operation).await, None, "{operation}");
            }
        })
        .await;
}

#[tokio::test]
async fn anything_but_a_flag_is_refused_for_the_suggestion_before_the_project_is_looked_for_and_changes_nothing(
) {
    LocalSet::new()
        .run_until(async {
            let held = asked();
            let project = held.project;
            held.set(json!({ "project": project, "promptSuggestion": true }))
                .await;
            let kicks = held.rig.kicks.get();
            for said in [
                json!("true"),
                json!(1),
                json!(0),
                json!([true]),
                json!(null),
                json!(400_000),
            ] {
                let answer = held
                    .set(json!({ "project": project, "promptSuggestion": said }))
                    .await;
                assert_eq!(answer, json!({ "ok": false, "error": WORDS }), "{said}");
            }
            assert_eq!(
                held.set(json!({ "project": 99, "promptSuggestion": "yes" }))
                    .await["error"],
                WORDS,
                "checked before the project is looked for"
            );
            assert_eq!(
                held.set(json!({ "project": 99, "promptSuggestion": true }))
                    .await,
                json!({ "ok": false, "error": "no project 99" })
            );
            // A missing field is no flag either.
            assert_eq!(
                held.set(json!({ "project": project })).await,
                json!({ "ok": false, "error": WORDS })
            );
            assert_eq!(
                held.read("board.get").await,
                Some(json!(true)),
                "nothing changed"
            );
            assert_eq!(held.rig.kicks.get(), kicks, "a refusal wakes nothing");
        })
        .await;
}

#[tokio::test]
async fn a_windows_token_reaches_no_route_that_writes_the_suggestion_so_only_the_human_can() {
    let scene = scene();
    let screens = crate::screens::testing::inert();
    let body = r#"{"project":1,"promptSuggestion":true}"#;
    for token in [&scene.zeus, &scene.chief] {
        let ask_with = |method: Method, target: &str| {
            let request = request(method, target, Some(token), body);
            let (context, screens) = (&scene.context, &screens);
            async move { said(crate::api::handle(context, screens, request).await) }
        };
        for target in [
            "/api/prompt-suggestion",
            "/api/prompt_suggestion",
            "/api/suggestion",
            "/api/project/prompt-suggestion",
            "/api/project.prompt_suggestion",
            "/api/projects/1/prompt-suggestion",
            "/api/projects/1/prompt_suggestion",
        ] {
            for method in [Method::POST, Method::PUT, Method::PATCH] {
                let (status, answered) = ask_with(method.clone(), target).await;
                assert_eq!(status, 404, "{method} {target}: {answered}");
                assert_eq!(answered["error"], "unknown-route", "{method} {target}");
            }
        }
    }
    let after = scene
        .context
        .ledger
        .borrow()
        .project(scene.project.id)
        .unwrap()
        .unwrap();
    assert!(!after.prompt_suggestion, "nothing a window asked wrote it");
}

#[tokio::test]
async fn creation_with_own_hands_sends_the_initial_mode_in_the_project_open_write() {
    LocalSet::new().run_until(async {
        let (_home, env) = super::reading::home_installed("[]");
        let rig = rig_over(env, None);
        let (daemon, app) = bridge_pair_over(&rig.spawn);
        register(&daemon, &rig.page, &rig.spawn);
        let base = json!({"directory":"/work/on", "harness":"claude-code", "model":"claude-sonnet-5", "staff":[]});
        let mut on_body = base.clone();
        on_body["ownHands"] = json!(true);
        let on = ask(&app, "project.open", on_body).await;
        assert_eq!(on["ok"], true, "{on}");
        assert_eq!(on["project"]["ownHands"], true, "the first returned project");
        let id = on["project"]["id"].as_i64().unwrap();
        let ledger = rig.kit.ledger.borrow();
        assert!(ledger.project(id).unwrap().unwrap().own_hands);
        let events: Vec<_> = ledger.events(id, 0, 500).unwrap().into_iter()
            .filter(|event| event.kind == "project.own_hands").map(|event| event.data).collect();
        assert_eq!(events, [json!({"from":false, "to":true})]);
        drop(ledger);
        let off = ask(&app, "project.open", base.clone()).await;
        assert_eq!(off["ok"], true, "{off}");
        assert!(off["project"].get("ownHands").is_none());
        for value in [json!(null), json!("true"), json!(1)] {
            let mut invalid = base.clone();
            invalid["ownHands"] = value.clone();
            let before = rig.kit.ledger.borrow().projects().unwrap().len();
            let refused = ask(&app, "project.open", invalid).await;
            assert_eq!(refused["ok"], false, "{value}: {refused}");
            assert_eq!(rig.kit.ledger.borrow().projects().unwrap().len(), before);
        }
    }).await;
}

#[tokio::test]
async fn own_hands_settings_round_trip_survives_the_ledger_reopening() {
    LocalSet::new()
        .run_until(async {
            let held = asked();
            let project = held.project;
            let on = ask(
                &held.app,
                "project.own_hands",
                json!({"project":project,"ownHands":true}),
            )
            .await;
            assert_eq!(on["project"]["ownHands"], true);
            held.rig.kit.edit_ledger(|_| {});
            let board = ask(&held.app, "board.get", json!({"project":project})).await;
            assert_eq!(board["board"]["project"]["ownHands"], true);
            let off = ask(
                &held.app,
                "project.own_hands",
                json!({"project":project,"ownHands":false}),
            )
            .await;
            assert_eq!(off["ok"], true);
            held.rig.kit.edit_ledger(|_| {});
            let board = ask(&held.app, "board.get", json!({"project":project})).await;
            assert!(board["board"]["project"].get("ownHands").is_none());
        })
        .await;
}

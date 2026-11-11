//! Whether the human has the project's members run with Claude Code's own advisor tool, on
//! the page: a flag, off for every project until it is ticked, refused in words when it is
//! anything else (before the project is looked for), kept, shown on every view of the project
//! the page reads only while it is on, the dispatcher woken. It is the human's to choose
//! alone: no route of the agents' API writes it, so no `cf` command does, whosever token it
//! holds. Not held to a Node recording: this operation is this build's own.

use hyper::Method;

use super::reading::home_with;
use super::*;
use crate::testing::{request, said, scene};

const WORDS: &str = "the advisor tool is on (true) or off (false)";

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
        project.get("advisorTool").cloned()
    }

    async fn set(&self, body: Value) -> Value {
        ask(&self.app, "project.advisor_tool", body).await
    }
}

#[tokio::test]
async fn the_human_switches_it_and_every_view_of_the_project_carries_it_only_while_it_is_on() {
    LocalSet::new()
        .run_until(async {
            let held = asked();
            let project = held.project;
            for operation in ["board.get", "projects.list"] {
                assert_eq!(held.read(operation).await, None, "{operation}: off, no key");
            }
            let answer = held
                .set(json!({ "project": project, "advisorTool": true }))
                .await;
            assert_eq!(answer["ok"], true, "{answer}");
            assert_eq!(answer["project"]["advisorTool"], true);
            assert_eq!(held.rig.kicks.get(), 1, "the dispatcher is woken once");
            for operation in ["board.get", "projects.list"] {
                assert_eq!(held.read(operation).await, Some(json!(true)), "{operation}");
            }
            let answer = held
                .set(json!({ "project": project, "advisorTool": false }))
                .await;
            assert_eq!(answer["ok"], true, "{answer}");
            assert!(answer["project"].get("advisorTool").is_none());
            for operation in ["board.get", "projects.list"] {
                assert_eq!(held.read(operation).await, None, "{operation}");
            }
        })
        .await;
}

#[tokio::test]
async fn anything_but_a_flag_is_refused_before_the_project_is_looked_for_and_changes_nothing() {
    LocalSet::new()
        .run_until(async {
            let held = asked();
            let project = held.project;
            held.set(json!({ "project": project, "advisorTool": true }))
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
                    .set(json!({ "project": project, "advisorTool": said }))
                    .await;
                assert_eq!(answer, json!({ "ok": false, "error": WORDS }), "{said}");
            }
            assert_eq!(
                held.set(json!({ "project": 99, "advisorTool": "yes" }))
                    .await["error"],
                WORDS,
                "checked before the project is looked for"
            );
            assert_eq!(
                held.set(json!({ "project": 99, "advisorTool": true }))
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
async fn a_windows_token_reaches_no_route_that_writes_it_so_only_the_human_can() {
    let scene = scene();
    let screens = crate::screens::testing::inert();
    let body = r#"{"project":1,"advisorTool":true}"#;
    for token in [&scene.zeus, &scene.chief] {
        let ask_with = |method: Method, target: &str| {
            let request = request(method, target, Some(token), body);
            let (context, screens) = (&scene.context, &screens);
            async move { said(crate::api::handle(context, screens, request).await) }
        };
        for target in [
            "/api/advisor",
            "/api/advisor-tool",
            "/api/advisor_tool",
            "/api/project/advisor-tool",
            "/api/project.advisor_tool",
            "/api/projects/1/advisor-tool",
            "/api/projects/1/advisor_tool",
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
    assert!(!after.advisor_tool, "nothing a window asked wrote it");
}

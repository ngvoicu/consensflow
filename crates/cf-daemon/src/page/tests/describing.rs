//! What the human calls a project and says of it on the page: a rename and a
//! line under the name, each checked before the project is looked for, kept,
//! shown on every view of the project, and the dispatcher woken. And that both
//! are the human's alone: no route of the agents' API says either, so a member's
//! token reaches nothing of them, and what a window is told of the project is
//! its name when it asks (`whoami`) and never the line.
//! Not held to a Node recording: these operations are this build's own.

use hyper::Method;

use super::reading::home_with;
use super::*;
use crate::testing::{request, said, scene};

const NAME_REFUSED: &str = "name must be text, not empty, at most 100 characters";
const TOO_LONG: &str =
    "a project's description is one line of plain text: it is at most 200 characters";
const ONE_LINE: &str = "a project's description is one line of plain text: it has no line breaks or other control characters";

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

impl Asked {
    /// The project as the page reads it from `operation`: the list or the board.
    async fn read(&self, operation: &str) -> Value {
        let body = json!({ "project": self.project });
        let answer = ask(&self.app, operation, body).await;
        match operation {
            "projects.list" => answer["projects"][0].clone(),
            _ => answer["board"]["project"].clone(),
        }
    }
}

#[tokio::test]
async fn the_human_renames_a_project_and_every_view_of_it_shows_the_new_name_its_folder_staying() {
    LocalSet::new()
        .run_until(async {
            let held = asked();
            let before = held.read("board.get").await;
            let answer = ask(
                &held.app,
                "project.rename",
                json!({ "project": held.project, "name": "harbour" }),
            )
            .await;
            assert_eq!(answer["ok"], true, "{answer}");
            assert_eq!(answer["project"]["name"], "harbour");
            assert_eq!(
                answer["project"]["directory"], before["directory"],
                "the folder stays"
            );
            assert_eq!(held.rig.kicks.get(), 1, "the dispatcher is woken once");
            for operation in ["board.get", "projects.list"] {
                let shown = held.read(operation).await;
                assert_eq!(shown["name"], "harbour", "{operation}");
                assert_eq!(shown["directory"], before["directory"], "{operation}");
            }
            // The ledger logged it, as every change is.
            let events = held
                .rig
                .kit
                .ledger
                .borrow()
                .events(held.project, 0, 500)
                .unwrap();
            let renamed: Vec<_> = events
                .iter()
                .filter(|event| event.kind == "project.renamed")
                .map(|event| event.data.clone())
                .collect();
            assert_eq!(
                renamed,
                [json!({ "from": before["name"], "to": "harbour" })]
            );
        })
        .await;
}

#[tokio::test]
async fn the_human_describes_a_project_and_every_view_of_it_carries_the_line_until_it_is_taken_away(
) {
    LocalSet::new()
        .run_until(async {
            let held = asked();
            let project = held.project;
            for operation in ["board.get", "projects.list"] {
                assert!(
                    held.read(operation).await.get("description").is_none(),
                    "{operation}: a project with none has no key"
                );
            }
            let answer = ask(
                &held.app,
                "project.describe",
                json!({ "project": project, "description": "  Moves the cargo " }),
            )
            .await;
            assert_eq!(answer["ok"], true, "{answer}");
            assert_eq!(answer["project"]["description"], "Moves the cargo");
            assert_eq!(held.rig.kicks.get(), 1, "the dispatcher is woken once");
            for operation in ["board.get", "projects.list"] {
                assert_eq!(
                    held.read(operation).await["description"],
                    "Moves the cargo",
                    "{operation}"
                );
            }
            // Taken away by blank text, by null and by leaving it out.
            for body in [
                json!({ "project": project, "description": "   " }),
                json!({ "project": project, "description": null }),
                json!({ "project": project }),
            ] {
                ask(
                    &held.app,
                    "project.describe",
                    json!({ "project": project, "description": "Moves the cargo" }),
                )
                .await;
                let answer = ask(&held.app, "project.describe", body.clone()).await;
                assert_eq!(answer["ok"], true, "{body}: {answer}");
                assert!(answer["project"].get("description").is_none(), "{body}");
                for operation in ["board.get", "projects.list"] {
                    assert!(
                        held.read(operation).await.get("description").is_none(),
                        "{body}: {operation}"
                    );
                }
            }
        })
        .await;
}

#[tokio::test]
async fn a_name_that_is_empty_too_long_or_no_text_is_refused_and_wakes_nothing() {
    LocalSet::new()
        .run_until(async {
            let held = asked();
            let before = held.read("board.get").await;
            for name in [
                json!(""),
                json!("   "),
                json!("x".repeat(101)),
                json!("😀".repeat(51)),
                json!(5),
                json!(null),
                json!(["harbour"]),
            ] {
                assert_eq!(
                    ask(
                        &held.app,
                        "project.rename",
                        json!({ "project": held.project, "name": name })
                    )
                    .await,
                    json!({ "ok": false, "error": NAME_REFUSED }),
                    "{name:.20}"
                );
            }
            assert_eq!(
                ask(
                    &held.app,
                    "project.rename",
                    json!({ "project": held.project })
                )
                .await,
                json!({ "ok": false, "error": NAME_REFUSED }),
                "no name at all"
            );
            // A hundred characters is as long as a name may be.
            let longest = "x".repeat(100);
            let kept = ask(
                &held.app,
                "project.rename",
                json!({ "project": held.project, "name": longest }),
            )
            .await;
            assert_eq!(kept["project"]["name"], json!(longest));
            assert_eq!(
                held.rig.kicks.get(),
                1,
                "only the one that was taken woke it"
            );
            assert_ne!(held.read("board.get").await["name"], before["name"]);
        })
        .await;
}

#[tokio::test]
async fn a_description_that_is_too_long_has_a_line_break_or_is_no_text_is_refused_and_changes_nothing(
) {
    LocalSet::new()
        .run_until(async {
            let held = asked();
            let project = held.project;
            ask(
                &held.app,
                "project.describe",
                json!({ "project": project, "description": "Moves the cargo" }),
            )
            .await;
            let kicks = held.rig.kicks.get();
            for (description, error) in [
                (json!("a".repeat(201)), TOO_LONG),
                (json!("😀".repeat(101)), TOO_LONG),
                (json!("two\nlines"), ONE_LINE),
                (json!("two\r\nlines"), ONE_LINE),
                (json!("tab\there"), ONE_LINE),
                (
                    json!(5),
                    "a project's description is one line of plain text: it is text, or none to take it away",
                ),
            ] {
                assert_eq!(
                    ask(
                        &held.app,
                        "project.describe",
                        json!({ "project": project, "description": description })
                    )
                    .await,
                    json!({ "ok": false, "error": error }),
                    "{description:.20}"
                );
            }
            // Two hundred characters are as long as a description may be.
            let longest = "a".repeat(200);
            let kept = ask(
                &held.app,
                "project.describe",
                json!({ "project": project, "description": longest }),
            )
            .await;
            assert_eq!(kept["project"]["description"], json!(longest));
            assert_eq!(held.rig.kicks.get(), kicks + 1, "a refusal wakes nothing");
        })
        .await;
}

#[tokio::test]
async fn the_request_is_checked_before_the_project_is_looked_for_and_a_project_that_is_not_there_or_no_number_is_refused_in_the_usual_words(
) {
    LocalSet::new()
        .run_until(async {
            let held = asked();
            // The name and the line are refused first, whatever project is named.
            assert_eq!(
                ask(
                    &held.app,
                    "project.rename",
                    json!({ "project": 99, "name": "" })
                )
                .await["error"],
                NAME_REFUSED
            );
            assert_eq!(
                ask(
                    &held.app,
                    "project.describe",
                    json!({ "project": 99, "description": "a\nb" })
                )
                .await["error"],
                ONE_LINE
            );
            for (operation, body) in [
                (
                    "project.rename",
                    json!({ "project": 99, "name": "harbour" }),
                ),
                (
                    "project.describe",
                    json!({ "project": 99, "description": "Moves the cargo" }),
                ),
            ] {
                assert_eq!(
                    ask(&held.app, operation, body).await,
                    json!({ "ok": false, "error": "no project 99" }),
                    "{operation}"
                );
            }
            for (operation, body) in [
                ("project.rename", json!({ "name": "harbour" })),
                (
                    "project.describe",
                    json!({ "description": "Moves the cargo" }),
                ),
                (
                    "project.rename",
                    json!({ "project": "1", "name": "harbour" }),
                ),
            ] {
                let answer = ask(&held.app, operation, body).await;
                assert_eq!(answer["ok"], false, "{operation}");
                assert!(
                    answer["error"]
                        .as_str()
                        .is_some_and(|error| error.starts_with("project is a whole number, not ")),
                    "{operation}: {answer}"
                );
            }
            assert_eq!(
                held.rig.kicks.get(),
                0,
                "nothing succeeded, nothing is woken"
            );
        })
        .await;
}

#[tokio::test]
async fn a_member_s_token_reaches_no_route_that_renames_or_describes_a_project_so_only_the_human_can(
) {
    let scene = scene();
    let screens = crate::screens::testing::inert();
    let ask_with = |method: Method, target: &str, body: &str| {
        let request = request(method, target, Some(&scene.zeus), body);
        let (context, screens) = (&scene.context, &screens);
        async move { said(crate::api::handle(context, screens, request).await) }
    };
    let body = r#"{"project":1,"name":"taken","description":"taken"}"#;
    for target in [
        "/api/rename",
        "/api/describe",
        "/api/project",
        "/api/project/rename",
        "/api/project/describe",
        "/api/project.rename",
        "/api/project.describe",
        "/api/projects/1",
        "/api/projects/1/name",
        "/api/projects/1/rename",
        "/api/projects/1/description",
    ] {
        for method in [Method::POST, Method::PUT, Method::PATCH, Method::GET] {
            let (status, answered) = ask_with(method.clone(), target, body).await;
            // The agents' routes say no such command, whatever they are asked.
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
    assert_eq!(
        (after.name.as_str(), after.description),
        ("app", None),
        "nothing a window asked changed either"
    );
}

#[tokio::test]
async fn a_window_that_asks_who_it_is_hears_the_new_name_and_never_the_line_under_it() {
    let scene = scene();
    let screens = crate::screens::testing::inert();
    let whoami = || async {
        let request = request(Method::GET, "/api/whoami", Some(&scene.zeus), "");
        said(crate::api::handle(&scene.context, &screens, request).await)
    };
    let (status, before) = whoami().await;
    assert_eq!(
        (status, before["project"]["name"].as_str()),
        (200, Some("app"))
    );
    {
        let mut ledger = scene.context.ledger.borrow_mut();
        ledger.rename_project(scene.project.id, "harbour").unwrap();
        ledger
            .describe_project(scene.project.id, Some("Moves the cargo, for the human"))
            .unwrap();
    }
    // Nothing was sent to the window: it is told the name when it asks.
    let (status, after) = whoami().await;
    assert_eq!(status, 200, "{after}");
    assert_eq!(after["project"]["name"], "harbour");
    assert!(
        after["project"].get("description").is_none(),
        "no field of the project's is the line: {after}"
    );
    assert!(
        !after.to_string().contains("Moves the cargo"),
        "the line is in no word of the answer: {after}"
    );
}

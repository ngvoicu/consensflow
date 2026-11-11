//! The size the human chooses for the project's member windows to compact at,
//! whatever their harness, on the page: checked before the project is looked for, kept, shown on every
//! view of the project the page reads, the dispatcher woken. It is the human's to
//! choose alone: no route of the agents' API writes it, so no `cf` command does,
//! whosever token it holds. And the page's own range is the ledger's.
//! Not held to a Node recording: this operation is this build's own.

use cf_ledger::{COMPACTION_DEFAULT, COMPACTION_MAX, COMPACTION_MIN};
use hyper::Method;

use super::reading::home_with;
use super::*;
use crate::testing::{request, said, scene};

const WORDS: &str = "the compaction size is a whole number of tokens from 250000 to 1000000, or none for each harness's own point";

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
    /// The project's size as the page reads it from `operation`: the list or the
    /// board. None where the project has no key for it.
    async fn read(&self, operation: &str) -> Option<Value> {
        let body = json!({ "project": self.project });
        let answer = ask(&self.app, operation, body).await;
        let project = match operation {
            "projects.list" => answer["projects"][0].clone(),
            _ => answer["board"]["project"].clone(),
        };
        project.get("compaction").cloned()
    }

    async fn set(&self, body: Value) -> Value {
        ask(&self.app, "project.compaction", body).await
    }
}

#[tokio::test]
async fn the_human_chooses_the_size_and_every_view_of_the_project_carries_it_until_it_is_taken_away(
) {
    LocalSet::new()
        .run_until(async {
            let held = asked();
            let project = held.project;
            for operation in ["board.get", "projects.list"] {
                assert_eq!(
                    held.read(operation).await,
                    None,
                    "{operation}: none, no key"
                );
            }
            let answer = held
                .set(json!({ "project": project, "compaction": 400_000 }))
                .await;
            assert_eq!(answer["ok"], true, "{answer}");
            assert_eq!(answer["project"]["compaction"], 400_000);
            assert_eq!(held.rig.kicks.get(), 1, "the dispatcher is woken once");
            for operation in ["board.get", "projects.list"] {
                assert_eq!(
                    held.read(operation).await,
                    Some(json!(400_000)),
                    "{operation}"
                );
            }
            for body in [
                json!({ "project": project, "compaction": null }),
                json!({ "project": project }),
            ] {
                held.set(json!({ "project": project, "compaction": 400_000 }))
                    .await;
                let answer = held.set(body.clone()).await;
                assert_eq!(answer["ok"], true, "{body}: {answer}");
                assert!(answer["project"].get("compaction").is_none(), "{body}");
                for operation in ["board.get", "projects.list"] {
                    assert_eq!(held.read(operation).await, None, "{body}: {operation}");
                }
            }
        })
        .await;
}

#[tokio::test]
async fn a_size_outside_the_range_or_no_number_is_refused_and_changes_nothing() {
    LocalSet::new()
        .run_until(async {
            let held = asked();
            let project = held.project;
            held.set(json!({ "project": project, "compaction": 400_000 }))
                .await;
            let kicks = held.rig.kicks.get();
            for size in [
                json!(0),
                json!(99_999),
                json!(100_000),
                json!(200_000),
                json!(249_999),
                json!(1_000_001),
                json!(-1),
                json!(400_000.5),
                json!("400000"),
                json!(true),
                json!([400_000]),
            ] {
                assert_eq!(
                    held.set(json!({ "project": project, "compaction": size }))
                        .await,
                    json!({ "ok": false, "error": WORDS }),
                    "{size}"
                );
            }
            // Checked before the project is looked for.
            assert_eq!(
                held.set(json!({ "project": 99, "compaction": 99_999 }))
                    .await["error"],
                WORDS
            );
            assert_eq!(
                held.set(json!({ "project": 99, "compaction": 400_000 }))
                    .await,
                json!({ "ok": false, "error": "no project 99" })
            );
            assert_eq!(held.read("board.get").await, Some(json!(400_000)));
            // The ends of the range are sizes.
            for size in [COMPACTION_MIN, COMPACTION_MAX] {
                let kept = held
                    .set(json!({ "project": project, "compaction": size }))
                    .await;
                assert_eq!(kept["project"]["compaction"], size);
            }
            assert_eq!(held.rig.kicks.get(), kicks + 2, "a refusal wakes nothing");
        })
        .await;
}

#[tokio::test]
async fn a_windows_token_reaches_no_route_that_writes_the_size_so_only_the_human_can() {
    let scene = scene();
    let screens = crate::screens::testing::inert();
    let body = r#"{"project":1,"compaction":400000}"#;
    for token in [&scene.zeus, &scene.chief] {
        let ask_with = |method: Method, target: &str| {
            let request = request(method, target, Some(token), body);
            let (context, screens) = (&scene.context, &screens);
            async move { said(crate::api::handle(context, screens, request).await) }
        };
        for target in [
            "/api/compaction",
            "/api/claude-compaction",
            "/api/project/compaction",
            "/api/project.compaction",
            "/api/projects/1/compaction",
            "/api/projects/1/claude-compaction",
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
    assert_eq!(after.compaction, None, "nothing a window asked wrote it");
}

/// The page checks the size the human types against the range and says it in its
/// words: its copy of the range is held to the ledger's, and so is the size New project
/// opens on (T-1400).
#[test]
fn the_pages_range_for_the_size_is_the_ledgers() {
    let source = include_str!("../../../../../app/ui/core/compaction.js");
    for (constant, value) in [
        ("LEAST", COMPACTION_MIN),
        ("MOST", COMPACTION_MAX),
        ("DEFAULT_SIZE", COMPACTION_DEFAULT),
    ] {
        let line = format!("const {constant} = {}\n", group(value));
        assert!(
            source.contains(&line),
            "the page's {constant} is not the ledger's: {line}"
        );
    }
}

/// `value` as JavaScript writes a number with its thousands grouped: 100_000.
fn group(value: u32) -> String {
    let digits = value.to_string();
    let mut grouped = String::new();
    for (at, digit) in digits.chars().enumerate() {
        if at > 0 && (digits.len() - at).is_multiple_of(3) {
            grouped.push('_');
        }
        grouped.push(digit);
    }
    grouped
}

//! The rules the human writes for a project's staff, on the page: checked before
//! the project is looked for, kept, shown on every view of the project the page
//! reads, the dispatcher woken. They are the human's to write alone: no route of
//! the agents' API writes them, so no `cf` command does, whosever token it holds.
//! And the page's own bounds and words are the ledger's.
//! Not held to a Node recording: this operation is this build's own.

use cf_ledger::{STAFF_RULES_MAX_BYTES, STAFF_RULES_MAX_LINES};
use hyper::Method;

use super::reading::home_with;
use super::*;
use crate::testing::{request, said, scene};

const RULES: &str = "- Push to origin only.\n- Tests use the .smoke bundle id.";

/// The page over a project with a worker, on a bridge.
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
    /// The project's rules as the page reads them from `operation`: the list or
    /// the board. None where the project has no key for them.
    async fn read(&self, operation: &str) -> Option<Value> {
        let body = json!({ "project": self.project });
        let answer = ask(&self.app, operation, body).await;
        let project = match operation {
            "projects.list" => answer["projects"][0].clone(),
            _ => answer["board"]["project"].clone(),
        };
        project.get("staffRules").cloned()
    }

    async fn set(&self, body: Value) -> Value {
        ask(&self.app, "project.rules", body).await
    }
}

#[tokio::test]
async fn the_human_writes_the_staffs_rules_and_every_view_of_the_project_carries_them_until_taken_away(
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
                .set(json!({ "project": project, "rules": format!("\n  {RULES}  \n") }))
                .await;
            assert_eq!(answer["ok"], true, "{answer}");
            assert_eq!(answer["project"]["staffRules"], RULES, "kept trimmed");
            assert_eq!(held.rig.kicks.get(), 1, "the dispatcher is woken once");
            for operation in ["board.get", "projects.list"] {
                assert_eq!(
                    held.read(operation).await,
                    Some(json!(RULES)),
                    "{operation}"
                );
            }
            for body in [
                json!({ "project": project, "rules": " \n " }),
                json!({ "project": project, "rules": null }),
                json!({ "project": project }),
            ] {
                held.set(json!({ "project": project, "rules": RULES }))
                    .await;
                let answer = held.set(body.clone()).await;
                assert_eq!(answer["ok"], true, "{body}: {answer}");
                assert!(answer["project"].get("staffRules").is_none(), "{body}");
                for operation in ["board.get", "projects.list"] {
                    assert_eq!(held.read(operation).await, None, "{body}: {operation}");
                }
            }
        })
        .await;
}

#[tokio::test]
async fn rules_over_the_bounds_with_a_control_character_or_no_text_are_refused_and_change_nothing()
{
    LocalSet::new()
        .run_until(async {
            let held = asked();
            let project = held.project;
            held.set(json!({ "project": project, "rules": RULES })).await;
            let kicks = held.rig.kicks.get();
            for (rules, error) in [
                (
                    json!("a".repeat(8_001)),
                    "the rules for the staff are plain text: they are at most 8000 bytes, and these are 8001",
                ),
                (
                    json!("日".repeat(2_667)),
                    "the rules for the staff are plain text: they are at most 8000 bytes, and these are 8001",
                ),
                (
                    json!(vec!["a"; 151].join("\n")),
                    "the rules for the staff are plain text: they are at most 150 lines, and these are 151",
                ),
                (
                    json!("a\r\nb"),
                    "the rules for the staff are plain text: they have no control characters but line breaks and tabs",
                ),
                (
                    json!(["a rule"]),
                    "the rules for the staff are plain text: they are text, or none to take them away",
                ),
            ] {
                assert_eq!(
                    held.set(json!({ "project": project, "rules": rules })).await,
                    json!({ "ok": false, "error": error }),
                    "{rules:.20}"
                );
            }
            // Checked before the project is looked for.
            assert_eq!(
                held.set(json!({ "project": 99, "rules": "a".repeat(8_001) }))
                    .await["error"],
                "the rules for the staff are plain text: they are at most 8000 bytes, and these are 8001"
            );
            assert_eq!(
                held.set(json!({ "project": 99, "rules": RULES })).await,
                json!({ "ok": false, "error": "no project 99" })
            );
            assert_eq!(held.read("board.get").await, Some(json!(RULES)));
            // As long as rules may be, in bytes and in lines.
            let longest = vec!["a".repeat(52); 150].join("\n");
            assert_eq!((longest.len(), longest.split('\n').count()), (7_949, 150));
            let kept = held.set(json!({ "project": project, "rules": longest })).await;
            assert_eq!(kept["project"]["staffRules"], json!(longest));
            assert_eq!(held.rig.kicks.get(), kicks + 1, "a refusal wakes nothing");
        })
        .await;
}

#[tokio::test]
async fn a_windows_token_reaches_no_route_that_writes_the_staffs_rules_so_only_the_human_can() {
    let scene = scene();
    let screens = crate::screens::testing::inert();
    let body = r#"{"project":1,"rules":"taken"}"#;
    for token in [&scene.zeus, &scene.chief] {
        let ask_with = |method: Method, target: &str| {
            let request = request(method, target, Some(token), body);
            let (context, screens) = (&scene.context, &screens);
            async move { said(crate::api::handle(context, screens, request).await) }
        };
        for target in [
            "/api/rules",
            "/api/staff-rules",
            "/api/project/rules",
            "/api/project.rules",
            "/api/projects/1/rules",
            "/api/projects/1/staff-rules",
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
    assert_eq!(after.staff_rules, None, "nothing a window asked wrote them");
}

/// The page counts what the human writes against the ledger's bounds, and says
/// them in its words: its copy of them is held to the ledger's.
#[test]
fn the_pages_bounds_for_the_staffs_rules_are_the_ledgers() {
    let source = include_str!("../../../../../app/ui/core/staff-rules.js");
    for (constant, value) in [
        ("MOST_BYTES", STAFF_RULES_MAX_BYTES),
        ("MOST_LINES", STAFF_RULES_MAX_LINES),
    ] {
        let line = format!("const {constant} = {}\n", group(value));
        assert!(
            source.contains(&line),
            "the page's {constant} is not the ledger's: {line}"
        );
    }
}

/// `value` as JavaScript writes a number with its thousands grouped: 8_000.
fn group(value: usize) -> String {
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

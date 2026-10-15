//! A project opened from the page with its wake-up calls and its specs said
//! (`project.open` with `wakeUp` and `specs`, as `wakeup.set` and
//! `project.specs` take them): they are kept in the write that makes the project,
//! so the chief's first launch has them, and one the ledger refuses refuses the
//! whole start in the words the Project settings dialog is refused in, with no
//! project made and no window asked for. Not held to a Node recording: Node's
//! page said neither. The real dispatcher, over the engine's kit, stands behind
//! the page.

use cf_ledger::DEFAULT_WAKE_UP_TEXT;

use super::reading::home_with;
use super::*;

/// The saved agents of the home: apollo, whom a chief may run on.
const APOLLO: &str = r#"[{"id":"apollo","kind":"claude-code","model":"m","workTier":"light"}]"#;

/// The page over a home with apollo saved, on a bridge, with a project of the
/// kit's already open (what the dialogs of an existing project are asked of).
struct Asked {
    _home: tempfile::TempDir,
    _daemon: Bridge,
    app: Bridge,
    rig: Rig,
    existing: i64,
}

fn asked() -> Asked {
    let (home, env) = home_with(APOLLO);
    let rig = rig_over(env, None);
    let (daemon, app) = bridge_pair_over(&rig.spawn);
    register(&daemon, &rig.page, &rig.spawn);
    let existing = rig.kit.with_staff(&["zeus"]).id;
    Asked {
        _home: home,
        _daemon: daemon,
        app,
        rig,
        existing,
    }
}

/// What New project sends: the folder, the chief, no staff, and `said` besides.
fn opening(said: Value) -> Value {
    let mut body = json!({ "directory": "/work/fresh", "agent": "apollo", "staff": [] });
    if let (Some(body), Some(said)) = (body.as_object_mut(), said.as_object()) {
        body.extend(said.clone());
    }
    body
}

/// The launches the engine's adapter was asked for, the chief's of each project.
fn chief_launches(held: &Asked) -> Vec<Value> {
    held.rig.kit.settle();
    held.rig
        .kit
        .adapter
        .prepared()
        .into_iter()
        .filter(|launch| launch["role"] == "chief")
        .collect()
}

#[tokio::test]
async fn a_project_opened_with_calls_and_specs_has_them_and_its_chief_is_given_the_specs_at_its_first_launch(
) {
    LocalSet::new()
        .run_until(async {
            let held = asked();
            let before = chief_launches(&held).len();
            let answer = ask(
                &held.app,
                "project.open",
                opening(json!({
                    "gate": true,
                    "wakeUp": { "on": true, "every": 90, "unit": "minutes", "text": "Anything left?" },
                    "specs": { "on": true, "folder": ".\\docs//specs/", "variant": "tdd" },
                })),
            )
            .await;
            assert_eq!(answer["ok"], true, "{answer}");
            let opened = answer["project"]["id"].as_i64().unwrap();
            assert_eq!(
                answer["project"]["specs"],
                json!({ "on": true, "folder": "docs/specs", "variant": "tdd" })
            );
            assert_eq!(answer["project"]["gate"], true);
            let calls = held.rig.kit.ledger.borrow().wake_up(opened).unwrap();
            assert_eq!(
                (calls.on, calls.every_ms, calls.text.as_deref()),
                (true, 5_400_000, Some("Anything left?"))
            );
            // The chief was launched once, as the project opened, and was given the specs then.
            let launches = chief_launches(&held);
            assert_eq!(launches.len(), before + 1);
            let launch = launches.last().unwrap();
            assert_eq!(launch["project"]["id"], opened);
            assert_eq!(launch["specs"]["variant"], "tdd");
            assert!(
                launch["instructions"]
                    .as_str()
                    .unwrap()
                    .contains("tdd in docs/specs at "),
                "{launch}"
            );
            // The dialog of an existing project reads it back as it was said.
            let read = ask(&held.app, "wakeup.get", json!({ "project": opened })).await;
            assert_eq!(read["wakeUp"]["every"], 90);
            assert_eq!(read["wakeUp"]["unit"], "minutes");
            assert_eq!(read["wakeUp"]["defaultText"], false);
            assert_eq!(held.rig.kicks.get(), 1, "the open woke the dispatcher; the read did not");
        })
        .await;
}

#[tokio::test]
async fn a_start_that_says_nothing_of_them_or_says_what_a_project_has_leaves_them_as_they_were() {
    LocalSet::new()
        .run_until(async {
            let held = asked();
            for said in [
                json!({}),
                json!({ "wakeUp": null, "specs": null }),
                json!({ "wakeUp": { "on": false, "every": 1, "unit": "hours", "text": DEFAULT_WAKE_UP_TEXT } }),
            ] {
                let answer = ask(&held.app, "project.open", opening(said.clone())).await;
                assert_eq!(answer["ok"], true, "{said}: {answer}");
                assert!(answer["project"].get("specs").is_none(), "{said}: {answer}");
                let opened = answer["project"]["id"].as_i64().unwrap();
                let ledger = held.rig.kit.ledger.borrow();
                let calls = ledger.wake_up(opened).unwrap();
                assert_eq!((calls.on, calls.every_ms, calls.text), (false, 3_600_000, None));
                let kinds: Vec<String> = ledger
                    .events(opened, 0, 500)
                    .unwrap()
                    .into_iter()
                    .map(|event| event.kind)
                    .collect();
                assert!(
                    !kinds.iter().any(|kind| kind.starts_with("project.specs")
                        || kind == "project.wake_up_setting"),
                    "{said}: {kinds:?}"
                );
            }
        })
        .await;
}

#[tokio::test]
async fn a_setting_the_daemon_refuses_refuses_the_whole_start_in_the_dialogs_words_and_makes_no_project(
) {
    LocalSet::new()
        .run_until(async {
            let held = asked();
            let (project, launched) = (held.existing, chief_launches(&held).len());
            let inside = "the specs folder is a folder inside the project: ";
            let every = |every: Value, unit: Value| {
                json!({ "wakeUp": { "on": true, "every": every, "unit": unit } })
            };
            // (what New project sends, and what the dialog of an existing project sends for it)
            let cases = [
                (
                    json!({ "specs": { "on": true, "folder": "../elsewhere" } }),
                    json!({ "project": project, "on": true, "folder": "../elsewhere" }),
                    "project.specs",
                    format!("{inside}it stays inside it, with no `..`"),
                ),
                (
                    json!({ "specs": { "folder": "/etc/specs" } }),
                    json!({ "project": project, "folder": "/etc/specs" }),
                    "project.specs",
                    format!("{inside}it is relative to the project, not absolute"),
                ),
                (
                    json!({ "specs": { "variant": "full" } }),
                    json!({ "project": project, "variant": "full" }),
                    "project.specs",
                    "the specs variant is one of core, core-html, tdd, tdd-html, not \"full\""
                        .to_owned(),
                ),
                (
                    json!({ "specs": { "on": "yes" } }),
                    json!({ "project": project, "on": "yes" }),
                    "project.specs",
                    "specs are on (true), off (false) or follow the app's setting (null)"
                        .to_owned(),
                ),
                (
                    every(json!(0), json!("minutes")),
                    json!({ "project": project, "on": true, "every": 0, "unit": "minutes" }),
                    "wakeup.set",
                    "a wake-up comes every whole number of minutes, hours or days, not 0 \"minutes\""
                        .to_owned(),
                ),
                (
                    every(json!(5), json!("seconds")),
                    json!({ "project": project, "on": true, "every": 5, "unit": "seconds" }),
                    "wakeup.set",
                    "a wake-up comes every whole number of minutes, hours or days, not 5 \"seconds\""
                        .to_owned(),
                ),
                (
                    every(json!(9_007_199_254_740_991_i64), json!("days")),
                    json!({ "project": project, "on": true, "every": 9_007_199_254_740_991_i64, "unit": "days" }),
                    "wakeup.set",
                    "9007199254740991 days is longer than a wake-up can wait".to_owned(),
                ),
                (
                    json!({ "wakeUp": { "on": "yes", "every": 1, "unit": "hours" } }),
                    json!({ "project": project, "on": "yes", "every": 1, "unit": "hours" }),
                    "wakeup.set",
                    "wake-up calls are on (true) or off (false)".to_owned(),
                ),
                (
                    json!({ "wakeUp": { "on": true, "every": 1, "unit": "hours", "text": "  " } }),
                    json!({ "project": project, "on": true, "every": 1, "unit": "hours", "text": "  " }),
                    "wakeup.set",
                    "the wake-up text must be text, not empty, at most 1000000 characters"
                        .to_owned(),
                ),
            ];
            for (starting, saving, operation, words) in cases {
                let refused = ask(&held.app, "project.open", opening(starting.clone())).await;
                assert_eq!(refused, json!({ "ok": false, "error": words }), "{starting}");
                // The words the dialog of a project is refused in for the same.
                let dialog = ask(&held.app, operation, saving.clone()).await;
                assert_eq!(dialog["error"], refused["error"], "{operation} {saving}");
            }
            assert_eq!(held.rig.kicks.get(), 0, "nothing succeeded, nothing is woken");
            let projects = held.rig.kit.ledger.borrow().projects().unwrap();
            assert_eq!(projects.len(), 1, "only the project that was open");
            assert_eq!(projects[0].id, project);
            assert_eq!(chief_launches(&held).len(), launched, "no window was asked for");
        })
        .await;
}

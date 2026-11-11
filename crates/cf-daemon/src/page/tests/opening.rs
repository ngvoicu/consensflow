//! A project opened from the page with its wake-up calls and its specs said
//! (`project.open` with `wakeUp` and `specs`, as `wakeup.set` and
//! `project.specs` take them): they are kept in the write that makes the project,
//! so the chief's first launch has them, and one the ledger refuses refuses the
//! whole start in the words the Project settings dialog is refused in, with no
//! project made and no window asked for. Not held to a Node recording: Node's
//! page said neither. The real dispatcher, over the engine's kit, stands behind
//! the page.

use cf_ledger::DEFAULT_WAKE_UP_TEXT;
use cf_proto::models::{ModelChoice, ModelListing};

use super::reading::home_installed;
use super::*;

/// The saved agents of the home: apollo, whom a chief may run on.
const APOLLO: &str = r#"[{"id":"apollo","kind":"claude-code","model":"m","workTier":"light"}]"#;

/// The page over a home with apollo saved, on a bridge, with a project of the
/// kit's already open (what the dialogs of an existing project are asked of).
struct Asked {
    _daemon: Bridge,
    app: Bridge,
    rig: Rig,
    existing: i64,
    _home: tempfile::TempDir,
}

fn asked() -> Asked {
    let (home, env) = home_installed(APOLLO);
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
    let mut body = json!({ "directory": "/work/fresh", "harness": "claude-code", "model": "claude-opus-5-5", "staff": [] });
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

const RULES: &str = "- Push to origin only.\n- Tests use the .smoke bundle id.";

#[tokio::test]
async fn a_project_opened_with_rules_and_a_compaction_has_them_and_its_chief_is_given_the_rules_at_its_first_launch(
) {
    LocalSet::new()
        .run_until(async {
            let held = asked();
            let before = chief_launches(&held).len();
            let answer = ask(
                &held.app,
                "project.open",
                opening(json!({ "rules": RULES, "compaction": 250_000, "advisorTool": true })),
            )
            .await;
            assert_eq!(answer["ok"], true, "{answer}");
            let opened = answer["project"]["id"].as_i64().unwrap();
            assert_eq!(answer["project"]["staffRules"], RULES);
            assert_eq!(answer["project"]["compaction"], 250_000);
            assert_eq!(answer["project"]["advisorTool"], true);
            // The chief was launched once, as the project opened, and was given the rules then.
            let launches = chief_launches(&held);
            assert_eq!(launches.len(), before + 1);
            let launch = launches.last().unwrap();
            assert_eq!(launch["project"]["id"], opened);
            assert!(
                launch["instructions"]
                    .as_str()
                    .unwrap()
                    .contains("Tests use the .smoke bundle id."),
                "{launch}"
            );
            // The dialog of an existing project reads them back as they were said.
            let board = ask(&held.app, "board.get", json!({ "project": opened })).await;
            assert_eq!(board["board"]["project"]["staffRules"], RULES);
            assert_eq!(board["board"]["project"]["compaction"], 250_000);
            assert_eq!(board["board"]["project"]["advisorTool"], true);
            // The roles its cards name come with it, in the table's order.
            assert_eq!(board["roles"], cf_ledger::roles::page_view());
        })
        .await;
}

#[tokio::test]
async fn a_project_opened_without_the_flag_has_the_advisor_tool_off_and_one_the_daemon_refuses_makes_no_project(
) {
    LocalSet::new()
        .run_until(async {
            let held = asked();
            let answer = ask(&held.app, "project.open", opening(json!({}))).await;
            assert_eq!(answer["ok"], true, "{answer}");
            assert!(
                answer["project"].get("advisorTool").is_none(),
                "off, no key"
            );
            let projects = held.rig.kit.ledger.borrow().projects().unwrap().len();
            let refused = ask(
                &held.app,
                "project.open",
                opening(json!({ "advisorTool": "yes" })),
            )
            .await;
            assert_eq!(
                refused,
                json!({ "ok": false, "error": "the advisor tool is on (true) or off (false)" })
            );
            // The words the dialog of a project is refused in for the same.
            let dialog = ask(
                &held.app,
                "project.advisor_tool",
                json!({ "project": answer["project"]["id"], "advisorTool": "yes" }),
            )
            .await;
            assert_eq!(dialog["error"], refused["error"]);
            assert_eq!(
                held.rig.kit.ledger.borrow().projects().unwrap().len(),
                projects,
                "no project was made"
            );
        })
        .await;
}

#[tokio::test]
async fn rules_and_a_size_the_daemon_refuses_refuse_the_whole_start_in_the_dialogs_words_and_make_no_project(
) {
    LocalSet::new()
        .run_until(async {
            let held = asked();
            let (project, launched) = (held.existing, chief_launches(&held).len());
            let cases = [
                (
                    json!({ "rules": "x".repeat(8_001) }),
                    json!({ "project": project, "rules": "x".repeat(8_001) }),
                    "project.rules",
                ),
                (
                    json!({ "rules": 42 }),
                    json!({ "project": project, "rules": 42 }),
                    "project.rules",
                ),
                (
                    json!({ "compaction": 99_999 }),
                    json!({ "project": project, "compaction": 99_999 }),
                    "project.compaction",
                ),
                (
                    json!({ "compaction": "400000" }),
                    json!({ "project": project, "compaction": "400000" }),
                    "project.compaction",
                ),
            ];
            for (starting, saving, operation) in cases {
                let refused = ask(&held.app, "project.open", opening(starting.clone())).await;
                assert_eq!(refused["ok"], false, "{starting}: {refused}");
                // The words the dialog of a project is refused in for the same.
                let dialog = ask(&held.app, operation, saving.clone()).await;
                assert_eq!(dialog["ok"], false, "{operation} {saving}");
                assert_eq!(dialog["error"], refused["error"], "{operation} {saving}");
            }
            assert_eq!(
                held.rig.kicks.get(),
                0,
                "nothing succeeded, nothing is woken"
            );
            let projects = held.rig.kit.ledger.borrow().projects().unwrap();
            assert_eq!(projects.len(), 1, "only the project that was open");
            assert_eq!(
                chief_launches(&held).len(),
                launched,
                "no window was asked for"
            );
        })
        .await;
}

/// A chief's model is one its harness lists and runs, where the home keeps its list
/// (CHF-08): the picker read it to offer it.
#[tokio::test]
async fn a_chief_on_a_model_its_harness_does_not_list_or_will_not_run_is_refused() {
    LocalSet::new()
        .run_until(async {
            let held = asked();
            let mut gated = ModelChoice::new("claude-opus-9", "Opus 9");
            gated.disabled = true;
            let listing = ModelListing::read(
                vec![ModelChoice::new("claude-opus-5-5", "Opus 5.5"), gated],
                1_793_000_000_000,
                "the test's",
            );
            // The list the picker showed, whose read the page names with what it submits.
            let read = held.rig.page.models.record(cf_proto::agents::Harness::Claude, listing);
            for (model, said) in [
                (
                    "claude-gone-9",
                    "claude-code does not list claude-gone-9: pick one of its models",
                ),
                (
                    "claude-opus-9",
                    "claude-code lists claude-opus-9 but will not run it: pick another of its models",
                ),
            ] {
                let answer = ask(
                    &held.app,
                    "project.open",
                    opening(json!({ "model": model, "read": read })),
                )
                .await;
                assert_eq!(answer, json!({ "ok": false, "error": said }), "{model}");
            }
            let answer = ask(&held.app, "project.open", opening(json!({ "read": read }))).await;
            assert_eq!(answer["ok"], true, "a model it lists: {answer}");
        })
        .await;
}

/// A chief on Muse Code is a harness and a model as any other's (the Muse round of
/// CHF-04 to CHF-08): refused on a model its kept listing does not list, and one it lists
/// goes on to the engine.
#[tokio::test]
async fn a_chief_on_muse_is_its_harness_and_a_model_muse_lists() {
    LocalSet::new()
        .run_until(async {
            let held = asked();
            let listing = ModelListing::read(
                vec![ModelChoice::new("muse-spark-1.3", "Muse Spark 1.3")],
                1_793_000_000_000,
                "the test's",
            );
            let read = held.rig.page.models.record(cf_proto::agents::Harness::Muse, listing);
            let refused = ask(
                &held.app,
                "project.open",
                opening(json!({ "harness": "muse", "model": "muse-gone-9", "read": read })),
            )
            .await;
            assert_eq!(
                refused,
                json!({ "ok": false, "error": "muse does not list muse-gone-9: pick one of its models" })
            );
            // A model it lists passes the daemon's word: what answers next is the engine, whose
            // kit here fakes no Muse window (its launch is the engine's chief_models cases').
            let taken = ask(
                &held.app,
                "project.open",
                opening(json!({ "harness": "muse", "model": "muse-spark-1.3", "read": read })),
            )
            .await;
            assert_eq!(
                taken,
                json!({ "ok": false, "error": "ConsensFlow cannot open muse windows" })
            );
        })
        .await;
}

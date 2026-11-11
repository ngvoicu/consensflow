//! What the operations read of the body the page sent, where Node's recordings do
//! not look: the order of the checks of a body the page would never send, the words
//! of a harness no window opens, and the lanes of a board, which say nothing of saved
//! agents (a member holds its own model: harness-models). They run on the engine's
//! own kit, with a real ledger in a home of the test's.

use cf_ledger::{NewChief, NewMember, NewProject};
use serde_json::Value;

use super::*;

/// A home whose agents file holds `rows`, and the environment that names it.
pub(super) fn home_with(rows: &str) -> (tempfile::TempDir, Env) {
    let home = tempfile::tempdir().unwrap();
    std::fs::write(
        home.path().join("agents.json"),
        format!(r#"{{"schemaVersion":1,"agents":{rows}}}"#),
    )
    .unwrap();
    let folder = home.path().to_str().unwrap().to_owned();
    let env = Env::from_vars([
        ("CONSENSFLOW_HOME", folder.as_str()),
        ("HOME", folder.as_str()),
    ]);
    (home, env)
}

/// A home with these saved agents, where every harness ConsensFlow opens windows of is
/// installed: a stand-in of each on its `PATH`, as a chief's harness must be (CHF-08).
pub(super) fn home_installed(rows: &str) -> (tempfile::TempDir, Env) {
    let (home, _) = home_with(rows);
    let bin = home.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    for name in ["claude", "codex", "pi", "devin", "muse"] {
        cf_harness::testing::fake_executable(&bin.join(name));
    }
    let folder = home.path().to_str().unwrap().to_owned();
    let env = Env::from_vars([
        ("CONSENSFLOW_HOME", folder.as_str()),
        ("HOME", folder.as_str()),
        ("PATH", bin.to_str().unwrap()),
    ]);
    (home, env)
}

/// The page over a home with these saved agents and every harness, on a bridge, asked
/// `body` for `operation`.
async fn asked(rows: &str, operation: &str, body: Value) -> (Value, Rig) {
    let (_home, env) = home_installed(rows);
    let rig = rig_over(env, None);
    let (daemon, app) = bridge_pair_over(&rig.spawn);
    register(&daemon, &rig.page, &rig.spawn);
    let answer = ask(&app, operation, body).await;
    (answer, rig)
}

#[tokio::test]
async fn a_staff_member_on_a_harness_no_window_opens_is_refused_in_the_engines_words_not_the_ledgers(
) {
    LocalSet::new()
        .run_until(async {
            // A member on Kimi, which was dropped.
            let staff = json!([{ "harness": "kimi", "model": "m", "tier": "light", "roles": ["worker"] }]);
            let (answer, rig) = asked(
                "[]",
                "project.open",
                json!({ "directory": "/work/app", "harness": "claude-code", "model": "claude-sonnet-5", "staff": staff }),
            )
            .await;
            // The page refuses a harness ConsensFlow runs no windows of before the
            // ledger is asked: the harness Node's ledger did not know is not what it says.
            assert_eq!(
                answer,
                json!({ "ok": false, "error": "kimi is no harness ConsensFlow runs" })
            );
            assert_eq!(rig.kicks.get(), 0);
            assert!(rig.kit.ledger.borrow().projects().unwrap().is_empty());
        })
        .await;
}

#[tokio::test]
async fn staff_that_is_no_list_is_refused_and_so_is_a_member_whose_roles_are_no_list() {
    LocalSet::new()
        .run_until(async {
            let (answer, _) = asked(
                "[]",
                "project.open",
                json!({ "directory": "/work/app", "harness": "claude-code", "model": "claude-sonnet-5", "staff": "zeus" }),
            )
            .await;
            assert_eq!(
                answer,
                json!({ "ok": false, "error": "staff is a list of members, not zeus" })
            );
            // The ledger's own words for roles, which it reads as Node's did.
            let staff = json!([{ "harness": "claude-code", "model": "claude-sonnet-5", "tier": "standard", "roles": "worker" }]);
            let (answer, rig) = asked(
                "[]",
                "project.open",
                json!({ "directory": "/work/app", "harness": "claude-code", "model": "claude-sonnet-5", "staff": staff }),
            )
            .await;
            assert_eq!(
                answer["error"],
                r#"a member is one or more of worker, frontend, architect, researcher, advisor, reviewer, tester, integrator, designer, not "worker""#
            );
            assert!(rig.kit.ledger.borrow().projects().unwrap().is_empty());
        })
        .await;
}

#[tokio::test]
async fn a_gate_or_roles_that_are_no_good_are_refused_before_the_project_is_looked_for() {
    LocalSet::new()
        .run_until(async {
            let (_home, env) = home_with("[]");
            let rig = rig_over(env, None);
            let (daemon, app) = bridge_pair_over(&rig.spawn);
            register(&daemon, &rig.page, &rig.spawn);
            assert_eq!(
                ask(
                    &app,
                    "project.gate",
                    json!({ "project": 99, "gate": "yes" })
                )
                .await,
                json!({ "ok": false, "error": "human approval is required (true) or not (false)" })
            );
            assert_eq!(
                ask(&app, "project.gate", json!({ "project": 99, "gate": true })).await,
                json!({ "ok": false, "error": "no project 99" })
            );
            assert_eq!(
                ask(
                    &app,
                    "member.roles",
                    json!({ "project": 99, "agent": "zeus", "roles": "worker" })
                )
                .await["error"],
                r#"a member is one or more of worker, frontend, architect, researcher, advisor, reviewer, tester, integrator, designer, not "worker""#
            );
            assert_eq!(
                ask(
                    &app,
                    "member.roles",
                    json!({ "project": 99, "agent": "zeus", "roles": ["worker"] })
                )
                .await["error"],
                "no project 99"
            );
            assert_eq!(
                rig.kicks.get(),
                0,
                "nothing was refused and woke the dispatcher"
            );
        })
        .await;
}

#[tokio::test]
async fn an_id_that_is_no_whole_number_is_refused_by_name() {
    LocalSet::new()
        .run_until(async {
            let (_home, env) = home_with("[]");
            let rig = rig_over(env, None);
            let (daemon, app) = bridge_pair_over(&rig.spawn);
            register(&daemon, &rig.page, &rig.spawn);
            for (operation, body, words) in [
                (
                    "board.get",
                    json!({}),
                    "project is a whole number, not undefined",
                ),
                (
                    "board.get",
                    json!({ "project": 1.5 }),
                    "project is a whole number, not 1.5",
                ),
                (
                    "task.get",
                    json!({ "project": 1, "task": "2" }),
                    "task is a whole number, not 2",
                ),
                (
                    "message.read",
                    json!({ "message": null }),
                    "message is a whole number, not null",
                ),
                (
                    "tasks.delete",
                    json!({ "project": 1, "tasks": [1.5] }),
                    "no task T-1.5 in project 1",
                ),
                (
                    "tasks.delete",
                    json!({ "project": 1, "tasks": "1" }),
                    "tasks is a list of task numbers, not 1",
                ),
            ] {
                assert_eq!(
                    ask(&app, operation, body).await,
                    json!({ "ok": false, "error": words }),
                    "{operation}"
                );
            }
        })
        .await;
}

#[tokio::test]
async fn no_tasks_to_delete_is_a_delete_of_nothing_that_still_wakes_the_dispatcher() {
    LocalSet::new()
        .run_until(async {
            // `[...new Set(undefined)]` is an empty list: Node answered `{tasks: []}`.
            let (answer, rig) = asked("[]", "tasks.delete", json!({ "project": 1 })).await;
            assert_eq!(answer, json!({ "ok": true, "tasks": [] }));
            assert_eq!(rig.kicks.get(), 1);
        })
        .await;
}

#[tokio::test]
async fn an_inbox_is_the_humans_unless_the_page_names_a_handle_in_text() {
    LocalSet::new()
        .run_until(async {
            let (_home, env) = home_with("[]");
            let rig = rig_over(env, None);
            let (daemon, app) = bridge_pair_over(&rig.spawn);
            register(&daemon, &rig.page, &rig.spawn);
            let project = rig
                .kit
                .ledger
                .borrow_mut()
                .create_project(&NewProject {
                    directory: "/work/app".to_owned(),
                    name: "app".to_owned(),
                    chief: NewChief {
                        harness: "claude-code".to_owned(),
                        model: Some("zeus".to_owned()),
                        effort: None,
                        model_name: None,
                    },
                    // A member whose handle is a number's digits.
                    staff: vec![NewMember {
                        model: None,
                        effort: None,
                        model_name: Some("7".to_owned()),
                        harness: "claude-code".to_owned(),
                        designer: false,
                        roles: vec!["worker".to_owned()],
                        tier: "standard".to_owned(),
                    }],
                    gate: false,
                    wake_up: None,
                    specs: None,
                    staff_rules: None,
                    compaction: None,
                    advisor_tool: false,
                    prompt_suggestion: false,
                })
                .unwrap()
                .id;
            let empty = json!({ "ok": true, "messages": [], "total": 0, "shown": 0 });
            // None named is the human; the text of a handle is the member's.
            let asked = |body: Value| ask(&app, "inbox.get", body);
            assert_eq!(asked(json!({ "project": project })).await, empty);
            assert_eq!(
                asked(json!({ "project": project, "participant": "7" })).await,
                empty
            );
            // Only text names a handle (`p.handle === participant`): a number does
            // not, and neither does `null`, which `participant = 'human'` leaves as it is.
            assert_eq!(
                asked(json!({ "project": project, "participant": 7 })).await,
                json!({ "ok": false, "error": format!("7 is not in project {project}") })
            );
            assert_eq!(
                asked(json!({ "project": project, "participant": null })).await,
                json!({ "ok": false, "error": format!("null is not in project {project}") })
            );
        })
        .await;
}

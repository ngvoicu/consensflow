//! What the operations read of the body the page sent and of the saved agents,
//! where Node's recordings do not look: the order of the checks of a body the
//! page would never send, the words of a harness no window opens, and the
//! lanes of a board whose agents are gone. They run on the engine's own kit,
//! with a real ledger and a roster file in a home of the test's.

use cf_ledger::{NewChief, NewMember, NewProject, NewTask};
use serde_json::Value;

use super::scripted::Scripted;
use super::*;
use crate::roster::offerable;

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

/// The page over a home with these saved agents, on a bridge, asked `body` for `operation`.
async fn asked(rows: &str, operation: &str, body: Value) -> (Value, Rig) {
    let (_home, env) = home_with(rows);
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
            // An agent saved for Kimi, which was dropped, stays in the file.
            let rows = r#"[{"id":"legacy","kind":"kimi","model":"m","workTier":"light"}]"#;
            let staff = json!([{ "agent": "legacy", "roles": ["worker"] }]);
            let (answer, rig) = asked(
                rows,
                "project.open",
                json!({ "directory": "/work/app", "agent": "zeus", "staff": staff }),
            )
            .await;
            // The dispatcher asks its adapters before the ledger checks the
            // request: the harness Node's ledger did not know is not what it says.
            assert_eq!(
                answer,
                json!({ "ok": false, "error": "ConsensFlow cannot open kimi windows" })
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
                json!({ "directory": "/work/app", "agent": "zeus", "staff": "zeus" }),
            )
            .await;
            assert_eq!(
                answer,
                json!({ "ok": false, "error": "staff is a list of members, not zeus" })
            );
            // The ledger's own words for roles, which it reads as Node's did.
            let staff = json!([{ "agent": "zeus", "roles": "worker" }]);
            let (answer, rig) = asked(
                "[]",
                "project.open",
                json!({ "directory": "/work/app", "agent": "zeus", "staff": staff }),
            )
            .await;
            assert_eq!(
                answer["error"],
                r#"a member is one or more of worker, advisor, reviewer, designer, not "worker""#
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
                r#"a member is one or more of worker, advisor, reviewer, designer, not "worker""#
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
async fn the_board_marks_the_agent_of_a_member_gone_and_not_that_of_the_session_it_gave_work_to() {
    LocalSet::new()
        .run_until(async {
            let (_home, env) = home_with("[]");
            let rig = rig_over(env, None);
            let (daemon, app) = bridge_pair_over(&rig.spawn);
            register(&daemon, &rig.page, &rig.spawn);
            let project = {
                let mut ledger = rig.kit.ledger.borrow_mut();
                let project = ledger
                    .create_project(&NewProject {
                        directory: "/work/app".to_owned(),
                        name: "app".to_owned(),
                        chief: NewChief {
                            harness: "claude-code".to_owned(),
                            agent: Some("zeus".to_owned()),
                        },
                        // An agent the file does not have: the human removed it.
                        staff: vec![NewMember {
                            agent: "mine".to_owned(),
                            harness: "codex".to_owned(),
                            designer: false,
                            roles: vec!["worker".to_owned()],
                            tier: "standard".to_owned(),
                        }],
                        gate: false,
                    })
                    .unwrap();
                let mine = project
                    .participants
                    .iter()
                    .find(|p| p.handle == "mine")
                    .unwrap()
                    .id;
                ledger
                    .create_task(
                        project.id,
                        &NewTask {
                            from: "chief".to_owned(),
                            pool: Some("worker".to_owned()),
                            tier: Some("standard".to_owned()),
                            body: "Lexer".to_owned(),
                            ..NewTask::default()
                        },
                    )
                    .unwrap();
                ledger.assign_task(project.id, 1, mine).unwrap();
                project.id
            };
            let board =
                ask(&app, "board.get", json!({ "project": project })).await["board"].clone();
            let lanes = board["lanes"].as_array().unwrap();
            let missing: Vec<(&str, &Value)> = lanes
                .iter()
                .map(|lane| {
                    (
                        lane["participant"]["handle"].as_str().unwrap(),
                        &lane["agentMissing"],
                    )
                })
                .collect();
            assert_eq!(
                missing.len(),
                4,
                "the human, the chief, the member and its session"
            );
            assert_eq!(missing[0], ("human", &json!(false)));
            assert_eq!(missing[1], ("chief", &json!(false)));
            assert_eq!(missing[2], ("mine", &json!(true)));
            assert_eq!(
                missing[3].1,
                &json!(false),
                "its session is no member whose agent is gone"
            );
            // What the dispatcher knows of a window it never opened.
            let keys: Vec<&str> = lanes[2]
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect();
            assert_eq!(
                keys,
                [
                    "participant",
                    "tasks",
                    "agentMissing",
                    "activity",
                    "pane",
                    "hidden",
                    "switching",
                    "holding"
                ]
            );
            assert_eq!(lanes[2]["activity"], json!({ "state": "closed" }));
            assert_eq!(lanes[2]["pane"], Value::Null);
        })
        .await;
}

#[tokio::test]
async fn the_agents_file_is_read_at_each_lane_and_not_once_for_the_board() {
    LocalSet::new()
        .run_until(async {
            let row = |id: &str| {
                format!(r#"{{"id":"{id}","kind":"claude-code","model":"m","workTier":"light"}}"#)
            };
            let (home, env) = home_with(&format!("[{},{}]", row("a1"), row("a2")));
            // The human takes a2 away as the first lane's activity is read: the
            // lanes after it find the file as it is then.
            let file = home.path().join("agents.json");
            let only_a1 = format!(r#"{{"schemaVersion":1,"agents":[{}]}}"#, row("a1"));
            let mut engine = Scripted::new();
            engine.reading = Box::new(move |participant| {
                if participant == 1 {
                    std::fs::write(&file, &only_a1).unwrap();
                }
            });
            let rig = rig_over(env, Some(Rc::new(engine)));
            let (daemon, app) = bridge_pair_over(&rig.spawn);
            register(&daemon, &rig.page, &rig.spawn);
            let member = |agent: &str| NewMember {
                agent: agent.to_owned(),
                harness: "claude-code".to_owned(),
                designer: false,
                roles: vec!["worker".to_owned()],
                tier: "light".to_owned(),
            };
            let project = rig
                .kit
                .ledger
                .borrow_mut()
                .create_project(&NewProject {
                    directory: "/work/app".to_owned(),
                    name: "app".to_owned(),
                    chief: NewChief {
                        harness: "claude-code".to_owned(),
                        agent: Some("zeus".to_owned()),
                    },
                    staff: vec![member("a1"), member("a2")],
                    gate: false,
                })
                .unwrap()
                .id;
            let board =
                ask(&app, "board.get", json!({ "project": project })).await["board"].clone();
            let gone: Vec<(&str, bool)> = board["lanes"]
                .as_array()
                .unwrap()
                .iter()
                .map(|lane| {
                    (
                        lane["participant"]["handle"].as_str().unwrap(),
                        lane["agentMissing"].as_bool().unwrap(),
                    )
                })
                .collect();
            assert_eq!(
                gone,
                [
                    ("human", false),
                    ("chief", false),
                    ("a1", false),
                    ("a2", true)
                ]
            );
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
                        agent: Some("zeus".to_owned()),
                    },
                    // A member whose handle is a number's digits.
                    staff: vec![NewMember {
                        agent: "7".to_owned(),
                        harness: "claude-code".to_owned(),
                        designer: false,
                        roles: vec!["worker".to_owned()],
                        tier: "standard".to_owned(),
                    }],
                    gate: false,
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

/// An agent as the roster lists it, written as the page reads it.
fn view(name: &str, harness: &str, hidden: bool, unsupported: bool) -> cf_catalog::AgentView {
    cf_catalog::AgentView {
        name: Some(name.to_owned()),
        harness: Some(harness.to_owned()),
        designer: false,
        model: Some("m".to_owned()),
        work_tier: None,
        effort: None,
        description: None,
        preset: None,
        custom: false,
        profile: cf_catalog::Profile {
            model_key: "m".to_owned(),
            model_label: "M".to_owned(),
            route_label: "R".to_owned(),
            route_note: None,
            work_tier: cf_catalog::WorkTier::Standard,
        },
        unsupported,
        hidden,
    }
}

#[test]
fn an_agent_on_a_harness_not_installed_or_not_run_is_hidden_and_marked_as_javascript_spreads() {
    let agents = [
        view("here", "claude", false, false),
        view("away", "codex", false, false),
        view("kept", "codex", true, false),
        view("dropped", "kimi", false, true),
    ];
    let offered = offerable(&agents, &[cf_catalog::Harness::Codex]).unwrap();
    let written: Vec<String> = offered.iter().map(Value::to_string).collect();
    assert!(!written[0].contains("notInstalled"), "{}", written[0]);
    assert!(!written[0].contains("hidden"), "{}", written[0]);
    for (at, name) in [(1, "away"), (3, "dropped")] {
        assert!(
            written[at].ends_with(r#","hidden":true,"notInstalled":true}"#),
            "{name}: {}",
            written[at]
        );
    }
    // One the preference hid already keeps `hidden` where the roster put it, and
    // `notInstalled` comes last.
    let kept: Vec<&str> = offered[2]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        kept,
        [
            "name",
            "harness",
            "model",
            "profile",
            "hidden",
            "notInstalled"
        ]
    );
}

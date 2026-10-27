//! `tasks.delete` and the keep rule (T-721, 3.0.6): a finished task another task
//! needs goes when its state cannot change (an accepted task, a cancelled one), and
//! stays while the chief can still reopen it (a failed one), the page answering in the
//! ledger's words. Not held to a Node recording: Node refused every needed task. The
//! real ledger stands behind the page.

use cf_ledger::{NewChief, NewMember, NewProject, NewTask};

use super::reading::home_with;
use super::*;

/// The ids of a project at /work/app with zeus its standard worker, and the project.
fn staffed(rig: &Rig) -> (i64, i64) {
    let project = rig
        .kit
        .ledger
        .borrow_mut()
        .create_project(&NewProject {
            directory: "/work/app".to_owned(),
            name: "app".to_owned(),
            chief: NewChief {
                harness: "claude-code".to_owned(),
                agent: Some("apollo".to_owned()),
            },
            staff: vec![NewMember {
                agent: "zeus".to_owned(),
                harness: "claude-code".to_owned(),
                designer: false,
                roles: vec!["worker".to_owned()],
                tier: "standard".to_owned(),
            }],
            gate: false,
            wake_up: None,
            specs: None,
        })
        .unwrap();
    let zeus = project
        .participants
        .iter()
        .find(|participant| participant.handle == "zeus")
        .unwrap()
        .id;
    (project.id, zeus)
}

/// The chief opens a task for a standard worker, waiting for `needs`.
fn open(rig: &Rig, project: i64, body: &str, needs: &[u64]) {
    rig.kit.create_task(
        project,
        NewTask {
            from: "chief".to_owned(),
            pool: Some("worker".to_owned()),
            tier: Some("standard".to_owned()),
            body: body.to_owned(),
            needs: needs.to_vec(),
            ..NewTask::default()
        },
    );
}

/// The numbers of the tasks the board lists.
async fn on_board(app: &Bridge, project: i64) -> Vec<i64> {
    let board = ask(app, "board.get", json!({ "project": project })).await["board"].clone();
    let mut numbers: Vec<i64> = board["open"]
        .as_array()
        .unwrap()
        .iter()
        .chain(
            board["lanes"]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|lane| lane["tasks"].as_array().unwrap().iter()),
        )
        .map(|task| task["number"].as_i64().unwrap())
        .collect();
    numbers.sort_unstable();
    numbers
}

/// A project with T-1 accepted, T-2 failed, and the chief's T-3 waiting for both, on the page.
fn held_for_both(rig: &Rig) -> i64 {
    let (project, zeus) = staffed(rig);
    open(rig, project, "Stage one", &[]);
    open(rig, project, "Stage two", &[]);
    open(rig, project, "Ship", &[1, 2]);
    let mut ledger = rig.kit.ledger.borrow_mut();
    let brief = ledger
        .assign_task(project, 1, zeus)
        .unwrap()
        .message
        .unwrap()
        .id;
    ledger.begin_delivery(brief).unwrap();
    ledger
        .confirm_delivery(brief, Some(&json!({ "item": "i-1" })))
        .unwrap();
    ledger.record_result(project, 1, "One").unwrap();
    ledger.accept_task(project, 1, "chief").unwrap();
    ledger
        .fail_task(project, 2, "its launch never came up")
        .unwrap();
    project
}

#[tokio::test]
async fn the_page_deletes_an_accepted_task_others_need_and_they_still_read_it_accepted() {
    LocalSet::new()
        .run_until(async {
            let (_home, env) = home_with("[]");
            let rig = rig_over(env, None);
            let (daemon, app) = bridge_pair_over(&rig.spawn);
            register(&daemon, &rig.page, &rig.spawn);
            let project = held_for_both(&rig);
            assert_eq!(on_board(&app, project).await, [1, 2, 3]);

            let answer = ask(
                &app,
                "tasks.delete",
                json!({ "project": project, "tasks": [1] }),
            )
            .await;
            assert_eq!(answer["ok"], true, "{answer}");
            assert_eq!(answer["tasks"][0]["number"], 1);
            assert!(answer["tasks"][0]["deletedAt"].is_string(), "{answer}");
            assert_eq!(on_board(&app, project).await, [2, 3]);
            let held = rig.kit.task(project, 3).task;
            assert_eq!(
                held.needs
                    .iter()
                    .map(|need| (need.number, need.state.as_str()))
                    .collect::<Vec<_>>(),
                [(1, "accepted"), (2, "failed")]
            );
            assert_eq!(held.blocked_by, [2]);
        })
        .await;
}

#[tokio::test]
async fn the_page_is_refused_a_failed_task_a_task_needs_in_the_ledgers_words_and_a_batch_with_it_deletes_none(
) {
    LocalSet::new()
        .run_until(async {
            let (_home, env) = home_with("[]");
            let rig = rig_over(env, None);
            let (daemon, app) = bridge_pair_over(&rig.spawn);
            register(&daemon, &rig.page, &rig.spawn);
            let project = held_for_both(&rig);

            let kept = "T-3 still needs T-2, which failed and may be reopened: it stays on the board until T-3 is finished";
            for tasks in [json!([2]), json!([1, 2])] {
                assert_eq!(
                    ask(&app, "tasks.delete", json!({ "project": project, "tasks": tasks })).await,
                    json!({ "ok": false, "error": kept })
                );
            }
            assert_eq!(on_board(&app, project).await, [1, 2, 3], "none went");
        })
        .await;
}

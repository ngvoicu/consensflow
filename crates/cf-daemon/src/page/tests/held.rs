//! A lane says `held` only while its task waits on a background job or agent that its
//! harness wakes its agent for (A2, turn-end's smallest rule of 2026-10-21), and no other
//! lane, and no other time, has the key; and the human's Finish (`task.finish`) takes what
//! the worker of any working task said last as its result, ending nothing (the owner's
//! decision of 2026-10-18). Not held to a Node recording: Node's board had neither. The
//! real dispatcher stands behind the page.

use cf_harness::records::Pending;
use cf_ledger::NewTask;

use super::reading::home_with;
use super::*;

/// What the board says of the lane of `handle`, asked as the app asks.
async fn lane(app: &Bridge, project: i64, handle: &str) -> Value {
    let board = ask(app, "board.get", json!({ "project": project })).await["board"].clone();
    board["lanes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|lane| lane["participant"]["handle"] == handle)
        .unwrap_or_else(|| panic!("no lane of @{handle}"))
        .clone()
}

/// What the app asks and gets back, the engine's own work run meanwhile as the
/// daemon's executor runs it (the kit's is run by whoever drives the test).
async fn asked(app: &Bridge, kit: &Context, operation: &str, body: Value) -> Value {
    let answer = ask(app, operation, body);
    tokio::pin!(answer);
    loop {
        tokio::select! {
            answer = &mut answer => return answer,
            () = tokio::task::yield_now() => kit.settle(),
        }
    }
}

/// A task for the standard workers, at work in zeus's session's window.
fn serving_the_docs(context: &Context) -> (i64, String) {
    let project = context.with_tiers(&["zeus"]).id;
    context.create_task(
        project,
        NewTask {
            from: "chief".to_owned(),
            pool: Some("worker".to_owned()),
            tier: Some("standard".to_owned()),
            body: "Serve the docs".to_owned(),
            ..NewTask::default()
        },
    );
    context.pass().unwrap();
    context.pass().unwrap();
    let session = context.task(project, 1).task.assignee.unwrap();
    (project, session)
}

#[tokio::test]
async fn a_lane_says_what_its_task_waits_on_while_it_does_and_finish_ends_the_wait() {
    LocalSet::new()
        .run_until(async {
            let (_home, env) = home_with("[]");
            let rig = rig_over(env, None);
            let (daemon, app) = bridge_pair_over(&rig.spawn);
            register(&daemon, &rig.page, &rig.spawn);
            let context = &rig.kit;
            let (project, session) = serving_the_docs(context);
            assert_eq!(lane(&app, project, &session).await.get("held"), None);

            // Its turn ends with a job that never ends running: the lane says so, after the
            // keys every lane has, and no other lane does.
            context.adapter.answer_leaving(
                "zeus",
                "Serving on 8000",
                Pending {
                    jobs: 1,
                    ..Pending::default()
                },
            );
            context.pass().unwrap();
            let zeus = lane(&app, project, &session).await;
            assert_eq!(zeus["held"], json!({ "task": 1, "jobs": 1, "agents": 0 }));
            let keys: Vec<&str> = zeus
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect();
            assert_eq!(keys.last(), Some(&"held"), "{keys:?}");
            for other in ["human", "chief"] {
                assert_eq!(
                    lane(&app, project, other).await.get("held"),
                    None,
                    "{other}"
                );
            }
            let kicked = rig.kicks.get();

            // The human's Finish: the task done with what the worker said last, the
            // dispatcher woken, nothing ended.
            let said = asked(
                &app,
                context,
                "task.finish",
                json!({ "project": project, "task": 1 }),
            )
            .await;
            assert_eq!(said["ok"], true, "{said}");
            assert_eq!(said["task"]["number"], 1);
            assert_eq!(said["task"]["state"], "done");
            assert_eq!((&said["jobs"], &said["agents"]), (&json!(1), &json!(0)));
            assert_eq!(rig.kicks.get(), kicked + 1);
            let result = context
                .task(project, 1)
                .messages
                .into_iter()
                .find(|message| message.kind == "result")
                .expect("a result");
            assert!(
                result.body.starts_with(
                    "Serving on 8000\n\n(ConsensFlow: Finished at the human's word while"
                ),
                "{}",
                result.body
            );
            context.pass().unwrap();
            assert_eq!(lane(&app, project, &session).await.get("held"), None);

            // Over, the task is no one to finish, and the page is told why; nothing is woken.
            let kicked = rig.kicks.get();
            let said = asked(
                &app,
                context,
                "task.finish",
                json!({ "project": project, "task": 1 }),
            )
            .await;
            assert_eq!(
                said,
                json!({ "ok": false, "error": "cannot finish T-1: it is done" })
            );
            assert_eq!(rig.kicks.get(), kicked);
        })
        .await;
}

#[tokio::test]
async fn finish_is_refused_for_a_task_its_worker_has_not_answered_and_for_one_there_is_none_of() {
    LocalSet::new()
        .run_until(async {
            let (_home, env) = home_with("[]");
            let rig = rig_over(env, None);
            let (daemon, app) = bridge_pair_over(&rig.spawn);
            register(&daemon, &rig.page, &rig.spawn);
            let context = &rig.kit;
            let (project, session) = serving_the_docs(context);
            context.adapter.busy("zeus");
            context.pass().unwrap();

            let said = asked(
                &app,
                context,
                "task.finish",
                json!({ "project": project, "task": 1 }),
            )
            .await;
            assert_eq!(
                said,
                json!({ "ok": false, "error": format!("cannot finish T-1: @{session} has written no complete answer since its latest message about it; wait for one, or cancel it") })
            );
            let said = asked(
                &app,
                context,
                "task.finish",
                json!({ "project": project, "task": 9 }),
            )
            .await;
            assert_eq!(
                said,
                json!({ "ok": false, "error": "no task T-9 in this project" })
            );
            assert_eq!(rig.kicks.get(), 0, "a refusal wakes nothing");
            assert_eq!(context.task(project, 1).task.state, "working");
        })
        .await;
}

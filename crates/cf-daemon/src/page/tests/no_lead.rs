//! The human's own cancel, pause, resume and finish from the page, on a task the chief
//! gave and on one the human gave, in a project with no lead (the leads design, step 0):
//! the page passes the human as the actor, and the board takes it whoever the task's
//! requester is. A later step checks who may decide a task on the board; these stay as
//! they are. The real dispatcher stands behind the page.

use cf_harness::records::Pending;
use cf_ledger::NewTask;

use super::reading::home_with;
use super::*;

/// What the app asks and gets back, the engine's own work run meanwhile as the daemon's
/// executor runs it.
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

/// A task `requester` gave for the standard workers, at work in zeus's session's window.
fn at_work(context: &Context, requester: &str) -> i64 {
    let project = context.with_tiers(&["zeus"]).id;
    context.create_task(
        project,
        NewTask {
            from: requester.to_owned(),
            pool: Some("worker".to_owned()),
            tier: Some("standard".to_owned()),
            body: "Serve the docs".to_owned(),
            ..NewTask::default()
        },
    );
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.task(project, 1).task.state, "working");
    project
}

#[tokio::test]
async fn the_human_cancels_a_task_at_work_whoever_gave_it() {
    for requester in ["chief", "human"] {
        LocalSet::new()
            .run_until(async {
                let (_home, env) = home_with("[]");
                let rig = rig_over(env, None);
                let (daemon, app) = bridge_pair_over(&rig.spawn);
                register(&daemon, &rig.page, &rig.spawn);
                let context = &rig.kit;
                let project = at_work(context, requester);
                let said = asked(
                    &app,
                    context,
                    "task.cancel",
                    json!({ "project": project, "task": 1 }),
                )
                .await;
                assert_eq!(said["ok"], true, "{requester}: {said}");
                assert_eq!(said["task"]["state"], "cancelled", "{requester}");
                assert_eq!(said["task"]["requester"], requester);
                assert_eq!(context.task(project, 1).task.state, "cancelled");
                assert_eq!(rig.kicks.get(), 1, "the dispatcher is woken");
            })
            .await;
    }
}

#[tokio::test]
async fn the_human_pauses_and_resumes_a_task_at_work_whoever_gave_it() {
    for requester in ["chief", "human"] {
        LocalSet::new()
            .run_until(async {
                let (_home, env) = home_with("[]");
                let rig = rig_over(env, None);
                let (daemon, app) = bridge_pair_over(&rig.spawn);
                register(&daemon, &rig.page, &rig.spawn);
                let context = &rig.kit;
                let project = at_work(context, requester);
                let paused = asked(
                    &app,
                    context,
                    "task.pause",
                    json!({ "project": project, "task": 1 }),
                )
                .await;
                assert_eq!(paused["ok"], true, "{requester}: {paused}");
                assert_eq!(paused["task"]["state"], "paused", "{requester}");
                let resumed = asked(
                    &app,
                    context,
                    "task.resume",
                    json!({ "project": project, "task": 1 }),
                )
                .await;
                assert_eq!(resumed["ok"], true, "{requester}: {resumed}");
                // The words that resume it wait for its window: queued until they are in.
                assert_eq!(resumed["task"]["state"], "queued", "{requester}: {resumed}");
                assert_eq!(resumed["task"]["requester"], requester);
            })
            .await;
    }
}

#[tokio::test]
async fn the_human_finishes_a_task_the_human_gave_with_its_workers_last_words() {
    LocalSet::new()
        .run_until(async {
            let (_home, env) = home_with("[]");
            let rig = rig_over(env, None);
            let (daemon, app) = bridge_pair_over(&rig.spawn);
            register(&daemon, &rig.page, &rig.spawn);
            let context = &rig.kit;
            let project = at_work(context, "human");
            context.adapter.answer_leaving(
                "zeus",
                "Serving on 8000",
                Pending {
                    jobs: 1,
                    ..Pending::default()
                },
            );
            context.pass().unwrap();
            let said = asked(
                &app,
                context,
                "task.finish",
                json!({ "project": project, "task": 1 }),
            )
            .await;
            assert_eq!(said["ok"], true, "{said}");
            assert_eq!(said["task"]["state"], "done");
            assert_eq!(said["task"]["requester"], "human");
            assert_eq!(said["jobs"], 1);
            let result = context
                .task(project, 1)
                .messages
                .into_iter()
                .find(|message| message.kind == "result")
                .expect("a result");
            assert_eq!(result.recipient, "human");
        })
        .await;
}

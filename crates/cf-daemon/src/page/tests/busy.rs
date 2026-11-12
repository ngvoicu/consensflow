//! A lane says `busy` only while its member is out because its model is busy for a moment
//! (T-1112), and no other lane, and no other time, has the key: the page cannot tell a busy
//! hold from a quota by the time the member is out until, and says the first has no button
//! (T-1357). The member's lane and its session's say it alike; a member out of quota, or a
//! member back, has none. Not held to a Node recording: Node's board never had it. The real
//! dispatcher stands behind the page.

use cf_harness::records::Quota;
use cf_harness::seams::Time;
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

#[tokio::test]
async fn a_lane_has_busy_while_its_members_model_is_busy_and_in_no_other_lane_or_time() {
    LocalSet::new()
        .run_until(async {
            let (_home, env) = home_with("[]");
            let rig = rig_over(env, None);
            let (daemon, app) = bridge_pair_over(&rig.spawn);
            register(&daemon, &rig.page, &rig.spawn);
            let context = &rig.kit;
            let project = context.with_staff(&["zeus", "diana"]).id;
            // Given by tier, so that a session of zeus's takes it.
            context.create_task(
                project,
                NewTask {
                    from: "chief".to_owned(),
                    pool: Some("worker".to_owned()),
                    tier: Some("standard".to_owned()),
                    body: "Parser".to_owned(),
                    ..NewTask::default()
                },
            );
            context.pass().unwrap();
            context.pass().unwrap();
            let session = context
                .task(project, 1)
                .task
                .assignee
                .expect("T-1 given to a session");
            let member = session.split('-').next().unwrap().to_owned();
            let other = if member == "zeus" { "diana" } else { "zeus" };
            assert_eq!(lane(&app, project, &member).await.get("busy"), None);
            // The human has its window open, so the session has a lane beside its member's.
            context.open_window(project, &session).unwrap();

            // Its model is busy for a moment: zeus is out a minute, and says why.
            let tried_at = cf_base::time::iso(context.time.wall_ms() + 60_000);
            context.adapter.refuse(
                &member,
                Quota::Busy {
                    at: Some(cf_base::time::iso(context.time.wall_ms())),
                    because: "Selected model is at capacity.".to_owned(),
                },
            );
            context.pass().unwrap();
            context.pass().unwrap();
            for alike in [member.as_str(), session.as_str()] {
                let said = lane(&app, project, alike).await;
                assert_eq!(said["busy"], json!(tried_at), "@{alike}: {said}");
            }
            for other in ["human", "chief", other] {
                assert_eq!(
                    lane(&app, project, other).await.get("busy"),
                    None,
                    "@{other}"
                );
            }

            // The minute is out and the task is tried again: the key goes.
            context.advance(61_000);
            context.pass().unwrap();
            context.pass().unwrap();
            assert_eq!(lane(&app, project, &member).await.get("busy"), None);
        })
        .await;
}

#[tokio::test]
async fn a_lane_of_a_member_out_of_quota_has_no_busy() {
    LocalSet::new()
        .run_until(async {
            let (_home, env) = home_with("[]");
            let rig = rig_over(env, None);
            let (daemon, app) = bridge_pair_over(&rig.spawn);
            register(&daemon, &rig.page, &rig.spawn);
            let context = &rig.kit;
            let project = context.with_staff(&["zeus"]).id;
            let zeus = context.id(project, "zeus");
            let until = cf_base::time::iso(context.time.wall_ms() + 3 * 3_600_000);
            context
                .ledger
                .borrow_mut()
                .mark_out(zeus, &until, "out of quota")
                .unwrap();
            let said = lane(&app, project, "zeus").await;
            assert_eq!(said["participant"]["outUntil"], json!(until));
            assert_eq!(said.get("busy"), None, "{said}");
        })
        .await;
}

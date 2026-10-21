//! A lane says `unstopped` only while its window ignores the stop of its
//! task and the host would not close it, and no other lane, and no other
//! time, has the key: a window closed for it has none. Not held to a Node
//! recording: Node's board never had it. The real dispatcher stands behind the
//! page, with a harness that does not stop.

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
async fn a_lane_has_unstopped_while_its_window_ignores_a_stop_and_in_no_other_lane_or_time() {
    LocalSet::new()
        .run_until(async {
            let (_home, env) = home_with("[]");
            let rig = rig_over(env, None);
            let (daemon, app) = bridge_pair_over(&rig.spawn);
            register(&daemon, &rig.page, &rig.spawn);
            let context = &rig.kit;
            let project = context.with_staff(&["zeus"]).id;
            context.give(project, "zeus", "Parser");
            context.pass().unwrap();
            context.pass().unwrap();
            assert_eq!(
                lane(&app, project, "zeus").await.get("unstopped"),
                None,
                "a window at work on its task has nothing it ignores"
            );

            // The harness takes no key: three rounds, and the host will not close the window.
            context.adapter.busy("zeus");
            context.pause_task(project, 1);
            for _ in 0..3 {
                context.pass().unwrap();
                context.advance(3_100);
            }
            context.host.refuse_kills.set(true);
            context.pass().unwrap();
            let zeus = lane(&app, project, "zeus").await;
            assert_eq!(zeus["unstopped"], json!({ "task": 1, "rounds": 3 }));
            let keys: Vec<&str> = zeus
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect();
            assert_eq!(
                keys.last(),
                Some(&"unstopped"),
                "after the keys every lane has: {keys:?}"
            );
            for other in ["human", "chief"] {
                assert_eq!(
                    lane(&app, project, other).await.get("unstopped"),
                    None,
                    "@{other}"
                );
            }

            // Its turn ends: the stop is paid, and the key goes.
            context.adapter.answer("zeus", "Where I was");
            context.pass().unwrap();
            assert_eq!(lane(&app, project, "zeus").await.get("unstopped"), None);
        })
        .await;
}

#[tokio::test]
async fn a_lane_whose_window_was_closed_for_a_stop_it_ignored_has_no_unstopped() {
    LocalSet::new()
        .run_until(async {
            let (_home, env) = home_with("[]");
            let rig = rig_over(env, None);
            let (daemon, app) = bridge_pair_over(&rig.spawn);
            register(&daemon, &rig.page, &rig.spawn);
            let context = &rig.kit;
            let project = context.with_staff(&["zeus"]).id;
            context.give(project, "zeus", "Parser");
            context.pass().unwrap();
            context.pass().unwrap();
            context.adapter.busy("zeus");
            context.pause_task(project, 1);
            for _ in 0..4 {
                context.pass().unwrap();
                context.advance(3_100);
            }
            let zeus = lane(&app, project, "zeus").await;
            assert_eq!(zeus["activity"]["state"], "closed", "{zeus}");
            assert_eq!(zeus.get("unstopped"), None, "{zeus}");
        })
        .await;
}

//! The human's Start meeting and Stop meeting on the page (`project.meeting`, 3.0.7
//! chief-meeting): the project says when its meeting began, the board lists what it
//! holds, and the dispatcher is woken, so the Stop lets what was held go at once. A
//! closed project's meeting is refused, and so is an `on` that is no flag, and
//! nothing is woken. Not held to a Node recording. The real dispatcher stands behind
//! the page.

use super::reading::home_with;
use super::*;

struct Meeting {
    _home: tempfile::TempDir,
    _daemon: Bridge,
    app: Bridge,
    rig: Rig,
    project: i64,
}

fn meeting() -> Meeting {
    let (home, env) = home_with("[]");
    let rig = rig_over(env, None);
    let (daemon, app) = bridge_pair_over(&rig.spawn);
    register(&daemon, &rig.page, &rig.spawn);
    let project = rig.kit.with_staff(&["zeus"]).id;
    Meeting {
        _home: home,
        _daemon: daemon,
        app,
        rig,
        project,
    }
}

#[tokio::test]
async fn a_meeting_starts_and_stops_on_the_page_and_the_board_lists_what_it_holds() {
    LocalSet::new()
        .run_until(async {
            let meeting = meeting();
            let context = &meeting.rig.kit;
            let started = ask(
                &meeting.app,
                "project.meeting",
                json!({ "project": meeting.project, "on": true }),
            )
            .await;
            assert_eq!(started["ok"], true, "{started}");
            assert!(started["project"]["meeting"].is_string(), "{started}");
            assert_eq!(meeting.rig.kicks.get(), 1);
            let note = context.note(meeting.project, "zeus", "chief", "Mind the tests");
            let board = ask(
                &meeting.app,
                "board.get",
                json!({ "project": meeting.project }),
            )
            .await;
            assert_eq!(
                board["board"]["project"]["meeting"],
                started["project"]["meeting"]
            );
            assert_eq!(board["board"]["held"][0]["id"], note.id, "{board}");
            assert_eq!(board["board"]["held"][0]["sender"], "zeus");

            let stopped = ask(
                &meeting.app,
                "project.meeting",
                json!({ "project": meeting.project, "on": false }),
            )
            .await;
            assert_eq!(stopped["ok"], true, "{stopped}");
            assert!(stopped["project"].get("meeting").is_none(), "{stopped}");
            assert_eq!(meeting.rig.kicks.get(), 2, "the Stop wakes the dispatcher");
            let board = ask(
                &meeting.app,
                "board.get",
                json!({ "project": meeting.project }),
            )
            .await;
            assert!(board["board"].get("held").is_none(), "{board}");
        })
        .await;
}

#[tokio::test]
async fn a_closed_projects_meeting_and_an_on_that_is_no_flag_are_refused_and_wake_nothing() {
    LocalSet::new()
        .run_until(async {
            let meeting = meeting();
            let context = &meeting.rig.kit;
            let refused = ask(
                &meeting.app,
                "project.meeting",
                json!({ "project": meeting.project, "on": "yes" }),
            )
            .await;
            assert_eq!(
                refused,
                json!({ "ok": false, "error": "a meeting with the chief is on (true) or off (false)" })
            );
            context.close_project(meeting.project).unwrap();
            let refused = ask(
                &meeting.app,
                "project.meeting",
                json!({ "project": meeting.project, "on": true }),
            )
            .await;
            assert_eq!(
                refused,
                json!({ "ok": false, "error": "app is closed: resume it first" })
            );
            assert_eq!(meeting.rig.kicks.get(), 0, "nothing succeeded, nothing is woken");
        })
        .await;
}

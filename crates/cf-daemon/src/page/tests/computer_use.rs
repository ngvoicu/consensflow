//! A project's computer use on the page (T-540): what Project settings' row is
//! drawn from, and the System Settings panes it opens. Not held to a Node
//! recording: these operations are this build's own.

use cf_engine::seams::ScreenPermissions;

use super::*;
use crate::computer_use::Pane;

/// The page on a bridge, with the test context's machine.
fn held() -> (Bridge, Bridge, Rig) {
    let rig = rig();
    let (daemon, app) = bridge_pair_over(&rig.spawn);
    register(&daemon, &rig.page, &rig.spawn);
    (daemon, app, rig)
}

#[tokio::test]
async fn the_row_is_told_the_system_the_grants_and_claudes_word_for_the_projects_folder() {
    LocalSet::new()
        .run_until(async {
            let (_daemon, app, rig) = held();
            let project = rig.kit.with_staff(&["zeus"]).id;
            rig.kit
                .computer_use
                .permissions
                .set(Some(ScreenPermissions {
                    accessibility: true,
                    screen_recording: false,
                }));
            let answer = ask(&app, "computer_use.get", json!({ "project": project })).await;
            assert_eq!(answer["ok"], true, "{answer}");
            let system = if cfg!(target_os = "macos") {
                "macos"
            } else if cfg!(windows) {
                "windows"
            } else {
                "other"
            };
            assert_eq!(answer["system"], system);
            assert_eq!(
                answer["permissions"],
                json!({ "accessibility": true, "screenRecording": false })
            );
            assert_eq!(
                answer["harnesses"]["claude-code"],
                json!({ "on": false, "folder": "/work/app" })
            );
            rig.kit
                .computer_use
                .on_in
                .borrow_mut()
                .insert("/work/app".to_owned());
            let answer = ask(&app, "computer_use.get", json!({ "project": project })).await;
            assert_eq!(answer["harnesses"]["claude-code"]["on"], true);
            assert_eq!(rig.kicks.get(), 0, "a read wakes nothing");
        })
        .await;
}

#[tokio::test]
async fn a_system_with_no_grants_says_none_and_an_unknown_project_is_refused() {
    LocalSet::new()
        .run_until(async {
            let (_daemon, app, rig) = held();
            let project = rig.kit.with_staff(&["zeus"]).id;
            let answer = ask(&app, "computer_use.get", json!({ "project": project })).await;
            assert_eq!(answer["permissions"], Value::Null, "{answer}");
            let refused = ask(&app, "computer_use.get", json!({ "project": 99 })).await;
            assert_eq!(refused["ok"], false, "{refused}");
        })
        .await;
}

#[tokio::test]
async fn the_row_is_told_whose_grants_they_are_where_another_app_started_the_daemon() {
    LocalSet::new()
        .run_until(async {
            for (app, said) in [
                (Some("com.apple.Terminal"), json!("com.apple.Terminal")),
                (Some("dev.ngvoicu.consensflow"), Value::Null),
                (None, Value::Null),
            ] {
                let mut vars = vec![("HOME", "/home/owner")];
                vars.extend(app.map(|app| ("__CFBundleIdentifier", app)));
                let rig = rig_over(Env::from_vars(vars), None);
                let (daemon, app_end) = bridge_pair_over(&rig.spawn);
                register(&daemon, &rig.page, &rig.spawn);
                let project = rig.kit.with_staff(&["zeus"]).id;
                let answer = ask(&app_end, "computer_use.get", json!({ "project": project })).await;
                assert_eq!(answer["grantsOf"], said, "{app:?}: {answer}");
            }
        })
        .await;
}

#[tokio::test]
async fn open_opens_only_the_two_named_panes() {
    LocalSet::new()
        .run_until(async {
            let (_daemon, app, rig) = held();
            for pane in ["accessibility", "screen-recording"] {
                let answer = ask(&app, "computer_use.open", json!({ "pane": pane })).await;
                assert_eq!(answer["ok"], true, "{answer}");
                assert_eq!(answer["opened"], pane);
            }
            for other in [
                json!("x-apple.systempreferences:evil"),
                json!(""),
                json!(3),
                Value::Null,
            ] {
                let answer = ask(&app, "computer_use.open", json!({ "pane": other })).await;
                assert_eq!(answer["ok"], false, "{other}: {answer}");
            }
            assert_eq!(
                *rig.opened.borrow(),
                [Pane::Accessibility, Pane::ScreenRecording]
            );
        })
        .await;
}

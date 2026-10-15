//! The human's wake-up calls on the page: Project settings reads a project's
//! (`wakeup.get`) and saves them (`wakeup.set`, a number and a unit and the
//! words); a save the ledger refuses says why and wakes nothing, one that holds
//! wakes the dispatcher, and no token of a window reaches either. New project
//! starts with the calls a project has that never set them, and the words of
//! those are the ledger's. Not held to a Node recording: Node's page had no
//! such operation. The real dispatcher stands behind the page.

use cf_ledger::DEFAULT_WAKE_UP_TEXT;
use hyper::Method;

use super::reading::home_with;
use super::*;
use crate::testing::{request, said, scene};

/// The page over a project whose chief and zeus the engine's kit opened.
struct Asked {
    _home: tempfile::TempDir,
    _daemon: Bridge,
    app: Bridge,
    rig: Rig,
    project: i64,
}

fn asked() -> Asked {
    let (home, env) = home_with("[]");
    let rig = rig_over(env, None);
    let (daemon, app) = bridge_pair_over(&rig.spawn);
    register(&daemon, &rig.page, &rig.spawn);
    let project = rig.kit.with_staff(&["zeus"]).id;
    Asked {
        _home: home,
        _daemon: daemon,
        app,
        rig,
        project,
    }
}

fn saved(project: i64, every: i64, unit: &str) -> Value {
    json!({ "project": project, "on": true, "every": every, "unit": unit })
}

#[tokio::test]
async fn a_project_that_never_set_its_calls_reads_them_off_an_hour_apart_with_the_default_words() {
    LocalSet::new()
        .run_until(async {
            let page = asked();
            let read = ask(&page.app, "wakeup.get", json!({ "project": page.project })).await;
            assert_eq!(
                read,
                json!({
                    "ok": true,
                    "wakeUp": {
                        "on": false,
                        "everyMs": 3_600_000,
                        "every": 1,
                        "unit": "hours",
                        "text": DEFAULT_WAKE_UP_TEXT,
                        "defaultText": true,
                    },
                })
            );
            assert_eq!(page.rig.kicks.get(), 0, "a read wakes nothing");
        })
        .await;
}

#[tokio::test]
async fn the_human_saves_a_number_and_a_unit_and_the_words_and_reads_them_back_as_saved() {
    LocalSet::new()
        .run_until(async {
            let page = asked();
            let words = "Anything left on the board? — “carry on”";
            let mut body = saved(page.project, 90, "minutes");
            body["text"] = json!(words);
            let said = ask(&page.app, "wakeup.set", body).await;
            let expected = json!({
                "on": true,
                "everyMs": 5_400_000,
                "every": 90,
                "unit": "minutes",
                "text": words,
                "defaultText": false,
            });
            assert_eq!(said, json!({ "ok": true, "wakeUp": expected }));
            assert_eq!(
                page.rig.kicks.get(),
                1,
                "a save applies at once: the dispatcher is woken"
            );
            let read = ask(&page.app, "wakeup.get", json!({ "project": page.project })).await;
            assert_eq!(read, json!({ "ok": true, "wakeUp": expected }));
            // The same calls, said in the unit that holds them whole.
            for (every, unit, ms, then, in_unit) in [
                (120, "minutes", 7_200_000, 2, "hours"),
                (24, "hours", 86_400_000, 1, "days"),
                (3, "days", 259_200_000, 3, "days"),
                (1, "minutes", 60_000, 1, "minutes"),
            ] {
                let saved = ask(&page.app, "wakeup.set", saved(page.project, every, unit)).await;
                assert_eq!(saved["wakeUp"]["everyMs"], ms, "{every} {unit}");
                assert_eq!(
                    (
                        saved["wakeUp"]["every"].clone(),
                        saved["wakeUp"]["unit"].clone()
                    ),
                    (json!(then), json!(in_unit)),
                    "{every} {unit}"
                );
            }
            // The project the board shows is as it was: the setting is no part of it.
            let board = ask(&page.app, "board.get", json!({ "project": page.project })).await;
            assert_eq!(board["board"]["project"].get("wakeUp"), None, "{board}");
        })
        .await;
}

#[tokio::test]
async fn words_the_human_left_out_or_never_changed_are_the_default() {
    LocalSet::new()
        .run_until(async {
            let page = asked();
            for text in [json!(null), json!(DEFAULT_WAKE_UP_TEXT)] {
                let mut body = saved(page.project, 1, "hours");
                body["text"] = text;
                let said = ask(&page.app, "wakeup.set", body).await;
                assert_eq!(said["wakeUp"]["text"], DEFAULT_WAKE_UP_TEXT);
                assert_eq!(said["wakeUp"]["defaultText"], true);
            }
            let said = ask(&page.app, "wakeup.set", saved(page.project, 1, "hours")).await;
            assert_eq!(said["wakeUp"]["defaultText"], true, "no text sent at all");
        })
        .await;
}

#[tokio::test]
async fn a_save_that_is_under_a_minute_or_no_whole_number_or_no_such_unit_or_says_nothing_is_refused_in_words(
) {
    LocalSet::new()
        .run_until(async {
            let page = asked();
            let project = page.project;
            let with = |field: &str, value: Value| {
                let mut body = saved(project, 5, "minutes");
                body[field] = value;
                body
            };
            let mut cases = vec![
                (with("every", json!(0)), "a wake-up comes every whole number of minutes, hours or days, not 0 \"minutes\""),
                (with("every", json!(-5)), "a wake-up comes every whole number of minutes, hours or days, not -5 \"minutes\""),
                (with("every", json!(0.5)), "a wake-up comes every whole number of minutes, hours or days, not 0.5 \"minutes\""),
                (with("every", json!("5")), "a wake-up comes every whole number of minutes, hours or days, not \"5\" \"minutes\""),
                (with("unit", json!("seconds")), "a wake-up comes every whole number of minutes, hours or days, not 5 \"seconds\""),
                (with("unit", json!("Hours")), "a wake-up comes every whole number of minutes, hours or days, not 5 \"Hours\""),
                (with("unit", json!(null)), "a wake-up comes every whole number of minutes, hours or days, not 5 null"),
                (with("on", json!("yes")), "wake-up calls are on (true) or off (false)"),
                (with("on", json!(null)), "wake-up calls are on (true) or off (false)"),
                (with("text", json!("   ")), "the wake-up text must be text, not empty, at most 1000000 characters"),
                (with("text", json!("")), "the wake-up text must be text, not empty, at most 1000000 characters"),
                (with("text", json!(7)), "the wake-up text must be text, not 7"),
                (with("project", json!(9_999)), "no project 9999"),
                (with("project", json!("1")), "project is a whole number, not 1"),
                // The request is checked before the project is looked for.
                (saved(9_999, 0, "minutes"), "a wake-up comes every whole number of minutes, hours or days, not 0 \"minutes\""),
            ];
            let mut unit_missing = saved(project, 5, "minutes");
            unit_missing.as_object_mut().unwrap().remove("unit");
            cases.push((unit_missing, "a wake-up comes every whole number of minutes, hours or days, not 5 undefined"));
            // Days too many to count in milliseconds.
            cases.push((saved(project, 9_007_199_254_740_991, "days"), "9007199254740991 days is longer than a wake-up can wait"));
            for (body, words) in cases {
                assert_eq!(
                    ask(&page.app, "wakeup.set", body.clone()).await,
                    json!({ "ok": false, "error": words }),
                    "{body}"
                );
            }
            assert_eq!(page.rig.kicks.get(), 0, "nothing was saved, nothing is woken");
            let read = ask(&page.app, "wakeup.get", json!({ "project": project })).await;
            assert_eq!(read["wakeUp"]["on"], false, "a refused save changed nothing");
            // Asked of a project that is not there, or with no project at all.
            for operation in ["wakeup.get", "wakeup.set"] {
                let gone = ask(&page.app, operation, saved(9_999, 5, "minutes")).await;
                assert_eq!(gone, json!({ "ok": false, "error": "no project 9999" }), "{operation}");
                let none = ask(&page.app, operation, json!({})).await;
                assert_eq!(none["ok"], false, "{operation}");
            }
        })
        .await;
}

#[tokio::test]
async fn a_save_is_in_the_ledger_at_once_and_each_one_wakes_the_dispatcher_so_nothing_waits_for_a_relaunch(
) {
    LocalSet::new()
        .run_until(async {
            let page = asked();
            ask(&page.app, "wakeup.set", saved(page.project, 1, "minutes")).await;
            let kit = &page.rig.kit;
            let setting = kit.ledger.borrow().wake_up(page.project).unwrap();
            assert!(setting.on);
            assert_eq!(setting.every_ms, 60_000);
            // The calls turned off by the dialog's own call.
            let mut off = saved(page.project, 1, "minutes");
            off["on"] = json!(false);
            ask(&page.app, "wakeup.set", off).await;
            assert!(!kit.ledger.borrow().wake_up(page.project).unwrap().on);
            assert_eq!(page.rig.kicks.get(), 2);
        })
        .await;
}

/// New project has no project to read the calls of, so its section of them
/// starts with a copy of what a project has that never set them: off, how often,
/// and the words. One that is not the ledger's would be shown as the default and
/// sent as the human's own once it was unlike what the section opened with, and
/// kept as such. The copy is in the page's own file, and is held to what the
/// daemon says of a project that never set its calls.
#[tokio::test]
async fn new_project_starts_the_wake_up_calls_as_a_project_that_never_set_them_has_them() {
    LocalSet::new()
        .run_until(async {
            let page = asked();
            let read = ask(&page.app, "wakeup.get", json!({ "project": page.project })).await;
            let fresh = &read["wakeUp"];
            let source = include_str!("../../../../../app/ui/core/wake-up.js");
            let words = format!("DEFAULT_WAKE_UP_WORDS =\n  '{DEFAULT_WAKE_UP_TEXT}'\n");
            assert!(
                source.contains(&words),
                "the page's constant is not the ledger's default words: {DEFAULT_WAKE_UP_TEXT}"
            );
            let unset = format!(
                "const UNSET = {{ on: {}, every: {}, unit: '{}', text: DEFAULT_WAKE_UP_WORDS }}",
                fresh["on"],
                fresh["every"],
                fresh["unit"].as_str().unwrap_or_default()
            );
            assert!(
                source.contains(&unset),
                "the page does not start the calls as a fresh project has them ({fresh}): {unset}"
            );
        })
        .await;
}

/// The human's own: no window's token, the chief's or a member's, has a route
/// that reads or changes a project's wake-up calls. The routes of the agents'
/// API are a frozen table; these paths are none of them.
#[tokio::test]
async fn a_window_s_token_the_chief_s_or_a_member_s_reaches_no_route_that_changes_them() {
    let scene = scene();
    let screens = crate::screens::testing::inert();
    let body = r#"{"on":true,"every":1,"unit":"minutes","text":"Wake up"}"#;
    let ledger_says = || {
        scene
            .context
            .ledger
            .borrow()
            .wake_up(scene.project.id)
            .unwrap()
    };
    let before = ledger_says();
    assert!(!before.on);
    for token in [&scene.zeus, &scene.chief] {
        for method in [Method::POST, Method::PUT, Method::PATCH, Method::GET] {
            for target in [
                "/api/wakeup",
                "/api/wakeup.set",
                "/api/wake-up",
                "/api/wake_up",
                "/api/project",
                "/api/project/wakeup",
                "/api/projects/1/wakeup",
                "/api/settings",
            ] {
                let (status, answer) = said(
                    crate::api::handle(
                        &scene.context,
                        &screens,
                        request(method.clone(), target, Some(token), body),
                    )
                    .await,
                );
                assert_eq!(
                    (status, answer["error"].clone()),
                    (404, json!("unknown-route")),
                    "{method} {target}"
                );
            }
        }
    }
    assert_eq!(
        ledger_says(),
        before,
        "nothing a window sent changed the setting"
    );
    // What the page forwards is the app's list, and the daemon's bridge is the app's alone.
    let allowed = include_str!("../../../../../app/src-tauri/src/commands.rs");
    assert!(allowed.contains("\"wakeup.set\"") && allowed.contains("\"wakeup.get\""));
}

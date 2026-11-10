//! The window's role, as Codex's server is given it: the developer instructions of
//! the TUI's main start and of its resumes and forks of the window's own threads, and of
//! nothing else that passes through the broker.

use serde_json::{json, Value};

use super::fixture::{run, Fixture, Options};

const A: &str = "01a09094-938f-7fd1-a2d3-315cf92b4559";
const B: &str = "01a09094-a559-7db0-bf50-e2309856c3c0";
const C: &str = "01a09094-c0de-7aaa-8bbb-0123456789ab";

const ROLE: &str = "Codex's own instructions.\n\nYour ConsensFlow role is worker.";

#[test]
fn the_main_start_and_its_resume_and_fork_reach_codex_with_the_role_and_a_side_thread_as_the_tui_sent_it(
) {
    run(async {
        let f = Fixture::with(Options {
            role: Some(ROLE.to_owned()),
            ..Options::default()
        })
        .await;
        let tui = f.connect().await;
        let started = f
            .call(
                &tui,
                1,
                "thread/start",
                json!({ "ephemeral": false, "threadSource": "user", "developerInstructions": null }),
                json!({ "thread": { "id": A, "turns": [], "status": { "type": "idle" } } }),
            )
            .await;
        assert_eq!(started["developerInstructions"], ROLE);
        assert_eq!(f.read().await["sessionId"], A);
        let side =
            json!({ "ephemeral": true, "threadSource": "user", "developerInstructions": null });
        let sent = f
            .call(
                &tui,
                2,
                "thread/start",
                side.clone(),
                json!({ "thread": { "id": B } }),
            )
            .await;
        assert_eq!(sent, side, "a side start goes as the TUI sent it");
        // The side thread resumed as the TUI resumes a sub-agent it attaches, its roots null:
        // no thread of the window's (T-1402).
        let attach = json!({ "threadId": B, "runtimeWorkspaceRoots": null });
        let sent = f
            .call(
                &tui,
                3,
                "thread/resume",
                attach.clone(),
                json!({ "thread": { "id": B } }),
            )
            .await;
        assert_eq!(
            sent, attach,
            "a resume of a side thread goes as the TUI sent it"
        );
        // The window's own resume and fork: a compaction rebuilds the thread's developer
        // message from the instructions they name (T-1394).
        for (id, method, params, answer) in [
            (
                4,
                "thread/resume",
                json!({ "threadId": A, "runtimeWorkspaceRoots": [], "developerInstructions": null }),
                A,
            ),
            (
                5,
                "thread/fork",
                json!({ "threadId": A, "threadSource": "user", "runtimeWorkspaceRoots": [] }),
                C,
            ),
        ] {
            let sent = f
                .call(
                    &tui,
                    id,
                    method,
                    params.clone(),
                    json!({ "thread": { "id": answer } }),
                )
                .await;
            let mut expected = params;
            expected["developerInstructions"] = ROLE.into();
            assert_eq!(sent, expected, "{method}");
        }
        assert_eq!(f.read().await["sessionId"], C);
    });
}

#[test]
fn the_thread_a_window_was_launched_to_resume_reaches_codex_with_the_role() {
    run(async {
        let f = Fixture::with(Options {
            role: Some(ROLE.to_owned()),
            resumed: Some(B.to_owned()),
            ..Options::default()
        })
        .await;
        let tui = f.connect().await;
        let sent = f
            .call(
                &tui,
                1,
                "thread/resume",
                json!({ "threadId": B, "runtimeWorkspaceRoots": null }),
                json!({ "thread": { "id": B, "status": { "type": "idle" } } }),
            )
            .await;
        assert_eq!(sent["developerInstructions"], ROLE);
        assert_eq!(f.read().await["sessionId"], B);
    });
}

#[test]
fn a_window_given_no_role_has_its_main_start_go_as_the_tui_sent_it() {
    run(async {
        let f = Fixture::with(Options::default()).await;
        let tui = f.connect().await;
        let params =
            json!({ "ephemeral": false, "threadSource": "user", "developerInstructions": null });
        let started = f
            .call(
                &tui,
                1,
                "thread/start",
                params.clone(),
                json!({ "thread": { "id": A } }),
            )
            .await;
        assert_eq!(started, params);
        assert_eq!(started["developerInstructions"], Value::Null);
    });
}

#[test]
fn a_start_the_broker_cannot_tell_goes_as_it_came_and_the_window_says_so_once_in_a_note() {
    use std::sync::Arc;

    use cf_board::scripted::{reply, scripted};
    use cf_board::Board;

    use super::fixture::wait;

    run(async {
        let api = scripted(vec![
            reply(200, json!({ "id": 9, "recipient": "chief" })),
            reply(200, json!({ "id": 10, "recipient": "chief" })),
        ]);
        let f = Fixture::with(Options {
            role: Some(ROLE.to_owned()),
            board: Some(Arc::new(Board::new(Some(&api.url), "window-token"))),
            ..Options::default()
        })
        .await;
        let tui = f.connect().await;
        let unknown = json!({ "ephemeral": false, "developerInstructions": null });
        for id in [1, 2] {
            let sent = f
                .call(
                    &tui,
                    id,
                    "thread/start",
                    unknown.clone(),
                    json!({ "thread": { "id": A } }),
                )
                .await;
            assert_eq!(sent, unknown, "it goes as it came");
        }
        wait(|| !api.received().is_empty()).await;
        let notes = api.received();
        assert_eq!(notes.len(), 1, "said once a window");
        assert_eq!(
            (notes[0].method.as_str(), notes[0].path.as_str()),
            ("POST", "/api/notes")
        );
        let body = notes[0].json().unwrap()["body"]
            .as_str()
            .unwrap()
            .to_owned();
        assert!(body.contains("was not given this window's role"), "{body}");
        assert!(body.contains("it names no `threadSource`"), "{body}");
    });
}

//! The window's role, as Codex's server is given it: the developer instructions of
//! the TUI's main start, and of nothing else that passes through the broker.

use serde_json::{json, Value};

use super::fixture::{run, Fixture, Options};

const A: &str = "01a09094-938f-7fd1-a2d3-315cf92b4559";
const B: &str = "01a09094-a559-7db0-bf50-e2309856c3c0";

const ROLE: &str = "Codex's own instructions.\n\nYour ConsensFlow role is worker.";

#[test]
fn the_main_start_reaches_codex_with_the_role_and_a_resume_fork_or_side_start_as_the_tui_sent_it() {
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
        for (id, method, params) in [
            (
                2,
                "thread/start",
                json!({ "ephemeral": true, "threadSource": "user", "developerInstructions": null }),
            ),
            (
                3,
                "thread/resume",
                json!({ "threadId": B, "runtimeWorkspaceRoots": [], "developerInstructions": null }),
            ),
            (
                4,
                "thread/fork",
                json!({ "threadId": B, "threadSource": "user", "runtimeWorkspaceRoots": [] }),
            ),
        ] {
            let sent = f
                .call(
                    &tui,
                    id,
                    method,
                    params.clone(),
                    json!({ "thread": { "id": B } }),
                )
                .await;
            assert_eq!(sent, params, "{method}");
        }
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

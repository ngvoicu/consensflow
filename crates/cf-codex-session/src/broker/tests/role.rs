//! The window's role, as Codex's server is given it: the developer instructions of
//! the TUI's main start, and of its resumes and forks of a thread Codex says is
//! top-level, and of nothing else that passes through the broker. Which threads those
//! are is the table's (`top_level.rs`); here, how the broker asks and holds.

use std::time::Duration;

use serde_json::{json, Value};

use super::fixture::{run, wait, Fixture, Options, Reads};

const A: &str = "01a09094-938f-7fd1-a2d3-315cf92b4559";
const B: &str = "01a09094-a559-7db0-bf50-e2309856c3c0";

const C: &str = "01a09094-c0de-7aaa-8bbb-0123456789ab";

const ROLE: &str = "Codex's own instructions.\n\nYour ConsensFlow role is worker.";

/// `id` as Codex 0.167.0 reads a main thread of the TUI's (`codex_0_167_threads.json`).
fn top_level(id: &str) -> Value {
    json!({
        "id": id, "sessionId": id, "forkedFromId": null, "parentThreadId": null,
        "ephemeral": false, "source": "vscode", "threadSource": "user",
        "status": { "type": "idle" }, "turns": [],
    })
}

/// `id` as Codex 0.167.0 reads a sub-agent's thread under `parent`.
fn sub_agent(id: &str, parent: &str) -> Value {
    json!({
        "id": id, "sessionId": id, "forkedFromId": null, "parentThreadId": parent,
        "ephemeral": false,
        "source": { "subAgent": { "thread_spawn": { "parent_thread_id": parent, "depth": 1 } } },
        "threadSource": "subagent", "status": { "type": "idle" }, "turns": [],
    })
}

#[test]
fn the_main_start_reaches_codex_with_the_role_and_a_side_start_or_a_sub_agents_resume_or_fork_as_the_tui_sent_it(
) {
    run(async {
        let f = Fixture::with(Options {
            role: Some(ROLE.to_owned()),
            ..Options::default()
        })
        .await;
        f.codex.knows(&sub_agent(B, A));
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
fn a_resume_and_a_fork_of_a_thread_codex_says_is_top_level_reach_codex_with_the_role_in_place_of_what_they_named(
) {
    run(async {
        let f = Fixture::with(Options {
            role: Some(ROLE.to_owned()),
            ..Options::default()
        })
        .await;
        f.codex.knows(&top_level(A));
        let tui = f.connect().await;
        tui.send(json!({
            "id": 1,
            "method": "thread/resume",
            "params": { "threadId": A, "developerInstructions": "the TUI's", "runtimeWorkspaceRoots": null },
        }));
        wait(|| f.codex.is_held("thread/resume")).await;
        let asked = f.codex.requests_of("thread/read");
        assert_eq!(asked.len(), 1, "{asked:?}");
        assert_eq!(
            asked[0]["params"],
            json!({ "threadId": A, "includeTurns": false })
        );
        assert_eq!(
            f.codex.peer_of("thread/resume"),
            1,
            "the TUI's own connection; the read went on the broker's"
        );
        let sent = f
            .codex
            .texts()
            .into_iter()
            .map(|(_, text)| text)
            .find(|text| text.contains("thread/resume"))
            .expect("the resume");
        assert_eq!(
            sent,
            format!(
                r#"{{"id":1,"method":"thread/resume","params":{{"threadId":"{A}","developerInstructions":{},"runtimeWorkspaceRoots":null}}}}"#,
                json!(ROLE)
            ),
            "the role in the place of what it named"
        );
        f.respond("thread/resume", json!({ "thread": top_level(A) }))
            .await;
        wait(|| tui.has_answered(&json!(1))).await;
        let sent = f
            .call(
                &tui,
                2,
                "thread/fork",
                json!({ "threadId": A, "threadSource": "user", "runtimeWorkspaceRoots": [] }),
                json!({ "thread": { "id": C } }),
            )
            .await;
        assert_eq!(sent["developerInstructions"], ROLE);
        assert_eq!(f.read().await["sessionId"], C);
    });
}

#[test]
fn what_the_tui_says_while_a_resume_waits_for_codexs_word_follows_it_in_order_and_takes_no_delivery_meanwhile(
) {
    run(async {
        let f = Fixture::with(Options {
            role: Some(ROLE.to_owned()),
            ..Options::default()
        })
        .await;
        f.codex.knows(&top_level(A));
        f.codex.knows(&top_level(B));
        let tui = f.connect().await;
        f.start_thread(&tui, 1, top_level(A)).await;
        f.codex.reads(Reads::Late(Duration::from_millis(150)));
        tui.send(json!({ "id": 2, "method": "thread/resume", "params": { "threadId": B, "runtimeWorkspaceRoots": [] } }));
        tui.send(
            json!({ "id": 3, "method": "turn/start", "params": { "threadId": B, "input": [] } }),
        );
        wait(|| !f.codex.requests_of("thread/read").is_empty()).await;
        // The window shows no thread while it waits: a message for the one it showed is not taken.
        assert_eq!(f.read().await["available"], false);
        assert_eq!(
            f.deliver(A, json!({})).await["error"],
            "native-session-unavailable"
        );
        assert!(!f.codex.is_held("thread/resume") && !f.codex.is_held("turn/start"));
        wait(|| f.codex.is_held("turn/start")).await;
        let order: Vec<Value> = f
            .codex
            .requests()
            .into_iter()
            .filter(|request| request["id"] == 2 || request["id"] == 3)
            .map(|request| request["method"].clone())
            .collect();
        assert_eq!(order, ["thread/resume", "turn/start"]);
        let resumed = f
            .respond("thread/resume", json!({ "thread": top_level(B) }))
            .await;
        assert_eq!(resumed["params"]["developerInstructions"], ROLE);
        f.respond("turn/start", json!({ "turn": { "id": "t" } }))
            .await;
        wait(|| tui.has_answered(&json!(3))).await;
        assert_eq!(f.read().await["sessionId"], B);
    });
}

#[test]
fn a_resume_whose_read_codex_never_answers_goes_without_the_role_by_the_deadline() {
    run(async {
        let f = Fixture::with(Options {
            role: Some(ROLE.to_owned()),
            ..Options::default()
        })
        .await;
        f.codex.knows(&top_level(A));
        f.codex.reads(Reads::Silent);
        let tui = f.connect().await;
        let sent = f
            .call(
                &tui,
                1,
                "thread/resume",
                json!({ "threadId": A, "runtimeWorkspaceRoots": [] }),
                json!({ "thread": top_level(A) }),
            )
            .await;
        assert_eq!(sent, json!({ "threadId": A, "runtimeWorkspaceRoots": [] }));
        assert_eq!(f.read().await["sessionId"], A);
        assert_eq!(
            f.broker.shared.pending_requests(),
            0,
            "the read is given up, not left waiting"
        );
    });
}

/// The lines the broker wrote to the daemon's log in `home`.
fn logged(home: &std::path::Path) -> Vec<String> {
    std::fs::read_to_string(home.join("daemon.log"))
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}

#[test]
fn a_resume_given_no_role_by_the_rule_is_one_line_in_the_daemons_log_with_its_thread_and_why() {
    run(async {
        let home = tempfile::tempdir().unwrap();
        let f = Fixture::with(Options {
            role: Some(ROLE.to_owned()),
            log_home: Some(home.path().to_owned()),
            ..Options::default()
        })
        .await;
        f.codex.knows(&top_level(A));
        f.codex.knows(&sub_agent(B, A));
        let tui = f.connect().await;
        // The window's own thread is given its role, and nothing is written of it.
        f.call(
            &tui,
            1,
            "thread/resume",
            json!({ "threadId": A, "runtimeWorkspaceRoots": [] }),
            json!({ "thread": top_level(A) }),
        )
        .await;
        assert_eq!(logged(home.path()), Vec::<String>::new());
        // A sub-agent's is not, and the log says which thread and which of Codex's words.
        f.call(
            &tui,
            2,
            "thread/resume",
            json!({ "threadId": B, "runtimeWorkspaceRoots": [] }),
            json!({ "thread": sub_agent(B, A) }),
        )
        .await;
        let lines = logged(home.path());
        assert_eq!(lines.len(), 1, "{lines:?}");
        let line = &lines[0];
        assert!(
            line.contains(" info cf codex-session gave Codex's thread/resume of thread 01a09094-a559-7db0-bf50-e2309856c3c0 no role: Codex's thread/read says it is not top-level: its parentThreadId is 01a09094-938f-7fd1-a2d3-315cf92b4559"),
            "{line}"
        );
        assert!(!line.contains(ROLE), "{line}");
    });
}

#[test]
fn a_resume_whose_read_codex_never_answers_is_one_line_in_the_daemons_log_with_its_thread_and_why()
{
    run(async {
        let home = tempfile::tempdir().unwrap();
        let f = Fixture::with(Options {
            role: Some(ROLE.to_owned()),
            log_home: Some(home.path().to_owned()),
            ..Options::default()
        })
        .await;
        f.codex.knows(&top_level(A));
        f.codex.reads(Reads::Silent);
        let tui = f.connect().await;
        f.call(
            &tui,
            1,
            "thread/resume",
            json!({ "threadId": A, "runtimeWorkspaceRoots": [] }),
            json!({ "thread": top_level(A) }),
        )
        .await;
        let lines = logged(home.path());
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(
            lines[0].contains(" info cf codex-session gave Codex's thread/resume of thread 01a09094-938f-7fd1-a2d3-315cf92b4559 no role: Codex did not answer its thread/read within 0.3 s"),
            "{}",
            lines[0]
        );
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

#[test]
fn a_note_of_an_unclassified_start_that_the_board_refuses_is_one_line_in_the_daemons_log_without_the_token(
) {
    use std::sync::Arc;

    use cf_board::scripted::{reply, scripted};
    use cf_board::Board;

    use super::fixture::wait;

    run(async {
        let home = tempfile::tempdir().unwrap();
        let api = scripted(vec![reply(
            403,
            json!({ "message": "notes are closed", "error": "closed" }),
        )]);
        let f = Fixture::with(Options {
            role: Some(ROLE.to_owned()),
            board: Some(Arc::new(Board::new(Some(&api.url), "window-token"))),
            log_home: Some(home.path().to_owned()),
            ..Options::default()
        })
        .await;
        let tui = f.connect().await;
        let unknown = json!({ "ephemeral": false, "developerInstructions": null });
        f.call(
            &tui,
            1,
            "thread/start",
            unknown.clone(),
            json!({ "thread": { "id": A } }),
        )
        .await;
        wait(|| !logged(home.path()).is_empty()).await;
        let lines = logged(home.path());
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(
            lines[0].contains(" info cf codex-session could not tell the board that a thread started without its role: notes are closed"),
            "{}",
            lines[0]
        );
        assert!(!lines[0].contains("window-token"), "{}", lines[0]);
        assert_eq!(api.received().len(), 1, "no second try");
    });
}

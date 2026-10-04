//! Which thread the window shows, as the broker learns it from what passes
//! between the TUI and Codex's server.

use serde_json::{json, Value};

use super::fixture::{run, wait, wait_for, Fixture, Options, A, B, TEXT};

#[test]
fn follows_successful_main_new_resume_while_ignoring_title_threads_child_focus_and_picker_connections(
) {
    run(async {
        let f = Fixture::start().await;
        let tui = f.connect().await;
        assert_eq!(f.read().await["sessionId"], Value::Null);
        tui.send(json!({
            "id": 1,
            "method": "thread/start",
            "params": { "ephemeral": false, "threadSource": "user" },
        }));
        f.respond("thread/start", json!({ "thread": { "id": A } }))
            .await;
        wait(|| f.codex.requests().iter().any(|request| request["id"] == 1)).await;
        wait(|| tui.has_answered(&json!(1))).await;
        assert_eq!(f.read().await["sessionId"], A);
        let picker = f.connect().await;
        picker.close();
        wait(|| picker.is_closed()).await;
        assert_eq!(f.read().await["sessionId"], A);
        tui.send(json!({
            "id": 2,
            "method": "thread/start",
            "params": { "ephemeral": true, "threadSource": "system" },
        }));
        f.respond("thread/start", json!({ "thread": { "id": B } }))
            .await;
        wait(|| tui.has_answered(&json!(2))).await;
        assert_eq!(f.read().await["sessionId"], A);
        tui.send(json!({ "id": 3, "method": "thread/resume", "params": { "threadId": B } }));
        f.respond("thread/resume", json!({ "thread": { "id": B } }))
            .await;
        wait(|| tui.has_answered(&json!(3))).await;
        assert_eq!(f.read().await["sessionId"], A);
        tui.send(json!({
            "id": 4,
            "method": "thread/resume",
            "params": { "threadId": B, "runtimeWorkspaceRoots": [] },
        }));
        wait(|| f.codex.is_held_by_id(&json!(4))).await;
        assert_eq!(f.read().await["sessionId"], Value::Null);
        assert_eq!(
            f.deliver(A, json!({})).await,
            json!({
                "ok": false,
                "admitted": false,
                "bytesWritten": 0,
                "error": "native-session-unavailable",
            })
        );
        f.respond("thread/resume", json!({ "thread": { "id": B } }))
            .await;
        wait(|| tui.has_answered(&json!(4))).await;
        assert_eq!(f.read().await["sessionId"], B);
        assert_eq!(
            f.deliver(A, json!({})).await["error"],
            "native-session-changed"
        );
        assert!(f.codex.requests_of("thread/queue/add").is_empty());
        let (delivered, queued) = tokio::join!(
            f.deliver(B, json!({})),
            f.respond(
                "thread/queue/add",
                json!({ "queuedMessage": { "id": "queue-1" } })
            ),
        );
        assert_eq!(queued["params"]["threadId"], B);
        assert_eq!(queued["params"]["input"][0]["text"], TEXT);
        assert_eq!(delivered, json!({ "ok": true, "admitted": true }));
    });
}

#[test]
fn a_window_opened_on_its_thread_is_named_by_its_resume_whose_roots_codex_0_159_sends_as_null() {
    run(async {
        let f = Fixture::start().await;
        let tui = f.connect().await;
        // `codex resume <thread> <message>`, as Codex 0.159.2 sends it (traced 2026-10-03).
        let resume = json!({
            "threadId": B,
            "history": null,
            "path": null,
            "model": "gpt-5.6-luna",
            "modelProvider": null,
            "serviceTier": "default",
            "cwd": null,
            "runtimeWorkspaceRoots": null,
            "approvalPolicy": null,
            "approvalsReviewer": null,
            "sandbox": null,
            "permissions": null,
            "config": { "model_reasoning_effort": "low" },
            "baseInstructions": null,
            "developerInstructions": null,
            "personality": null,
            "excludeTurns": true,
            "initialTurnsPage": null,
        });
        tui.send(json!({ "id": 5, "method": "thread/resume", "params": resume }));
        wait(|| f.codex.is_held_by_id(&json!(5))).await;
        assert_eq!(
            f.read().await["available"],
            false,
            "nothing is taken while it resumes"
        );
        f.respond(
            "thread/resume",
            json!({ "thread": { "id": B, "status": { "type": "idle" } } }),
        )
        .await;
        wait(|| tui.has_answered(&json!(5))).await;
        assert_eq!(
            [
                f.read().await["sessionId"].clone(),
                f.read().await["available"].clone()
            ],
            [json!(B), json!(true)]
        );
        let (delivered, started) = tokio::join!(
            f.deliver(B, json!({})),
            f.respond("turn/start", json!({ "turn": { "id": "turn-1" } })),
        );
        assert_eq!(started["params"]["threadId"], B);
        assert_eq!(delivered, json!({ "ok": true, "admitted": true }));
    });
}

#[test]
fn promotes_durable_forks_and_preserves_native_permission_changes_after_the_initial_launch() {
    run(async {
        let f = Fixture::with(Options {
            fresh_bypass: true,
            ..Options::default()
        })
        .await;
        let tui = f.connect().await;
        let start = json!({
            "ephemeral": false,
            "threadSource": "user",
            "approvalPolicy": "on-request",
            "sandbox": "read-only",
            "permissions": { "profile": "home" },
        });
        let initial = f
            .call(
                &tui,
                1,
                "thread/start",
                start.clone(),
                json!({ "thread": { "id": A, "turns": [], "status": { "type": "idle" } } }),
            )
            .await;
        assert_eq!(initial["approvalPolicy"], "never");
        assert_eq!(initial["sandbox"], "danger-full-access");
        assert_eq!(initial["permissions"], Value::Null);
        assert_eq!(f.read().await["empty"], true);
        f.call(
            &tui,
            2,
            "thread/fork",
            json!({ "threadId": A, "threadSource": "user", "runtimeWorkspaceRoots": [], "ephemeral": true }),
            json!({ "thread": { "id": B } }),
        )
        .await;
        assert_eq!(f.read().await["sessionId"], A);
        f.call(
            &tui,
            3,
            "thread/fork",
            json!({ "threadId": A, "threadSource": "user", "runtimeWorkspaceRoots": [] }),
            json!({ "thread": { "id": B, "turns": [{}] } }),
        )
        .await;
        assert_eq!(f.read().await["sessionId"], B);
        assert_eq!(f.read().await["empty"], false);
        f.call(
            &tui,
            4,
            "thread/settings/update",
            json!({ "threadId": B, "approvalPolicy": "on-request" }),
            json!({}),
        )
        .await;
        assert_eq!(
            f.call(
                &tui,
                5,
                "thread/start",
                start.clone(),
                json!({ "thread": { "id": A } })
            )
            .await,
            start
        );
    });
}

#[test]
fn consumes_native_empty_thread_proof_at_admission_and_never_forces_fresh_permissions_onto_resume()
{
    run(async {
        let f = Fixture::with(Options {
            fresh_bypass: true,
            ..Options::default()
        })
        .await;
        let tui = f.connect().await;
        f.start_thread(
            &tui,
            1,
            json!({ "id": A, "turns": [], "status": { "type": "idle" } }),
        )
        .await;
        assert_eq!(f.read().await["empty"], true);
        // An idle thread takes the delivery as its turn.
        let (delivered, _) =
            tokio::join!(f.deliver(A, json!({})), f.respond("turn/start", json!({})),);
        assert_eq!(delivered, json!({ "ok": true, "admitted": true }));
        assert_eq!(f.read().await["empty"], false);
        let resume = json!({
            "threadId": B,
            "runtimeWorkspaceRoots": [],
            "approvalPolicy": null,
            "sandbox": null,
        });
        assert_eq!(
            f.call(
                &tui,
                2,
                "thread/resume",
                resume.clone(),
                json!({ "thread": { "id": B } })
            )
            .await,
            resume
        );
        let start = json!({ "ephemeral": false, "threadSource": "user", "sandbox": "read-only" });
        assert_eq!(
            f.call(
                &tui,
                3,
                "thread/start",
                start.clone(),
                json!({ "thread": { "id": A } })
            )
            .await,
            start
        );
    });
}

#[test]
fn a_request_of_codexs_own_that_shares_an_id_with_the_tuis_thread_switch_is_not_its_answer() {
    // The server numbers the requests it sends the TUI on its own; one may
    // carry the id of a switch the TUI is waiting on.
    run(async {
        let f = Fixture::start().await;
        let tui = f.connect().await;
        tui.send(json!({
            "id": 1,
            "method": "thread/start",
            "params": { "ephemeral": false, "threadSource": "user" },
        }));
        wait(|| f.codex.is_held("thread/start")).await;
        let peer = f.codex.peer_of("thread/start");
        f.codex.send_json(
            peer,
            &json!({ "id": 1, "method": "item/commandExecution/requestApproval", "params": {} }),
        );
        wait(|| {
            tui.has_seen(|message| message["method"] == "item/commandExecution/requestApproval")
        })
        .await;
        f.respond(
            "thread/start",
            json!({ "thread": { "id": A, "status": { "type": "idle" }, "turns": [] } }),
        )
        .await;
        wait(|| tui.has_seen(|message| message["id"] == 1 && message.get("result").is_some()))
            .await;
        assert_eq!(f.read().await["sessionId"], A);
    });
}

#[test]
fn overlapping_switches_apply_the_latest_and_a_stale_reply_after_it_changes_nothing() {
    run(async {
        let f = Fixture::start().await;
        let tui = f.connect().await;
        tui.send(json!({
            "id": 1,
            "method": "thread/start",
            "params": { "ephemeral": false, "threadSource": "user" },
        }));
        tui.send(json!({
            "id": 2,
            "method": "thread/resume",
            "params": { "threadId": B, "runtimeWorkspaceRoots": [] },
        }));
        wait(|| f.codex.is_held_by_id(&json!(1)) && f.codex.is_held_by_id(&json!(2))).await;
        // The window is switching until the latest answers; the first one's
        // answer, first to come, is stale and leaves it so.
        let revision = f.read().await["revision"].clone();
        f.respond("thread/start", json!({ "thread": { "id": A } }))
            .await;
        wait(|| tui.has_answered(&json!(1))).await;
        let shown = f.read().await;
        assert_eq!(shown["sessionId"], Value::Null);
        assert_eq!(shown["available"], false);
        assert_eq!(shown["revision"], revision);
        f.respond("thread/resume", json!({ "thread": { "id": B } }))
            .await;
        wait(|| tui.has_answered(&json!(2))).await;
        assert_eq!(f.read().await["sessionId"], B);

        // And the other way round: the latest answers first, the stale one after it.
        tui.send(json!({
            "id": 3,
            "method": "thread/start",
            "params": { "ephemeral": false, "threadSource": "user" },
        }));
        tui.send(json!({
            "id": 4,
            "method": "thread/resume",
            "params": { "threadId": A, "runtimeWorkspaceRoots": [] },
        }));
        wait(|| f.codex.is_held_by_id(&json!(3)) && f.codex.is_held_by_id(&json!(4))).await;
        f.respond("thread/resume", json!({ "thread": { "id": A } }))
            .await;
        wait(|| tui.has_answered(&json!(4))).await;
        assert_eq!(f.read().await["sessionId"], A);
        f.respond("thread/start", json!({ "thread": { "id": B } }))
            .await;
        wait(|| tui.has_answered(&json!(3))).await;
        let shown = f.read().await;
        assert_eq!(
            shown["sessionId"], A,
            "the stale answer named a thread the window left"
        );
        assert_eq!(shown["available"], true);
    });
}

#[test]
fn a_failed_switch_goes_back_to_the_thread_and_its_emptiness() {
    run(async {
        let f = Fixture::start().await;
        let tui = f.connect().await;
        f.start_thread(
            &tui,
            1,
            json!({ "id": A, "turns": [], "status": { "type": "idle" } }),
        )
        .await;
        let before = f.read().await;
        assert_eq!(
            [&before["sessionId"], &before["empty"]],
            [&json!(A), &json!(true)]
        );
        tui.send(json!({
            "id": 2,
            "method": "thread/resume",
            "params": { "threadId": B, "runtimeWorkspaceRoots": [] },
        }));
        wait(|| f.codex.is_held_by_id(&json!(2))).await;
        let during = f.read().await;
        assert_eq!(
            [&during["sessionId"], &during["empty"], &during["available"]],
            [&Value::Null, &json!(false), &json!(false)]
        );
        f.respond_error(
            "thread/resume",
            json!({ "code": -1, "message": "not found" }),
        )
        .await;
        wait(|| tui.has_answered(&json!(2))).await;
        let after = f.read().await;
        assert_eq!(
            [&after["sessionId"], &after["empty"], &after["available"]],
            [&json!(A), &json!(true), &json!(true)]
        );
        // Delivering to the thread it went back to works.
        let (delivered, _) = tokio::join!(
            f.deliver(A, json!({})),
            f.respond("turn/start", json!({ "turn": { "id": "turn-1" } })),
        );
        assert_eq!(delivered, json!({ "ok": true, "admitted": true }));
    });
}

#[test]
fn the_owners_connection_lost_forgets_the_thread_and_another_tuis_does_not() {
    run(async {
        let f = Fixture::start().await;
        let owner = f.connect().await;
        f.start_thread(&owner, 1, json!({ "id": A })).await;
        let picker = f.connect().await;
        let before = f.read().await["revision"].clone();
        picker.terminate();
        wait_for(|| async { f.codex.connections() == 3 && !f.codex.is_open(2) }).await;
        let shown = f.read().await;
        assert_eq!(
            shown["sessionId"], A,
            "a picker's connection never owned the thread"
        );
        assert_eq!(shown["revision"], before);
        // The owner's end changes the window's revision and shows no thread.
        owner.terminate();
        wait_for(|| async { f.read().await["sessionId"].is_null() }).await;
        let shown = f.read().await;
        assert_eq!(
            shown["revision"].as_u64(),
            before.as_u64().map(|revision| revision + 1)
        );
        assert_eq!(shown["available"], false);
        assert_eq!(
            f.deliver(A, json!({})).await["error"],
            "native-session-unavailable"
        );
    });
}

#[test]
fn a_second_tui_that_chose_the_thread_owns_it_and_the_first_ending_changes_nothing() {
    run(async {
        let f = Fixture::start().await;
        let first = f.connect().await;
        f.start_thread(&first, 1, json!({ "id": A })).await;
        let second = f.connect().await;
        f.start_thread(&second, 1, json!({ "id": B })).await;
        assert_eq!(f.read().await["sessionId"], B);
        // The first connection no longer owns the window.
        first.terminate();
        wait_for(|| async { !f.codex.is_open(1) }).await;
        assert_eq!(f.read().await["sessionId"], B);
        second.terminate();
        wait_for(|| async { f.read().await["sessionId"].is_null() }).await;
    });
}

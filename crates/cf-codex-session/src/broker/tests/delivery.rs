//! What the daemon asks of the broker over HTTP, and what the broker does
//! with a delivery.

use std::future::Future;
use std::task::{Context, Waker};
use std::time::Duration;

use cf_proto::codex::{Delivery, DeliveryReply};
use serde_json::{json, Value};

use super::fixture::{http, run, wait, wait_for, Fixture, A, B, TEXT, TOKEN};
use crate::broker::now_ms;

fn refused(error: &str) -> Value {
    json!({ "ok": false, "admitted": false, "bytesWritten": 0, "error": error })
}

fn admitted() -> Value {
    json!({ "ok": true, "admitted": true })
}

fn uncertain() -> Value {
    json!({ "ok": false, "admitted": null, "error": "uncertain" })
}

#[test]
fn starts_a_turn_with_a_delivery_when_the_thread_is_idle_queues_it_only_while_a_turn_runs_and_says_when_it_can_take_one(
) {
    run(async {
        let f = Fixture::start().await;
        let tui = f.connect().await;
        assert_eq!(f.read().await["available"], false, "no thread yet");
        f.start_thread(&tui, 1, json!({ "id": A, "status": { "type": "idle" } }))
            .await;
        assert_eq!(
            [
                f.read().await["sessionId"].clone(),
                f.read().await["available"].clone()
            ],
            [json!(A), json!(true)]
        );
        // Idle, as after an interrupt: the message starts the turn itself.
        let (first, started) = tokio::join!(
            f.deliver(A, json!({})),
            f.respond("turn/start", json!({ "turn": { "id": "turn-1" } })),
        );
        assert_eq!(started["params"]["threadId"], A);
        assert_eq!(started["params"]["input"][0]["text"], TEXT);
        assert_eq!(first, admitted());
        // A turn runs: the next one waits in Codex's queue.
        let (second, queued) = tokio::join!(
            f.deliver(A, json!({})),
            f.respond(
                "thread/queue/add",
                json!({ "queuedMessage": { "id": "queue-1" } })
            ),
        );
        assert_eq!(queued["params"]["threadId"], A);
        assert_eq!(second, admitted());
        // The turn ends (completed or interrupted): idle again, a turn again.
        f.codex.send_json(
            0,
            &json!({ "method": "turn/completed", "params": { "threadId": A } }),
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
        let (third, _) = tokio::join!(
            f.deliver(A, json!({})),
            f.respond("turn/start", json!({ "turn": { "id": "turn-2" } })),
        );
        assert_eq!(third, admitted());
        // While the TUI switches threads, nothing can be taken.
        tui.send(json!({
            "id": 2,
            "method": "thread/resume",
            "params": { "threadId": B, "runtimeWorkspaceRoots": [] },
        }));
        wait(|| f.codex.is_held_by_id(&json!(2))).await;
        assert_eq!(f.read().await["available"], false);
    });
}

#[test]
fn restores_a_rejected_switch_rejects_invalid_ingress_and_reports_a_possible_write_as_uncertain() {
    run(async {
        let f = Fixture::start().await;
        let tui = f.connect().await;
        f.start_thread(&tui, 1, json!({ "id": A })).await;
        tui.send(json!({
            "id": 2,
            "method": "thread/resume",
            "params": { "threadId": B, "runtimeWorkspaceRoots": [] },
        }));
        f.respond_error(
            "thread/resume",
            json!({ "code": -1, "message": "not found" }),
        )
        .await;
        wait(|| tui.has_answered(&json!(2))).await;
        assert_eq!(f.read().await["sessionId"], A);
        assert_eq!(
            http(f.address(), "GET", "/session", &[], b"").await.status,
            401
        );
        // A window's connection without the launch's token, or to another path, never opens.
        let authorized = format!("Bearer {TOKEN}");
        let strangers = [
            super::fixture::Tui::connect(f.address(), "/", None).await,
            super::fixture::Tui::connect(f.address(), "/other", Some(&authorized)).await,
        ];
        for stranger in strangers {
            assert!(
                stranger.is_err(),
                "a connection the broker should have turned away opened"
            );
        }
        let garbled = f.post_deliver(b"{\"launchId\":").await;
        assert_eq!(
            (garbled.status, garbled.json()),
            (400, refused("invalid-record"))
        );
        assert_eq!(
            f.deliver(A, json!({ "launchId": "foreign" })).await["error"],
            "invalid-record"
        );
        assert_eq!(
            f.deliver(A, json!({ "expiresAt": now_ms() - 1.0 })).await["error"],
            "expired"
        );
        let delivery = f.deliver(A, json!({}));
        let hung_up = async {
            wait(|| f.codex.is_held("thread/queue/add")).await;
            f.codex.terminate(f.codex.peer_of("thread/queue/add"));
        };
        let (reply, ()) = tokio::join!(delivery, hung_up);
        assert_eq!(reply, uncertain());
        tui.terminate();
        wait_for(|| async { f.read().await["sessionId"].is_null() }).await;
    });
}

#[test]
fn a_turn_the_human_starts_in_the_window_makes_the_next_delivery_wait_in_codexs_queue() {
    run(async {
        let f = Fixture::start().await;
        let tui = f.connect().await;
        f.start_thread(
            &tui,
            1,
            json!({ "id": A, "turns": [], "status": { "type": "idle" } }),
        )
        .await;
        assert_eq!(f.read().await["empty"], true);
        tui.send(
            json!({ "id": 2, "method": "turn/start", "params": { "threadId": A, "input": [] } }),
        );
        wait(|| f.codex.is_held_by_id(&json!(2))).await;
        assert_eq!(f.read().await["empty"], false);
        let (delivered, queued) = tokio::join!(
            f.deliver(A, json!({})),
            f.respond(
                "thread/queue/add",
                json!({ "queuedMessage": { "id": "queue-1" } })
            ),
        );
        assert_eq!(queued["params"]["input"][0]["text"], TEXT);
        assert_eq!(delivered, admitted());
        let turns: Vec<_> = f
            .codex
            .requests_of("turn/start")
            .iter()
            .map(|r| r["id"].clone())
            .collect();
        assert_eq!(
            turns,
            [json!(2)],
            "the only turn is the one the human started"
        );
    });
}

#[test]
fn a_delivery_whose_turn_codex_refuses_a_turn_having_just_started_waits_in_the_queue_instead() {
    run(async {
        let f = Fixture::start().await;
        let tui = f.connect().await;
        f.start_thread(&tui, 1, json!({ "id": A, "status": { "type": "idle" } }))
            .await;
        let delivery = f.deliver(A, json!({}));
        let codex = async {
            f.respond_error(
                "turn/start",
                json!({ "code": -32600, "message": "a turn is already running" }),
            )
            .await;
            f.respond(
                "thread/queue/add",
                json!({ "queuedMessage": { "id": "queue-1" } }),
            )
            .await
        };
        let (delivered, queued) = tokio::join!(delivery, codex);
        assert_eq!(queued["params"]["threadId"], A);
        assert_eq!(queued["params"]["input"][0]["text"], TEXT);
        assert_eq!(delivered, admitted());
    });
}

#[test]
fn a_refused_turn_is_never_queued_on_a_thread_the_window_moved_to_meanwhile() {
    // A delivery to idle thread A starts a turn; while Codex weighs it, the
    // window opens thread B. A's refusal must not put A's message in B's queue.
    run(async {
        let f = Fixture::start().await;
        let tui = f.connect().await;
        f.start_thread(&tui, 1, json!({ "id": A, "status": { "type": "idle" } }))
            .await;
        let delivery = f.deliver(A, json!({}));
        let window = async {
            wait(|| f.codex.is_held("turn/start")).await;
            f.start_thread(&tui, 2, json!({ "id": B, "status": { "type": "idle" } }))
                .await;
            f.respond_error(
                "turn/start",
                json!({ "code": -32600, "message": "a turn is already running" }),
            )
            .await;
        };
        let (delivered, ()) = tokio::join!(delivery, window);
        assert_eq!(delivered, refused("native-session-changed"));
        assert!(
            f.codex.requests_of("thread/queue/add").is_empty(),
            "nothing was queued"
        );
    });
}

#[test]
fn a_refused_turn_is_queued_on_the_same_thread_with_a_message_id_of_its_own() {
    run(async {
        let f = Fixture::start().await;
        let tui = f.connect().await;
        f.start_thread(&tui, 1, json!({ "id": A, "status": { "type": "idle" } }))
            .await;
        let delivery = f.deliver(A, json!({}));
        let codex = async {
            f.respond_error("turn/start", json!({ "message": "busy" }))
                .await;
            f.respond("thread/queue/add", json!({})).await
        };
        let (_, queued) = tokio::join!(delivery, codex);
        let id = queued["params"]["clientUserMessageId"].as_str().unwrap();
        assert_eq!(id.len(), 36, "{id}");
        assert_eq!(queued["params"]["input"][0]["text_elements"], json!([]));
        assert_eq!(queued["params"]["input"][0]["type"], "text");
    });
}

#[test]
fn a_delivery_is_put_to_codex_in_the_same_step_that_checks_the_thread() {
    // Nothing can switch the window between the check and the request: they
    // are one step, and the request is out when the delivery first waits.
    run(async {
        let f = Fixture::start().await;
        let tui = f.connect().await;
        f.start_thread(&tui, 1, json!({ "id": A })).await;
        let delivery = Delivery {
            launch_id: "launch-1".into(),
            session_id: A.into(),
            text: TEXT.into(),
            expires_at: now_ms() + 2000.0,
        };
        let mut delivering = Box::pin(f.broker.shared.deliver(&delivery));
        let polled = delivering
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()));
        assert!(polled.is_pending());
        // The request reaches Codex though the delivery is not polled again.
        wait(|| f.codex.is_held("thread/queue/add")).await;
        f.respond("thread/queue/add", json!({})).await;
        assert_eq!(delivering.await, DeliveryReply::Admitted);
    });
}

#[test]
fn a_delivery_dropped_while_it_waits_for_codex_leaves_no_request_waiting() {
    // The daemon hung up on its delivery: the task that held it ends, and what
    // it was waiting for is forgotten, not kept until the connection ends.
    run(async {
        let f = Fixture::start().await;
        let tui = f.connect().await;
        f.start_thread(&tui, 1, json!({ "id": A })).await;
        let delivery = Delivery {
            launch_id: "launch-1".into(),
            session_id: A.into(),
            text: TEXT.into(),
            expires_at: now_ms() + 2000.0,
        };
        let mut delivering = Box::pin(f.broker.shared.deliver(&delivery));
        let polled = delivering
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()));
        assert!(polled.is_pending());
        assert_eq!(f.broker.shared.pending_requests(), 1);
        drop(delivering);
        assert_eq!(f.broker.shared.pending_requests(), 0);
        // Codex answers it all the same, to nobody.
        wait(|| f.codex.is_held("thread/queue/add")).await;
        f.respond("thread/queue/add", json!({})).await;
        assert_eq!(f.read().await["available"], true);
    });
}

#[test]
fn a_request_with_no_answer_by_its_deadline_is_uncertain_and_never_sent_again() {
    run(async {
        let f = Fixture::start().await;
        let tui = f.connect().await;
        f.start_thread(&tui, 1, json!({ "id": A })).await;
        // Nobody answers: the deadline is the delivery's own.
        let started = std::time::Instant::now();
        let reply = f.deliver(A, json!({ "expiresAt": now_ms() + 300.0 })).await;
        assert_eq!(reply, uncertain());
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "{:?}",
            started.elapsed()
        );
        assert_eq!(
            f.codex.requests_of("thread/queue/add").len(),
            1,
            "never retried"
        );
        assert_eq!(f.codex.requests_of("turn/start").len(), 0);
    });
}

#[test]
fn a_request_has_three_seconds_whatever_the_deadline_the_daemon_gave() {
    run(async {
        let f = Fixture::start().await;
        let tui = f.connect().await;
        f.start_thread(&tui, 1, json!({ "id": A })).await;
        let started = std::time::Instant::now();
        let reply = f
            .deliver(A, json!({ "expiresAt": now_ms() + 600_000.0 }))
            .await;
        assert_eq!(reply["admitted"], Value::Null);
        let took = started.elapsed();
        assert!(
            took >= Duration::from_millis(2900) && took < Duration::from_secs(8),
            "{took:?}"
        );
    });
}

#[test]
fn a_delivery_has_one_deadline_for_its_turn_and_for_the_queue_that_follows_a_refused_turn() {
    // Three seconds from when it came, not three more from when its turn was
    // refused: the daemon is not kept waiting for a second window.
    run(async {
        let f = Fixture::start().await;
        let tui = f.connect().await;
        f.start_thread(&tui, 1, json!({ "id": A, "status": { "type": "idle" } }))
            .await;
        let started = std::time::Instant::now();
        let delivery = f.deliver(A, json!({ "expiresAt": now_ms() + 600_000.0 }));
        let codex = async {
            wait(|| f.codex.is_held("turn/start")).await;
            tokio::time::sleep(Duration::from_millis(1500)).await;
            f.respond_error("turn/start", json!({ "message": "busy" }))
                .await;
            // The queue is never answered.
        };
        let (reply, ()) = tokio::join!(delivery, codex);
        assert_eq!(reply, uncertain());
        let took = started.elapsed();
        assert!(
            took >= Duration::from_millis(2900) && took < Duration::from_millis(4200),
            "{took:?}"
        );
        assert_eq!(f.codex.requests_of("thread/queue/add").len(), 1);
    });
}

#[test]
fn a_queue_codex_refuses_is_not_asked_again_and_the_delivery_is_uncertain() {
    run(async {
        let f = Fixture::start().await;
        let tui = f.connect().await;
        // A turn is running: the message goes straight to the queue, which refuses it.
        f.start_thread(&tui, 1, json!({ "id": A })).await;
        let (reply, _) = tokio::join!(
            f.deliver(A, json!({})),
            f.respond_error("thread/queue/add", json!({ "message": "queue full" })),
        );
        assert_eq!(reply, uncertain());
        assert_eq!(f.codex.requests_of("thread/queue/add").len(), 1);
        assert!(f.codex.requests_of("turn/start").is_empty());
        // The thread is idle: its turn is refused, the queue is too, and that is all.
        f.start_thread(&tui, 2, json!({ "id": B, "status": { "type": "idle" } }))
            .await;
        let delivery = f.deliver(B, json!({}));
        let codex = async {
            f.respond_error("turn/start", json!({ "message": "busy" }))
                .await;
            f.respond_error("thread/queue/add", json!({ "message": "queue full" }))
                .await;
        };
        let (reply, ()) = tokio::join!(delivery, codex);
        assert_eq!(reply, uncertain());
        assert_eq!(f.codex.requests_of("turn/start").len(), 1);
        assert_eq!(f.codex.requests_of("thread/queue/add").len(), 2);
        assert!(!f.codex.is_held("turn/start") && !f.codex.is_held("thread/queue/add"));
    });
}

#[test]
fn every_message_put_in_codexs_queue_has_a_client_message_id_of_its_own() {
    run(async {
        let f = Fixture::start().await;
        let tui = f.connect().await;
        f.start_thread(&tui, 1, json!({ "id": A })).await;
        let mut ids = Vec::new();
        for _ in 0..3 {
            let (_, queued) = tokio::join!(
                f.deliver(A, json!({})),
                f.respond("thread/queue/add", json!({})),
            );
            ids.push(queued["params"]["clientUserMessageId"].clone());
            assert_ne!(queued["params"]["clientUserMessageId"], queued["id"]);
        }
        for id in &ids {
            let id = id.as_str().unwrap();
            assert_eq!(id.len(), 36, "{id}");
            assert_eq!(id.as_bytes()[14], b'4', "a random UUID: {id}");
        }
        ids.sort_by_key(Value::to_string);
        ids.dedup();
        assert_eq!(ids.len(), 3, "no two share one: {ids:?}");
        // A turn refused and queued behind has an id too, its own.
        f.start_thread(&tui, 2, json!({ "id": B, "status": { "type": "idle" } }))
            .await;
        let delivery = f.deliver(B, json!({}));
        let codex = async {
            f.respond_error("turn/start", json!({ "message": "busy" }))
                .await;
            f.respond("thread/queue/add", json!({})).await
        };
        let (_, queued) = tokio::join!(delivery, codex);
        assert!(!ids.contains(&queued["params"]["clientUserMessageId"]));
    });
}

#[test]
fn a_request_of_codexs_own_that_carries_the_id_of_a_delivery_is_not_its_answer() {
    // The server numbers its requests too. One that shares the id of the
    // delivery's request, with an error in it, must not decide the delivery.
    run(async {
        let f = Fixture::start().await;
        let tui = f.connect().await;
        f.start_thread(&tui, 1, json!({ "id": A })).await;
        let delivery = f.deliver(A, json!({}));
        let codex = async {
            wait(|| f.codex.is_held("thread/queue/add")).await;
            let id = f.codex.requests_of("thread/queue/add")[0]["id"].clone();
            f.codex.send_json(
                0,
                &json!({ "id": id, "method": "server/request", "params": {}, "error": { "message": "no" } }),
            );
            tokio::time::sleep(Duration::from_millis(100)).await;
            assert_eq!(f.broker.shared.pending_requests(), 1, "still waiting");
            f.respond("thread/queue/add", json!({})).await;
        };
        let (reply, ()) = tokio::join!(delivery, codex);
        assert_eq!(reply, admitted());
        assert_eq!(f.broker.shared.pending_requests(), 0);
    });
}

#[test]
fn a_delivery_in_flight_when_the_connection_to_codex_is_lost_is_uncertain() {
    run(async {
        let f = Fixture::start().await;
        let tui = f.connect().await;
        f.start_thread(&tui, 1, json!({ "id": A })).await;
        let delivery = f.deliver(A, json!({}));
        let lost = async {
            wait(|| f.codex.is_held("thread/queue/add")).await;
            f.codex.terminate(0);
        };
        let (reply, ()) = tokio::join!(delivery, lost);
        assert_eq!(reply, uncertain());
        assert_eq!(f.read().await["available"], false);
        assert_eq!(
            f.deliver(A, json!({})).await,
            refused("native-session-unavailable")
        );
    });
}

#[test]
fn the_checks_come_in_order_expired_then_unavailable_then_changed() {
    run(async {
        let f = Fixture::start().await;
        let expired = json!({ "expiresAt": now_ms() - 5.0 });
        // No thread shown: expired still comes first, then unavailable.
        assert_eq!(f.deliver(A, expired.clone()).await["error"], "expired");
        assert_eq!(
            f.deliver(A, json!({})).await["error"],
            "native-session-unavailable"
        );
        let tui = f.connect().await;
        f.start_thread(&tui, 1, json!({ "id": A })).await;
        // A thread shown: another thread's message is changed, unless it expired.
        assert_eq!(f.deliver(B, expired).await["error"], "expired");
        assert_eq!(
            f.deliver(B, json!({})).await["error"],
            "native-session-changed"
        );
    });
}

#[test]
fn a_deadline_is_any_finite_number() {
    run(async {
        let f = Fixture::start().await;
        let tui = f.connect().await;
        f.start_thread(&tui, 1, json!({ "id": A })).await;
        for expires in [json!(0), json!(-5), json!(1.5), json!(-1e300)] {
            let reply = f.deliver(A, json!({ "expiresAt": expires })).await;
            assert_eq!(reply["error"], "expired", "{expires}");
        }
        for invalid in [json!("soon"), json!(null), json!(true)] {
            let reply = f.deliver(A, json!({ "expiresAt": invalid })).await;
            assert_eq!(reply["error"], "invalid-record", "{invalid}");
        }
    });
}

#[test]
fn a_record_that_is_not_a_delivery_for_this_launch_is_refused_as_invalid() {
    run(async {
        let f = Fixture::start().await;
        let tui = f.connect().await;
        f.start_thread(&tui, 1, json!({ "id": A })).await;
        for overrides in [
            json!({ "sessionId": "not-a-uuid" }),
            json!({ "text": "" }),
            json!({ "text": 5 }),
            json!({ "text": "x".repeat(64 * 1024 + 1) }),
        ] {
            let reply = f.deliver(A, overrides.clone()).await;
            assert_eq!(reply, refused("invalid-record"), "{overrides}");
        }
        // The limit is in bytes: a text of exactly 64 KiB passes the record check.
        let at_limit = f
            .deliver(
                A,
                json!({ "text": "x".repeat(64 * 1024), "expiresAt": now_ms() + 300.0 }),
            )
            .await;
        assert_eq!(
            at_limit["admitted"],
            Value::Null,
            "taken as a delivery, which nobody answered"
        );
        assert!(f.codex.is_held("thread/queue/add"));
    });
}

#[test]
fn a_body_over_128_kib_is_refused_as_it_streams_in_and_one_at_it_is_a_record_to_judge() {
    run(async {
        let f = Fixture::start().await;
        let tui = f.connect().await;
        f.start_thread(&tui, 1, json!({ "id": A })).await;
        let big = f.post_deliver(&vec![b' '; 128 * 1024 + 1]).await;
        assert_eq!((big.status, big.json()), (413, refused("invalid-record")));
        // A body that never ends is refused with what came, not with all of it.
        let streamed = {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let mut stream = tokio::net::TcpStream::connect(f.address()).await.unwrap();
            let head = format!(
                "POST /deliver HTTP/1.1\r\nhost: x\r\nauthorization: Bearer {TOKEN}\r\ncontent-length: 900000000\r\n\r\n"
            );
            stream.write_all(head.as_bytes()).await.unwrap();
            stream.write_all(&vec![b' '; 200 * 1024]).await.unwrap();
            let mut seen = vec![0_u8; 4096];
            let read = stream.read(&mut seen).await.unwrap();
            String::from_utf8_lossy(&seen[..read]).into_owned()
        };
        assert!(streamed.starts_with("HTTP/1.1 413"), "{streamed}");
        // At the limit it is read whole, and is no delivery.
        let at = f.post_deliver(&vec![b' '; 128 * 1024]).await;
        assert_eq!((at.status, at.json()), (400, refused("invalid-record")));
    });
}

#[test]
fn routes_are_exact_and_every_request_needs_the_launchs_token() {
    run(async {
        let f = Fixture::start().await;
        let good = format!("Bearer {TOKEN}");
        let address = f.address();
        for (method, path) in [
            ("GET", "/other"),
            ("POST", "/session"),
            ("GET", "/deliver"),
            ("GET", "/session?x=1"),
            ("DELETE", "/session"),
            ("GET", "/"),
            // A target that names its host is not the path alone.
            ("GET", "http://127.0.0.1/session"),
            ("POST", "http://127.0.0.1/deliver"),
            ("GET", "http://localhost:1/session"),
        ] {
            let reply = http(address, method, path, &[("authorization", &good)], b"").await;
            assert_eq!(
                (reply.status, reply.json()),
                (404, refused("invalid-record")),
                "{method} {path}"
            );
        }
        // The same without a token, or with one that is not the launch's, whatever the route.
        let wrong = [
            None,
            Some("Bearer private-launch-token-123456789X".to_string()),
            Some(format!("Bearer {TOKEN}x")),
            Some(format!("bearer {TOKEN}")),
            Some(TOKEN.to_string()),
        ];
        for header in &wrong {
            for (method, path) in [
                ("GET", "/session"),
                ("POST", "/deliver"),
                ("GET", "/nothing"),
            ] {
                let headers: Vec<(&str, &str)> = header
                    .iter()
                    .map(|value| ("authorization", value.as_str()))
                    .collect();
                let reply = http(address, method, path, &headers, b"{}").await;
                assert_eq!(
                    (reply.status, reply.json()),
                    (401, refused("unauthorized")),
                    "{header:?} {method} {path}"
                );
            }
        }
        let session = http(address, "GET", "/session", &[("authorization", &good)], b"").await;
        assert_eq!(session.status, 200);
        assert_eq!(
            session.header("content-type").as_deref(),
            Some("application/json")
        );
        assert_eq!(session.header("cache-control").as_deref(), Some("no-store"));
        assert_eq!(
            session.json(),
            json!({ "launchId": "launch-1", "sessionId": null, "revision": 0, "empty": false, "available": false })
        );
    });
}

#[test]
fn the_session_view_reports_a_thread_its_revision_and_whether_a_delivery_would_be_taken() {
    run(async {
        let f = Fixture::start().await;
        let tui = f.connect().await;
        f.start_thread(
            &tui,
            1,
            json!({ "id": A, "turns": [], "status": { "type": "idle" } }),
        )
        .await;
        assert_eq!(
            f.read().await,
            json!({ "launchId": "launch-1", "sessionId": A, "revision": 1, "empty": true, "available": true })
        );
    });
}

#[test]
fn a_request_that_never_finishes_its_headers_or_its_body_is_ended_after_five_seconds() {
    run(async {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let f = Fixture::start().await;
        let started = std::time::Instant::now();
        // Headers that never end.
        let mut slow = tokio::net::TcpStream::connect(f.address()).await.unwrap();
        slow.write_all(b"POST /deliver HTTP/1.1\r\nhost: x\r\n")
            .await
            .unwrap();
        // A body that never comes.
        let mut stalled = tokio::net::TcpStream::connect(f.address()).await.unwrap();
        let head = format!(
            "POST /deliver HTTP/1.1\r\nhost: x\r\nauthorization: Bearer {TOKEN}\r\ncontent-length: 50\r\n\r\n{{"
        );
        stalled.write_all(head.as_bytes()).await.unwrap();
        let (mut one, mut two) = (Vec::new(), Vec::new());
        let (a, b) = tokio::join!(slow.read_to_end(&mut one), stalled.read_to_end(&mut two));
        a.unwrap();
        b.unwrap();
        let took = started.elapsed();
        assert!(
            took >= Duration::from_millis(4900) && took < Duration::from_secs(9),
            "{took:?}"
        );
        assert!(
            String::from_utf8_lossy(&two).starts_with("HTTP/1.1 408"),
            "{:?}",
            String::from_utf8_lossy(&two)
        );
    });
}

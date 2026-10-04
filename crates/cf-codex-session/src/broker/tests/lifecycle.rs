//! How a broker starts, loses Codex's server, and closes.

use std::time::{Duration, Instant};

use cf_proto::codex::{DeliveryReply, Refusal};
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use super::fixture::{
    config_for, run, wait, wait_for, Behaviour, FakeCodex, Fixture, Options, A, B, TEXT, TOKEN,
};
use crate::broker::{now_ms, Broker, StartError};

#[test]
fn does_not_start_on_a_codex_server_that_will_not_initialize_nor_on_a_port_already_taken() {
    run(async {
        let refusing = FakeCodex::start(Behaviour::Refusing).await;
        let failed = Broker::start(config_for(&refusing, Options::default()))
            .await
            .err();
        assert_eq!(
            failed.map(|cause| cause.to_string()).as_deref(),
            Some("Codex native server did not initialize")
        );

        let f = Fixture::start().await;
        let connections = f.codex.connections();
        let mut config = config_for(&f.codex, Options::default());
        config.bridge.port = f.broker.port();
        let failed = Broker::start(config).await.err();
        assert!(
            matches!(&failed, Some(StartError::Listen { cause, .. })
                if cause.kind() == std::io::ErrorKind::AddrInUse),
            "{:?}",
            failed.map(|cause| cause.to_string())
        );
        // Neither leaves its own connection to Codex's server open.
        wait(|| refusing.all_closed() && !f.codex.is_open(connections)).await;
        assert!(
            f.codex.is_open(0),
            "the broker that was running is untouched"
        );
    });
}

#[test]
fn does_not_start_on_a_codex_server_that_does_not_answer_initialize_in_three_seconds() {
    run(async {
        let silent = FakeCodex::start(Behaviour::Silent).await;
        let started = Instant::now();
        let failed = Broker::start(config_for(&silent, Options::default()))
            .await
            .err();
        let took = started.elapsed();
        assert_eq!(
            failed.map(|cause| cause.to_string()).as_deref(),
            Some("Codex native server did not initialize")
        );
        assert!(
            took >= Duration::from_millis(2900) && took < Duration::from_secs(10),
            "{took:?}"
        );
        wait(|| silent.all_closed()).await;
    });
}

#[test]
fn does_not_start_where_codexs_server_cannot_be_reached() {
    run(async {
        let gone = FakeCodex::start(Behaviour::Ready).await;
        let config = config_for(&gone, Options::default());
        drop(gone);
        // Its listener is gone with it, once its task has ended.
        tokio::task::yield_now().await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        let failed = Broker::start(config).await.err();
        assert!(
            matches!(failed, Some(StartError::Upstream(_))),
            "{:?}",
            failed.map(|cause| cause.to_string())
        );
    });
}

#[test]
fn announces_itself_to_codexs_server_as_its_delivery_client() {
    run(async {
        let f = Fixture::start().await;
        wait(|| f.codex.requests().len() == 2).await;
        let requests = f.codex.requests();
        assert_eq!(requests[0]["method"], "initialize");
        assert_eq!(
            requests[0]["params"],
            json!({
                "clientInfo": { "name": "consensflow-delivery", "version": "3.0.0" },
                "capabilities": { "experimentalApi": true },
            })
        );
        assert_eq!(requests[1], json!({ "method": "initialized" }));
        assert_eq!(
            f.read().await["available"],
            false,
            "ready, but no thread shown yet"
        );
    });
}

#[test]
fn closing_refuses_new_work_forgets_the_selection_ends_every_socket_and_stops_the_server() {
    run(async {
        let f = Fixture::start().await;
        let tui = f.connect().await;
        f.start_thread(&tui, 1, json!({ "id": A })).await;
        let picker = f.connect().await;
        let port = f.broker.port();
        f.broker.close().await;
        wait(|| tui.is_closed() && picker.is_closed()).await;
        wait(|| f.codex.all_closed()).await;
        assert!(
            TcpStream::connect(("127.0.0.1", port)).await.is_err(),
            "nothing listens"
        );
        // What was chosen is forgotten, and nothing more is taken.
        assert_eq!(f.broker.shared.state.borrow().session_id(), None);
        let delivery = cf_proto::codex::Delivery {
            launch_id: "launch-1".into(),
            session_id: A.into(),
            text: TEXT.into(),
            expires_at: now_ms() + 2000.0,
        };
        assert_eq!(
            f.broker.shared.deliver(&delivery).await,
            DeliveryReply::Refused(Refusal::NativeSessionUnavailable)
        );
        // Closing again is no harm.
        f.broker.close().await;
    });
}

#[test]
fn closing_ends_a_delivery_still_waiting_for_codex() {
    run(async {
        let f = Fixture::start().await;
        let tui = f.connect().await;
        f.start_thread(&tui, 1, json!({ "id": A })).await;
        let address = f.address();
        let waiting = tokio::task::spawn_local(async move {
            let body = json!({
                "launchId": "launch-1", "sessionId": A, "text": TEXT, "expiresAt": now_ms() + 2000.0,
            })
            .to_string();
            let mut stream = TcpStream::connect(address).await.unwrap();
            let request = format!(
                "POST /deliver HTTP/1.1\r\nhost: x\r\nauthorization: Bearer {TOKEN}\r\ncontent-length: {}\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(request.as_bytes()).await.unwrap();
            let mut reply = Vec::new();
            let _ = stream.read_to_end(&mut reply).await;
            reply
        });
        wait(|| f.codex.is_held("thread/queue/add")).await;
        f.broker.close().await;
        // Its connection ended with the broker, and nothing was said on it.
        assert_eq!(waiting.await.unwrap(), Vec::<u8>::new());
    });
}

#[test]
fn a_lost_connection_to_codexs_server_stops_deliveries_but_not_the_window() {
    run(async {
        let f = Fixture::start().await;
        let tui = f.connect().await;
        f.start_thread(&tui, 1, json!({ "id": A })).await;
        assert_eq!(f.read().await["available"], true);
        f.codex.terminate(0);
        wait_for(|| async { f.read().await["available"] == false }).await;
        let shown = f.read().await;
        assert_eq!(shown["sessionId"], A, "the window still shows its thread");
        assert_eq!(
            f.deliver(A, json!({})).await["error"],
            "native-session-unavailable"
        );
        // The TUI's own connection goes on: it speaks to Codex through its pair.
        tui.send(
            json!({ "id": 2, "method": "turn/start", "params": { "threadId": A, "input": [] } }),
        );
        wait(|| f.codex.is_held_by_id(&json!(2))).await;
        assert!(!tui.is_closed());
        // And a switch it makes is learned, though nothing can be delivered after it.
        f.call(
            &tui,
            3,
            "thread/resume",
            json!({ "threadId": B, "runtimeWorkspaceRoots": [] }),
            json!({ "thread": { "id": B } }),
        )
        .await;
        let shown = f.read().await;
        assert_eq!(
            (shown["sessionId"].clone(), shown["available"].clone()),
            (json!(B), json!(false))
        );
    });
}

//! The sockets: what passes through a pair, in what order, how big, and what
//! ends it.

use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::{Bytes, Message};

use super::fixture::{
    config_for, http, run, wait, wait_for, Behaviour, FakeCodex, Fixture, Options, Tui, A, B, TOKEN,
};
use crate::broker::Broker;
use crate::endpoint::{Target, Upstream};

/// A JSON request with `size` bytes of padding in it.
fn padded(id: &str, size: usize) -> String {
    format!(
        r#"{{"id":"{id}","method":"pad","params":{{"padding":"{}"}}}}"#,
        "x".repeat(size)
    )
}

fn authorization() -> String {
    format!("Bearer {TOKEN}")
}

#[test]
fn a_connection_that_is_not_speaking_json_is_closed_and_the_thread_it_chose_is_forgotten() {
    run(async {
        let f = Fixture::start().await;
        let tui = f.connect().await;
        f.start_thread(&tui, 1, json!({ "id": A })).await;
        assert_eq!(f.read().await["sessionId"], A);
        tui.send_text("not json");
        wait(|| tui.is_closed()).await;
        assert_eq!(f.read().await["sessionId"], Value::Null);
        // The same from Codex's end of a window's connection.
        let next = f.connect().await;
        f.start_thread(&next, 2, json!({ "id": B })).await;
        assert_eq!(f.read().await["sessionId"], B);
        f.codex.send_text(f.codex.connections() - 1, "not json");
        wait(|| next.is_closed()).await;
        assert_eq!(f.read().await["sessionId"], Value::Null);
        // And from Codex's end of the broker's own: nothing can be delivered then.
        let window = f.connect().await;
        f.start_thread(&window, 3, json!({ "id": A })).await;
        assert_eq!(f.read().await["available"], true);
        f.codex.send_text(0, "not json");
        wait(|| !f.codex.is_open(0)).await;
        assert_eq!(f.read().await["available"], false);
        assert_eq!(
            f.deliver(A, json!({})).await["error"],
            "native-session-unavailable"
        );
    });
}

#[test]
fn frames_the_tui_sends_before_its_connection_to_codex_opens_are_held_and_sent_in_order() {
    run(async {
        let f = Fixture::start().await;
        // The broker's own `initialize` and `initialized` have come.
        wait(|| f.codex.requests().len() == 2).await;
        f.codex.delay_handshakes(Duration::from_millis(400));
        let before = f.codex.requests().len();
        let tui = Tui::connect(f.address(), "/", Some(&authorization()))
            .await
            .unwrap();
        tui.send(json!({ "id": "a", "method": "thread/start", "params": { "ephemeral": false, "threadSource": "user" } }));
        tui.send(json!({ "id": "b", "method": "one" }));
        tui.send(json!({ "id": "c", "method": "two" }));
        // What the TUI says is read when it goes to Codex, not before.
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(f.read().await["revision"], 0);
        assert_eq!(f.codex.requests().len(), before, "nothing has passed yet");
        wait(|| f.codex.requests().len() == before + 3).await;
        let ids: Vec<_> = f.codex.requests()[before..]
            .iter()
            .map(|r| r["id"].clone())
            .collect();
        assert_eq!(ids, [json!("a"), json!("b"), json!("c")]);
        assert_eq!(f.read().await["revision"], 1);
    });
}

#[test]
fn frames_held_for_a_connection_that_never_opens_end_the_pair_at_64_mib() {
    run(async {
        let f = Fixture::start().await;
        f.codex.delay_handshakes(Duration::from_secs(60));
        let tui = Tui::connect(f.address(), "/", Some(&authorization()))
            .await
            .unwrap();
        for at in 0..4 {
            tui.send_text(&padded(&at.to_string(), 20 * 1024 * 1024));
        }
        wait(|| tui.is_closed()).await;
    });
}

#[test]
fn a_connection_to_codex_that_does_not_open_in_three_seconds_ends_the_pair() {
    run(async {
        let f = Fixture::start().await;
        f.codex.delay_handshakes(Duration::from_secs(10));
        let started = Instant::now();
        let tui = Tui::connect(f.address(), "/", Some(&authorization()))
            .await
            .unwrap();
        wait(|| tui.is_closed()).await;
        let took = started.elapsed();
        assert!(
            took >= Duration::from_millis(2900) && took < Duration::from_secs(8),
            "{took:?}"
        );
    });
}

#[test]
fn a_ping_from_either_end_is_answered_and_never_ends_the_pair() {
    run(async {
        let f = Fixture::start().await;
        let tui = f.connect().await;
        tui.send_message(Message::Ping(Bytes::from_static(b"are you there")));
        wait(|| tui.pongs() == 1).await;
        f.codex
            .send_message(1, Message::Ping(Bytes::from_static(b"and you")));
        wait(|| f.codex.pongs(1) == 1).await;
        // Both ends are still joined: a frame each way passes.
        tui.send(json!({ "id": "after", "method": "ping-test" }));
        wait(|| f.codex.is_held_by_id(&json!("after"))).await;
        f.codex
            .send_json(1, &json!({ "method": "hello", "params": {} }));
        wait(|| tui.has_seen(|message| message["method"] == "hello")).await;
        assert!(!tui.is_closed() && f.codex.is_open(1));
        // The same on the broker's own connection.
        f.codex.send_message(0, Message::Ping(Bytes::new()));
        wait(|| f.codex.pongs(0) == 1).await;
        assert_eq!(
            f.read().await["available"],
            false,
            "no thread yet, but its connection is up"
        );
        f.start_thread(&tui, 1, json!({ "id": A })).await;
        assert_eq!(f.read().await["available"], true);
    });
}

#[test]
fn a_binary_message_is_read_as_json_and_passed_on_as_text() {
    run(async {
        let f = Fixture::start().await;
        let tui = f.connect().await;
        let start = json!({
            "id": 1,
            "method": "thread/start",
            "params": { "ephemeral": false, "threadSource": "user" },
        });
        tui.send_message(Message::Binary(Bytes::from(start.to_string())));
        wait(|| f.codex.is_held("thread/start")).await;
        assert_eq!(f.codex.requests_of("thread/start")[0], start);
        let peer = f.codex.peer_of("thread/start");
        f.respond("thread/start", json!({ "thread": { "id": A } }))
            .await;
        // And Codex's own binary message reaches the TUI as the JSON it is.
        f.codex.send_message(
            peer,
            Message::Binary(Bytes::from(
                json!({ "method": "later", "params": 1 }).to_string(),
            )),
        );
        wait(|| tui.has_seen(|message| message["method"] == "later")).await;
        assert_eq!(f.read().await["sessionId"], A);
        // A binary message that is no text ends the pair like any other garbage.
        tui.send_message(Message::Binary(Bytes::from_static(&[0xFF, 0xFE, 0x00])));
        wait(|| tui.is_closed()).await;
    });
}

#[test]
fn messages_of_20_mib_pass_both_ways_where_a_frame_limit_of_16_mib_would_stop_them() {
    run(async {
        let f = Fixture::start().await;
        let tui = f.connect().await;
        tui.send_text(&padded("up", 20 * 1024 * 1024));
        wait(|| f.codex.is_held_by_id(&json!("up"))).await;
        let held = f.codex.requests_of("pad");
        assert_eq!(
            held[0]["params"]["padding"].as_str().map(str::len),
            Some(20 * 1024 * 1024)
        );
        f.codex.send_text(
            1,
            json!({ "method": "down", "params": { "padding": "y".repeat(20 * 1024 * 1024) } })
                .to_string(),
        );
        wait(|| tui.has_seen(|message| message["method"] == "down")).await;
        assert!(!tui.is_closed() && f.codex.is_open(1));
    });
}

#[test]
fn a_message_over_64_mib_ends_the_pair_whichever_end_sent_it() {
    run(async {
        let f = Fixture::start().await;
        let tui = f.connect().await;
        f.start_thread(&tui, 1, json!({ "id": A })).await;
        tui.send_text(&padded("huge", 64 * 1024 * 1024 + 1024));
        wait(|| tui.is_closed()).await;
        assert!(!f
            .codex
            .requests()
            .iter()
            .any(|request| request["id"] == "huge"));
        assert_eq!(
            f.read().await["sessionId"],
            Value::Null,
            "the owner's end forgot the thread"
        );
        wait(|| !f.codex.is_open(1)).await;

        let again = f.connect().await;
        f.start_thread(&again, 2, json!({ "id": A })).await;
        let peer = f.codex.connections() - 1;
        f.codex
            .send_text(peer, padded("from-codex", 64 * 1024 * 1024 + 1024));
        wait(|| again.is_closed()).await;
        wait(|| !f.codex.is_open(peer)).await;
    });
}

#[test]
// The pressure this needs is the system's: Windows' loopback takes far more
// than 64 MiB from a peer that reads nothing, so what waits unsent never builds.
#[cfg_attr(
    windows,
    ignore = "Windows' loopback absorbs what a peer does not read"
)]
fn a_socket_that_takes_nothing_ends_its_pair_once_64_mib_wait_unsent_for_it() {
    run(async {
        let f = Fixture::start().await;
        f.codex.stall();
        let tui = Tui::connect(f.address(), "/", Some(&authorization()))
            .await
            .unwrap();
        // Three of 20 MiB wait; the fourth would make it more than 64 MiB.
        for at in 0..4 {
            tui.send_text(&padded(&at.to_string(), 20 * 1024 * 1024));
        }
        wait(|| tui.is_closed()).await;
        assert_eq!(
            f.read().await["available"],
            false,
            "no thread yet, but the broker goes on"
        );
        // The server that reads nothing cannot tell its end is gone; the broker's pair is.
        assert!(f.broker.shared.pairs.borrow().is_empty());
        assert!(f.codex.is_open(0));
    });
}

#[test]
fn what_cannot_be_forwarded_to_the_tui_ends_the_pair_too() {
    run(async {
        let f = Fixture::start().await;
        let tui = f.connect().await;
        // The TUI's end goes away without telling: Codex's next message cannot be sent.
        tui.terminate();
        wait(|| tui.is_closed()).await;
        wait(|| !f.codex.is_open(1)).await;
    });
}

#[test]
fn every_connection_to_codex_carries_the_authorization_its_endpoint_asks_for() {
    run(async {
        let codex = FakeCodex::start(Behaviour::Ready).await;
        let mut config = config_for(&codex, Options::default());
        config.upstream = Upstream {
            target: Target::Tcp(codex.address),
            authorization: Some("Bearer window-secret".into()),
        };
        let broker = Broker::start(config).await.unwrap();
        let address = std::net::SocketAddr::from(([127, 0, 0, 1], broker.port()));
        let tui = Tui::connect(address, "/", Some(&authorization()))
            .await
            .unwrap();
        tui.send(json!({ "id": 1, "method": "initialize", "params": {} }));
        wait(|| tui.has_answered(&json!(1))).await;
        for peer in 0..2 {
            let header = |name: &str| {
                codex
                    .headers(peer)
                    .into_iter()
                    .find(|(found, _)| found == name)
                    .map(|(_, value)| value)
            };
            assert_eq!(
                header("authorization").as_deref(),
                Some("Bearer window-secret"),
                "connection {peer}"
            );
        }
    });
}

#[test]
fn no_connection_offers_or_accepts_compression() {
    run(async {
        let f = Fixture::start().await;
        let _tui = f.connect().await;
        for peer in 0..2 {
            let headers = f.codex.headers(peer);
            assert!(
                !headers.iter().any(|(name, _)| name.contains("extensions")),
                "connection {peer}: {headers:?}"
            );
        }
        // A TUI that offers it is not echoed it: the 101 names no extension.
        let (head, _) = handshake(
            &f,
            &[(
                "sec-websocket-extensions",
                Some("permessage-deflate; client_max_window_bits"),
            )],
        )
        .await;
        assert!(head.starts_with("http/1.1 101"), "{head}");
        assert!(
            head.contains("sec-websocket-accept: s3pplmbitxaq9kygzzhzrbk+xoo="),
            "{head}"
        );
        assert!(!head.contains("extensions"), "{head}");
    });
}

/// The head (in lower case) and the body of the broker's answer to a TUI's
/// upgrade request, which has the headers of a good one but for `changes`: a
/// header set, or taken away.
async fn handshake(f: &Fixture, changes: &[(&str, Option<&str>)]) -> (String, String) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let authorized = authorization();
    let mut headers = vec![
        ("host", Some("x")),
        ("connection", Some("Upgrade")),
        ("upgrade", Some("websocket")),
        ("sec-websocket-version", Some("13")),
        ("sec-websocket-key", Some("dGhlIHNhbXBsZSBub25jZQ==")),
        ("authorization", Some(authorized.as_str())),
    ];
    for (name, value) in changes {
        headers.retain(|(kept, _)| kept != name);
        headers.push((name, *value));
    }
    let mut request = String::from("GET / HTTP/1.1\r\n");
    for (name, value) in headers {
        if let Some(value) = value {
            request.push_str(&format!("{name}: {value}\r\n"));
        }
    }
    request.push_str("\r\n");
    let mut stream = tokio::net::TcpStream::connect(f.address()).await.unwrap();
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut head = Vec::new();
    let mut byte = [0_u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        stream.read_exact(&mut byte).await.unwrap();
        head.push(byte[0]);
    }
    let head = String::from_utf8(head).unwrap().to_ascii_lowercase();
    // A refusal carries what it says; an upgrade says no more.
    let length = head
        .lines()
        .find_map(|line| line.strip_prefix("content-length: "))
        .map_or(0, |length| length.trim().parse().unwrap());
    let mut body = vec![0; length];
    stream.read_exact(&mut body).await.unwrap();
    (head, String::from_utf8(body).unwrap())
}

#[test]
fn a_frame_sent_with_the_upgrade_request_is_not_lost() {
    // A TUI need not wait for the 101 before it speaks: what it sent after its
    // request was read with it, and is the proxy's all the same.
    run(async {
        use tokio::io::AsyncWriteExt;
        let f = Fixture::start().await;
        let mut stream = tokio::net::TcpStream::connect(f.address()).await.unwrap();
        let json = r#"{"id":"early","method":"early"}"#;
        let mut bytes = format!(
            "GET / HTTP/1.1\r\nhost: x\r\nconnection: Upgrade\r\nupgrade: websocket\r\n\
             sec-websocket-version: 13\r\nsec-websocket-key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
             authorization: {}\r\n\r\n",
            authorization()
        )
        .into_bytes();
        // One masked text frame (the mask is all zeros), in the same write.
        bytes.extend([0x81, 0x80 | u8::try_from(json.len()).unwrap(), 0, 0, 0, 0]);
        bytes.extend(json.as_bytes());
        stream.write_all(&bytes).await.unwrap();
        wait(|| f.codex.is_held_by_id(&json!("early"))).await;
    });
}

#[test]
fn the_first_subprotocol_a_tui_offers_is_the_one_answered() {
    run(async {
        let f = Fixture::start().await;
        let answered = |head: &str| {
            head.lines()
                .find_map(|line| line.strip_prefix("sec-websocket-protocol: "))
                .map(str::to_string)
        };
        for (offered, first) in [
            ("chat, superchat", "chat"),
            ("chat,superchat", "chat"),
            ("ok.two ,\tother", "ok.two"),
            ("a", "a"),
        ] {
            let (head, _) = handshake(&f, &[("sec-websocket-protocol", Some(offered))]).await;
            assert!(head.starts_with("http/1.1 101"), "{offered}: {head}");
            assert_eq!(answered(&head).as_deref(), Some(first), "{offered}");
        }
        let (head, _) = handshake(&f, &[]).await;
        assert!(
            head.starts_with("http/1.1 101") && answered(&head).is_none(),
            "{head}"
        );
    });
}

#[test]
fn a_handshake_that_is_not_a_websocket_upgrade_is_refused_as_ws_refuses_it_not_dropped() {
    run(async {
        let f = Fixture::start().await;
        // Version 8 is as good as 13 to the `ws` package the broker was; any other is not.
        let (head, _) = handshake(&f, &[("sec-websocket-version", Some("8"))]).await;
        assert!(head.starts_with("http/1.1 101"), "{head}");
        let version = "Missing or invalid Sec-WebSocket-Version header";
        let key = "Missing or invalid Sec-WebSocket-Key header";
        let protocol = "Invalid Sec-WebSocket-Protocol header";
        for (name, value, said) in [
            ("sec-websocket-version", Some("7"), version),
            ("sec-websocket-version", Some("14"), version),
            ("sec-websocket-version", Some("x"), version),
            ("sec-websocket-version", None, version),
            ("sec-websocket-key", Some("not a key"), key),
            ("sec-websocket-key", Some(""), key),
            ("sec-websocket-key", None, key),
            ("upgrade", Some("h2c"), "Invalid Upgrade header"),
            ("sec-websocket-protocol", Some("chat chat"), protocol),
            ("sec-websocket-protocol", Some("a,a"), protocol),
            ("sec-websocket-protocol", Some("a,"), protocol),
            ("sec-websocket-protocol", Some(",a"), protocol),
            ("sec-websocket-protocol", Some("bad(one"), protocol),
        ] {
            let (head, body) = handshake(&f, &[(name, value)]).await;
            assert!(
                head.starts_with("http/1.1 400 "),
                "{name}={value:?}: {head}"
            );
            assert_eq!(body, said, "{name}={value:?}");
            assert!(head.contains("connection: close"), "{head}");
            assert!(head.contains("content-type: text/html"), "{head}");
            assert_eq!(
                head.contains("sec-websocket-version: 13, 8"),
                said == version,
                "{head}"
            );
        }
        // Not a GET: 405.
        let authorized = authorization();
        let headers = [
            ("authorization", authorized.as_str()),
            ("connection", "Upgrade"),
            ("upgrade", "websocket"),
        ];
        let reply = http(f.address(), "POST", "/", &headers, b"").await;
        assert_eq!(reply.status, 405);
        assert_eq!(reply.body, b"Invalid HTTP method");
    });
}

#[test]
fn a_tui_that_says_goodbye_is_answered_in_kind() {
    run(async {
        let f = Fixture::start().await;
        let tui = f.connect().await;
        tui.close();
        wait(|| tui.heard_goodbye()).await;
        wait(|| tui.is_closed()).await;
        wait(|| !f.codex.is_open(1)).await;
    });
}

#[test]
fn json_nested_deeper_than_serde_json_reads_goes_on_as_it_came_and_ends_nothing() {
    // `JSON.parse` has no limit of depth, and Codex's tools may send what is
    // deep: valid JSON the broker cannot learn from is not JSON it ends a
    // connection for.
    run(async {
        let deep = format!("{}{}", "[".repeat(200), "]".repeat(200));
        let f = Fixture::start().await;
        let tui = f.connect().await;
        f.start_thread(&tui, 1, json!({ "id": A })).await;
        let up = format!(r#"{{"id":"deep","method":"deep","params":{deep}}}"#);
        tui.send_text(&up);
        wait(|| f.codex.texts().contains(&(1, up.clone()))).await;
        let down = format!(r#"{{"method":"deep","params":{deep}}}"#);
        f.codex.send_text(1, down.clone());
        wait(|| tui.texts().contains(&down)).await;
        // On the broker's own connection it is passed over.
        f.codex.send_text(0, down);
        let (delivered, _) = tokio::join!(
            f.deliver(A, json!({})),
            f.respond("thread/queue/add", json!({})),
        );
        assert_eq!(delivered, json!({ "ok": true, "admitted": true }));
        assert!(!tui.is_closed() && f.codex.is_open(0) && f.codex.is_open(1));
        assert_eq!(f.read().await["sessionId"], A);
    });
}

#[test]
fn frames_held_for_a_pair_that_ends_are_not_processed_after_it() {
    // A TUI whose connection to Codex opens late has said what is no JSON and
    // then started a thread: the pair ends at the first, and the second
    // changes nothing of the window another TUI chose.
    run(async {
        let f = Fixture::start().await;
        let owner = f.connect().await;
        f.start_thread(&owner, 1, json!({ "id": A })).await;
        let before = f.read().await;
        f.codex.delay_handshakes(Duration::from_millis(300));
        let late = Tui::connect(f.address(), "/", Some(&authorization()))
            .await
            .unwrap();
        late.send_text("not json");
        late.send(json!({
            "id": 9,
            "method": "thread/start",
            "params": { "ephemeral": false, "threadSource": "user" },
        }));
        wait(|| late.is_closed()).await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(f.read().await, before, "the window shows what it showed");
        assert!(!f.codex.requests().iter().any(|request| request["id"] == 9));
    });
}

#[cfg(unix)]
#[test]
fn the_broker_reaches_codex_over_a_unix_socket_too() {
    run(async {
        let dir = tempfile::Builder::new()
            .prefix("cf-sock-")
            .tempdir_in("/tmp")
            .unwrap();
        let socket = dir.path().join("native.sock");
        let codex = FakeCodex::start_on_socket(&socket).await;
        let mut config = config_for(&codex, Options::default());
        config.upstream = Upstream {
            target: Target::Unix(socket),
            authorization: None,
        };
        let broker = Broker::start(config).await.unwrap();
        let address = std::net::SocketAddr::from(([127, 0, 0, 1], broker.port()));
        let tui = Tui::connect(address, "/", Some(&authorization()))
            .await
            .unwrap();
        tui.send(json!({
            "id": 1,
            "method": "thread/start",
            "params": { "ephemeral": false, "threadSource": "user" },
        }));
        codex
            .respond("thread/start", json!({ "thread": { "id": A } }), None)
            .await;
        wait(|| tui.has_answered(&json!(1))).await;
        let session = http(
            address,
            "GET",
            "/session",
            &[("authorization", &authorization())],
            b"",
        )
        .await
        .json();
        assert_eq!(session["sessionId"], A);
        assert_eq!(session["available"], true);
        // Its own connection and the TUI's, two, on the one socket.
        assert_eq!(codex.connections(), 2);
    });
}

#[test]
fn a_pair_ends_as_a_whole_from_either_side() {
    run(async {
        let f = Fixture::start().await;
        let tui = f.connect().await;
        let other = f.connect().await;
        tui.close();
        wait(|| tui.is_closed()).await;
        wait(|| !f.codex.is_open(1)).await;
        assert!(
            f.codex.is_open(2) && !other.is_closed(),
            "the other pair is its own"
        );
        f.codex.terminate(2);
        wait(|| other.is_closed()).await;
        wait_for(|| async { f.read().await["available"] == false }).await;
        assert!(
            f.codex.is_open(0),
            "the broker's own connection is not a pair's"
        );
    });
}

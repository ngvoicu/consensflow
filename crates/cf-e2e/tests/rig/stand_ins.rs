//! The stand-ins themselves, where the cases that run them do not reach: the
//! stand-in Codex's server on loopback, the transport Windows' supervisor uses
//! (on Unix it listens on a private socket, which the supervisor's cases cover).
//! A bearer token whose SHA-256 it was given gets in, and any other, or none,
//! is turned away at the handshake.

use std::thread;
use std::time::{Duration, Instant};

use cf_e2e::files;
use cf_e2e::process::Run;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tungstenite::client::IntoClientRequest;
use tungstenite::http::HeaderValue;
use tungstenite::Message;

use crate::Outcome;

const FAKE_CODEX: &str = env!("CARGO_BIN_EXE_fake-codex");

/// The token the stand-in's server lets in, and the hash it is given.
const TOKEN: &str = "capability-token-for-the-test";

/// The request to connect to `port` with `authorization` as the header, if any.
fn request_to(
    port: u16,
    authorization: Option<&str>,
) -> Result<tungstenite::handshake::client::Request, Box<dyn std::error::Error>> {
    let mut request = format!("ws://127.0.0.1:{port}").into_client_request()?;
    if let Some(value) = authorization {
        request
            .headers_mut()
            .insert("authorization", HeaderValue::from_str(value)?);
    }
    Ok(request)
}

#[test]
fn the_stand_in_codexs_server_on_loopback_lets_in_only_the_token_whose_hash_it_was_given() -> Outcome
{
    let _turn = cf_e2e::serial::turn();
    let root = tempfile::tempdir()?;
    let log = root.path().join("codex.jsonl");
    let hash: String = Sha256::digest(TOKEN.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let server = Run::new(FAKE_CODEX)
        .args(["app-server", "--listen", "ws://127.0.0.1:0"])
        .args(["--ws-auth", "capability-token", "--ws-token-sha256", &hash])
        .var("CF_TEST_CODEX_LOG", &log)
        .closed_input()
        .spawn()?;
    // It takes a port, and says which on its error output.
    let started = Instant::now();
    let port = loop {
        let said = server.errors();
        if let Some(port) = said
            .split("listening on: ws://127.0.0.1:")
            .nth(1)
            .and_then(|rest| rest.split_whitespace().next())
            .and_then(|port| port.parse::<u16>().ok())
        {
            break port;
        }
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "no port: {said}"
        );
        thread::sleep(Duration::from_millis(20));
    };

    // The token it was given the hash of gets in, and starts a thread.
    let (mut socket, _) =
        tungstenite::connect(request_to(port, Some(&format!("Bearer {TOKEN}")))?)?;
    socket.send(Message::text(
        json!({ "id": 7, "method": "thread/start", "params": { "ephemeral": false } }).to_string(),
    ))?;
    let Message::Text(answer) = socket.read()? else {
        panic!("the server did not answer with text");
    };
    let answer: Value = serde_json::from_str(answer.as_str())?;
    assert_eq!(answer["id"], 7);
    assert_eq!(
        answer["result"]["thread"],
        json!({ "id": "01a09094-938f-7fd1-a2d3-315cf92b4559", "turns": [], "status": { "type": "idle" } })
    );
    let log = files::read_string(&log)?;
    assert!(log.contains("\"started\":{\"ephemeral\":false}"), "{log}");

    // Any other token, a token that is not a bearer's, and none are turned away.
    for authorization in [
        Some("Bearer another-token"),
        Some("Basic abc"),
        Some(TOKEN),
        None,
    ] {
        let refused = tungstenite::connect(request_to(port, authorization)?);
        assert!(
            matches!(&refused, Err(tungstenite::Error::Http(response)) if response.status() == 401),
            "{authorization:?}: {:?}",
            refused.err()
        );
    }
    Ok(())
}

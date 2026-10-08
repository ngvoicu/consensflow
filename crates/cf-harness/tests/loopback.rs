//! The system's `Loopback` against Node's own `http` server, the peer it
//! is frozen against: it refuses what hyper's server forgives, and is the
//! kind of peer OpenCode's plugin and server are. The server writes back
//! what it was sent, so each test reads the request as a peer read it.
//! Node is the tests' own, as the build tooling has it, and not the
//! product's: `CF_TEST_NODE`, or `node` on the PATH.

// The test starts the peer it asks; a failure in its helpers is the test's.
#![allow(clippy::disallowed_methods, clippy::expect_used, clippy::unwrap_used)]

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};

use cf_harness::seams::loopback::{BodyFailed, Loopback, Method, Request, SystemLoopback};
use serde_json::Value;

/// A server that writes back what it was sent, and a few that answer
/// otherwise: a refusal, a body in chunks, one cut short, one with no body.
const SERVER: &str = r"
import { createServer } from 'node:http'
const server = createServer((request, response) => {
  let body = ''
  request.on('data', (chunk) => { body += chunk })
  request.on('end', () => {
    const header = (name) => request.headers[name] ?? null
    if (request.url === '/unauthorized') {
      response.writeHead(401, { 'content-type': 'text/plain' })
      return response.end('no')
    }
    if (request.url === '/chunked') {
      response.writeHead(200)
      response.write('abc')
      return setTimeout(() => response.end('de'), 20)
    }
    if (request.url === '/cut') {
      response.writeHead(200, { 'content-length': '10' })
      response.write('0123')
      return setTimeout(() => response.socket.destroy(), 20)
    }
    if (request.url === '/empty') {
      response.writeHead(204)
      return response.end()
    }
    response.writeHead(200, { 'content-type': 'application/json' })
    response.end(JSON.stringify({
      method: request.method,
      url: request.url,
      host: header('host'),
      authorization: header('authorization'),
      type: header('content-type'),
      length: header('content-length'),
      encoding: header('accept-encoding'),
      body,
    }))
  })
})
server.listen(0, '127.0.0.1', () => console.log(server.address().port))
";

/// Node's server, running until dropped, and the port it listens on.
struct Peer {
    node: Child,
    port: u16,
}

impl Peer {
    fn start() -> Self {
        let program = std::env::var_os("CF_TEST_NODE").unwrap_or_else(|| "node".into());
        let mut node = Command::new(program)
            .args(["--input-type=module", "-e", SERVER])
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("node, to serve the test");
        let mut line = String::new();
        BufReader::new(node.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        let port = line.trim().parse().unwrap();
        Self { node, port }
    }

    fn url(&self, target: &str) -> String {
        format!("http://127.0.0.1:{}{target}", self.port)
    }
}

impl Drop for Peer {
    fn drop(&mut self) {
        let _ = self.node.kill();
        let _ = self.node.wait();
    }
}

fn block_on<T>(work: impl std::future::Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(work)
}

/// The status and body of `request`, its body within `limit`.
fn ask(request: Request, limit: usize) -> (u16, Result<Vec<u8>, BodyFailed>) {
    block_on(async {
        let mut reply = SystemLoopback.send(request).await.unwrap();
        (reply.status(), reply.body(limit).await)
    })
}

fn get(url: String) -> Request {
    Request {
        method: Method::Get,
        url,
        headers: vec![("authorization".to_owned(), "Bearer token".to_owned())],
        body: None,
    }
}

#[test]
fn node_reads_the_target_as_written_with_its_host_and_no_compression_asked() {
    let peer = Peer::start();
    let (status, body) = ask(get(peer.url("/session?directory=a+b%20c%27")), 1 << 20);
    assert_eq!(status, 200);
    let seen: Value = serde_json::from_slice(&body.unwrap()).unwrap();
    assert_eq!(seen["method"], "GET");
    assert_eq!(seen["url"], "/session?directory=a+b%20c%27");
    assert_eq!(seen["host"], format!("127.0.0.1:{}", peer.port));
    assert_eq!(seen["authorization"], "Bearer token");
    assert_eq!(seen["encoding"], Value::Null);
}

#[test]
fn node_reads_a_post_s_body_by_its_length() {
    let peer = Peer::start();
    let request = Request {
        method: Method::Post,
        url: peer.url("/deliver"),
        headers: vec![("content-type".to_owned(), "application/json".to_owned())],
        body: Some("{\"text\":\"hé\"}".as_bytes().to_vec()),
    };
    let (status, body) = ask(request, 1 << 20);
    assert_eq!(status, 200);
    let seen: Value = serde_json::from_slice(&body.unwrap()).unwrap();
    assert_eq!(seen["method"], "POST");
    assert_eq!(seen["type"], "application/json");
    assert_eq!(seen["length"], "14", "its bytes, not its characters");
    assert_eq!(seen["body"], "{\"text\":\"hé\"}");
}

#[test]
fn a_refusal_carries_its_body_and_a_body_comes_whole_in_chunks_or_not_at_all() {
    let peer = Peer::start();
    assert_eq!(
        ask(get(peer.url("/unauthorized")), 1 << 20),
        (401, Ok(b"no".to_vec()))
    );
    assert_eq!(
        ask(get(peer.url("/chunked")), 1 << 20),
        (200, Ok(b"abcde".to_vec()))
    );
    assert_eq!(ask(get(peer.url("/empty")), 1 << 20), (204, Ok(Vec::new())));
}

#[test]
fn a_body_cut_short_or_past_its_size_fails_after_its_head() {
    let peer = Peer::start();
    assert_eq!(
        ask(get(peer.url("/cut")), 1 << 20),
        (200, Err(BodyFailed::Cut))
    );
    assert_eq!(
        ask(get(peer.url("/chunked")), 4),
        (200, Err(BodyFailed::TooLarge))
    );
}

#[test]
fn no_peer_is_fetch_failed_before_any_head() {
    let free = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = free.local_addr().unwrap().port();
    drop(free);
    let failed = block_on(async {
        SystemLoopback
            .send(get(format!("http://127.0.0.1:{port}/session")))
            .await
            .err()
    });
    assert_eq!(failed.as_deref(), Some("fetch failed"));
}

//! The harness feed's client, against servers on loopback (never the network):
//! the head and then the body a chunk at a time, no redirect followed, Node's
//! words where the connection fails, and, through [`Feed`], the rules that are
//! Node's own. TLS is the platform's and is not run here: no server of ours has
//! a certificate the platform trusts.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::rc::Rc;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::Duration;

use cf_harness::admin::feed::Feed;
use cf_harness::admin::{Format, Latest, Source};
use cf_harness::seams::SystemTime;
use cf_proto::agents::Harness;

use super::*;

/// A server that answers each connection it is asked with the next of `replies`,
/// as the bytes they are, and ends the connection. What it was asked is kept.
struct Server {
    port: u16,
    heads: Arc<Mutex<Vec<String>>>,
}

impl Server {
    fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.port)
    }

    /// The heads of the requests made so far.
    fn asked(&self) -> Vec<String> {
        self.heads.lock().unwrap().clone()
    }
}

/// The request head on `stream`, up to its empty line.
fn read_head(stream: &mut TcpStream) -> String {
    let mut head = Vec::new();
    let mut byte = [0];
    while !head.ends_with(b"\r\n\r\n") {
        if stream.read(&mut byte).unwrap_or(0) == 0 {
            break;
        }
        head.push(byte[0]);
    }
    String::from_utf8_lossy(&head).into_owned()
}

fn serve(replies: Vec<Vec<u8>>) -> Server {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let heads = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&heads);
    drop(thread::spawn(move || {
        for reply in replies {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            seen.lock().unwrap().push(read_head(&mut stream));
            stream.write_all(&reply).ok();
        }
    }));
    Server { port, heads }
}

/// A reply with `status` and a body of known length, the connection closed after it.
fn answer(status: &str, headers: &str, body: &str) -> Vec<u8> {
    format!(
        "HTTP/1.1 {status}\r\n{headers}content-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    )
    .into_bytes()
}

fn asked(url: &str) -> Request<'_> {
    Request {
        url,
        timeout: Duration::from_secs(5),
        follow_redirects: false,
    }
}

/// The status of the answer to a GET of `url`, and its whole body.
async fn fetch(url: &str) -> Result<(u16, Vec<u8>), String> {
    let network = HttpsFeed::new();
    let request = asked(url);
    let mut reply = network.get(&request).await?;
    let mut body = Vec::new();
    while let Some(chunk) = reply.chunk().await? {
        body.extend(chunk);
    }
    Ok((reply.status(), body))
}

#[tokio::test]
async fn an_answer_is_its_status_and_then_its_body_a_chunk_at_a_time() {
    let server = serve(vec![
        b"HTTP/1.1 200 OK\r\ntransfer-encoding: chunked\r\nconnection: close\r\n\r\n5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n"
            .to_vec(),
    ]);
    let network = HttpsFeed::new();
    let url = server.url("/latest");
    let request = asked(&url);
    let mut reply = network.get(&request).await.unwrap();
    assert_eq!(reply.status(), 200);
    let mut body = Vec::new();
    while let Some(chunk) = reply.chunk().await.unwrap() {
        assert!(!chunk.is_empty());
        body.extend(chunk);
    }
    assert_eq!(body, b"hello world");
    assert_eq!(reply.chunk().await, Ok(None), "and none after the end");
}

#[tokio::test]
async fn a_status_that_is_no_success_is_an_answer_with_its_body() {
    let server = serve(vec![
        answer("404 Not Found", "", "not here"),
        answer("500 Internal Server Error", "", ""),
    ]);
    assert_eq!(
        fetch(&server.url("/a")).await,
        Ok((404, b"not here".to_vec()))
    );
    assert_eq!(fetch(&server.url("/b")).await, Ok((500, Vec::new())));
}

#[tokio::test]
async fn a_redirect_is_an_answer_like_another_and_is_not_followed() {
    let server = serve(vec![
        answer("302 Found", "location: /elsewhere\r\n", ""),
        answer("200 OK", "", "the other place"),
    ]);
    assert_eq!(fetch(&server.url("/latest")).await, Ok((302, Vec::new())));
    let heads = server.asked();
    assert_eq!(heads.len(), 1, "the one request: {heads:?}");
    for status in [301, 303, 307, 308] {
        let server = serve(vec![answer(
            &format!("{status} Moved"),
            "location: /elsewhere\r\n",
            "",
        )]);
        assert_eq!(
            fetch(&server.url("/latest")).await,
            Ok((status, Vec::new())),
            "{status}"
        );
    }
}

#[tokio::test]
async fn the_request_is_a_plain_get_that_names_its_client() {
    let server = serve(vec![answer("200 OK", "", "{}")]);
    fetch(&server.url("/latest?x=1")).await.unwrap();
    let heads = server.asked();
    let head = heads.first().unwrap().to_lowercase();
    assert!(head.starts_with("get /latest?x=1 http/1.1\r\n"), "{head}");
    assert!(
        head.contains(&format!(
            "user-agent: consensflow/{}\r\n",
            VERSION.to_lowercase()
        )),
        "{head}"
    );
    assert!(
        head.contains(&format!("host: 127.0.0.1:{}\r\n", server.port)),
        "{head}"
    );
    // A feed is read as it is: nothing is asked of its encoding, and no body goes with the GET.
    assert!(!head.contains("accept-encoding"), "{head}");
    assert!(!head.contains("content-length"), "{head}");
}

#[tokio::test]
async fn a_connection_that_cannot_be_made_is_fetch_failed() {
    // A port nobody listens on: the one a listener just let go of.
    let port = TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    assert_eq!(
        fetch(&format!("http://127.0.0.1:{port}/latest")).await,
        Err(FETCH_FAILED.to_owned())
    );
}

#[tokio::test]
async fn a_connection_that_closes_while_the_body_comes_is_terminated() {
    // It says 100 bytes and sends 5.
    let server = serve(vec![
        b"HTTP/1.1 200 OK\r\ncontent-length: 100\r\nconnection: close\r\n\r\nhello".to_vec(),
    ]);
    let network = HttpsFeed::new();
    let url = server.url("/latest");
    let request = asked(&url);
    let mut reply = network.get(&request).await.unwrap();
    assert_eq!(reply.status(), 200);
    let mut got = Vec::new();
    let ended = loop {
        match reply.chunk().await {
            Ok(Some(chunk)) => got.extend(chunk),
            other => break other,
        }
    };
    assert_eq!(got, b"hello", "what came is read first");
    assert_eq!(ended, Err(TERMINATED.to_owned()));
}

#[tokio::test]
async fn a_request_given_up_ends_its_connection() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let (told, ended) = mpsc::channel();
    drop(thread::spawn(move || {
        let Ok((mut stream, _)) = listener.accept() else {
            return;
        };
        read_head(&mut stream);
        // It answers nothing, and waits for the client to go: its end of the
        // connection reads nothing more, or is reset (Windows says so), and is
        // not out of time.
        stream.set_read_timeout(Some(Duration::from_secs(20))).ok();
        let closed = match stream.read(&mut [0; 1]) {
            Ok(read) => read == 0,
            Err(failed) => !matches!(
                failed.kind(),
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
            ),
        };
        told.send(closed).ok();
    }));
    let network = HttpsFeed::new();
    let url = format!("http://127.0.0.1:{port}/latest");
    let request = asked(&url);
    let waited = tokio::time::timeout(Duration::from_millis(300), network.get(&request)).await;
    assert!(waited.is_err(), "no head came");
    // Gone from the client's side as soon as the request is let go of.
    let closed = tokio::task::spawn_blocking(move || ended.recv_timeout(Duration::from_secs(15)))
        .await
        .unwrap();
    assert_eq!(closed, Ok(true));
}

/// The latest release of a codex, asked of `server` over the real network and the real clock.
async fn latest(server: &Server, format: Format) -> Result<String, String> {
    let feed = Feed::new(Rc::new(SystemTime), Rc::new(HttpsFeed::new()));
    let source = Source {
        url: server.url("/latest"),
        format,
        distribution: None,
        update: None,
    };
    feed.latest(Harness::Codex, &source).await
}

#[tokio::test]
async fn through_the_feed_a_release_is_read_where_the_feed_keeps_it() {
    let server = serve(vec![answer(
        "200 OK",
        "",
        r#"{"name":"x","version":"1.2.3"}"#,
    )]);
    assert_eq!(latest(&server, Format::Npm).await, Ok("1.2.3".to_owned()));
    let server = serve(vec![answer("200 OK", "", "  2.1.280\n")]);
    assert_eq!(
        latest(&server, Format::Text).await,
        Ok("2.1.280".to_owned())
    );
}

#[tokio::test]
async fn through_the_feed_each_way_it_can_fail_is_said_as_node_said_it() {
    for (reply, words) in [
        (
            answer("404 Not Found", "", "{}"),
            "Release service returned HTTP 404",
        ),
        (
            answer("500 Oops", "", ""),
            "Release service returned HTTP 500",
        ),
        (
            answer("302 Found", "location: /elsewhere\r\n", ""),
            "fetch failed",
        ),
        (
            answer("301 Moved", "location: /elsewhere\r\n", "{}"),
            "fetch failed",
        ),
        (
            answer("200 OK", "", r#"{"version":"latest"}"#),
            "Release version is unavailable",
        ),
        (
            answer("200 OK", "", "<html>"),
            "Release metadata is not valid JSON",
        ),
        (
            b"HTTP/1.1 200 OK\r\ncontent-length: 100\r\nconnection: close\r\n\r\n{\"vers".to_vec(),
            "terminated",
        ),
    ] {
        let server = serve(vec![reply]);
        assert_eq!(
            latest(&server, Format::Npm).await,
            Err(words.to_owned()),
            "{words}"
        );
    }
}

#[tokio::test]
async fn a_body_past_the_size_a_feed_may_have_is_refused_as_node_refused_it() {
    let big = format!(r#"{{"version":"1.2.3","pad":"{}"}}"#, "a".repeat(2_000_000));
    let server = serve(vec![answer("200 OK", "", &big)]);
    assert_eq!(
        latest(&server, Format::Npm).await,
        Err("Release metadata exceeds size limit".to_owned())
    );
}

#[test]
fn the_client_is_made_on_ring_and_installs_nothing_process_wide() {
    assert!(HttpsFeed::new().client().is_ok());
    assert!(
        rustls::crypto::CryptoProvider::get_default().is_none(),
        "no default provider was installed for the process"
    );
}

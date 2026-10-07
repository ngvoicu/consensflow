//! The server over real sockets: what goes out for each kind of answer, that
//! a handler outlives its client and a panic in one is a 500, and how it
//! closes: doors answered, the listener let go, idle connections ended, and
//! what will not end dropped when the caller says so.

use std::cell::Cell;
use std::time::Duration;

use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Notify;
use tokio::task::LocalSet;

use super::*;
use crate::testing::{worked, Worked};

mod engine;

/// What came back on a connection that was asked to close after the answer.
struct Reply {
    status: u16,
    head: String,
    body: String,
}

impl Reply {
    fn header(&self, name: &str) -> Option<&str> {
        self.head.lines().skip(1).find_map(|line| {
            let (found, value) = line.split_once(": ")?;
            found.eq_ignore_ascii_case(name).then_some(value)
        })
    }
}

fn parse(bytes: &[u8]) -> Reply {
    let text = String::from_utf8_lossy(bytes).into_owned();
    let (head, body) = text.split_once("\r\n\r\n").expect("a head and a body");
    let status = head.split(' ').nth(1).and_then(|code| code.parse().ok());
    Reply {
        status: status.expect("a status"),
        head: head.to_owned(),
        body: body.to_owned(),
    }
}

fn address(api: &Api) -> String {
    api.url().strip_prefix("http://").unwrap().to_owned()
}

/// One request on a connection of its own, which the server ends after answering.
async fn ask(api: &Api, request: &str) -> Reply {
    let mut stream = TcpStream::connect(address(api)).await.unwrap();
    let request = format!("{request}Connection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut answered = Vec::new();
    stream.read_to_end(&mut answered).await.unwrap();
    parse(&answered)
}

struct Rig {
    home: tempfile::TempDir,
}

/// A front whose handler is `handler`, on a home of its own.
async fn serving(
    handler: impl Fn(Request) -> LocalBoxFuture<'static, Result<Answer, Failure>> + 'static,
) -> (Api, Rig) {
    serving_with(Closing::new(), handler).await
}

/// A front that sets `closing` when it closes.
async fn serving_with(
    closing: Closing,
    handler: impl Fn(Request) -> LocalBoxFuture<'static, Result<Answer, Failure>> + 'static,
) -> (Api, Rig) {
    let Worked { home, spawn } = worked();
    let api = Api::start(Rc::new(handler), closing, spawn).await.unwrap();
    (api, Rig { home })
}

fn answering(
    answer: Answer,
) -> impl Fn(Request) -> LocalBoxFuture<'static, Result<Answer, Failure>> {
    move |_| {
        let answer = answer.clone();
        Box::pin(async move { Ok(answer) })
    }
}

#[tokio::test]
async fn a_json_answer_goes_out_with_its_status_its_type_and_its_text_as_stringify_writes_it() {
    LocalSet::new()
        .run_until(async {
            let (api, _rig) = serving(answering(Answer::created(
                json!({ "b": 1.0, "a": [true, null] }),
            )))
            .await;
            let reply = ask(&api, "POST /x HTTP/1.1\r\nHost: t\r\nContent-Length: 0\r\n").await;
            assert_eq!(reply.status, 201);
            assert_eq!(reply.header("content-type"), Some("application/json"));
            assert_eq!(reply.body, r#"{"b":1,"a":[true,null]}"#);
            let port = api.url().strip_prefix("http://127.0.0.1:").unwrap();
            assert!(
                port.parse::<u16>().is_ok(),
                "no slash at its end: {}",
                api.url()
            );
            api.close().await;
        })
        .await;
}

#[tokio::test]
async fn a_page_is_html_in_utf_8_and_nothing_is_a_status_alone() {
    LocalSet::new()
        .run_until(async {
            let (api, _rig) = serving(answering(Answer::html("<p>caf\u{e9}</p>"))).await;
            let reply = ask(&api, "GET / HTTP/1.1\r\nHost: t\r\n").await;
            assert_eq!(
                reply.header("content-type"),
                Some("text/html; charset=utf-8")
            );
            assert_eq!(reply.body, "<p>caf\u{e9}</p>");
            api.close().await;

            let (api, _rig) = serving(answering(Answer::nothing(204))).await;
            let reply = ask(&api, "DELETE /x HTTP/1.1\r\nHost: t\r\n").await;
            assert_eq!((reply.status, reply.body.as_str()), (204, ""));
            assert_eq!(reply.header("content-type"), None);
            api.close().await;
        })
        .await;
}

#[tokio::test]
async fn a_failure_is_its_status_and_the_error_and_message() {
    LocalSet::new()
        .run_until(async {
            let (api, _rig) = serving(|_| {
                Box::pin(async {
                    Err(Failure::refuse(
                        403,
                        "not-yours",
                        "T-2 is assigned to @hera",
                    ))
                })
            })
            .await;
            let reply = ask(&api, "POST /x HTTP/1.1\r\nHost: t\r\nContent-Length: 0\r\n").await;
            assert_eq!(reply.status, 403);
            assert_eq!(
                reply.body,
                r#"{"error":"not-yours","message":"T-2 is assigned to @hera"}"#
            );
            api.close().await;
        })
        .await;
}

#[tokio::test]
async fn the_handler_is_given_the_request_as_the_client_wrote_it() {
    LocalSet::new()
        .run_until(async {
            let (api, _rig) = serving(|request| {
                Box::pin(async move {
                    Ok(Answer::ok(json!({
                        "at": request.at(),
                        "wait": request.param("wait"),
                        "bearer": request.bearer(),
                    })))
                })
            })
            .await;
            let reply = ask(
                &api,
                "GET /api/questions/5?wait=20&x=y HTTP/1.1\r\nHost: t\r\nAuthorization: Bearer abc123\r\n",
            )
            .await;
            assert_eq!(
                reply.body,
                r#"{"at":"GET /api/questions/5","wait":"20","bearer":"abc123"}"#
            );
            api.close().await;
        })
        .await;
}

#[tokio::test]
async fn a_handler_that_panics_is_a_500_internal_and_the_front_goes_on() {
    LocalSet::new()
        .run_until(async {
            let times = Rc::new(Cell::new(0));
            let counted = Rc::clone(&times);
            let (api, rig) = serving(move |_| {
                counted.set(counted.get() + 1);
                let first = counted.get() == 1;
                Box::pin(async move {
                    if first {
                        panic!("a bug in a handler");
                    }
                    Ok(Answer::ok(json!({ "fine": true })))
                })
            })
            .await;
            let reply = ask(&api, "GET /x HTTP/1.1\r\nHost: t\r\n").await;
            assert_eq!(reply.status, 500);
            assert_eq!(
                reply.body,
                r#"{"error":"internal","message":"a bug in a handler"}"#
            );
            let again = ask(&api, "GET /x HTTP/1.1\r\nHost: t\r\n").await;
            assert_eq!(
                (again.status, again.body.as_str()),
                (200, r#"{"fine":true}"#)
            );
            let log = std::fs::read_to_string(rig.home.path().join("daemon.log")).unwrap();
            assert!(log.contains("error a request failed"), "{log}");
            assert!(log.contains("panic: a bug in a handler"), "{log}");
            let trace = std::fs::read_to_string(rig.home.path().join("events.jsonl")).unwrap();
            assert!(
                trace.contains(r#""reason":"a request failed: a bug in a handler""#),
                "{trace}"
            );
            api.close().await;
        })
        .await;
}

#[tokio::test]
async fn a_request_target_that_is_no_url_is_not_answered_200() {
    LocalSet::new()
        .run_until(async {
            let handled = Rc::new(Cell::new(false));
            let seen = Rc::clone(&handled);
            let (api, _rig) = serving(move |_| {
                seen.set(true);
                Box::pin(async { Ok(Answer::ok(json!({}))) })
            })
            .await;
            let reply = ask(&api, "GET http:// HTTP/1.1\r\nHost: t\r\n").await;
            // The connection's parser refuses it (400), or the request does (500 `Invalid URL`).
            assert!(
                reply.status == 400 || reply.status == 500,
                "{}",
                reply.status
            );
            assert!(!handled.get());
            api.close().await;
        })
        .await;
}

#[tokio::test]
async fn a_body_its_request_was_answered_before_reading_is_read_after_so_its_client_reads_the_answer(
) {
    LocalSet::new()
        .run_until(async {
            // A check that comes before the body (a member creating a task):
            // the handler answers without reading it.
            let (api, _rig) = serving(|_| {
                Box::pin(async {
                    Err(Failure::refuse(403, "not-a-coordinator", "members do not"))
                })
            })
            .await;
            let length = 1024 * 1024;
            let first = 64 * 1024;
            let mut stream = TcpStream::connect(address(&api)).await.unwrap();
            let head = format!(
                "POST /api/tasks HTTP/1.1\r\nHost: t\r\nContent-Length: {length}\r\nConnection: close\r\n\r\n"
            );
            stream.write_all(head.as_bytes()).await.unwrap();
            stream.write_all(&vec![b' '; first]).await.unwrap();
            // The answer comes while the client is still sending.
            let mut answered = Vec::new();
            let mut buffer = [0_u8; 4096];
            while !String::from_utf8_lossy(&answered).contains("members do not") {
                let read = stream.read(&mut buffer).await.unwrap();
                assert!(read > 0, "the connection ended before the answer");
                answered.extend_from_slice(&buffer[..read]);
            }
            assert_eq!(parse(&answered).status, 403);
            // The rest is read and let go, not met with a reset: on Windows
            // a reset loses an answer the client has not read yet.
            stream.write_all(&vec![b' '; length - first]).await.unwrap();
            stream.shutdown().await.unwrap();
            let mut rest = Vec::new();
            stream
                .read_to_end(&mut rest)
                .await
                .expect("the connection ends, not reset");
            api.close().await;
        })
        .await;
}

#[tokio::test]
async fn a_client_that_leaves_does_not_drop_its_handler() {
    LocalSet::new()
        .run_until(async {
            let reached = Rc::new(Cell::new(false));
            let gate = Rc::new(Notify::new());
            let (seen, held) = (Rc::clone(&reached), Rc::clone(&gate));
            let (api, _rig) = serving(move |_| {
                let (seen, held) = (Rc::clone(&seen), Rc::clone(&held));
                Box::pin(async move {
                    held.notified().await;
                    seen.set(true);
                    Ok(Answer::ok(json!({})))
                })
            })
            .await;
            let mut stream = TcpStream::connect(address(&api)).await.unwrap();
            stream
                .write_all(b"POST /x HTTP/1.1\r\nHost: t\r\nContent-Length: 0\r\n\r\n")
                .await
                .unwrap();
            tokio::time::sleep(Duration::from_millis(50)).await;
            drop(stream);
            tokio::time::sleep(Duration::from_millis(100)).await;
            assert!(!reached.get());
            gate.notify_one();
            tokio::time::sleep(Duration::from_millis(50)).await;
            assert!(reached.get(), "it ran to its end with nobody to hear it");
            api.close().await;
        })
        .await;
}

/// A door waits for its answer on behalf of its client: when the client goes,
/// the handler is told, though the connection's work for it is what is
/// dropped, and the handler waits on for nothing else.
#[tokio::test]
async fn a_handler_is_told_when_its_client_leaves_and_not_before() {
    LocalSet::new()
        .run_until(async {
            let told = Rc::new(Cell::new(None));
            let gate = Rc::new(Notify::new());
            let (seen, held) = (Rc::clone(&told), Rc::clone(&gate));
            let (api, _rig) = serving(move |request| {
                let (seen, held) = (Rc::clone(&seen), Rc::clone(&held));
                Box::pin(async move {
                    let at_first = request.consumer().has_left();
                    tokio::select! {
                        () = request.consumer().left() => {}
                        () = held.notified() => {}
                    }
                    seen.set(Some((at_first, request.consumer().has_left())));
                    Ok(Answer::ok(json!({})))
                })
            })
            .await;
            let mut stream = TcpStream::connect(address(&api)).await.unwrap();
            stream
                .write_all(b"GET /x HTTP/1.1\r\nHost: t\r\n\r\n")
                .await
                .unwrap();
            tokio::time::sleep(Duration::from_millis(100)).await;
            assert_eq!(told.get(), None, "its client is there: it waits");
            drop(stream);
            tokio::time::sleep(Duration::from_millis(100)).await;
            assert_eq!(
                told.get(),
                Some((false, true)),
                "it was told the client had left, and the gate was never opened"
            );
            api.close().await;
        })
        .await;
}

/// The other way: a client that stays is never told it has left, though the
/// request's work in the connection ends with the answer.
#[tokio::test]
async fn a_handler_whose_client_stays_is_not_told_it_left() {
    LocalSet::new()
        .run_until(async {
            let told = Rc::new(Cell::new(None));
            let seen = Rc::clone(&told);
            let (api, _rig) = serving(move |request| {
                let seen = Rc::clone(&seen);
                Box::pin(async move {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    seen.set(Some(request.consumer().has_left()));
                    Ok(Answer::ok(json!({ "fine": true })))
                })
            })
            .await;
            let reply = ask(&api, "GET /x HTTP/1.1\r\nHost: t\r\n").await;
            assert_eq!(
                (reply.status, reply.body.as_str()),
                (200, r#"{"fine":true}"#)
            );
            assert_eq!(told.get(), Some(false));
            api.close().await;
        })
        .await;
}

#[tokio::test]
async fn keep_alive_serves_several_requests_on_one_connection() {
    LocalSet::new()
        .run_until(async {
            let (api, _rig) = serving(answering(Answer::ok(json!({ "n": 1 })))).await;
            let mut stream = TcpStream::connect(address(&api)).await.unwrap();
            for _ in 0..3 {
                stream
                    .write_all(b"GET /x HTTP/1.1\r\nHost: t\r\n\r\n")
                    .await
                    .unwrap();
                let mut head = vec![0; 1024];
                let read = stream.read(&mut head).await.unwrap();
                let text = String::from_utf8_lossy(&head[..read]);
                assert!(text.starts_with("HTTP/1.1 200 OK"), "{text}");
                assert!(text.ends_with(r#"{"n":1}"#), "{text}");
            }
            drop(stream);
            api.close().await;
        })
        .await;
}

#[test]
fn a_connection_waits_five_seconds_for_its_next_request_as_node_s_keep_alive_did() {
    assert_eq!(IDLE, Duration::from_secs(5));
}

/// Real time, over a short wait: sockets and a paused clock do not mix (the
/// clock moves on while the sockets are still being read).
#[tokio::test]
async fn a_connection_that_waits_for_its_request_longer_than_its_idle_time_is_ended() {
    LocalSet::new()
        .run_until(async {
            let Worked { spawn, .. } = worked();
            let handler: Rc<Handler> = Rc::new(|_| Box::pin(async { Ok(Answer::ok(json!({}))) }));
            let api = Api::start_holding_idle_to(
                Duration::from_millis(300),
                handler,
                Closing::new(),
                spawn,
            )
            .await
            .unwrap();
            let mut stream = TcpStream::connect(address(&api)).await.unwrap();
            // One request is answered, and then the connection waits for the next.
            stream
                .write_all(b"GET /x HTTP/1.1\r\nHost: t\r\n\r\n")
                .await
                .unwrap();
            let mut answered = vec![0; 1024];
            assert!(stream.read(&mut answered).await.unwrap() > 0);
            let waited = std::time::Instant::now();
            let mut nothing = [0; 16];
            let read = tokio::time::timeout(Duration::from_secs(10), stream.read(&mut nothing))
                .await
                .expect("the server ended it");
            assert_eq!(read.unwrap(), 0);
            let took = waited.elapsed();
            assert!(
                took >= Duration::from_millis(250) && took < Duration::from_secs(3),
                "{took:?}"
            );
            api.close().await;
        })
        .await;
}

#[tokio::test]
async fn closing_answers_a_door_that_waits_at_once_and_lets_the_listener_go() {
    LocalSet::new()
        .run_until(async {
            let closing = Closing::new();
            let door_closing = closing.clone();
            let (api, _rig) = serving_with(closing.clone(), move |_| {
                let closing = door_closing.clone();
                Box::pin(async move {
                    // A door: it waits until the daemon stops.
                    closing.wait().await;
                    Ok(Answer::ok(json!({ "answer": null })))
                })
            })
            .await;
            let address = address(&api);
            let to_door = address.clone();
            let door = tokio::task::spawn_local(async move {
                let mut stream = TcpStream::connect(to_door).await.unwrap();
                stream
                    .write_all(b"GET /door HTTP/1.1\r\nHost: t\r\n\r\n")
                    .await
                    .unwrap();
                let mut answered = Vec::new();
                stream.read_to_end(&mut answered).await.unwrap();
                parse(&answered)
            });
            tokio::time::sleep(Duration::from_millis(100)).await;
            assert!(!door.is_finished(), "the door waits");

            let started = std::time::Instant::now();
            tokio::time::timeout(Duration::from_secs(5), api.close())
                .await
                .expect("it closes");
            assert!(started.elapsed() < Duration::from_secs(1));
            assert!(closing.is_set(), "the doors are told");
            let reply = door.await.unwrap();
            assert_eq!(
                (reply.status, reply.body.as_str()),
                (200, r#"{"answer":null}"#)
            );
            assert!(
                TcpStream::connect(address).await.is_err(),
                "nothing is listened to any more"
            );
        })
        .await;
}

#[tokio::test]
async fn closing_ends_a_connection_that_is_idle_at_once() {
    LocalSet::new()
        .run_until(async {
            let (api, _rig) = serving(answering(Answer::ok(json!({})))).await;
            let mut idle = TcpStream::connect(address(&api)).await.unwrap();
            let mut quiet = TcpStream::connect(address(&api)).await.unwrap();
            // One has had its answer and waits for the next request, one never asked.
            quiet
                .write_all(b"GET /x HTTP/1.1\r\nHost: t\r\n\r\n")
                .await
                .unwrap();
            let mut answered = vec![0; 1024];
            let _ = quiet.read(&mut answered).await.unwrap();
            let started = std::time::Instant::now();
            tokio::time::timeout(Duration::from_secs(2), api.close())
                .await
                .expect("an idle connection does not hold the close");
            assert!(started.elapsed() < Duration::from_millis(500));
            let mut end = [0; 1];
            assert_eq!(idle.read(&mut end).await.unwrap(), 0);
            assert_eq!(quiet.read(&mut end).await.unwrap(), 0);
        })
        .await;
}

#[tokio::test]
async fn closing_waits_for_a_request_in_hand_and_the_answer_goes_out_first() {
    LocalSet::new()
        .run_until(async {
            let gate = Rc::new(Notify::new());
            let held = Rc::clone(&gate);
            let (api, _rig) = serving(move |_| {
                let held = Rc::clone(&held);
                Box::pin(async move {
                    held.notified().await;
                    Ok(Answer::ok(json!({ "late": true })))
                })
            })
            .await;
            let address = address(&api);
            let client = tokio::task::spawn_local(async move {
                let mut stream = TcpStream::connect(address).await.unwrap();
                stream
                    .write_all(b"GET /x HTTP/1.1\r\nHost: t\r\n\r\n")
                    .await
                    .unwrap();
                let mut answered = Vec::new();
                stream.read_to_end(&mut answered).await.unwrap();
                parse(&answered)
            });
            tokio::time::sleep(Duration::from_millis(50)).await;
            let closing = tokio::time::timeout(Duration::from_millis(100), api.close()).await;
            assert!(closing.is_err(), "a request in hand holds the close");
            gate.notify_one();
            let reply = client.await.unwrap();
            assert_eq!(reply.body, r#"{"late":true}"#);
            tokio::time::timeout(Duration::from_secs(2), api.close())
                .await
                .expect("and then the close ends");
        })
        .await;
}

#[tokio::test]
async fn what_will_not_end_is_dropped_when_the_caller_says_so_a_body_half_sent_among_it() {
    LocalSet::new()
        .run_until(async {
            let (api, _rig) = serving(|mut request| {
                Box::pin(async move {
                    request.json().await?;
                    Ok(Answer::ok(json!({})))
                })
            })
            .await;
            let mut stuck = TcpStream::connect(address(&api)).await.unwrap();
            // Fifty bytes promised, ten sent: the handler waits for the rest.
            stuck
                .write_all(
                    b"POST /x HTTP/1.1\r\nHost: t\r\nContent-Length: 50\r\n\r\n{\"a\":1,\"b\"",
                )
                .await
                .unwrap();
            tokio::time::sleep(Duration::from_millis(50)).await;
            let closing = tokio::time::timeout(Duration::from_millis(100), api.close()).await;
            assert!(closing.is_err(), "it does not end by itself");
            api.drop_connections();
            tokio::time::timeout(Duration::from_secs(2), api.close())
                .await
                .expect("dropped, it no longer holds the close");
            let mut end = [0; 1];
            let read = stuck.read(&mut end).await;
            assert!(matches!(read, Ok(0) | Err(_)), "the client sees it end");
        })
        .await;
}

//! A request's work is the engine's work: its handler is begun in the
//! request's first part, as JavaScript ran it, and what that part woke is run
//! to its end before anything else gets a turn, as Node's microtasks ran
//! before the next callback of its event loop.

use std::cell::{OnceCell, RefCell};
use std::time::Instant;

use cf_engine::runtime::{begin, next_turn};
use serde_json::json;
use tokio::sync::Notify;
use tokio::task::LocalSet;

use super::*;

#[tokio::test]
async fn a_requests_engine_work_is_begun_in_its_first_part_and_its_chain_ends_before_the_next_callback(
) {
    LocalSet::new()
        .run_until(async {
            let Worked { spawn, .. } = worked();
            let order: Rc<RefCell<Vec<&'static str>>> = Rc::default();
            // A task of tokio's own that waits, which the request's first part
            // wakes before it begins its work: the next callback.
            let wake = Rc::new(Notify::new());
            let (waiting, said) = (Rc::clone(&wake), Rc::clone(&order));
            drop(tokio::task::spawn_local(async move {
                waiting.notified().await;
                said.borrow_mut().push("next callback");
            }));
            tokio::task::yield_now().await;

            let (engine, log, woken) = (Rc::clone(&spawn), Rc::clone(&order), wake);
            let handler: Rc<Handler> = Rc::new(move |_| {
                let (engine, log, woken) = (Rc::clone(&engine), Rc::clone(&log), Rc::clone(&woken));
                Box::pin(async move {
                    log.borrow_mut().push("first part");
                    woken.notify_one();
                    // The engine's work, begun where it is called: what comes
                    // before its first wait is done here.
                    let chain = Rc::clone(&log);
                    let work = begin(&*engine, async move {
                        for turn in ["turn 0", "turn 1", "turn 2"] {
                            chain.borrow_mut().push(turn);
                            next_turn().await;
                        }
                    })
                    .await;
                    log.borrow_mut().push("begun");
                    work.await;
                    log.borrow_mut().push("answered");
                    Ok(Answer::ok(json!({})))
                })
            });
            let api = Api::start(handler, Closing::new(), spawn).await.unwrap();
            let reply = ask(&api, "GET /x HTTP/1.1\r\nHost: t\r\n").await;
            for _ in 0..5 {
                tokio::task::yield_now().await;
            }
            assert_eq!(reply.status, 200);
            assert_eq!(
                *order.borrow(),
                [
                    "first part",
                    "turn 0",
                    "begun",
                    "turn 1",
                    "turn 2",
                    "answered",
                    "next callback"
                ]
            );
            api.close().await;
        })
        .await;
}

/// Node wrote a handler's answer to its socket as the handler ended it: an
/// answer whose work ends in the chain of the request's first part is out on
/// the connection before the next callback runs, and the client can read it
/// there.
#[tokio::test]
async fn an_answer_whose_work_ends_in_the_requests_first_part_is_out_before_the_next_callback() {
    LocalSet::new()
        .run_until(async {
            let Worked { spawn, .. } = worked();
            // The client's socket as the system has it, read in a blocking
            // wait (tokio's would answer by what its reactor last saw).
            let client: Rc<OnceCell<std::net::TcpStream>> = Rc::default();
            let read: Rc<RefCell<Vec<u8>>> = Rc::default();
            let wake = Rc::new(Notify::new());
            let (waiting, reader, kept) = (Rc::clone(&wake), Rc::clone(&client), Rc::clone(&read));
            let next = tokio::task::spawn_local(async move {
                waiting.notified().await;
                // The wait holds the one thread the test runs on, so no poll of
                // the connection writes while it waits: what it reads was out
                // before the next callback. It waits because loopback hands the
                // written bytes to the client a while later, under load after a
                // read that asked at once.
                let deadline = Instant::now() + Duration::from_secs(10);
                let mut buffer = [0_u8; 4096];
                let mut stream = reader.get().expect("the client");
                while !kept.borrow().ends_with(br#"{"out":true}"#) {
                    let left = deadline.saturating_duration_since(Instant::now());
                    if left.is_zero() {
                        break;
                    }
                    stream.set_read_timeout(Some(left)).unwrap();
                    match std::io::Read::read(&mut stream, &mut buffer) {
                        Ok(count @ 1..) => kept.borrow_mut().extend_from_slice(&buffer[..count]),
                        Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                        Ok(0) | Err(_) => break,
                    }
                }
            });
            tokio::task::yield_now().await;

            let (engine, woken) = (Rc::clone(&spawn), wake);
            let handler: Rc<Handler> = Rc::new(move |_| {
                let (engine, woken) = (Rc::clone(&engine), Rc::clone(&woken));
                Box::pin(async move {
                    woken.notify_one();
                    let work = begin(&*engine, async {
                        for _ in 0..3 {
                            next_turn().await;
                        }
                    })
                    .await;
                    work.await;
                    Ok(Answer::ok(json!({ "out": true })))
                })
            });
            let api = Api::start(handler, Closing::new(), spawn).await.unwrap();
            let mut stream = std::net::TcpStream::connect(address(&api)).unwrap();
            std::io::Write::write_all(
                &mut stream,
                b"GET /x HTTP/1.1\r\nHost: t\r\nConnection: close\r\n\r\n",
            )
            .unwrap();
            client.set(stream).unwrap();
            next.await.unwrap();

            let answered = read.borrow().clone();
            assert!(
                String::from_utf8_lossy(&answered).ends_with(r#"{"out":true}"#),
                "the next callback read: {:?}",
                String::from_utf8_lossy(&answered)
            );
            assert_eq!(parse(&answered).status, 200);
            api.close().await;
        })
        .await;
}

#[tokio::test]
async fn a_handler_that_panics_after_a_wait_is_a_500_and_the_executor_goes_on() {
    LocalSet::new()
        .run_until(async {
            let (api, rig) = serving(|_| {
                Box::pin(async {
                    next_turn().await;
                    tokio::task::yield_now().await;
                    panic!("a bug after a wait");
                })
            })
            .await;
            let reply = ask(&api, "GET /x HTTP/1.1\r\nHost: t\r\n").await;
            assert_eq!(reply.status, 500);
            assert_eq!(
                reply.body,
                r#"{"error":"internal","message":"a bug after a wait"}"#
            );
            let log = std::fs::read_to_string(rig.home.path().join("daemon.log")).unwrap();
            assert!(log.contains("error a request failed"), "{log}");
            api.close().await;
        })
        .await;
}

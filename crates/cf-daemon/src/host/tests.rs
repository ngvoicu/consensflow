//! The host over the bridge, asked as the engine asks it, with the test as the
//! app's end (`Role::Host`): what is sent for an open and a kill, how each
//! answer is read, the bound on an open, and what the host's exits do.

use std::cell::{Cell, RefCell};
use std::future::pending;

use serde_json::json;
use tokio::task::LocalSet;

use super::*;
use crate::testing::{bridge_pair, bridge_pair_over, worked, Worked};

mod turns;

fn pane(id: &str, generation: u64) -> Pane {
    Pane {
        id: id.to_owned(),
        generation,
    }
}

fn open(env: &[(&str, &str)]) -> OpenPane {
    OpenPane {
        pane: pane("p1-chief", 1),
        cwd: "/work/app".to_owned(),
        argv: vec!["claude".to_owned(), "--x".to_owned()],
        env: env
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect(),
        drop_env: vec!["TERM".to_owned()],
    }
}

fn host_over(daemon: &Bridge) -> BridgeHost {
    BridgeHost::new(daemon.clone(), Env::default())
}

#[tokio::test]
async fn an_open_is_sent_with_the_keys_node_sent_and_its_env_as_the_engine_gave_it() {
    LocalSet::new()
        .run_until(async {
            let (daemon, app) = bridge_pair();
            let seen = Rc::new(RefCell::new(Vec::new()));
            let heard = Rc::clone(&seen);
            app.on("pane.open", move |_, body| {
                heard.borrow_mut().push(body);
                async { Ok(json!({ "ok": true, "id": "p1-chief", "generation": 1, "pid": 4242 })) }
            });
            let host = host_over(&daemon);
            // A name given twice keeps its first place and its last value, as an object spread does.
            let answer = host
                .open(open(&[("A", "1"), ("B", "2"), ("A", "3"), ("CONSENSFLOW_TOKEN", "t")]))
                .await;
            assert_eq!(answer, Ok(Opened::Open { pid: Some(4242) }));
            let body = seen.borrow()[0].clone();
            assert_eq!(
                serde_json::to_string(&body).unwrap(),
                r#"{"id":"p1-chief","generation":1,"cwd":"/work/app","argv":["claude","--x"],"env":{"A":"3","B":"2","CONSENSFLOW_TOKEN":"t"},"dropEnv":["TERM"]}"#
            );
        })
        .await;
}

#[test]
fn an_open_s_answer_is_read_as_the_dispatcher_read_it() {
    let refused = |error: &str| Opened::Refused {
        error: error.to_owned(),
    };
    assert_eq!(opened(&json!({ "ok": true })), Opened::Open { pid: None });
    assert_eq!(
        opened(&json!({ "ok": true, "pid": 12 })),
        Opened::Open { pid: Some(12) }
    );
    assert_eq!(
        opened(&json!({ "ok": true, "pid": "12" })),
        Opened::Open { pid: None }
    );
    assert_eq!(
        opened(&json!({ "ok": true, "pid": -1 })),
        Opened::Open { pid: None }
    );
    assert_eq!(
        opened(&json!({ "ok": false, "error": "no pty" })),
        refused("no pty")
    );
    // Anything but `ok: true` is a refusal, with the words it gave or none.
    assert_eq!(
        opened(&json!({ "ok": false })),
        refused("no answer from the pane host")
    );
    assert_eq!(opened(&json!({})), refused("no answer from the pane host"));
    assert_eq!(
        opened(&json!(null)),
        refused("no answer from the pane host")
    );
    assert_eq!(
        opened(&json!({ "ok": "yes" })),
        refused("no answer from the pane host")
    );
    assert_eq!(
        opened(&json!({ "ok": 1, "error": null })),
        refused("no answer from the pane host")
    );
    assert_eq!(opened(&json!({ "error": 5 })), refused("5"));
    assert_eq!(
        opened(&json!({ "ok": false, "error": "deadline" })),
        refused("deadline")
    );
}

#[test]
fn a_kill_s_answer_is_taken_or_the_words_the_host_gave() {
    let refused = |error: &str| Killed::Refused {
        error: error.to_owned(),
    };
    assert_eq!(killed(&json!({ "ok": true })), Killed::Killed);
    assert_eq!(
        killed(&json!({ "ok": false, "error": "gone" })),
        refused("gone")
    );
    assert_eq!(killed(&json!({})), refused(""));
    assert_eq!(killed(&json!({ "ok": "true" })), refused(""));
    assert_eq!(killed(&json!({ "ok": false, "error": 7 })), refused("7"));
}

#[tokio::test(start_paused = true)]
async fn an_open_the_host_never_answers_is_refused_as_the_deadline_after_sixty_seconds() {
    LocalSet::new()
        .run_until(async {
            let (daemon, app) = bridge_pair();
            app.on("pane.open", |_, _| async {
                pending::<()>().await;
                Ok(json!({}))
            });
            let host = host_over(&daemon);
            let started = tokio::time::Instant::now();
            let answer = host.open(open(&[])).await;
            assert_eq!(
                answer,
                Ok(Opened::Refused {
                    error: "deadline".to_owned()
                })
            );
            assert_eq!(started.elapsed(), Duration::from_secs(60));
        })
        .await;
}

#[tokio::test]
async fn a_bridge_that_ended_is_a_request_that_never_came_back_and_says_eof() {
    LocalSet::new()
        .run_until(async {
            let (daemon, app) = bridge_pair();
            app.on("pane.open", |_, _| async {
                pending::<()>().await;
                Ok(json!({}))
            });
            let host = host_over(&daemon);
            let asking = tokio::task::spawn_local(async move { host.open(open(&[])).await });
            tokio::time::sleep(Duration::from_millis(20)).await;
            // The app's end of the pipe goes: the daemon's input ends.
            app.close();
            let answer = asking.await.unwrap();
            assert_eq!(
                answer,
                Err(HostError {
                    error: Some("eof".to_owned()),
                    message: "eof".to_owned()
                })
            );
            let after = host_over(&daemon).kill(&pane("p1-chief", 1)).await;
            assert_eq!(after.unwrap_err().error.as_deref(), Some("eof"));
        })
        .await;
}

#[tokio::test]
async fn a_program_that_cannot_open_in_a_window_fails_the_open_with_the_sentence_and_sends_nothing()
{
    LocalSet::new()
        .run_until(async {
            let (daemon, app) = bridge_pair();
            let sent = Rc::new(Cell::new(0));
            let counted = Rc::clone(&sent);
            app.on("pane.open", move |_, _| {
                counted.set(counted.get() + 1);
                async { Ok(json!({ "ok": true })) }
            });
            let host = host_over(&daemon);
            let mut shim = open(&[]);
            shim.argv = vec!["/nowhere/claude.cmd".to_owned()];
            let failed = host.open(shim).await.unwrap_err();
            assert_eq!(failed.error, None);
            assert_eq!(
                failed.message,
                "/nowhere/claude.cmd is not an npm shim, and only cmd.exe could run it in a window"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
            assert_eq!(sent.get(), 0);
        })
        .await;
}

#[tokio::test]
async fn a_kill_is_asked_by_the_panes_id_and_generation() {
    LocalSet::new()
        .run_until(async {
            let (daemon, app) = bridge_pair();
            let seen = Rc::new(RefCell::new(Vec::new()));
            let heard = Rc::clone(&seen);
            app.on("pane.kill", move |_, body| {
                let refuse = body["generation"] == 2;
                heard.borrow_mut().push(body);
                async move {
                    Ok(if refuse {
                        json!({ "ok": false, "error": "no such pane" })
                    } else {
                        json!({ "ok": true })
                    })
                }
            });
            let host = host_over(&daemon);
            assert_eq!(host.kill(&pane("p1-zeus", 1)).await, Ok(Killed::Killed));
            assert_eq!(
                host.kill(&pane("p1-zeus", 2)).await,
                Ok(Killed::Refused {
                    error: "no such pane".to_owned()
                })
            );
            assert_eq!(
                *seen.borrow(),
                [
                    json!({ "id": "p1-zeus", "generation": 1 }),
                    json!({ "id": "p1-zeus", "generation": 2 })
                ]
            );
        })
        .await;
}

#[tokio::test]
async fn an_adapters_own_request_goes_straight_through_and_is_asked_where_it_is_called() {
    LocalSet::new()
        .run_until(async {
            let (daemon, app) = bridge_pair();
            let seen = Rc::new(RefCell::new(Vec::new()));
            let heard = Rc::clone(&seen);
            app.on("pane.snapshot", move |_, body| {
                heard.borrow_mut().push(body.clone());
                async move { Ok(json!({ "ok": true, "echo": body, "quietMs": 300 })) }
            });
            let host = host_over(&daemon);
            let asked = host.request("pane.snapshot", json!({ "id": "p1", "generation": 3 }));
            // Not polled yet: the frame was queued when the call was made.
            tokio::time::sleep(Duration::from_millis(20)).await;
            assert_eq!(seen.borrow().len(), 1);
            assert_eq!(
                asked.await,
                Ok(json!({ "ok": true, "echo": { "id": "p1", "generation": 3 }, "quietMs": 300 }))
            );
            // What the host does not know answers `unknown-op`, which is an answer.
            assert_eq!(
                host.request("pane.mystery", json!({})).await,
                Ok(json!({ "ok": false, "error": "unknown-op" }))
            );
        })
        .await;
}

#[tokio::test]
async fn an_exit_is_told_where_it_is_read_before_the_frame_after_it() {
    LocalSet::new()
        .run_until(async {
            let Worked { spawn, .. } = worked();
            let (daemon, app) = bridge_pair_over(&spawn);
            let order = Rc::new(RefCell::new(Vec::new()));
            let (exits, pings) = (Rc::clone(&order), Rc::clone(&order));
            let _watching = watch_exits(&daemon, spawn, move |pane| {
                exits
                    .borrow_mut()
                    .push(format!("exit {} {}", pane.id, pane.generation));
                None
            });
            daemon.on("ping", move |_, _| {
                pings.borrow_mut().push("ping".to_owned());
                async { Ok(json!({ "ok": true })) }
            });
            assert!(app.event("pane.exit", json!({ "id": "p1-chief", "generation": 4 })));
            assert!(app.event("pane.exit", json!({ "id": "p1-zeus", "generation": 2 })));
            app.request("ping", json!({}), Some(Duration::from_secs(5)))
                .await
                .unwrap();
            assert_eq!(
                *order.borrow(),
                ["exit p1-chief 4", "exit p1-zeus 2", "ping"]
            );
        })
        .await;
}

#[tokio::test]
async fn an_exit_tells_how_the_window_ended_and_what_its_screen_showed_where_the_host_said() {
    LocalSet::new()
        .run_until(async {
            let Worked { spawn, .. } = worked();
            let (daemon, app) = bridge_pair_over(&spawn);
            let heard = Rc::new(RefCell::new(Vec::new()));
            let told = Rc::clone(&heard);
            let _watching = watch_exits(&daemon, spawn, move |exit| {
                told.borrow_mut().push(exit);
                None
            });
            daemon.on("ping", |_, _| async { Ok(json!({ "ok": true })) });
            app.event(
                "pane.exit",
                json!({
                    "id": "p1-zeus", "generation": 2, "exitCode": 3, "signal": "Hangup: 1",
                    "tail": ["No API key found", "Use /login"],
                }),
            );
            // A field that is not what it should be was not said; the exit stands.
            app.event(
                "pane.exit",
                json!({ "id": "p1-zeus", "generation": 3, "exitCode": "three", "tail": "x" }),
            );
            // An older host says which pane and no more.
            app.event("pane.exit", json!({ "id": "p1-zeus", "generation": 4 }));
            app.request("ping", json!({}), Some(Duration::from_secs(5)))
                .await
                .unwrap();
            let bare = |generation| PaneExit {
                id: "p1-zeus".to_owned(),
                generation,
                exit_code: None,
                signal: None,
                tail: None,
            };
            assert_eq!(
                *heard.borrow(),
                [
                    PaneExit {
                        exit_code: Some(3),
                        signal: Some("Hangup: 1".to_owned()),
                        tail: Some(vec!["No API key found".to_owned(), "Use /login".to_owned()]),
                        ..bare(2)
                    },
                    bare(3),
                    bare(4),
                ]
            );
        })
        .await;
}

#[tokio::test]
async fn what_an_exit_has_still_to_do_goes_on_apart_and_is_never_waited_for_there() {
    LocalSet::new()
        .run_until(async {
            let Worked { spawn, .. } = worked();
            let (daemon, app) = bridge_pair_over(&spawn);
            let done = Rc::new(Cell::new(false));
            let gate = Rc::new(tokio::sync::Notify::new());
            let (finished, held) = (Rc::clone(&done), Rc::clone(&gate));
            let _watching = watch_exits(&daemon, spawn, move |_| {
                let (finished, held) = (Rc::clone(&finished), Rc::clone(&held));
                let rest: LocalWork = Box::pin(async move {
                    held.notified().await;
                    finished.set(true);
                });
                Some(rest)
            });
            daemon.on("ping", |_, _| async { Ok(json!({ "ok": true })) });
            app.event("pane.exit", json!({ "id": "p1-chief", "generation": 1 }));
            // The frame after it is answered while the rest of the exit waits.
            let answered = app
                .request("ping", json!({}), Some(Duration::from_secs(5)))
                .await;
            assert_eq!(answered.unwrap(), json!({ "ok": true }));
            assert!(!done.get());
            gate.notify_one();
            tokio::time::sleep(Duration::from_millis(20)).await;
            assert!(done.get());
        })
        .await;
}

#[tokio::test]
async fn an_exit_that_names_no_pane_exits_nothing() {
    LocalSet::new()
        .run_until(async {
            let Worked { spawn, .. } = worked();
            let (daemon, app) = bridge_pair_over(&spawn);
            let told = Rc::new(Cell::new(0));
            let counted = Rc::clone(&told);
            let _watching = watch_exits(&daemon, spawn, move |_| {
                counted.set(counted.get() + 1);
                None
            });
            daemon.on("ping", |_, _| async { Ok(json!({ "ok": true })) });
            for body in [
                json!({}),
                json!({ "id": "p1" }),
                json!({ "generation": 1 }),
                json!({ "id": 7, "generation": 1 }),
                json!({ "id": "p1", "generation": "1" }),
                json!({ "id": "p1", "generation": -1 }),
                json!(null),
            ] {
                app.event("pane.exit", body);
            }
            app.request("ping", json!({}), Some(Duration::from_secs(5)))
                .await
                .unwrap();
            assert_eq!(told.get(), 0);
        })
        .await;
}

#[tokio::test]
async fn an_exit_that_panics_is_written_down_and_the_bridge_goes_on() {
    LocalSet::new()
        .run_until(async {
            let Worked { home, spawn } = worked();
            let (daemon, app) = bridge_pair_over(&spawn);
            let _watching = watch_exits(&daemon, spawn, |_| panic!("a bug in an exit"));
            daemon.on("ping", |_, _| async { Ok(json!({ "ok": true })) });
            app.event("pane.exit", json!({ "id": "p1-chief", "generation": 1 }));
            let answered = app
                .request("ping", json!({}), Some(Duration::from_secs(5)))
                .await;
            assert_eq!(
                answered.unwrap(),
                json!({ "ok": true }),
                "the reader went on"
            );
            assert!(!daemon.closed());
            let log = std::fs::read_to_string(home.path().join("daemon.log")).unwrap();
            assert!(log.contains("error a window's exit failed"), "{log}");
            assert!(log.contains("panic: a bug in an exit"), "{log}");
        })
        .await;
}

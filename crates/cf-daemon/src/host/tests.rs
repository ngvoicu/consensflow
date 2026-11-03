//! The host over the bridge, asked as the engine asks it, with the test as the
//! app's end (`Role::Host`): what is sent for an open and a kill, how each
//! answer is read, the bound on an open, and what the host's exits do.

use std::cell::{Cell, RefCell};
use std::future::pending;

use cf_proto::panes::SilentPaste;
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
        silent_paste: None,
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

/// A window whose harness reads a paste silently is opened saying how, for the pane host to
/// hold the human's keys after their paste by (T-1048); every other open is as Node sent it.
#[tokio::test]
async fn an_open_of_a_harness_that_reads_a_paste_silently_says_how() {
    LocalSet::new()
        .run_until(async {
            let (daemon, app) = bridge_pair();
            let seen = Rc::new(RefCell::new(Vec::new()));
            let heard = Rc::clone(&seen);
            app.on("pane.open", move |_, body| {
                heard.borrow_mut().push(body);
                async { Ok(json!({ "ok": true, "id": "p1-chief", "generation": 1 })) }
            });
            let host = host_over(&daemon);
            let silent = SilentPaste {
                pace_us_per_character: 600,
                drawn_after_ms: 50,
                quiet_ms: 120,
                most_ms: 1_000,
                most_us_per_character: 1_000,
            };
            let declared = OpenPane {
                silent_paste: Some(silent),
                ..open(&[])
            };
            host.open(declared).await.unwrap();
            assert_eq!(
                seen.borrow()[0]["silentPaste"],
                json!({
                    "paceUsPerCharacter": 600, "drawnAfterMs": 50, "quietMs": 120,
                    "mostMs": 1000, "mostUsPerCharacter": 1000,
                })
            );
        })
        .await;
}

/// How a window declares that it reads a paste at 0.5 ms a character and may take a second and
/// 2 ms a character at the most (Muse on Windows, T-1126): the pane host holds a delivery's
/// Enter that long, and the daemon waits for its answer as long.
const SLOW_READER: SilentPaste = SilentPaste {
    pace_us_per_character: 500,
    drawn_after_ms: 0,
    quiet_ms: 200,
    most_ms: 1_000,
    most_us_per_character: 2_000,
};

/// A host that answers `pane.open` and holds each `pane.write_paste` for `held` before it
/// answers `ok`, as the pane host does while it waits to press a delivery's Enter: the daemon's
/// end of it (with the app's, which it needs to go on), and the pane `p1-chief` opened as
/// `silent` says it reads a paste.
async fn a_host_that_holds_a_paste_for(
    held: Duration,
    silent: Option<SilentPaste>,
) -> (BridgeHost, Bridge) {
    let (daemon, app) = bridge_pair();
    app.on("pane.open", |_, _| async {
        Ok(json!({ "ok": true, "id": "p1-chief", "generation": 1 }))
    });
    app.on("pane.write_paste", move |_, _| async move {
        tokio::time::sleep(held).await;
        Ok(json!({ "ok": true }))
    });
    let host = host_over(&daemon);
    host.open(OpenPane {
        silent_paste: silent,
        ..open(&[])
    })
    .await
    .unwrap();
    (host, app)
}

fn a_paste_of(characters: usize) -> Value {
    json!({ "id": "p1-chief", "generation": 1, "body": "x".repeat(characters) })
}

/// The pane host holds a delivery's Enter for the window's own read of the paste, up to its cap
/// (a second and 2 ms a character for a window like Muse's): 33 s for a brief of 16,000
/// characters. The daemon's request is answered by then, not cut at the bridge's 30 s, which
/// every long delivery to such a window was (T-1127, T-1126).
#[tokio::test(start_paused = true)]
async fn a_paste_into_a_window_that_declared_its_read_is_answered_after_the_hosts_own_cap() {
    LocalSet::new()
        .run_until(async {
            // 1 s and 2 ms each of 16,000 characters.
            let cap = Duration::from_secs(33);
            let (host, _app) = a_host_that_holds_a_paste_for(cap, Some(SLOW_READER)).await;
            let started = tokio::time::Instant::now();
            let answer = host
                .request("pane.write_paste", a_paste_of(16_000))
                .await
                .unwrap();
            assert_eq!(
                answer,
                json!({ "ok": true }),
                "after {:?}",
                started.elapsed()
            );
            assert_eq!(started.elapsed(), cap);
        })
        .await;
}

/// Its time is the cap and five seconds more: a pane host that has not answered by then has not
/// held the Enter for the window's read, and the daemon stops waiting.
#[tokio::test(start_paused = true)]
async fn a_paste_into_a_window_that_declared_its_read_waits_five_seconds_past_the_hosts_cap() {
    LocalSet::new()
        .run_until(async {
            let (host, _app) =
                a_host_that_holds_a_paste_for(Duration::from_secs(60), Some(SLOW_READER)).await;
            let started = tokio::time::Instant::now();
            let answer = host
                .request("pane.write_paste", a_paste_of(16_000))
                .await
                .unwrap();
            assert_eq!(answer, json!({ "ok": false, "error": "deadline" }));
            assert_eq!(started.elapsed(), Duration::from_secs(38));
        })
        .await;
}

/// A short brief to such a window has the bridge's thirty seconds, as every paste has had: the
/// cap and five seconds are the floor's only where they are more.
#[tokio::test(start_paused = true)]
async fn a_short_paste_into_a_window_that_declared_its_read_has_thirty_seconds_at_the_least() {
    LocalSet::new()
        .run_until(async {
            // 100 characters: a cap of 1.2 s, and 6.2 s with the margin.
            let (host, _app) =
                a_host_that_holds_a_paste_for(Duration::from_secs(29), Some(SLOW_READER)).await;
            let answer = host
                .request("pane.write_paste", a_paste_of(100))
                .await
                .unwrap();
            assert_eq!(answer, json!({ "ok": true }));

            let (host, _app) =
                a_host_that_holds_a_paste_for(Duration::from_secs(40), Some(SLOW_READER)).await;
            let started = tokio::time::Instant::now();
            let answer = host
                .request("pane.write_paste", a_paste_of(100))
                .await
                .unwrap();
            assert_eq!(answer, json!({ "ok": false, "error": "deadline" }));
            assert_eq!(started.elapsed(), Duration::from_secs(30));
        })
        .await;
}

/// A window whose harness declared nothing has the bridge's thirty seconds for a paste, however
/// long: the pane host holds a delivery's Enter for 2 s at the most there.
#[tokio::test(start_paused = true)]
async fn a_paste_into_a_window_that_declared_nothing_has_the_bridges_thirty_seconds() {
    LocalSet::new()
        .run_until(async {
            let (host, _app) = a_host_that_holds_a_paste_for(Duration::from_secs(33), None).await;
            let started = tokio::time::Instant::now();
            let answer = host
                .request("pane.write_paste", a_paste_of(16_000))
                .await
                .unwrap();
            assert_eq!(answer, json!({ "ok": false, "error": "deadline" }));
            assert_eq!(started.elapsed(), Duration::from_secs(30));
        })
        .await;
}

/// Only a paste has the longer time, and only the pane the declaration was made for: a newer
/// generation of the pane, opened with none, has the bridge's.
#[tokio::test(start_paused = true)]
async fn what_a_window_declared_is_for_its_paste_and_its_generation_alone() {
    LocalSet::new()
        .run_until(async {
            let (daemon, app) = bridge_pair();
            app.on("pane.open", |_, _| async {
                Ok(json!({ "ok": true, "id": "p1-chief", "generation": 1 }))
            });
            for op in ["pane.write_paste", "pane.snapshot"] {
                app.on(op, |_, _| async {
                    tokio::time::sleep(Duration::from_secs(33)).await;
                    Ok(json!({ "ok": true }))
                });
            }
            let host = host_over(&daemon);
            host.open(OpenPane {
                silent_paste: Some(SLOW_READER),
                ..open(&[])
            })
            .await
            .unwrap();
            // A request that is not a paste has the bridge's, whatever it says of the pane (a
            // body as long as a brief would give a paste 38 s).
            let snapshot = host
                .request("pane.snapshot", a_paste_of(16_000))
                .await
                .unwrap();
            assert_eq!(snapshot, json!({ "ok": false, "error": "deadline" }));
            let newer = json!({ "id": "p1-chief", "generation": 2, "body": "x".repeat(16_000) });
            let answer = host.request("pane.write_paste", newer).await.unwrap();
            assert_eq!(answer, json!({ "ok": false, "error": "deadline" }));
            // And what the last open of a pane said is what it declares: opened again with
            // nothing, the pane has the bridge's.
            host.open(open(&[])).await.unwrap();
            let again = host
                .request("pane.write_paste", a_paste_of(16_000))
                .await
                .unwrap();
            assert_eq!(again, json!({ "ok": false, "error": "deadline" }));
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
    // No answer by the open's deadline is no refusal: the host goes on with the open,
    // and its window may start after (kluris, 2026-10-26).
    assert_eq!(
        opened(&json!({ "ok": false, "error": "deadline" })),
        Opened::Unanswered
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
async fn an_open_the_host_never_answers_is_unanswered_after_sixty_seconds_not_refused() {
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
            assert_eq!(answer, Ok(Opened::Unanswered));
            assert_eq!(started.elapsed(), Duration::from_secs(60));
        })
        .await;
}

#[tokio::test(start_paused = true)]
async fn an_open_waits_as_long_as_the_daemon_gives_the_host() {
    LocalSet::new()
        .run_until(async {
            let (daemon, app) = bridge_pair();
            app.on("pane.open", |_, _| async {
                pending::<()>().await;
                Ok(json!({}))
            });
            let host = host_over(&daemon).opening_within(Duration::from_secs(3));
            let started = tokio::time::Instant::now();
            assert_eq!(host.open(open(&[])).await, Ok(Opened::Unanswered));
            assert_eq!(started.elapsed(), Duration::from_secs(3));
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
async fn a_window_opens_on_an_npm_shim_with_its_node_beside_it_and_is_refused_without_one() {
    LocalSet::new()
        .run_until(async {
            let (daemon, app) = bridge_pair();
            let seen = Rc::new(RefCell::new(Vec::new()));
            let heard = Rc::clone(&seen);
            app.on("pane.open", move |_, body| {
                heard.borrow_mut().push(body);
                async { Ok(json!({ "ok": true })) }
            });
            // A Windows environment with the folder of the shim on its PATH and
            // no other: what the daemons of the tests are given.
            let root = tempfile::tempdir().unwrap();
            let folder = root.path();
            let shim = cf_harness::testing::npm_shim_stand_in(&folder.join("claude"));
            let env = Env::from_vars([("OS", "Windows_NT"), ("PATH", folder.to_str().unwrap())]);
            let host = BridgeHost::new(daemon.clone(), env);
            let window = || {
                let mut window = open(&[]);
                window.argv = vec![shim.to_string_lossy().into_owned(), "--x".to_owned()];
                window
            };

            assert_eq!(host.open(window()).await, Ok(Opened::Open { pid: None }));
            let text = |file: &str| folder.join(file).to_string_lossy().into_owned();
            assert_eq!(
                seen.borrow()[0]["argv"],
                json!([text("node.exe"), text("claude.js"), "--x"])
            );

            // Its Node taken away, the same shim is a limit, said so that
            // whoever meets it knows what to do, and nothing is sent.
            std::fs::remove_file(folder.join("node.exe")).unwrap();
            let failed = host.open(window()).await.unwrap_err();
            assert_eq!(failed.error, None);
            for said in [
                shim.to_string_lossy().as_ref(),
                "finds no Node for it",
                "Make the harness's Node visible to ConsensFlow",
                "install the harness's own build",
            ] {
                assert!(
                    failed.message.contains(said),
                    "{said:?}: {}",
                    failed.message
                );
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
            assert_eq!(seen.borrow().len(), 1);
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
async fn how_long_a_panes_screen_has_held_still_is_what_its_snapshot_says_and_none_where_it_says_nothing(
) {
    LocalSet::new()
        .run_until(async {
            let (daemon, app) = bridge_pair();
            let seen = Rc::new(RefCell::new(Vec::new()));
            let heard = Rc::clone(&seen);
            app.on("pane.snapshot", move |_, body| {
                let answer = match body["generation"].as_u64() {
                    // A pane that prints all the time and changes nothing it shows: what
                    // its screen shows is what is asked, not its output (T-1083).
                    Some(1) => json!({ "ok": true, "outputQuietMs": 5, "screenQuietMs": 320 }),
                    // A pane whose screen has shown nothing yet.
                    Some(2) => json!({ "ok": true, "screenQuietMs": null }),
                    // An answer that does not say, as a host older than the field's.
                    Some(3) => json!({ "ok": true }),
                    _ => json!({ "ok": false, "error": "no such pane" }),
                };
                heard.borrow_mut().push(body);
                async move { Ok(answer) }
            });
            let host = host_over(&daemon);
            assert_eq!(host.quiet(&pane("p1-zeus", 1)).await, Ok(Some(320.0)));
            assert_eq!(host.quiet(&pane("p1-zeus", 2)).await, Ok(None));
            assert_eq!(host.quiet(&pane("p1-zeus", 3)).await, Ok(None));
            assert_eq!(host.quiet(&pane("p1-zeus", 4)).await, Ok(None));
            // The request is the plain look at a pane: its id and generation, no tail asked.
            assert_eq!(
                seen.borrow()[0],
                json!({ "id": "p1-zeus", "generation": 1 })
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
                background: Vec::new(),
                possibly_left: Vec::new(),
                survivor: None,
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
async fn a_turn_is_marked_with_how_the_harness_keeps_its_servers_and_which_harness_it_is_by_the_pane_the_engine_names(
) {
    LocalSet::new()
        .run_until(async {
            let (daemon, app) = bridge_pair();
            let seen = Rc::new(RefCell::new(Vec::new()));
            let marked = Rc::clone(&seen);
            app.on("pane.mark", move |_, body| {
                marked.borrow_mut().push(body);
                async { Ok(json!({ "ok": true })) }
            });
            let host = host_over(&daemon);

            let said = [
                (Servers::Unknown, ""),
                (Servers::InGroup, "claude"),
                (Servers::AtLaunch, "codex"),
                (Servers::Unknown, "pi"),
            ];
            for (servers, harness) in said {
                host.mark(&pane("p1-zeus-amber-pine", 7), servers, harness)
                    .await;
            }

            assert_eq!(
                *seen.borrow(),
                [
                    json!({ "id": "p1-zeus-amber-pine", "generation": 7 }),
                    json!({
                        "id": "p1-zeus-amber-pine",
                        "generation": 7,
                        "servers": "inGroup",
                        "harness": "claude",
                    }),
                    json!({
                        "id": "p1-zeus-amber-pine",
                        "generation": 7,
                        "servers": "atLaunch",
                        "harness": "codex",
                    }),
                    json!({
                        "id": "p1-zeus-amber-pine",
                        "generation": 7,
                        "harness": "pi",
                    }),
                ]
            );
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

#[tokio::test]
async fn a_look_at_a_windows_commands_is_asked_with_the_rest_it_names_and_its_answer_read() {
    LocalSet::new()
        .run_until(async {
            let (daemon, app) = bridge_pair();
            let seen = Rc::new(RefCell::new(Vec::new()));
            let heard = Rc::clone(&seen);
            let answers = Rc::new(RefCell::new(vec![
                json!({ "ok": true, "commands": { "total": 3, "processes": [] } }),
                json!({ "ok": true, "commands": null }),
                json!({ "ok": true, "commands": null, "unreadable": true }),
                json!({ "ok": true, "commands": null, "unsettled": true }),
                json!({ "ok": true }),
                json!({ "ok": true, "commands": "three" }),
            ]));
            let next = Rc::clone(&answers);
            app.on("pane.commands", move |_, body| {
                heard.borrow_mut().push(body);
                let answer = next.borrow_mut().remove(0);
                async move { Ok(answer) }
            });
            let rested = Rc::new(RefCell::new(Vec::new()));
            let asked = Rc::clone(&rested);
            app.on("pane.rest", move |_, body| {
                asked.borrow_mut().push(body);
                async { Ok(json!({ "ok": true, "rest": 7 })) }
            });
            let host = host_over(&daemon);
            let at = pane("p1-zeus", 2);
            // So many run; the host cannot tell them (null); and what is no answer.
            assert_eq!(
                host.commands(&at, Some(7), Some(1_000)).await,
                CommandsSeen::Running(3)
            );
            assert_eq!(host.commands(&at, None, None).await, CommandsSeen::Untold);
            assert_eq!(
                host.commands(&at, None, None).await,
                CommandsSeen::Unreadable
            );
            assert_eq!(
                host.commands(&at, None, None).await,
                CommandsSeen::Unsettled
            );
            assert_eq!(
                host.commands(&at, None, None).await,
                CommandsSeen::Unanswered
            );
            assert_eq!(
                host.commands(&at, None, None).await,
                CommandsSeen::Unanswered
            );
            assert_eq!(
                serde_json::to_string(&seen.borrow()[0]).unwrap(),
                r#"{"id":"p1-zeus","generation":2,"since":7,"after":1000}"#
            );
            assert_eq!(
                serde_json::to_string(&seen.borrow()[1]).unwrap(),
                r#"{"id":"p1-zeus","generation":2}"#
            );
            assert_eq!(host.rest(&at, &[2, 5]).await, Some(7));
            assert_eq!(host.rest(&at, &[]).await, Some(7));
            assert_eq!(
                serde_json::to_string(&rested.borrow()[0]).unwrap(),
                r#"{"id":"p1-zeus","generation":2,"keep":[2,5]}"#
            );
            assert_eq!(
                serde_json::to_string(&rested.borrow()[1]).unwrap(),
                r#"{"id":"p1-zeus","generation":2}"#
            );
        })
        .await;
}

#[tokio::test]
async fn a_look_at_a_windows_commands_that_no_host_answers_is_unanswered_and_keeps_no_rest() {
    LocalSet::new()
        .run_until(async {
            let (daemon, app) = bridge_pair();
            app.on("pane.commands", |_, _| async {
                Err::<serde_json::Value, String>("no such pane".to_owned())
            });
            let host = host_over(&daemon);
            let at = pane("p1-zeus", 2);
            assert_eq!(
                host.commands(&at, None, None).await,
                CommandsSeen::Unanswered
            );
            // A host that knows no `pane.rest` keeps none.
            assert_eq!(host.rest(&at, &[]).await, None);
        })
        .await;
}

#[tokio::test]
async fn a_look_at_what_a_window_left_running_is_asked_by_its_pane_and_reads_what_may_be_its_own() {
    LocalSet::new()
        .run_until(async {
            let (daemon, app) = bridge_pair();
            let seen = Rc::new(RefCell::new(Vec::new()));
            let heard = Rc::clone(&seen);
            let answers = Rc::new(RefCell::new(vec![
                json!({
                    "ok": true,
                    "total": 2,
                    "processes": [{ "pid": 4711, "command": "bash retry.sh" }],
                    "possiblyLeft": [{ "pid": 4242, "program": "sleep" }],
                }),
                json!({ "ok": true, "total": 0, "processes": [] }),
                json!({ "ok": true }),
            ]));
            let next = Rc::clone(&answers);
            app.on("pane.background", move |_, body| {
                heard.borrow_mut().push(body);
                let answer = next.borrow_mut().remove(0);
                async move { Ok(answer) }
            });
            let host = host_over(&daemon);
            let at = pane("p1-zeus", 2);
            assert_eq!(
                host.left_running(&at).await,
                Some(Background {
                    total: 2,
                    processes: vec![cf_proto::panes::BackgroundProcess {
                        pid: 4711,
                        command: "bash retry.sh".to_owned(),
                    }],
                    possibly_left: vec![cf_proto::panes::PossiblyLeft {
                        pid: 4242,
                        program: "sleep".to_owned(),
                    }],
                })
            );
            // An older host names nothing outside the tree; one that says no look, nothing.
            assert_eq!(host.left_running(&at).await, Some(Background::default()));
            assert_eq!(host.left_running(&at).await, None);
            assert_eq!(
                serde_json::to_string(&seen.borrow()[0]).unwrap(),
                r#"{"id":"p1-zeus","generation":2}"#
            );
        })
        .await;
}

#[tokio::test]
async fn a_look_at_what_a_window_left_running_that_no_host_answers_names_nothing() {
    LocalSet::new()
        .run_until(async {
            let (daemon, app) = bridge_pair();
            app.on("pane.background", |_, _| async {
                Err::<serde_json::Value, String>("no such pane".to_owned())
            });
            let host = host_over(&daemon);
            assert_eq!(host.left_running(&pane("p1-zeus", 2)).await, None);
        })
        .await;
}

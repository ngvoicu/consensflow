//! The page's operations on the bridge, asked as the app asks them: every
//! one is registered and answers, `ping` says the bridge is up, an operation
//! that has not landed names itself, and the rules of how an operation
//! answers hold whatever serves it: `ok` first, a kick after a success and
//! never after a failure, and a panic at any poll an `{ok: false}` with the
//! bridge going on.

use std::cell::{Cell, RefCell};
use std::time::Duration;

use cf_bridge::local::Bridge;
use cf_engine::testing::Context;
use serde_json::json;
use tokio::task::LocalSet;

use super::*;
use crate::files::{Log, Trace};
use crate::testing::bridge_pair;

struct Rig {
    home: tempfile::TempDir,
    kicks: Rc<Cell<u32>>,
    page: Rc<Page>,
    errors: Rc<Errors>,
    _kit: Context,
}

fn rig() -> Rig {
    let home = tempfile::tempdir().unwrap();
    let kit = Context::new();
    let kicks = Rc::new(Cell::new(0));
    let counted = Rc::clone(&kicks);
    let page = Rc::new(Page {
        ledger: Rc::clone(&kit.ledger),
        engine: Rc::new(Rc::clone(&kit.dispatcher)),
        env: Env::default(),
        kick: Rc::new(move || counted.set(counted.get() + 1)),
    });
    let errors = Rc::new(Errors::new(
        Rc::new(Log::new(home.path())),
        Rc::new(Trace::new(home.path())),
    ));
    Rig {
        home,
        kicks,
        page,
        errors,
        _kit: kit,
    }
}

/// What the app asks, and gets back.
async fn ask(app: &Bridge, operation: &str, body: Value) -> Value {
    tokio::time::timeout(
        Duration::from_secs(5),
        app.request(operation, body, Some(Duration::from_secs(5))),
    )
    .await
    .expect("answered")
    .expect("the bridge is up")
}

/// Serves each operation as `serve` says.
fn serving(
    serve: impl Fn(Rc<Page>, PageOperation, Value) -> LocalBoxFuture<'static, Served> + 'static,
) -> Rc<Serve> {
    Rc::new(serve)
}

#[tokio::test]
async fn ping_says_the_bridge_is_up() {
    LocalSet::new()
        .run_until(async {
            let rig = rig();
            let (daemon, app) = bridge_pair();
            register(&daemon, &rig.page, &rig.errors);
            assert_eq!(ask(&app, "ping", json!({})).await, json!({ "ok": true }));
        })
        .await;
}

#[tokio::test]
async fn every_operation_is_registered_and_one_that_has_not_landed_names_itself() {
    LocalSet::new()
        .run_until(async {
            let rig = rig();
            let (daemon, app) = bridge_pair();
            register(&daemon, &rig.page, &rig.errors);
            for operation in PageOperation::ALL {
                let answer = ask(&app, operation.as_str(), json!({})).await;
                assert_eq!(answer["ok"], false, "{}", operation.as_str());
                assert_eq!(
                    answer["error"],
                    format!(
                        "the page operation {} is not served by this daemon yet",
                        operation.as_str()
                    )
                );
            }
            assert_eq!(rig.kicks.get(), 0, "nothing succeeded, nothing is woken");
        })
        .await;
}

#[tokio::test]
async fn what_the_app_does_not_forward_is_an_unknown_op() {
    LocalSet::new()
        .run_until(async {
            let rig = rig();
            let (daemon, app) = bridge_pair();
            register(&daemon, &rig.page, &rig.errors);
            assert_eq!(
                ask(&app, "task.delete", json!({})).await,
                json!({ "ok": false, "error": "unknown-op" })
            );
        })
        .await;
}

#[tokio::test]
async fn a_success_is_ok_first_then_its_fields_and_a_change_wakes_the_dispatcher_once() {
    LocalSet::new()
        .run_until(async {
            let rig = rig();
            let (daemon, app) = bridge_pair();
            let serve = serving(|_, operation, body| {
                Box::pin(async move {
                    let mut fields = Map::new();
                    fields.insert("operation".to_owned(), json!(operation.as_str()));
                    fields.insert("body".to_owned(), body);
                    Ok(fields)
                })
            });
            register_with(&daemon, &rig.page, &rig.errors, &serve);
            let answer = ask(&app, "project.open", json!({ "directory": "/work" })).await;
            let keys: Vec<&String> = answer.as_object().unwrap().keys().collect();
            assert_eq!(keys, ["ok", "operation", "body"]);
            assert_eq!(answer["ok"], true);
            assert_eq!(answer["body"], json!({ "directory": "/work" }));
            assert_eq!(rig.kicks.get(), 1);
        })
        .await;
}

#[tokio::test]
async fn an_operation_that_only_reads_wakes_nothing_and_a_failure_wakes_nothing_either() {
    LocalSet::new()
        .run_until(async {
            let rig = rig();
            let (daemon, app) = bridge_pair();
            let serve = serving(|_, operation, _| {
                Box::pin(async move {
                    if operation == PageOperation::ProjectDelete {
                        return Err("app is open: close it first".to_owned());
                    }
                    Ok(Map::new())
                })
            });
            register_with(&daemon, &rig.page, &rig.errors, &serve);
            assert_eq!(
                ask(&app, "board.get", json!({})).await,
                json!({ "ok": true })
            );
            assert_eq!(
                ask(&app, "projects.list", json!({})).await,
                json!({ "ok": true })
            );
            assert_eq!(rig.kicks.get(), 0, "a read changes nothing");
            assert_eq!(
                ask(&app, "project.delete", json!({ "project": 1 })).await,
                json!({ "ok": false, "error": "app is open: close it first" })
            );
            assert_eq!(rig.kicks.get(), 0, "a refusal changes nothing");
            assert_eq!(
                ask(&app, "task.cancel", json!({})).await,
                json!({ "ok": true })
            );
            assert_eq!(rig.kicks.get(), 1);
        })
        .await;
}

#[tokio::test]
async fn a_body_that_is_null_is_the_empty_one() {
    LocalSet::new()
        .run_until(async {
            let rig = rig();
            let (daemon, app) = bridge_pair();
            let given = Rc::new(RefCell::new(Vec::new()));
            let seen = Rc::clone(&given);
            let serve = serving(move |_, _, body| {
                seen.borrow_mut().push(body);
                Box::pin(async { Ok(Map::new()) })
            });
            register_with(&daemon, &rig.page, &rig.errors, &serve);
            ask(&app, "staff.last", Value::Null).await;
            ask(&app, "staff.last", json!({ "a": 1 })).await;
            assert_eq!(*given.borrow(), [json!({}), json!({ "a": 1 })]);
        })
        .await;
}

#[tokio::test]
async fn an_operation_that_panics_at_its_first_poll_answers_not_ok_and_the_bridge_goes_on() {
    LocalSet::new()
        .run_until(async {
            let rig = rig();
            let (daemon, app) = bridge_pair();
            // It panics before it makes its future: nothing was awaited.
            let serve = serving(|_, operation, _| {
                if operation == PageOperation::BoardGet {
                    panic!("a bug before the first wait");
                }
                Box::pin(async { Ok(Map::new()) })
            });
            register_with(&daemon, &rig.page, &rig.errors, &serve);
            assert_eq!(
                ask(&app, "board.get", json!({ "project": 1 })).await,
                json!({ "ok": false, "error": "a bug before the first wait" })
            );
            assert!(!daemon.closed(), "the bridge did not end over it");
            assert_eq!(ask(&app, "ping", json!({})).await, json!({ "ok": true }));
            assert_eq!(
                rig.kicks.get(),
                0,
                "a panic is a failure: nothing was woken"
            );
            assert_eq!(
                ask(&app, "member.add", json!({})).await,
                json!({ "ok": true })
            );
            let log = std::fs::read_to_string(rig.home.path().join("daemon.log")).unwrap();
            assert!(
                log.contains("error page operation board.get failed"),
                "{log}"
            );
            assert!(log.contains("panic: a bug before the first wait"), "{log}");
            let trace = std::fs::read_to_string(rig.home.path().join("events.jsonl")).unwrap();
            assert!(
                trace.contains(
                    r#""reason":"page operation board.get failed: a bug before the first wait""#
                ),
                "{trace}"
            );
            assert_eq!(
                rig.kicks.get(),
                1,
                "only the success after it woke the dispatcher"
            );
        })
        .await;
}

#[tokio::test]
async fn an_operation_that_panics_after_a_wait_answers_not_ok_too() {
    LocalSet::new()
        .run_until(async {
            let rig = rig();
            let (daemon, app) = bridge_pair();
            let serve = serving(|_, _, _| {
                Box::pin(async {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                    panic!("a bug after a wait");
                })
            });
            register_with(&daemon, &rig.page, &rig.errors, &serve);
            assert_eq!(
                ask(&app, "task.pause", json!({})).await,
                json!({ "ok": false, "error": "a bug after a wait" })
            );
            assert_eq!(rig.kicks.get(), 0);
            assert!(!daemon.closed());
        })
        .await;
}

#[tokio::test]
async fn operations_asked_together_begin_in_the_order_their_frames_were_read() {
    LocalSet::new()
        .run_until(async {
            let rig = rig();
            let (daemon, app) = bridge_pair();
            let began = Rc::new(RefCell::new(Vec::new()));
            let noted = Rc::clone(&began);
            let serve = serving(move |_, _, body| {
                // What it does before its first wait is done as its frame is read.
                noted.borrow_mut().push(body["n"].as_i64().unwrap());
                Box::pin(async {
                    tokio::time::sleep(Duration::from_millis(5)).await;
                    Ok(Map::new())
                })
            });
            register_with(&daemon, &rig.page, &rig.errors, &serve);
            let asked: Vec<_> = (0..8)
                .map(|n| {
                    app.request(
                        "member.add",
                        json!({ "n": n }),
                        Some(Duration::from_secs(5)),
                    )
                })
                .collect();
            for answer in futures_util::future::join_all(asked).await {
                assert_eq!(answer.unwrap(), json!({ "ok": true }));
            }
            assert_eq!(*began.borrow(), (0..8).collect::<Vec<i64>>());
        })
        .await;
}

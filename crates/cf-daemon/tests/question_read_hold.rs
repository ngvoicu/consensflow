#![allow(dead_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]
//! A whole read is accepted wherever transport says its reader already has the
//! message, and refused only where a hold keeps one the reader does not have yet
//! (Q2, review T-2293, 4).
#[path = "../../cf-ledger/tests/receipt/fixture.rs"]
mod fixture;
use cf_daemon::{
    api::{
        answer::Content,
        body::Body,
        callers::caller_of,
        context::Context,
        credentials::Credentials,
        request::Request,
        routes::{dispatch, recognize},
    },
    files::{Log, Trace},
};
use cf_engine::seams::QuestionWindowStart;
use fixture::{world, World};
use hyper::Method;
use serde_json::{json, Value};
use std::cell::RefCell;
use std::rc::Rc;

fn into_context(w: World) -> (Context, tempfile::TempDir) {
    let World { ledger, dir, .. } = w;
    let c = Context {
        ledger: Rc::new(RefCell::new(ledger)),
        credentials: Rc::new(Credentials::new()),
        kick: Rc::new(|| {}),
        changed: Rc::new(|| {}),
        finish: Rc::new(|_, _, _| panic!("unused finish")),
        held: Rc::new(|_| None),
        closing: Default::default(),
        log: Rc::new(Log::new(dir.path())),
        trace: Rc::new(Trace::new(dir.path())),
        cf: Rc::new(|_| cf_proto::cf_word::CfWord::of_word("cf").unwrap()),
        specs_folder: Rc::new(|_, _| None),
        ordinary_specs: Rc::new(|_, _| Ok(None)),
        tooling_calls: false,
    };
    (c, dir)
}
fn issued(c: &Context, p: i64, member: i64) -> String {
    let start = QuestionWindowStart::from_ledger(&c.ledger.borrow(), member).unwrap();
    c.credentials.issue_for_window(p, member, start).unwrap()
}
fn req(path: &str, credential: &str, body: Value) -> Request {
    let bytes = bytes::Bytes::from(body.to_string());
    Request::new(
        Method::POST,
        path,
        Some(format!("Bearer {credential}")),
        Body::new(futures_util::stream::iter(vec![Ok(bytes)])),
    )
    .unwrap()
}
async fn post(c: &Context, r: Request) -> (u16, Value) {
    let caller = caller_of(c, &r).unwrap();
    let route = recognize(&r.method, &r.path).unwrap();
    let ans = dispatch(c, &caller, route, r)
        .await
        .unwrap_or_else(|e| e.answer());
    let Content::Json(body) = ans.content else {
        panic!("expected JSON")
    };
    (ans.status, body)
}

async fn got(c: &Context, path: &str, auth: &str) -> (u16, Value) {
    let r = Request::new(
        Method::GET,
        path,
        Some(format!("Bearer {auth}")),
        Body::new(futures_util::stream::empty()),
    )
    .unwrap();
    post(c, r).await
}

async fn get(c: &Context, a: &str, id: i64) -> (u16, Value) {
    got(c, &format!("/api/inbox/{id}"), a).await
}

async fn receipt(c: &Context, a: &str, id: i64) -> (u16, Value) {
    post(
        c,
        req(
            "/api/answers/read",
            a,
            json!({"answers":[id],"via":"inbox"}),
        ),
    )
    .await
}

use cf_engine::testing::Context as Engine;

fn engine_routes(e: &Engine) -> (Context, tempfile::TempDir) {
    let (mut routes, dir) = into_context(world());
    routes.ledger = Rc::clone(&e.ledger);
    (routes, dir)
}

fn working(e: &Engine) -> (i64, i64, String) {
    let p = e.with_staff(&["zeus"]).id;
    e.pass().unwrap();
    let t = e.give(p, "zeus", "Implement the parser").task.number;
    for _ in 0..3 {
        e.pass().unwrap();
    }
    let task = e.task(p, t).task;
    assert_eq!(task.state, "working");
    (p, t, task.assignee.unwrap())
}

/// Review T-2293, 4: an answer a question's door claimed for its member is the
/// member's already. Read whole during a Close, its receipt is taken; the Close
/// does not return it to the chief, and the member's task goes on.
#[tokio::test]
async fn a_claimed_answer_read_during_a_close_is_received_and_not_returned() {
    let e = Engine::new();
    let (p, t, worker) = working(&e);
    let worker_id = e.id(p, &worker);
    let chief = e.id(p, "chief");
    let question = e
        .ledger
        .borrow_mut()
        .ask(
            p,
            &cf_ledger::NewQuestion {
                from: Some(worker.clone()),
                to: "chief".into(),
                task: Some(t),
                questions: Some(json!([{"question":"Which colour?","header":"Colour",
                    "options":[{"label":"red"},{"label":"blue"}],"multiple":false}])),
                ..Default::default()
            },
        )
        .unwrap();
    let ans = e
        .ledger
        .borrow_mut()
        .answer(question.id, chief, None, Some(&json!([["blue"]])))
        .unwrap();
    let claim = e
        .ledger
        .borrow_mut()
        .claim_answer(question.id, worker_id)
        .unwrap();
    assert!(matches!(claim, cf_ledger::Claim::Answered(ref m) if m.id == ans.id));
    let (r, _dir) = engine_routes(&e);
    let a = issued(&r, p, worker_id);
    let gate = e
        .host
        .hold_kills(&e.dispatcher.pane(worker_id).unwrap(), &[false])
        .remove(0);
    let closing = e.begin_close_project(p);
    e.settle();
    assert!(!closing.ended());
    assert!(e.dispatcher.pane(worker_id).is_some());
    assert_eq!(e.project(p).state, "suspended");
    assert!(e.ledger.borrow().handed_over(p).unwrap().contains(&ans.id));
    let (code, body) = get(&r, &a, ans.id).await;
    assert_eq!((code, &body["message"]["body"]), (200, &json!(ans.body)));
    assert_eq!(receipt(&r, &a, ans.id).await.0, 200);
    assert_eq!(e.message(ans.id).state, "read", "the receipt is taken");
    gate.open();
    e.finish(closing).unwrap();
    assert!(e.dispatcher.pane(worker_id).is_none());
    e.resume_project(p).unwrap();
    for _ in 0..3 {
        e.pass().unwrap();
    }
    let state = e.message(ans.id);
    let events = e.ledger.borrow().events(p, 0, 1000).unwrap();
    let returned = events
        .iter()
        .filter(|ev| ev.kind == "message.returned" && ev.data["message"] == ans.id)
        .count();
    assert_eq!(
        (state.state.as_str(), returned),
        ("read", 0),
        "a Close does not return an answer its member read"
    );
}

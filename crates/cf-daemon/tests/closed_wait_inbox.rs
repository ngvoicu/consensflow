#![allow(dead_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]
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
use cf_ledger::NewNote;
use fixture::{world, World};
use hyper::Method;
use serde_json::{json, Value};
use std::cell::RefCell;
use std::rc::Rc;

fn assignment(w: &mut World, gated: bool) -> (i64, String, i64) {
    let t = w.open("Review fixture").task.number;
    let given = w.assign(t, "zeus");
    let b = given.message.unwrap().id;
    if gated {
        w.ledger.approve_message(b, "human").unwrap();
    }
    (t, given.task.assignee.unwrap(), b)
}
fn notices(w: &World, words: &str) -> usize {
    w.ledger
        .inbox(w.id("chief"), 100)
        .unwrap()
        .iter()
        .filter(|m| m.kind == "note" && m.body.contains(words))
        .count()
}
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


// Round 2 of Q2 (review T-2017, 3): the inbox lists, serves and takes the receipt of no
// reminder whose wait no longer allows it.
#[tokio::test]
async fn q2_the_inbox_serves_no_stale_reminder() {
    for (in_progress, list_first) in [(false, true), (true, false)] {
        let mut w = world();
        let task = w.open("Review fixture").task.number;
        let given = w.assign(task, "zeus");
        let session = given.task.assignee.clone().unwrap();
        w.deliver(given.message.unwrap().id);
        let question = w.ask(&session, task, "Which format?").id;
        w.deliver(question);
        w.ledger.begin_question_engine();
        w.ledger.close_questions_of_ended_engine().unwrap();
        let notice: i64 = w
            .ledger
            .sqlite()
            .query_row("SELECT MAX(id) FROM message WHERE kind='note' AND instr(body,'closed when')>0", [], |r| r.get(0))
            .unwrap();
        w.deliver(notice);
        let reminder = w
            .ledger
            .remind_question(w.project, question, None, &NewNote { from: None, to: "chief".into(), task: Some(task), body: "It closed".into() })
            .unwrap()
            .unwrap();
        if in_progress {
            w.begin(reminder.id);
        }
        w.ledger.tell_queued(w.project, task, "chief", "New words").unwrap();
        let (p, chief) = (w.project, w.id("chief"));
        let (c, _dir) = into_context(w);
        let auth = issued(&c, p, chief);
        if list_first {
            let (code, listed) = got(&c, "/api/inbox", &auth).await;
            assert_eq!(code, 200);
            assert!(!listed.to_string().contains("It closed"), "{in_progress}: listed {listed}");
        }
        let (code, body) = got(&c, &format!("/api/inbox/{}", reminder.id), &auth).await;
        assert_eq!(code, 404, "{in_progress}: served {body}");
        assert!(!body.to_string().contains("It closed"));
        if !list_first {
            let (code, listed) = got(&c, "/api/inbox", &auth).await;
            assert_eq!(code, 200);
            assert!(!listed.to_string().contains("It closed"), "{in_progress}: listed {listed}");
        }
        let (status, _) = post(&c, req("/api/answers/read", &auth, json!({"answers":[reminder.id],"via":"inbox"}))).await;
        assert!(status == 200 || status == 404 || status == 400, "{status}");
        let l = c.ledger.borrow();
        let after = l.message(reminder.id).unwrap().unwrap();
        assert_eq!(after.state, "cancelled", "{in_progress}");
        assert!(after.receipt.get("readEarly").is_none());
    }
}

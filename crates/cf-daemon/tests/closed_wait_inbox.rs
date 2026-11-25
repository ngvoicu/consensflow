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
            .query_row(
                "SELECT MAX(id) FROM message WHERE kind='note' AND instr(body,'closed when')>0",
                [],
                |r| r.get(0),
            )
            .unwrap();
        w.deliver(notice);
        let reminder = w
            .ledger
            .remind_question(
                w.project,
                question,
                None,
                &NewNote {
                    from: None,
                    to: "chief".into(),
                    task: Some(task),
                    body: "It closed".into(),
                },
            )
            .unwrap()
            .unwrap();
        if in_progress {
            w.begin(reminder.id);
        }
        w.ledger
            .tell_queued(w.project, task, "chief", "New words")
            .unwrap();
        let (p, chief) = (w.project, w.id("chief"));
        let (c, _dir) = into_context(w);
        let auth = issued(&c, p, chief);
        if list_first {
            let (code, listed) = got(&c, "/api/inbox", &auth).await;
            assert_eq!(code, 200);
            assert!(
                !listed.to_string().contains("It closed"),
                "{in_progress}: listed {listed}"
            );
        }
        let (code, body) = got(&c, &format!("/api/inbox/{}", reminder.id), &auth).await;
        assert_eq!(code, 404, "{in_progress}: served {body}");
        assert!(!body.to_string().contains("It closed"));
        if !list_first {
            let (code, listed) = got(&c, "/api/inbox", &auth).await;
            assert_eq!(code, 200);
            assert!(
                !listed.to_string().contains("It closed"),
                "{in_progress}: listed {listed}"
            );
        }
        let (status, _) = post(
            &c,
            req(
                "/api/answers/read",
                &auth,
                json!({"answers":[reminder.id],"via":"inbox"}),
            ),
        )
        .await;
        assert!(status == 200 || status == 404 || status == 400, "{status}");
        let l = c.ledger.borrow();
        let after = l.message(reminder.id).unwrap().unwrap();
        let pending = if in_progress { "delivering" } else { "queued" };
        assert_eq!(
            after.state, pending,
            "{in_progress}: a read changes nothing"
        );
        assert!(after.receipt.get("readEarly").is_none());
    }
}

/// T-n with a question of its member's that a restart closed, its note had by the
/// chief, and the closed question's reminder queued.
fn closed_and_reminded(w: &mut World) -> (i64, String, i64, i64) {
    let task = w.open("Review fixture").task.number;
    let given = w.assign(task, "zeus");
    let session = given.task.assignee.clone().unwrap();
    w.deliver(given.message.unwrap().id);
    let question = w.ask(&session, task, "Which format?").id;
    w.deliver(question);
    w.ledger.begin_question_engine();
    w.ledger.close_questions_of_ended_engine().unwrap();
    let notice = newest_close_notice(w);
    w.deliver(notice);
    let reminder = remind(w, task, question, "It closed");
    (task, session, question, reminder)
}

fn newest_close_notice(w: &World) -> i64 {
    w.ledger
        .sqlite()
        .query_row(
            "SELECT MAX(id) FROM message WHERE kind='note' AND instr(body,'closed when')>0",
            [],
            |r| r.get(0),
        )
        .unwrap()
}

fn remind(w: &mut World, task: i64, question: i64, body: &str) -> i64 {
    w.ledger
        .remind_question(
            w.project,
            question,
            None,
            &NewNote {
                from: None,
                to: "chief".into(),
                task: Some(task),
                body: body.into(),
            },
        )
        .unwrap()
        .expect("a reminder")
        .id
}

async fn listed(c: &Context, auth: &str) -> Vec<i64> {
    let (code, body) = got(c, "/api/inbox", auth).await;
    assert_eq!(code, 200);
    body["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["id"].as_i64().unwrap())
        .collect()
}

/// The inbox lists it and serves its text; or neither, and takes no receipt of it,
/// and changes nothing of it.
async fn shown(c: &Context, auth: &str, reminder: i64) -> bool {
    let in_list = listed(c, auth).await.contains(&reminder);
    let (code, body) = got(c, &format!("/api/inbox/{reminder}"), auth).await;
    assert_eq!(
        in_list,
        code == 200,
        "listed {in_list}, served {code}: {body}"
    );
    if !in_list {
        assert_eq!(code, 404, "{body}");
        assert!(!body.to_string().contains("It closed"), "{body}");
        let before = c.ledger.borrow().message(reminder).unwrap().unwrap();
        let (status, _) = post(
            c,
            req(
                "/api/answers/read",
                auth,
                json!({"answers":[reminder],"via":"inbox"}),
            ),
        )
        .await;
        assert!(status == 200 || status == 404, "{status}");
        let after = c.ledger.borrow().message(reminder).unwrap().unwrap();
        assert_eq!(before.state, after.state, "a read changed the reminder");
        assert!(after.receipt.get("readEarly").is_none());
    }
    in_list
}

fn chief_window(w: World) -> (Context, tempfile::TempDir, String, i64) {
    let (p, chief) = (w.project, w.id("chief"));
    let (c, dir) = into_context(w);
    let auth = issued(&c, p, chief);
    (c, dir, auth, p)
}

/// Round 3 (review T-2019): a reminder whose task was cancelled or deleted, or
/// whose member was removed, is cancelled by that and is shown to no reader.
#[tokio::test]
async fn q2_the_inbox_shows_no_reminder_of_an_ended_task_or_member() {
    for end in ["a cancel", "a deletion", "the member's removal"] {
        for in_progress in [false, true] {
            let mut w = world();
            let (task, session, _question, reminder) = closed_and_reminded(&mut w);
            if in_progress {
                w.begin(reminder);
            }
            match end {
                "a cancel" => {
                    w.ledger.cancel_task(w.project, task, "human").unwrap();
                }
                "a deletion" => {
                    w.ledger.cancel_task(w.project, task, "human").unwrap();
                    w.ledger.delete_tasks(w.project, &[task]).unwrap();
                }
                _ => {
                    let member = session
                        .rsplit_once("-session-")
                        .map_or(session.as_str(), |(h, _)| h);
                    w.ledger.remove_member(w.project, member).unwrap();
                }
            }
            let (c, _dir, auth, _) = chief_window(w);
            assert!(
                !shown(&c, &auth, reminder).await,
                "{end}, in progress {in_progress}"
            );
        }
    }
}

/// Round 3 (review T-2019): a reminder a project's Close holds is hidden, not
/// consumed, and comes back with the Resume.
#[tokio::test]
async fn q2_a_look_while_the_project_is_closed_consumes_no_reminder() {
    let mut w = world();
    let (task, _session, _question, reminder) = closed_and_reminded(&mut w);
    w.ledger.set_project_state(w.project, "suspended").unwrap();
    w.ledger
        .pause_with_project(w.project, task, "the project closed")
        .unwrap();
    let (c, _dir, auth, project) = chief_window(w);
    assert!(!shown(&c, &auth, reminder).await, "held by the Close");
    assert_eq!(
        c.ledger.borrow().message(reminder).unwrap().unwrap().state,
        "queued"
    );
    c.ledger
        .borrow_mut()
        .set_project_state(project, "open")
        .unwrap();
    assert!(shown(&c, &auth, reminder).await, "back at the Resume");
    let (status, body) = post(
        &c,
        req(
            "/api/answers/read",
            &auth,
            json!({"answers":[reminder],"via":"inbox"}),
        ),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        c.ledger.borrow().message(reminder).unwrap().unwrap().state,
        "read"
    );
}

/// Round 3 (review T-2019): a hundred reminders that no longer stand hide none
/// that does, older than they are.
#[tokio::test]
async fn q2_a_hundred_withdrawn_reminders_hide_no_eligible_one() {
    let mut w = world();
    let task = w.open("Review fixture").task.number;
    let given = w.assign(task, "zeus");
    let zeus = given.task.assignee.clone().unwrap();
    w.deliver(given.message.unwrap().id);
    let questions: Vec<i64> = (0..100)
        .map(|n| {
            let question = w.ask(&zeus, task, &format!("Question {n}?")).id;
            w.deliver(question);
            question
        })
        .collect();
    w.ledger.begin_question_engine();
    w.ledger.close_questions_of_ended_engine().unwrap();
    let notices: Vec<i64> = w
        .ledger
        .sqlite()
        .prepare("SELECT id FROM message WHERE kind='note' AND instr(body,'closed when')>0")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    for notice in notices {
        if w.message(notice).state == "queued" {
            w.deliver(notice);
        }
    }
    let other = w.open("Other fixture").task.number;
    let given = w.assign(other, "diana");
    let diana = given.task.assignee.clone().unwrap();
    w.deliver(given.message.unwrap().id);
    let waiting = w.ask(&diana, other, "Still open?").id;
    w.deliver(waiting);
    let eligible = remind(&mut w, other, waiting, "Answer the other");
    for question in &questions {
        remind(&mut w, task, *question, "It closed");
    }
    w.ledger
        .tell_queued(w.project, task, "chief", "New words")
        .unwrap();
    let (c, _dir, auth, _) = chief_window(w);
    let ids = listed(&c, &auth).await;
    assert!(
        ids.contains(&eligible),
        "m-{eligible} among {} listed",
        ids.len()
    );
}

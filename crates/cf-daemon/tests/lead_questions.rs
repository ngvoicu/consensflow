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
use cf_ledger::{HandOver, NewLead, NewTask};
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


/// The ledger's own clock, in ms, at its latest event.
fn latest(c: &Context) -> i64 {
    let at: String = c
        .ledger
        .borrow()
        .sqlite()
        .query_row("SELECT at FROM event ORDER BY id DESC LIMIT 1", [], |r| {
            r.get(0)
        })
        .unwrap();
    cf_base::time::parse(&at).unwrap()
}
fn handed_at(c: &Context, brief: i64) -> i64 {
    let at: String = c
        .ledger
        .borrow()
        .sqlite()
        .query_row("SELECT handed_at FROM message WHERE id = ?", [brief], |r| {
            r.get(0)
        })
        .unwrap();
    cf_base::time::parse(&at).unwrap()
}
fn asked(auth: &str, words: &str, at: Option<i64>) -> Request {
    let mut body = json!({ "body": words });
    if let Some(at) = at {
        body["askedAt"] = json!(at);
    }
    req("/api/questions", auth, body)
}
/// A brief handed over and taken, with its admission.
fn take(c: &Context, brief: i64) {
    let mut l = c.ledger.borrow_mut();
    l.begin_delivery(brief).unwrap();
    assert_eq!(l.admit(brief, &[]).unwrap(), HandOver::Admitted);
    l.question_brief_handed_over(brief).unwrap();
    l.confirm_delivery(brief, Some(&json!({"test":"received"})))
        .unwrap();
}
fn stored(body: &Value) -> Option<i64> {
    body["message"]["task"].as_i64()
}

/// Questions in a project with a lead (the leads design, section 3; step 4 of the build): a
/// member's question goes to the decider of the task its window holds, on every path into
/// `POST /api/questions` (`cf ask` on all five harnesses, and the question tool of Claude Code,
/// Devin and Codex, whose doors post here with options).

fn with_a_lead(w: &mut World) -> String {
    w.ledger
        .add_lead(
            w.project,
            &NewLead {
                harness: "codex".into(),
                model: None,
                effort: None,
                model_name: None,
                scope: "the site".into(),
                own_hands: false,
            },
        )
        .unwrap()
        .handle
}

/// A task `from` gives the standard workers, at work in a new session of zeus: the task, the
/// session and its brief.
fn given_by(w: &mut World, from: &str, gated: bool) -> (i64, String, i64) {
    let t = w
        .ledger
        .create_task(
            w.project,
            &NewTask {
                from: from.into(),
                pool: Some("worker".into()),
                tier: Some("standard".into()),
                body: "Parser".into(),
                ..NewTask::default()
            },
        )
        .unwrap()
        .task
        .number;
    let given = w.assign(t, "zeus");
    let b = given.message.unwrap().id;
    if gated {
        w.ledger.approve_message(b, "human").unwrap();
    }
    (t, given.task.assignee.unwrap(), b)
}

fn recipient(c: &Context, id: i64) -> String {
    c.ledger.borrow().message(id).unwrap().unwrap().recipient
}

fn questions_of(c: &Context, handle_id: i64) -> Vec<cf_ledger::MessageView> {
    c.ledger
        .borrow()
        .inbox(handle_id, 100)
        .unwrap()
        .into_iter()
        .filter(|m| m.kind == "question")
        .collect()
}

fn options() -> Value {
    json!([{ "question": "Which grammar?", "header": "Grammar", "options": [
        { "label": "PEG" }, { "label": "LR" },
    ] }])
}

#[tokio::test]
async fn a_members_question_on_a_leads_task_by_cf_ask_and_by_its_question_tool_is_the_leads() {
    for native in [false, true] {
        let mut w = world();
        let lead = with_a_lead(&mut w);
        let (task, session, brief) = given_by(&mut w, &lead, false);
        w.deliver(brief);
        let (p, window, chief, lead_id) = (w.project, w.id(&session), w.id("chief"), w.id(&lead));
        let (c, _dir) = into_context(w);
        let auth = issued(&c, p, window);
        let request = if native {
            req("/api/questions", &auth, json!({ "questions": options() }))
        } else {
            asked(&auth, "Which grammar?", None)
        };
        let (code, body) = post(&c, request).await;
        assert_eq!((code, stored(&body)), (201, Some(task)), "native={native}: {body}");
        let q = body["message"]["id"].as_i64().unwrap();
        assert_eq!(recipient(&c, q), lead, "native={native}");
        assert_eq!(questions_of(&c, lead_id).len(), 1, "native={native}");
        assert!(questions_of(&c, chief).is_empty(), "native={native}");
    }
}

#[tokio::test]
async fn a_members_question_on_the_chiefs_or_the_humans_task_is_the_chiefs_with_a_lead_there() {
    for requester in ["chief", "human"] {
        let mut w = world();
        let lead = with_a_lead(&mut w);
        let (task, session, brief) = given_by(&mut w, requester, false);
        w.deliver(brief);
        let (p, window, lead_id) = (w.project, w.id(&session), w.id(&lead));
        let (c, _dir) = into_context(w);
        let auth = issued(&c, p, window);
        let (code, body) = post(&c, asked(&auth, "Which grammar?", None)).await;
        assert_eq!((code, stored(&body)), (201, Some(task)), "{requester}: {body}");
        let q = body["message"]["id"].as_i64().unwrap();
        assert_eq!(recipient(&c, q), "chief", "{requester}");
        assert!(questions_of(&c, lead_id).is_empty(), "{requester}");
    }
}

#[tokio::test]
async fn an_idle_windows_question_after_a_leads_task_was_accepted_is_the_chiefs() {
    let mut w = world();
    let lead = with_a_lead(&mut w);
    let (t, session, brief) = given_by(&mut w, &lead, false);
    w.deliver(brief);
    let (p, window) = (w.project, w.id(&session));
    let (c, _dir) = into_context(w);
    let auth = issued(&c, p, window);
    {
        let mut l = c.ledger.borrow_mut();
        l.record_result(p, t, "Result").unwrap();
        l.accept_task(p, t, &lead).unwrap();
    }
    let after = latest(&c) + 1;
    let (code, fresh) = post(&c, asked(&auth, "From a fresh idle turn", Some(after))).await;
    assert_eq!((code, stored(&fresh)), (201, None), "{fresh}");
    let q = fresh["message"]["id"].as_i64().unwrap();
    assert_eq!(recipient(&c, q), "chief");
}

/// The window held a lead's task, and the chief's follow-up is on its way into it.
async fn moving_from_a_leads_task_to_the_chiefs(
) -> (Context, tempfile::TempDir, String, i64, i64, i64, String, i64) {
    let mut w = world();
    let lead = with_a_lead(&mut w);
    let (old, session, b) = given_by(&mut w, &lead, false);
    w.deliver(b);
    w.ledger.record_result(w.project, old, "Result").unwrap();
    let next = w.give_to(&session, &[], "Distinct next brief").unwrap();
    let brief = next.message.unwrap().id;
    let (p, worker) = (w.project, w.id(&session));
    let (c, dir) = into_context(w);
    let auth = issued(&c, p, worker);
    (c, dir, auth, old, next.task.number, brief, lead, worker)
}

#[tokio::test]
async fn a_question_stamped_in_the_leads_holding_while_the_chiefs_brief_is_handed_is_the_leads() {
    let (c, _dir, auth, old, _next, brief, lead, _worker) =
        moving_from_a_leads_task_to_the_chiefs().await;
    let asking = latest(&c) + 1;
    {
        let mut l = c.ledger.borrow_mut();
        l.begin_delivery(brief).unwrap();
        assert_eq!(l.admit(brief, &[]).unwrap(), HandOver::Admitted);
        l.question_brief_handed_over(brief).unwrap();
    }
    let (code, body) = post(&c, asked(&auth, "From the lead's turn", Some(asking))).await;
    assert_eq!((code, stored(&body)), (201, Some(old)), "{body}");
    let q = body["message"]["id"].as_i64().unwrap();
    assert_eq!(recipient(&c, q), lead, "the recipient follows the bound task");
}

#[tokio::test]
async fn a_hand_over_called_off_moves_an_unread_question_to_the_leads_and_leaves_a_read_one_with_its_reader(
) {
    for read in [false, true] {
        let (c, _dir, auth, old, next, brief, lead, worker) =
            moving_from_a_leads_task_to_the_chiefs().await;
        {
            let mut l = c.ledger.borrow_mut();
            l.begin_delivery(brief).unwrap();
            assert_eq!(l.admit(brief, &[]).unwrap(), HandOver::Admitted);
            l.question_brief_handed_over(brief).unwrap();
        }
        let (code, body) = post(&c, asked(&auth, "Asked during the send", None)).await;
        assert_eq!((code, stored(&body)), (201, Some(next)), "read={read}: {body}");
        let q = body["message"]["id"].as_i64().unwrap();
        assert_eq!(recipient(&c, q), "chief", "read={read}: the chief's task, the chief's");
        if read {
            let mut l = c.ledger.borrow_mut();
            l.begin_delivery(q).unwrap();
            l.confirm_delivery(q, Some(&json!({"test":"received"}))).unwrap();
        }
        c.ledger.borrow_mut().question_brief_refused(brief).unwrap();
        let held = c.ledger.borrow().question_holding(worker).unwrap().unwrap();
        assert!(held.active && held.task_number == old, "read={read}");
        let lead_id = c.ledger.borrow().project(c_project(&c)).unwrap().unwrap()
            .participants.iter().find(|p| p.handle == lead).unwrap().id;
        let told: Vec<String> = c
            .ledger
            .borrow()
            .inbox(lead_id, 100)
            .unwrap()
            .into_iter()
            .filter(|m| m.kind == "note" && m.body.contains("Asked during the send"))
            .map(|m| m.body)
            .collect();
        if read {
            assert_eq!(recipient(&c, q), "chief", "a read question stays with its reader");
            assert_eq!(told.len(), 1, "the lead is told its words once: {told:?}");
            assert!(told[0].contains(&format!("m-{q}")), "{told:?}");
            // The reader answers it.
            let chief = c.ledger.borrow().project(c_project(&c)).unwrap().unwrap()
                .participants.iter().find(|p| p.handle == "chief").unwrap().id;
            let a = c.ledger.borrow_mut()
                .answer(q, chief, Some(&json!("Use PEG")), None)
                .unwrap();
            assert_eq!(a.task_number, Some(old));
        } else {
            assert_eq!(recipient(&c, q), lead, "an unread question goes to the lead");
            assert!(told.is_empty(), "{told:?}");
            assert_eq!(questions_of(&c, lead_id).len(), 1);
            let a = c.ledger.borrow_mut()
                .answer(q, lead_id, Some(&json!("Use PEG")), None)
                .unwrap();
            assert_eq!(a.task_number, Some(old));
        }
    }
}

fn c_project(c: &Context) -> i64 {
    c.ledger.borrow().sqlite()
        .query_row("SELECT id FROM project ORDER BY id LIMIT 1", [], |r| r.get(0))
        .unwrap()
}

#[tokio::test]
async fn with_approval_a_question_on_a_leads_task_waits_at_the_gate_for_the_lead_and_goes_to_it() {
    let mut w = fixture::gated_world();
    let lead = with_a_lead(&mut w);
    let (task, session, brief) = given_by(&mut w, &lead, true);
    w.deliver(brief);
    let (p, window, lead_id) = (w.project, w.id(&session), w.id(&lead));
    let (c, _dir) = into_context(w);
    let auth = issued(&c, p, window);
    let (code, body) = post(&c, asked(&auth, "Which grammar?", None)).await;
    assert_eq!((code, stored(&body)), (201, Some(task)), "{body}");
    let q = body["message"]["id"].as_i64().unwrap();
    let gated = c.ledger.borrow().message(q).unwrap().unwrap();
    assert_eq!((gated.state.as_str(), gated.recipient.as_str()), ("gated", lead.as_str()));
    c.ledger.borrow_mut().approve_message(q, "human").unwrap();
    let released = c.ledger.borrow().message(q).unwrap().unwrap();
    assert_eq!((released.state.as_str(), released.recipient.as_str()), ("queued", lead.as_str()));
    assert_eq!(c.ledger.borrow().next_delivery(lead_id).unwrap().map(|m| m.id), Some(q));
}

#[tokio::test]
async fn a_question_its_codex_window_posts_twice_on_a_reconnect_goes_twice_to_the_same_lead() {
    let mut w = world();
    let lead = with_a_lead(&mut w);
    let (task, session, brief) = given_by(&mut w, &lead, false);
    w.deliver(brief);
    let (p, window) = (w.project, w.id(&session));
    let (c, _dir) = into_context(w);
    let auth = issued(&c, p, window);
    let mut asked_to = Vec::new();
    for _ in 0..2 {
        let (code, body) =
            post(&c, req("/api/questions", &auth, json!({ "questions": options() }))).await;
        assert_eq!((code, stored(&body)), (201, Some(task)), "{body}");
        asked_to.push(recipient(&c, body["message"]["id"].as_i64().unwrap()));
    }
    assert_eq!(asked_to, [lead.clone(), lead]);
}

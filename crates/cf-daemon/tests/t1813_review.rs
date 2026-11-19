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
use cf_ledger::{Claim, HandOver};
use fixture::{gated_world, world, World};
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

#[tokio::test]
async fn t1823_finding_01_request_authenticated_while_idle_keeps_its_session_binding() {
    let mut w = world();
    let (first, session, brief) = assignment(&mut w, false);
    w.deliver(brief);
    w.ledger.record_result(w.project, first, "Done").unwrap();
    w.ledger.accept_task(w.project, first, "human").unwrap();
    let (project, window, chief) = (w.project, w.id(&session), w.id("chief"));
    let next = w.give_to(&session, &[], "Different task").unwrap();
    let next_brief = next.message.unwrap().id;
    let (ctx, _dir) = into_context(w);
    let member = issued(&ctx, project, window);
    let request = req(
        "/api/questions",
        &member,
        json!({"body":"Idle session question"}),
    );
    let caller = caller_of(&ctx, &request).unwrap();
    assert!(caller.question_holding.is_none());
    {
        let mut l = ctx.ledger.borrow_mut();
        l.begin_delivery(next_brief).unwrap();
        l.question_brief_handed_over(next_brief).unwrap();
        l.confirm_delivery(next_brief, Some(&json!({"test":"received"})))
            .unwrap();
    }
    let response = dispatch(
        &ctx,
        &caller,
        recognize(&request.method, &request.path).unwrap(),
        request,
    )
    .await
    .unwrap();
    let Content::Json(asked) = response.content else {
        panic!()
    };
    assert!(
        asked["message"]["task"].is_null(),
        "the response reports the stored idle binding"
    );
    let q = asked["message"]["id"].as_i64().unwrap();
    let mut l = ctx.ledger.borrow_mut();
    let answer = l
        .answer(q, chief, Some(&json!("Idle full answer")), None)
        .unwrap();
    l.cancel_task(project, next.task.number, "human").unwrap();
    let state = l.message(answer.id).unwrap().unwrap().state;
    eprintln!("idle request before send: HTTP {}, question.task={}, answer.task={:?}, answer after cancelling T-{}={state}",response.status,asked["message"]["task"],answer.task_number,next.task.number);
    assert_eq!((answer.task_number, state.as_str()), (None, "queued"));
}

#[tokio::test]
async fn t1823_finding_01_pi_ask_authenticated_before_take_does_not_get_a_new_tasks_answer() {
    let mut w = world();
    let (previous, session, first) = assignment(&mut w, false);
    w.deliver(first);
    w.ledger.record_result(w.project, previous, "Done").unwrap();
    let (project, window, chief) = (w.project, w.id(&session), w.id("chief"));
    let next = w.give_to(&session, &[], "Different Pi task").unwrap();
    let brief = next.message.unwrap().id;
    w.begin(brief);
    let (ctx, _dir) = into_context(w);
    let member = issued(&ctx, project, window);
    let request = req(
        "/api/questions",
        &member,
        json!({"body":"Prior turn question still arriving"}),
    );
    let caller = caller_of(&ctx, &request).unwrap();
    assert_eq!(caller.question_holding.unwrap().task_number, previous);
    ctx.ledger
        .borrow_mut()
        .question_brief_handed_over(brief)
        .unwrap();
    // The request was authenticated before the new send.
    let result = dispatch(
        &ctx,
        &caller,
        recognize(&request.method, &request.path).unwrap(),
        request,
    )
    .await;
    let response = result.unwrap_or_else(|failure| failure.answer());
    let Content::Json(body) = response.content else {
        panic!()
    };
    if response.status != 201 {
        eprintln!("before-take Pi ask refused {}: {body}", response.status);
        return;
    }
    let q = body["message"]["id"].as_i64().unwrap();
    let mut l = ctx.ledger.borrow_mut();
    l.confirm_delivery(brief, Some(&json!({"test":"new brief received"})))
        .unwrap();
    let reply = l.answer(
        q,
        chief,
        Some(&json!("Reply to the previous Pi turn")),
        None,
    );
    let Ok(answer) = reply else {
        return;
    };
    let delivery = l.begin_delivery(answer.id).unwrap();
    assert_eq!(
        l.admit(delivery.message.id, &[]).unwrap(),
        HandOver::Admitted
    );
    l.confirm_delivery(answer.id, Some(&json!({"test":"new task received"})))
        .unwrap();
    eprintln!("Pi ask before rename: HTTP {}, question.task={}, answer.task={:?}, delivered={} while holding T-{}",response.status,body["message"]["task"],answer.task_number,l.message(answer.id).unwrap().unwrap().state,l.question_holding(window).unwrap().unwrap().task_number);
    assert_ne!(
        answer.task_number,
        Some(next.task.number),
        "request from the prior holding must never yield a reply for the new task"
    );
}

#[tokio::test]
async fn t1823_finding_02_pi_inbox_and_thread_reads_after_take_withhold_previous_answer() {
    let mut w = world();
    let (previous, session, first) = assignment(&mut w, false);
    w.deliver(first);
    let q = w.ask(&session, previous, "Old Pi choice").id;
    w.deliver(q);
    let words = "Full words for the prior Pi holding";
    let a = w.answer(q, words).id;
    w.ledger.record_result(w.project, previous, "Done").unwrap();
    let (p, window, chief) = (w.project, w.id(&session), w.id("chief"));
    let next = w.give_to(&session, &[], "Different Pi task").unwrap();
    let brief = next.message.unwrap().id;
    w.begin(brief);
    w.ledger.question_brief_handed_over(brief).unwrap();
    let (ctx, _dir) = into_context(w);
    let member = issued(&ctx, p, window);
    let (preview_code, preview) = got(&ctx, "/api/inbox", &member).await;
    let (inbox_code, inbox) = got(&ctx, &format!("/api/inbox/{a}"), &member).await;
    let (thread_code, thread) = got(&ctx, &format!("/api/tasks/{previous}"), &member).await;
    let first_line = preview.to_string().contains(words);
    let full = inbox.to_string().contains(words);
    let thread_words = thread.to_string().contains(words);
    if full {
        let (ack_status, _) = post(
            &ctx,
            req(
                "/api/answers/read",
                &member,
                json!({"answers":[a],"via":"inbox"}),
            ),
        )
        .await;
        assert_eq!(ack_status, 200);
    }
    let mut l = ctx.ledger.borrow_mut();
    l.confirm_delivery(brief, Some(&json!({"receipt":"next"})))
        .unwrap();
    let count = l
        .inbox(chief, 100)
        .unwrap()
        .iter()
        .filter(|m| m.kind == "note" && m.body.contains(words))
        .count();
    eprintln!("after Pi rename: inbox preview {preview_code} exposed={first_line}, inbox read {inbox_code} exposed={full}, task get {thread_code} exposed={thread_words}, final answer {}, full-word notes {count}",l.message(a).unwrap().unwrap().state);
    assert!(!first_line && !full && !thread_words && count == 1);
}

#[test]
fn t1823_finding_03_unknown_admission_closes_previous_answers_once() {
    let mut w = world();
    let (previous, session, old) = assignment(&mut w, false);
    w.deliver(old);
    let q = w.ask(&session, previous, "Previous question").id;
    let words = "Prior answer with unknown admission";
    let a = w.answer(q, words).id;
    w.ledger
        .record_result(w.project, previous, "Finished")
        .unwrap();
    let next = w.give_to(&session, &[], "Next task").unwrap();
    let brief = next.message.unwrap().id;
    w.begin(brief);
    w.ledger.question_brief_handed_over(brief).unwrap();
    let during = w.ask(&session, next.task.number, "During send").id;
    let pending = w.answer(during, "Answer during send").id;
    w.ledger.question_brief_uncertain(brief).unwrap();
    assert_eq!(w.message(a).state, "cancelled");
    assert_eq!(notices(&w, words), 1);
    assert_eq!(
        w.ledger
            .question_holding(w.id(&session))
            .unwrap()
            .unwrap()
            .task_number,
        next.task.number
    );
    w.ledger
        .retry_delivery(brief, "Admission is unknown", false)
        .unwrap();
    assert_eq!(
        w.ledger
            .question_holding(w.id(&session))
            .unwrap()
            .unwrap()
            .task_number,
        next.task.number
    );
    w.ledger.question_brief_refused(brief).unwrap();
    assert_eq!(w.message(pending).task_number, Some(previous));
    assert_eq!(notices(&w, words), 1);
}

#[tokio::test]
async fn t1823_finding_08_accepted_window_refusal_names_how_it_can_ask_again() {
    let mut w = world();
    let (t, s, b) = assignment(&mut w, false);
    w.deliver(b);
    let (p, window) = (w.project, w.id(&s));
    let (c, _dir) = into_context(w);
    let member = issued(&c, p, window);
    {
        let mut l = c.ledger.borrow_mut();
        l.record_result(p, t, "Result").unwrap();
        l.accept_task(p, t, "human").unwrap();
    }
    let (code, body) = post(
        &c,
        req(
            "/api/questions",
            &member,
            json!({"body":"Unrelated question from a fresh idle turn"}),
        ),
    )
    .await;
    eprintln!("same live pane after acceptance and no next brief: HTTP {code}, {body}");
    assert_eq!(code, 409);
    let words = body["message"].as_str().unwrap_or_default();
    assert!(
        words.contains("new brief") || words.contains("another brief"),
        "{words}"
    );
}

#[test]
fn t1823_finding_11_declined_done_answer_advice_agrees_with_the_actual_operation() {
    let mut w = gated_world();
    let (t, s, b) = assignment(&mut w, true);
    w.deliver(b);
    let q = w.ask(&s, t, "Which output?").id;
    w.ledger.approve_message(q, "human").unwrap();
    w.deliver(q);
    let a = w.answer(q, "First reply").id;
    w.ledger.record_result(w.project, t, "Result").unwrap();
    w.ledger.decline_message(a, "human").unwrap();
    let note = w
        .ledger
        .inbox(w.id("chief"), 100)
        .unwrap()
        .into_iter()
        .find(|m| m.kind == "note" && m.body.contains("declined your answer"))
        .unwrap()
        .body;
    let reply = w
        .ledger
        .answer(
            q,
            w.id("chief"),
            Some(&json!("Replacement in the held task")),
            None,
        )
        .unwrap();
    eprintln!(
        "decline after done: notice={note:?}, cf answer without reopening returned {}",
        reply.state
    );
    assert!(!note.contains("reopen it in the same window before answering"));
}

fn from_alpha10(name: &str) -> World {
    use std::io::Read;
    let fixture: &[u8] = match name {
        "backup" => include_bytes!("goldens/t1813-alpha10/backup.db.gz"),
        "reassign" => include_bytes!("goldens/t1813-alpha10/reassign.db.gz"),
        "working-suspended" => include_bytes!("goldens/t1813-alpha10/working-suspended.db.gz"),
        _ => panic!("unknown historical fixture"),
    };
    let mut bytes = Vec::new();
    flate2::read::GzDecoder::new(fixture)
        .read_to_end(&mut bytes)
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("consensflow.db"), bytes).unwrap();
    World::left_in(dir, 1)
}

#[test]
fn t1823_finding_09_unbound_legacy_taskless_answers_do_not_acquire_a_new_dispatch_route() {
    let mut admitted = Vec::new();
    for name in ["backup", "reassign"] {
        let mut w = from_alpha10(name);
        let (a, recipient): (i64, i64) = w
            .ledger
            .sqlite()
            .query_row(
                "SELECT id,recipient_id FROM message WHERE kind='answer' AND task_id IS NULL",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        w.ledger.begin_question_engine();
        w.ledger.close_questions_of_ended_engine().unwrap();
        let next = w.ledger.next_delivery_of(recipient, a).unwrap();
        if next.is_some() {
            w.begin(a);
            let result = w.ledger.admit(a, &[]).unwrap();
            if result == HandOver::Admitted {
                w.confirm(a);
            }
            eprintln!("upgrade {name}: pre-tracking taskless m-{a}, next eligible, admission={result:?}, final state {}",w.message(a).state);
            admitted.push(name);
        }
    }
    assert!(
        admitted.is_empty(),
        "the upgrade should preserve the old undeliverable answers"
    );
}

#[test]
fn t1823_finding_04_after_final_send_failure_the_prior_holding_can_still_answer() {
    let mut w = world();
    let (prev, session, first) = assignment(&mut w, false);
    w.deliver(first);
    let prior = w.ask_with_options(&session, prev).id;
    let old_answer = w.choose(prior, "red").id;
    w.ledger.record_result(w.project, prev, "Done").unwrap();
    let next = w.give_to(&session, &[], "Different task").unwrap();
    let brief = next.message.unwrap().id;
    for _ in 0..2 {
        w.begin(brief);
        w.ledger.question_brief_handed_over(brief).unwrap();
        w.ledger.retry_delivery(brief, "No receipt", false).unwrap();
    }
    w.begin(brief);
    w.ledger.question_brief_handed_over(brief).unwrap();
    let q = w
        .ask(&session, next.task.number, "Question during final send")
        .id;
    let words = "Answer during failed delivery";
    let a = w.answer(q, words).id;
    w.ledger
        .give_up_delivery(brief, "No receipt on the last attempt")
        .unwrap();
    let held = w.ledger.question_holding(w.id(&session)).unwrap().unwrap();
    let door = w.ledger.claim_answer(prior, w.id(&session)).unwrap();
    let available = matches!(door, Claim::Answered(_));
    eprintln!("last send fails: attempts {}, T-{} {}, previous T-{} {}; current holding T-{} active={}; older answer={} older door={door:?}; new answer={} T-{:?}, chief notes={}",w.message(brief).attempts,next.task.number,w.state(next.task.number),prev,w.state(prev),held.task_number,held.active,w.message(old_answer).state,w.message(a).state,w.message(a).task_number,notices(&w,words));
    assert!(
        available
            && held.active
            && held.task_number == prev
            && w.message(a).task_number == Some(prev)
    );
}

#[test]
fn t1823_finding_06_window_ends_on_last_answer_attempt_returns_words_to_chief() {
    let c = cf_engine::testing::Context::new();
    let p = c.with_tiers(&["zeus", "diana"]).id;
    let t = c
        .create_task(
            p,
            cf_ledger::NewTask {
                from: "chief".into(),
                pool: Some("worker".into()),
                tier: Some("standard".into()),
                body: "Test it".into(),
                ..Default::default()
            },
        )
        .task
        .number;
    c.pass().unwrap();
    c.pass().unwrap();
    let session = c.task(p, t).task.assignee.unwrap();
    let q = c.ask(p, &session, "chief", t, "Which output?");
    c.adapter.answer("zeus", "I asked.");
    c.pass().unwrap();
    let words = "All words from the undelivered last answer";
    let a = c.answer(p, q.id, words);
    for _ in 0..2 {
        let mut l = c.ledger.borrow_mut();
        l.begin_delivery(a.id).unwrap();
        l.retry_delivery(a.id, "No receipt", false).unwrap();
    }
    c.adapter.with("zeus", |agent| agent.arrive = false);
    c.pass().unwrap();
    let before = c.message(a.id);
    assert_eq!(before.state, "delivering");
    assert_eq!(before.attempts, 3);
    c.exit("zeus");
    let notes = c.inbox(c.id(p, "chief"));
    let count = notes
        .iter()
        .filter(|m| m.kind == "note" && m.body.contains(words))
        .count();
    let why = notes
        .iter()
        .filter(|m| m.kind == "note")
        .map(|m| m.body.clone())
        .collect::<Vec<_>>();
    eprintln!("physical exit at answer attempt {}: answer={}, chief full-word notes={count}, chief notes={why:?}",before.attempts,c.message(a.id).state);
    assert_eq!(count, 1);
}

#[tokio::test]
async fn t1823_finding_07_project_resume_after_real_alpha10_upgrade_binds_new_questions() {
    let w = from_alpha10("working-suspended");
    let p = w.project;
    let session = w.ledger.task(p, 1).unwrap().unwrap().task.assignee.unwrap();
    let window = w.id(&session);
    let c = cf_engine::testing::Context::new();
    c.adapter.resumes_records.set(true);
    *c.ledger.borrow_mut() = w.ledger;
    let old = c
        .ledger
        .borrow()
        .current_conversation(window)
        .unwrap()
        .unwrap();
    let restarted = c.make();
    restarted.settle_after_restart().unwrap();
    restarted.resume_project(p).unwrap();
    c.settle();
    let opened = c
        .host
        .last(&session)
        .expect("project Resume opened the worker pane");
    let resumed = c
        .ledger
        .borrow()
        .current_conversation(window)
        .unwrap()
        .unwrap();
    assert_eq!(old.native_session, resumed.native_session);
    assert_eq!(old.id, resumed.id);
    let start = c
        .credentials
        .question_starts
        .borrow()
        .iter()
        .rev()
        .find(|(id, _)| *id == window)
        .unwrap()
        .1;
    let mut dc = into_context(world()).0;
    dc.ledger = Rc::clone(&c.ledger);
    let member = dc.credentials.issue_for_window(p, window, start).unwrap();
    let (status, body) = post(
        &dc,
        req(
            "/api/questions",
            &member,
            json!({"body":"New question in the resumed conversation"}),
        ),
    )
    .await;
    assert_eq!(status, 201);
    let q = body["message"]["id"].as_i64().unwrap();
    let chief = c.id(p, "chief");
    let mut l = dc.ledger.borrow_mut();
    let a = l
        .answer(
            q,
            chief,
            Some(&json!("Entirely new answer since upgrade")),
            None,
        )
        .unwrap();
    l.cancel_task(p, 1, "human").unwrap();
    let notes = l
        .inbox(chief, 100)
        .unwrap()
        .iter()
        .filter(|m| m.kind == "note" && m.body.contains("Entirely new answer since upgrade"))
        .count();
    let adm = if l.message(a.id).unwrap().unwrap().state == "queued" {
        l.begin_delivery(a.id).unwrap();
        Some(l.admit(a.id, &[]).unwrap())
    } else {
        None
    };
    eprintln!("real upgraded project Resume opened {:?} on same conversation {:?}; start {start:?}; new question task={} answer task={:?}; after cancellation admission {adm:?}, full-word chief notes {notes}",opened.pane,old.native_session,body["message"]["task"],a.task_number);
    assert_eq!(body["message"]["task"], 1);
    assert_eq!(notes, 1);
}

#[cfg(unix)]
#[test]
fn t1823_finding_10_unreadable_pi_ack_does_not_refuse_a_valid_answer_at_another_project() {
    use std::os::unix::fs::PermissionsExt;
    let mut w = world();
    let (previous, s, first) = assignment(&mut w, false);
    w.deliver(first);
    w.ledger.record_result(w.project, previous, "Done").unwrap();
    let brief = w
        .give_to(&s, &[], "New Pi task")
        .unwrap()
        .message
        .unwrap()
        .id;
    w.begin(brief);
    let mut other = cf_ledger::NewProject {
        directory: "/work/B".into(),
        name: "B".into(),
        chief: cf_ledger::NewChief {
            harness: "claude-code".into(),
            model: None,
            effort: None,
            model_name: None,
        },
        staff: Vec::new(),
        gate: false,
        wake_up: None,
        specs: None,
        staff_rules: None,
        compaction: None,
        advisor_tool: false,
        prompt_suggestion: false,
        own_hands: false,
    };
    other.staff.push(cf_ledger::NewMember {
        harness: "claude-code".into(),
        model: None,
        effort: None,
        model_name: Some("hera".into()),
        designer: false,
        roles: vec!["worker".into()],
        tier: "standard".into(),
    });
    let second = w.ledger.create_project(&other).unwrap();
    let second_chief = second
        .participants
        .iter()
        .find(|p| p.role == "chief")
        .unwrap()
        .id;
    let second_worker = second
        .participants
        .iter()
        .find(|p| p.role == "worker")
        .unwrap()
        .handle
        .clone();
    let q = w
        .ledger
        .ask(
            second.id,
            &cf_ledger::NewQuestion {
                from: Some(second_worker),
                to: "chief".into(),
                body: Some("Unrelated live question".into()),
                ..Default::default()
            },
        )
        .unwrap();
    w.ledger.question_brief_handed_over(brief).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let ack = dir.path().join("inaccessible-ack");
    std::fs::write(&ack, br#"{"id":"m-8","admitted":true}"#).unwrap();
    std::fs::set_permissions(&ack, std::fs::Permissions::from_mode(0o000)).unwrap();
    let reply = w.ledger.answer(
        q.id,
        second_chief,
        Some(&json!("Other project's reply")),
        None,
    );
    let closing = w.ledger.close_question_window(w.id(&s), None);
    std::fs::set_permissions(&ack, std::fs::Permissions::from_mode(0o600)).unwrap();
    eprintln!(
        "Pi ack read I/O failure in project A, valid answer in project B: {:?}, A's close: {:?}",
        reply.as_ref().map(|r| &r.state).map_err(|e| e.code()),
        closing.as_ref().map(|_| ()).map_err(|e| e.code())
    );
    assert!(reply.is_ok() && closing.is_ok());
}

#[tokio::test]
async fn t1823_finding_01_request_received_during_a_refused_brief_returns_to_the_previous_holding()
{
    let mut w = world();
    let (old, session, b) = assignment(&mut w, false);
    w.deliver(b);
    w.ledger.record_result(w.project, old, "Result").unwrap();
    let next = w.give_to(&session, &[], "Distinct next brief").unwrap();
    let brief = next.message.unwrap().id;
    let (p, worker, chief) = (w.project, w.id(&session), w.id("chief"));
    let (c, _dir) = into_context(w);
    let auth = issued(&c, p, worker);
    c.ledger.borrow_mut().begin_delivery(brief).unwrap();
    c.ledger
        .borrow_mut()
        .question_brief_handed_over(brief)
        .unwrap();
    let r = req(
        "/api/questions",
        &auth,
        json!({"body":"Question during the attempted send"}),
    );
    let caller = caller_of(&c, &r).unwrap();
    assert_eq!(
        caller.question_holding.unwrap().task_number,
        next.task.number
    );
    c.ledger.borrow_mut().question_brief_refused(brief).unwrap();
    let reply = dispatch(&c, &caller, recognize(&r.method, &r.path).unwrap(), r)
        .await
        .unwrap_or_else(|failure| failure.answer());
    let Content::Json(body) = reply.content else {
        panic!()
    };
    let held = c.ledger.borrow().question_holding(worker).unwrap().unwrap();
    assert!(held.active && held.task_number == old);
    eprintln!("ask reached daemon during send, refusal before body finished: HTTP {} {body}, current holding T-{old}",reply.status);
    assert_eq!(reply.status, 201);
    let a = c
        .ledger
        .borrow_mut()
        .answer(
            body["message"]["id"].as_i64().unwrap(),
            chief,
            Some(&json!("Reply for prior holding")),
            None,
        )
        .unwrap();
    assert_eq!(a.task_number, Some(old));
}

#[test]
fn t1823_finding_11_cancelled_question_gives_the_chief_a_step_to_take() {
    let mut w = world();
    let (t, s, b) = assignment(&mut w, false);
    w.deliver(b);
    let q = w.ask(&s, t, "Question").id;
    w.deliver(q);
    w.ledger.cancel_task(w.project, t, "human").unwrap();
    let chief = w.id("chief");
    let refusal = w
        .ledger
        .answer(q, chief, Some(&json!("Still relevant")), None)
        .unwrap_err()
        .to_string();
    eprintln!("ordinary cancelled task refusal, complete words: {refusal}");
    assert!(refusal.to_lowercase().contains("new task") || refusal.contains("cf "));
}

#[test]
fn t1823_finding_11_project_resume_refusal_says_the_conversation_the_new_window_actually_opens() {
    let c = cf_engine::testing::Context::new();
    c.adapter.resumes_records.set(true);
    let p = c.with_tiers(&["zeus"]).id;
    let t = c
        .create_task(
            p,
            cf_ledger::NewTask {
                from: "chief".into(),
                pool: Some("worker".into()),
                tier: Some("standard".into()),
                body: "Task".into(),
                ..Default::default()
            },
        )
        .task
        .number;
    c.pass().unwrap();
    c.pass().unwrap();
    let s = c.task(p, t).task.assignee.unwrap();
    let w = c.id(p, &s);
    let old = c.ledger.borrow().current_conversation(w).unwrap().unwrap();
    let old_pane = c.host.last(&s).unwrap().pane;
    let q = c.ask(p, &s, "chief", t, "Question before restart");
    c.ledger.borrow_mut().suspend_for_restart().unwrap();
    c.advance(1000);
    let d = c.make();
    d.settle_after_restart().unwrap();
    d.resume_project(p).unwrap();
    c.settle();
    let new = c.ledger.borrow().current_conversation(w).unwrap().unwrap();
    assert_eq!(old.native_session, new.native_session);
    assert!(new.native_session.is_some());
    assert_eq!(old.id, new.id);
    assert_ne!(old_pane, c.host.last(&s).unwrap().pane);
    let chief = c.id(p, "chief");
    let words = c
        .ledger
        .borrow_mut()
        .answer(q.id, chief, Some(&json!("Old answer")), None)
        .unwrap_err()
        .to_string();
    eprintln!("same native conversation {:?} reopened in a new physical pane, refusal of old question: {words}",new.native_session);
    assert!(!words.contains("starts over without the old conversation"));
}

#[test]
fn t1823_finding_09_new_dispatcher_does_not_open_a_window_for_an_unbound_legacy_answer() {
    let mut delivered = Vec::new();
    for name in ["backup", "reassign"] {
        let w = from_alpha10(name);
        let p = w.project;
        let (a,session):(i64,String)=w.ledger.sqlite().query_row("SELECT m.id,p.handle FROM message m JOIN participant p ON p.id=m.recipient_id WHERE m.kind='answer' AND m.task_id IS NULL",[],|r|Ok((r.get(0)?,r.get(1)?))).unwrap();
        let c = cf_engine::testing::Context::new();
        *c.ledger.borrow_mut() = w.ledger;
        let d = c.make();
        d.settle_after_restart().unwrap();
        for _ in 0..4 {
            c.pass().unwrap();
        }
        let state = c.message(a).state;
        let pane = c.host.last(&session).map(|p| p.pane);
        eprintln!("upgraded dispatcher after {name}, former {session}, pane={pane:?}, unbound m-{a} state={state}");
        if state == "delivered" || state == "read" {
            delivered.push((p, name));
        }
    }
    assert!(
        delivered.is_empty(),
        "words entered newly opened windows: {delivered:?}"
    );
}

#[test]
fn t1823_finding_05_a_provisional_native_door_does_not_give_words_to_a_window_whose_send_fails() {
    let mut w = world();
    let (old, s, b) = assignment(&mut w, false);
    w.deliver(b);
    w.ledger.record_result(w.project, old, "Result").unwrap();
    w.ledger.accept_task(w.project, old, "human").unwrap();
    let to = w.give_to(&s, &[], "T-2").unwrap();
    let brief = to.message.unwrap().id;
    w.begin(brief);
    w.ledger.question_brief_handed_over(brief).unwrap();
    let q = w.ask_with_options(&s, to.task.number).id;
    let a = w.choose(q, "red");
    let worker = w.id(&s);
    let claim = w.ledger.claim_answer(q, worker).unwrap();
    let was_given = matches!(&claim, Claim::Answered(_));
    if was_given {
        w.ledger.settle_claim(a.id, worker, true).unwrap();
    }
    w.ledger.question_brief_refused(brief).unwrap();
    let full = notices(&w, &a.body);
    eprintln!("native door on a subsequently refused send, previous T-{old} accepted: exposed={was_given}, final answer={}, full-word notes={full}",w.message(a.id).state);
    assert!(!was_given && full == 1);
}

#[test]
fn t1823_finding_11_reply_during_a_provisional_send_does_not_call_a_live_question_permanently_moved(
) {
    let mut w = world();
    let (old, s, b) = assignment(&mut w, false);
    w.deliver(b);
    let q = w.ask(&s, old, "Which report?").id;
    w.deliver(q);
    w.ledger.record_result(w.project, old, "Result").unwrap();
    let to = w.give_to(&s, &[], "Unconfirmed T-2").unwrap();
    let brief = to.message.unwrap().id;
    w.begin(brief);
    w.ledger.question_brief_handed_over(brief).unwrap();
    let chief = w.id("chief");
    let words = json!("Reply to previous holding");
    let provisional = w.ledger.answer(q, chief, Some(&words), None);
    w.ledger.question_brief_refused(brief).unwrap();
    let retried = w.ledger.answer(q, chief, Some(&words), None);
    let first = provisional.as_ref().err().map(ToString::to_string);
    eprintln!("answer during pending T-2: {first:?}; after definitive refusal original T-{old} answer succeeds={}",retried.is_ok());
    assert!(
        provisional.is_ok() || first.is_some_and(|s| s.contains("wait") || s.contains("try again"))
    );
}

#[tokio::test]
async fn t1823_finding_01_refused_send_request_never_follows_a_later_third_task() {
    let mut w = world();
    let (old, session, first) = assignment(&mut w, false);
    w.deliver(first);
    w.ledger.record_result(w.project, old, "Done").unwrap();
    let second = w.give_to(&session, &[], "Second task").unwrap();
    let second_brief = second.message.unwrap().id;
    let (project, window) = (w.project, w.id(&session));
    let (c, _dir) = into_context(w);
    let auth = issued(&c, project, window);
    c.ledger.borrow_mut().begin_delivery(second_brief).unwrap();
    c.ledger
        .borrow_mut()
        .question_brief_handed_over(second_brief)
        .unwrap();
    let r = req(
        "/api/questions",
        &auth,
        json!({"body":"Asked during second task"}),
    );
    let caller = caller_of(&c, &r).unwrap();
    c.ledger
        .borrow_mut()
        .question_brief_refused(second_brief)
        .unwrap();
    c.ledger
        .borrow_mut()
        .cancel_task(project, second.task.number, "human")
        .unwrap();
    let third_task = c
        .ledger
        .borrow_mut()
        .create_task(
            project,
            &cf_ledger::NewTask {
                from: "chief".into(),
                to: Some(session.clone()),
                body: "Third task".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let third = third_task.task.number;
    let third_brief = third_task.message.unwrap().id;
    c.ledger.borrow_mut().begin_delivery(third_brief).unwrap();
    c.ledger
        .borrow_mut()
        .question_brief_handed_over(third_brief)
        .unwrap();
    let reply = dispatch(&c, &caller, recognize(&r.method, &r.path).unwrap(), r)
        .await
        .unwrap_or_else(|failure| failure.answer());
    assert_eq!(
        reply.status, 409,
        "a request from the refused second send cannot bind to T-{third}"
    );
}

#[test]
fn t1823_finding_09_already_delivered_legacy_answer_remains_visible() {
    let mut w = from_alpha10("backup");
    let (answer, window): (i64, i64) = w
        .ledger
        .sqlite()
        .query_row(
            "SELECT id, recipient_id FROM message WHERE kind = 'answer' AND task_id IS NOT NULL",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    w.edit(|db| {
        db.execute(
            "UPDATE message SET state = 'delivered' WHERE id = ?",
            [answer],
        )
        .unwrap();
    });
    let hidden = w
        .ledger
        .returned_answers_hidden_from(w.project, window)
        .unwrap();
    assert!(
        !hidden.contains(&answer),
        "limit 16 preserves an answer already delivered before tracking"
    );
}

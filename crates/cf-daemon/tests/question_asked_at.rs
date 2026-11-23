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
use cf_ledger::HandOver;
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

// ---- Q1: a question belongs to the holding of the moment it was asked (N2) ----

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

#[tokio::test]
async fn q1_a_taskless_turns_late_question_stays_the_sessions_and_a_fresh_one_is_the_tasks() {
    let mut w = world();
    let (task, session, brief) = assignment(&mut w, false);
    let (p, window) = (w.project, w.id(&session));
    let (c, _dir) = into_context(w);
    let auth = issued(&c, p, window);
    let before = latest(&c);
    take(&c, brief);
    let began = handed_at(&c, brief);
    assert!(before < began);
    let (code, late) = post(&c, asked(&auth, "From the turn before", Some(before + 1))).await;
    assert_eq!((code, stored(&late)), (201, None), "{late}");
    let (code, fresh) = post(&c, asked(&auth, "From the turn now", Some(began + 1))).await;
    assert_eq!((code, stored(&fresh)), (201, Some(task)), "{fresh}");
}

#[tokio::test]
async fn q1_a_stamp_at_the_begin_is_the_new_holdings_and_one_from_the_future_is_the_arrival() {
    let mut w = world();
    let (task, session, brief) = assignment(&mut w, false);
    let (p, window) = (w.project, w.id(&session));
    let (c, _dir) = into_context(w);
    let auth = issued(&c, p, window);
    take(&c, brief);
    let began = handed_at(&c, brief);
    let (code, before) = post(&c, asked(&auth, "Just before", Some(began - 1))).await;
    assert_eq!((code, stored(&before)), (201, None), "{before}");
    let (code, on) = post(&c, asked(&auth, "On the begin", Some(began))).await;
    assert_eq!((code, stored(&on)), (201, Some(task)), "{on}");
    let (code, future) = post(
        &c,
        asked(&auth, "From the future", Some(latest(&c) + 86_400_000)),
    )
    .await;
    assert_eq!((code, stored(&future)), (201, Some(task)), "{future}");
}

#[tokio::test]
async fn q1_an_unstamped_question_is_placed_as_before() {
    let mut w = world();
    let (task, session, brief) = assignment(&mut w, false);
    let (p, window) = (w.project, w.id(&session));
    let (c, _dir) = into_context(w);
    let auth = issued(&c, p, window);
    take(&c, brief);
    let (code, plain) = post(&c, asked(&auth, "From an older program", None)).await;
    assert_eq!((code, stored(&plain)), (201, Some(task)), "{plain}");
}

#[tokio::test]
async fn q1_the_earlier_tasks_question_in_the_gap_and_after_the_take_is_refused_where_it_is() {
    for taken in [false, true] {
        let mut w = world();
        let (first, session, brief) = assignment(&mut w, false);
        w.deliver(brief);
        let (p, window) = (w.project, w.id(&session));
        let (c, _dir) = into_context(w);
        let auth = issued(&c, p, window);
        let asking = latest(&c) + 1;
        let next = {
            let mut l = c.ledger.borrow_mut();
            l.record_result(p, first, "The first task ended").unwrap();
            l.accept_task(p, first, "human").unwrap();
            l.create_task(
                p,
                &cf_ledger::NewTask {
                    from: "chief".into(),
                    to: Some(session.clone()),
                    body: "The second task".into(),
                    ..Default::default()
                },
            )
            .unwrap()
        };
        let (next_number, next_brief) = (next.task.number, next.message.unwrap().id);
        if taken {
            take(&c, next_brief);
        } else {
            let mut l = c.ledger.borrow_mut();
            l.begin_delivery(next_brief).unwrap();
            l.question_brief_handed_over(next_brief).unwrap();
        }
        let (code, late) = post(&c, asked(&auth, "From the first task", Some(asking))).await;
        assert_eq!(code, 409, "taken={taken}: {late}");
        assert_ne!(stored(&late), Some(next_number));
        let words = late["message"].as_str().unwrap_or_default();
        assert!(words.contains(&format!("T-{first}")), "{words}");
    }
}

#[tokio::test]
async fn q1_after_an_acceptance_a_new_ask_is_taskless_and_a_late_one_keeps_its_409() {
    let mut w = world();
    let (t, session, brief) = assignment(&mut w, false);
    w.deliver(brief);
    let (p, window) = (w.project, w.id(&session));
    let (c, _dir) = into_context(w);
    let auth = issued(&c, p, window);
    let during = latest(&c) + 1;
    {
        let mut l = c.ledger.borrow_mut();
        l.record_result(p, t, "Result").unwrap();
        l.accept_task(p, t, "human").unwrap();
    }
    let after = latest(&c) + 1;
    let (code, late) = post(&c, asked(&auth, "From the task's turn", Some(during))).await;
    assert_eq!(code, 409, "{late}");
    let (code, fresh) = post(&c, asked(&auth, "From a fresh idle turn", Some(after))).await;
    assert_eq!((code, stored(&fresh)), (201, None), "{fresh}");
}

#[tokio::test]
async fn q1_after_a_cancel_a_new_ask_is_taskless_and_a_late_one_keeps_its_409() {
    let mut w = world();
    let (t, session, brief) = assignment(&mut w, false);
    w.deliver(brief);
    let (p, window) = (w.project, w.id(&session));
    let (c, _dir) = into_context(w);
    let auth = issued(&c, p, window);
    let during = latest(&c) + 1;
    c.ledger.borrow_mut().cancel_task(p, t, "human").unwrap();
    let after = latest(&c) + 1;
    let (code, late) = post(&c, asked(&auth, "From the cancelled turn", Some(during))).await;
    assert_eq!(
        (code, late["error"].as_str()),
        (409, Some("task-cancelled")),
        "{late}"
    );
    let (code, fresh) = post(&c, asked(&auth, "From a fresh idle turn", Some(after))).await;
    assert_eq!((code, stored(&fresh)), (201, None), "{fresh}");
}

#[tokio::test]
async fn q1_a_question_in_a_refused_hand_overs_attempt_goes_back_to_the_holding_it_was_asked_in() {
    let mut w = world();
    let (old, session, b) = assignment(&mut w, false);
    w.deliver(b);
    w.ledger.record_result(w.project, old, "Result").unwrap();
    let next = w.give_to(&session, &[], "Distinct next brief").unwrap();
    let brief = next.message.unwrap().id;
    let (p, worker, chief) = (w.project, w.id(&session), w.id("chief"));
    let (c, _dir) = into_context(w);
    let auth = issued(&c, p, worker);
    let asking = latest(&c) + 1;
    {
        let mut l = c.ledger.borrow_mut();
        l.begin_delivery(brief).unwrap();
        assert_eq!(l.admit(brief, &[]).unwrap(), HandOver::Admitted);
        l.question_brief_handed_over(brief).unwrap();
    }
    let (code, body) = post(
        &c,
        asked(
            &auth,
            "From the earlier turn, during the send",
            Some(asking),
        ),
    )
    .await;
    assert_eq!((code, stored(&body)), (201, Some(old)), "{body}");
    let q = body["message"]["id"].as_i64().unwrap();
    c.ledger.borrow_mut().question_brief_refused(brief).unwrap();
    let held = c.ledger.borrow().question_holding(worker).unwrap().unwrap();
    assert!(held.active && held.task_number == old);
    let a = c
        .ledger
        .borrow_mut()
        .answer(q, chief, Some(&json!("For the first")), None)
        .unwrap();
    assert_eq!(a.task_number, Some(old));
}

#[tokio::test]
async fn q1_a_launch_token_request_before_the_hosts_acknowledgement_is_the_launch_tasks() {
    // The brief is admitted; no holding exists until the request itself makes it.
    for case in ["after", "on", "before", "unstamped"] {
        let mut w = world();
        let (number, session, brief) = assignment(&mut w, false);
        let p = w.project;
        let window = w.id(&session);
        let task = w.ledger.task(p, number).unwrap().unwrap().task.id;
        let (c, _dir) = into_context(w);
        {
            let mut l = c.ledger.borrow_mut();
            l.begin_delivery(brief).unwrap();
            assert_eq!(l.admit(brief, &[]).unwrap(), HandOver::Admitted);
            assert!(l.question_holding(window).unwrap().is_none());
        }
        let admitted = handed_at(&c, brief);
        let start = QuestionWindowStart {
            last_brief: None,
            task_in_hand: Some(task),
            launch_brief: Some(brief),
        };
        let auth = c.credentials.issue_for_window(p, window, start).unwrap();
        let at = match case {
            "after" => Some(admitted + 1),
            "on" => Some(admitted),
            "before" => Some(admitted - 1),
            _ => None,
        };
        let (code, body) = post(&c, asked(&auth, "Launch question", at)).await;
        let want = if case == "before" { None } else { Some(number) };
        assert_eq!((code, stored(&body)), (201, want), "{case}: {body}");
        let h = c.ledger.borrow().question_holding(window).unwrap().unwrap();
        assert_eq!(h.task_number, number, "{case}");
    }
}

#[tokio::test]
async fn q1_a_question_from_two_holdings_back_is_asked_again_and_one_from_one_back_is_refused_where_it_is(
) {
    let mut w = world();
    let (t1, session, b1) = assignment(&mut w, false);
    w.deliver(b1);
    let (p, window) = (w.project, w.id(&session));
    let (c, _dir) = into_context(w);
    let auth = issued(&c, p, window);
    let in_first = latest(&c) + 1;
    let mut second = None;
    let mut in_second = 0;
    for round in 0..2 {
        let mut l = c.ledger.borrow_mut();
        let last = if round == 0 { t1 } else { second.unwrap() };
        l.record_result(p, last, "Result").unwrap();
        l.accept_task(p, last, "human").unwrap();
        let next = l
            .create_task(
                p,
                &cf_ledger::NewTask {
                    from: "chief".into(),
                    to: Some(session.clone()),
                    body: format!("Task {round}"),
                    ..Default::default()
                },
            )
            .unwrap();
        let brief = next.message.unwrap().id;
        drop(l);
        take(&c, brief);
        if round == 0 {
            second = Some(next.task.number);
            in_second = latest(&c) + 1;
        }
    }
    let (code, one_back) = post(&c, asked(&auth, "From the second task", Some(in_second))).await;
    assert_eq!(code, 409, "{one_back}");
    let (code, two_back) = post(&c, asked(&auth, "From the first task", Some(in_first))).await;
    assert_eq!(
        (code, two_back["error"].as_str()),
        (409, Some("question-earlier-turn")),
        "{two_back}"
    );
}

// ---- Round 2 (review T-1928) ----

#[tokio::test]
async fn q1_a_late_positive_report_of_the_brief_leaves_a_cancelled_holding_its_known_bounds() {
    let mut w = world();
    let (task, session, brief) = assignment(&mut w, false);
    let (project, window, chief) = (w.project, w.id(&session), w.id("chief"));
    let (ctx, _dir) = into_context(w);
    let auth = issued(&ctx, project, window);
    let (begin, end) = {
        let mut l = ctx.ledger.borrow_mut();
        l.begin_delivery(brief).unwrap();
        assert_eq!(l.admit(brief, &[]).unwrap(), HandOver::Admitted);
        l.question_brief_handed_over(brief).unwrap();
        let begin = l
            .question_holding(window)
            .unwrap()
            .unwrap()
            .began_at
            .unwrap();
        l.cancel_task(project, task, "human").unwrap();
        let end = l
            .question_holding(window)
            .unwrap()
            .unwrap()
            .ended_at
            .unwrap();
        (begin, end)
    };
    let old = begin + 1;
    assert!(old < end);
    let (code, _) = post(&ctx, asked(&auth, "Before the cancel", Some(old))).await;
    assert_eq!(code, 409);
    ctx.ledger
        .borrow_mut()
        .question_brief_reported(brief)
        .unwrap();
    let after = ctx
        .ledger
        .borrow()
        .question_holding(window)
        .unwrap()
        .unwrap();
    assert_eq!((after.began_at, after.ended_at), (Some(begin), Some(end)));
    let (code, late) = post(&ctx, asked(&auth, "Before the cancel, delayed", Some(old))).await;
    assert_eq!(
        (code, late["error"].as_str()),
        (409, Some("task-cancelled")),
        "{late}"
    );
    // After the cancel the session's own question stays taskless, and so does its answer.
    let (code, fresh) = post(&ctx, asked(&auth, "After the cancel", Some(end))).await;
    assert_eq!((code, stored(&fresh)), (201, None), "{fresh}");
    let q = fresh["message"]["id"].as_i64().unwrap();
    let a = ctx
        .ledger
        .borrow_mut()
        .answer(q, chief, Some(&json!("Fresh")), None)
        .unwrap();
    assert_eq!(a.task_number, None);
}

#[tokio::test]
async fn q1_an_idle_token_asking_while_its_previous_generation_resumes_is_taskless() {
    let mut w = world();
    let (task, session, brief) = assignment(&mut w, false);
    let (p, member) = (w.project, w.id(&session));
    w.deliver(brief);
    w.ledger.close_question_window(member, None).unwrap();
    let (c, _dir) = into_context(w);
    let auth = issued(&c, p, member);
    let r = asked(&auth, "An idle process", Some(latest(&c) + 1));
    let caller = caller_of(&c, &r).unwrap();
    assert!(caller.question_holding.is_none() && caller.question_launch_task.is_none());
    let route = recognize(&r.method, &r.path).unwrap();
    // The previous window's generation resumes while this authenticated body is in flight.
    assert!(c
        .ledger
        .borrow_mut()
        .resume_question_holding(member, task)
        .unwrap());
    let response = dispatch(&c, &caller, route, r)
        .await
        .unwrap_or_else(|e| e.answer());
    let Content::Json(body) = response.content else {
        panic!()
    };
    assert_eq!((response.status, stored(&body)), (201, None), "{body}");
}

#[tokio::test]
async fn q1_a_fresh_idle_window_asks_taskless_whatever_the_closed_holding_before_it() {
    let mut w = world();
    let (task, session, brief) = assignment(&mut w, false);
    w.deliver(brief);
    let (p, window) = (w.project, w.id(&session));
    w.ledger.close_question_window(window, None).unwrap();
    let start = QuestionWindowStart::from_ledger(&w.ledger, window).unwrap();
    assert!(start.task_in_hand.is_none());
    let (c, _dir) = into_context(w);
    let auth = c.credentials.issue_for_window(p, window, start).unwrap();
    let (code, stamped) = post(&c, asked(&auth, "Fresh window", Some(latest(&c) + 1))).await;
    assert_eq!((code, stored(&stamped)), (201, None), "T-{task}: {stamped}");
}

#[tokio::test]
async fn q1_a_migrated_idle_window_asks_taskless_after_its_preupdate_cancel() {
    let mut w = world();
    let (task, session, brief) = assignment(&mut w, false);
    let (p, window) = (w.project, w.id(&session));
    w.deliver(brief);
    w.ledger.cancel_task(p, task, "human").unwrap();
    w.edit(|db| {
        cf_ledger::testing::undo_migration_0044(db).unwrap();
        db.pragma_update(None, "user_version", 43).unwrap();
    });
    let (c, _dir) = into_context(w);
    let auth = issued(&c, p, window);
    let (code, plain) = post(&c, asked(&auth, "No stamp", None)).await;
    assert_eq!((code, stored(&plain)), (201, None), "{plain}");
    let (code, stamped) = post(&c, asked(&auth, "Stamped", Some(latest(&c) + 1))).await;
    assert_eq!((code, stored(&stamped)), (201, None), "{stamped}");
}

#[test]
fn q1_a_failed_end_date_write_fails_the_cancel() {
    let mut w = world();
    let (task, session, brief) = assignment(&mut w, false);
    w.deliver(brief);
    let window = w.id(&session);
    w.ledger
        .sqlite()
        .execute_batch(
            "CREATE TEMP TRIGGER test_end_write BEFORE UPDATE OF ended_at ON question_holding
             WHEN OLD.ended_at=-1 AND NEW.ended_at>0 BEGIN SELECT RAISE(ABORT,'test end write failed'); END;",
        )
        .unwrap();
    let outcome = w.ledger.cancel_task(w.project, task, "human");
    assert!(
        outcome.is_err(),
        "the cancel committed with its end date unwritten"
    );
    // The cancel did not happen, and the holding keeps no pending end.
    assert_ne!(w.state(task), "cancelled");
    let held = w.ledger.question_holding(window).unwrap().unwrap();
    assert_ne!(held.ended_at, Some(-1));
}

/// A window that held no task when it asked files its question with the session, whatever
/// brief reaches the window after the asking (review T-1964, finding 3).
#[tokio::test]
async fn q1_an_idle_question_stays_the_sessions_across_a_follow_up_brief_to_its_window() {
    let mut w = world();
    let (task, session, old) = assignment(&mut w, false);
    w.deliver(old);
    let (p, member) = (w.project, w.id(&session));
    w.ledger.close_question_window(member, None).unwrap();
    let (c, _dir) = into_context(w);
    let auth = issued(&c, p, member);
    let asked_at = latest(&c) + 1;
    let r = asked(
        &auth,
        "Before any brief in this idle window",
        Some(asked_at),
    );
    let caller = caller_of(&c, &r).unwrap();
    let route = recognize(&r.method, &r.path).unwrap();
    let next = {
        let mut l = c.ledger.borrow_mut();
        assert!(l.resume_question_holding(member, task).unwrap());
        l.record_result(p, task, "First result").unwrap();
        l.reopen_task(p, task, "chief", "Continue with a follow-up")
            .unwrap()
            .message
            .unwrap()
            .id
    };
    take(&c, next);
    let new_stamp = latest(&c) + 1;
    let response = dispatch(&c, &caller, route, r)
        .await
        .unwrap_or_else(|e| e.answer());
    let Content::Json(body) = response.content else {
        panic!()
    };
    // Authenticated after the follow-up, with the same old stamp.
    let late_auth = post(
        &c,
        asked(&auth, "Same old instant, late arrival", Some(asked_at)),
    )
    .await;
    // Asked in the follow-up's turn: the task's.
    let fresh = post(
        &c,
        asked(&auth, "After the follow-up brief", Some(new_stamp)),
    )
    .await;
    assert_eq!((response.status, stored(&body)), (201, None), "{body}");
    assert_eq!((late_auth.0, stored(&late_auth.1)), (201, None));
    assert_eq!((fresh.0, stored(&fresh.1)), (201, Some(task)));
}

#[tokio::test]
async fn q1_an_idle_question_stays_the_sessions_when_the_next_tasks_brief_arrives_first() {
    let mut w = world();
    let (task, session, old) = assignment(&mut w, false);
    let (p, member) = (w.project, w.id(&session));
    w.deliver(old);
    w.ledger.cancel_task(p, task, "human").unwrap();
    w.edit(|db| {
        cf_ledger::testing::undo_migration_0044(db).unwrap();
        db.pragma_update(None, "user_version", 43).unwrap();
    });
    let (c, _dir) = into_context(w);
    let auth = issued(&c, p, member);
    let asked_at = latest(&c) + 1;
    let r = asked(&auth, "Idle before a new first brief", Some(asked_at));
    let caller = caller_of(&c, &r).unwrap();
    let route = recognize(&r.method, &r.path).unwrap();
    let next = c
        .ledger
        .borrow_mut()
        .create_task(
            p,
            &cf_ledger::NewTask {
                from: "chief".into(),
                to: Some(session.clone()),
                body: "The window's next task".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let brief = next.message.unwrap().id;
    take(&c, brief);
    let response = dispatch(&c, &caller, route, r)
        .await
        .unwrap_or_else(|e| e.answer());
    let Content::Json(body) = response.content else {
        panic!()
    };
    let fresh = post(
        &c,
        asked(&auth, "The new task's question", Some(handed_at(&c, brief))),
    )
    .await;
    assert_eq!((response.status, stored(&body)), (201, None), "{body}");
    assert_eq!((fresh.0, stored(&fresh.1)), (201, Some(next.task.number)));
}

/// A fresh window with no task and no brief, beside a holding a Resume made with no brief
/// (its last brief is a negative anchor): its stamped question is the session's, in every
/// state of that holding (review T-1991).
#[tokio::test]
async fn q1_a_fresh_idle_window_beside_a_holding_a_resume_made_asks_taskless() {
    for case in [
        "window ended",
        "cancelled, end not dated",
        "cancelled, end dated",
    ] {
        let mut w = world();
        let (task, session, brief) = assignment(&mut w, false);
        w.deliver(brief);
        let (p, member) = (w.project, w.id(&session));
        w.edit(|db| {
            db.execute("DELETE FROM question_handover", []).unwrap();
            db.execute("DELETE FROM question_holding", []).unwrap();
        });
        assert!(w.ledger.resume_question_holding(member, task).unwrap());
        let made = w.ledger.question_holding(member).unwrap().unwrap();
        assert!(made.last_brief < 0, "{made:?}");
        match case {
            "window ended" => {
                w.ledger.close_question_window(member, None).unwrap();
            }
            "cancelled, end not dated" => {
                w.ledger.cancel_task(p, task, "human").unwrap();
                w.edit(|db| {
                    db.execute("UPDATE question_holding SET ended_at=NULL", [])
                        .unwrap();
                });
            }
            _ => {
                w.ledger.cancel_task(p, task, "human").unwrap();
            }
        }
        let (c, _dir) = into_context(w);
        let auth = issued(&c, p, member);
        let (code, plain) = post(&c, asked(&auth, "Unstamped", None)).await;
        assert_eq!((code, stored(&plain)), (201, None), "{case}: {plain}");
        let (code, stamped) = post(&c, asked(&auth, "Stamped", Some(latest(&c) + 1))).await;
        assert_eq!((code, stored(&stamped)), (201, None), "{case}: {stamped}");
    }
}

//! `POST /api/questions`: a member's question for the chief. Where a test says
//! what Node answered, it is what the real API answered the same request (the
//! probe that printed those answers went with Node's API).

use hyper::Method;
use serde_json::{json, Value};

use crate::api::callers::caller_of;
use crate::api::routes::tests::support::{
    api, open_task, session_task_at_work, state_of, working_task,
};
use crate::testing::{request, said, scene, Scene};

async fn ask(scene: &Scene, token: &str, body: &str) -> (u16, Value) {
    api(scene, Method::POST, "/api/questions", token, body).await
}

fn refused(answered: &(u16, Value)) -> (u16, &str, &str) {
    (
        answered.0,
        answered.1["error"].as_str().unwrap_or("-"),
        answered.1["message"].as_str().unwrap_or("-"),
    )
}

fn issue_on_open(scene: &Scene, participant: i64) -> String {
    let ledger = scene.context.ledger.borrow();
    let start = cf_engine::seams::QuestionWindowStart::from_ledger(&ledger, participant).unwrap();
    scene
        .context
        .credentials
        .issue_for_window(scene.project.id, participant, start)
        .unwrap()
}

fn holder(scene: &Scene, task: i64) -> i64 {
    let ledger = scene.context.ledger.borrow();
    let handle = ledger
        .task(scene.project.id, task)
        .unwrap()
        .unwrap()
        .task
        .assignee
        .unwrap();
    ledger
        .project(scene.project.id)
        .unwrap()
        .unwrap()
        .participants
        .into_iter()
        .find(|p| p.handle == handle)
        .unwrap()
        .id
}

fn first_brief_opening(scene: &Scene) -> (i64, i64, i64, i64) {
    let number = open_task(scene);
    let member = scene.id("zeus");
    let mut ledger = scene.context.ledger.borrow_mut();
    let assigned = ledger
        .assign_task(scene.project.id, number, member)
        .unwrap();
    let brief = assigned.message.unwrap().id;
    let task = assigned.task.id;
    let session = ledger
        .project(scene.project.id)
        .unwrap()
        .unwrap()
        .participants
        .into_iter()
        .find(|p| Some(&p.handle) == assigned.task.assignee.as_ref())
        .unwrap()
        .id;
    ledger.begin_delivery(brief).unwrap();
    (number, task, session, brief)
}

#[tokio::test]
async fn t1813_a_window_started_with_its_brief_can_ask_before_the_host_answers_open() {
    let scene = scene();
    let (number, task, session, brief) = first_brief_opening(&scene);
    let start = cf_engine::seams::QuestionWindowStart {
        last_brief: None,
        task_in_hand: Some(task),
        launch_brief: Some(brief),
    };
    let token = scene
        .context
        .credentials
        .issue_for_window(scene.project.id, session, start)
        .unwrap();
    let response = ask(&scene, &token, r#"{"body":"The new window asks"}"#).await;
    assert_eq!(response.0, 201, "{response:?}");
    assert_eq!(response.1["message"]["task"], number);
}

#[tokio::test]
async fn t1813_a_launch_whose_first_task_moved_cannot_ask_as_idle() {
    let scene = scene();
    let (number, task, session, brief) = first_brief_opening(&scene);
    let start = cf_engine::seams::QuestionWindowStart {
        last_brief: None,
        task_in_hand: Some(task),
        launch_brief: Some(brief),
    };
    let token = scene
        .context
        .credentials
        .issue_for_window(scene.project.id, session, start)
        .unwrap();
    scene
        .context
        .ledger
        .borrow_mut()
        .release_task(scene.project.id, number, "by @human")
        .unwrap();
    let response = ask(&scene, &token, r#"{"body":"The old launch asks"}"#).await;
    assert_eq!(response.0, 409, "{response:?}");
    let words = response.1["message"].as_str().unwrap();
    assert!(
        words.contains(&format!("T-{number}")) && words.contains("open"),
        "{words}"
    );
}

#[tokio::test]
async fn t1813_a_launch_token_follows_a_later_delivered_brief() {
    let scene = scene();
    let (number, task, session, brief) = first_brief_opening(&scene);
    let start = cf_engine::seams::QuestionWindowStart {
        last_brief: None,
        task_in_hand: Some(task),
        launch_brief: Some(brief),
    };
    let token = scene
        .context
        .credentials
        .issue_for_window(scene.project.id, session, start)
        .unwrap();
    assert_eq!(
        ask(&scene, &token, r#"{"body":"First question"}"#).await.0,
        201
    );
    let next = {
        let mut ledger = scene.context.ledger.borrow_mut();
        ledger
            .confirm_delivery(brief, Some(&json!({"item":"first"})))
            .unwrap();
        ledger
            .record_result(scene.project.id, number, "Done")
            .unwrap();
        let next = ledger
            .create_task(
                scene.project.id,
                &cf_ledger::NewTask {
                    from: "chief".into(),
                    after: Some(number),
                    body: "Follow-up".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        ledger
            .accept_task(scene.project.id, number, "chief")
            .unwrap();
        let next_brief = ledger
            .task(scene.project.id, next.task.number)
            .unwrap()
            .unwrap()
            .messages
            .into_iter()
            .find(|m| m.kind == "task")
            .unwrap()
            .id;
        ledger.begin_delivery(next_brief).unwrap();
        ledger
            .confirm_delivery(next_brief, Some(&json!({"item":"follow"})))
            .unwrap();
        next.task.number
    };
    let response = ask(&scene, &token, r#"{"body":"Question after follow-up"}"#).await;
    assert_eq!(response.0, 201, "{response:?}");
    assert_eq!(response.1["message"]["task"], next);
}

#[tokio::test]
async fn t1813_pi_question_binds_on_inbox_send_before_the_acknowledgement() {
    let scene = scene();
    let (number, _, session, first) = first_brief_opening(&scene);
    scene
        .context
        .ledger
        .borrow_mut()
        .question_brief_handed_over(first)
        .unwrap();
    let token = issue_on_open(&scene, session);
    let (next, brief) = {
        let mut ledger = scene.context.ledger.borrow_mut();
        ledger
            .confirm_delivery(first, Some(&json!({"item":"first"})))
            .unwrap();
        ledger
            .record_result(scene.project.id, number, "Done")
            .unwrap();
        let next = ledger
            .create_task(
                scene.project.id,
                &cf_ledger::NewTask {
                    from: "chief".into(),
                    after: Some(number),
                    body: "Follow-up".into(),
                    ..Default::default()
                },
            )
            .unwrap()
            .task
            .number;
        ledger
            .accept_task(scene.project.id, number, "chief")
            .unwrap();
        let brief = ledger
            .task(scene.project.id, next)
            .unwrap()
            .unwrap()
            .messages
            .into_iter()
            .find(|m| m.kind == "task")
            .unwrap()
            .id;
        ledger.begin_delivery(brief).unwrap();
        (next, brief)
    };
    let before = ask(&scene, &token, r#"{"body":"Before the send"}"#).await;
    assert_eq!(before.0, 409, "{before:?}");
    scene
        .context
        .ledger
        .borrow_mut()
        .question_brief_handed_over(brief)
        .unwrap();
    let after = ask(&scene, &token, r#"{"body":"After the take"}"#).await;
    assert_eq!(after.0, 201, "{after:?}");
    assert_eq!(after.1["message"]["task"], next);
    let question = after.1["message"]["id"].as_i64().unwrap();
    let chief = scene.id("chief");
    let mut ledger = scene.context.ledger.borrow_mut();
    let answer = ledger
        .answer(question, chief, Some(&json!("The whole Pi answer")), None)
        .unwrap()
        .id;
    ledger.cancel_task(scene.project.id, next, "chief").unwrap();
    assert_eq!(ledger.message(answer).unwrap().unwrap().state, "cancelled");
    assert_eq!(
        ledger
            .inbox(chief, 100)
            .unwrap()
            .iter()
            .filter(|m| m.kind == "note" && m.body.contains("The whole Pi answer"))
            .count(),
        1
    );
}

#[tokio::test]
async fn t1813_a_reported_different_brief_binds_the_http_question_before_delivery_reply() {
    let scene = scene();
    let (number, _, session, first) = first_brief_opening(&scene);
    scene
        .context
        .ledger
        .borrow_mut()
        .question_brief_handed_over(first)
        .unwrap();
    let token = issue_on_open(&scene, session);
    let (next, brief) = {
        let mut ledger = scene.context.ledger.borrow_mut();
        ledger
            .confirm_delivery(first, Some(&json!({"item":"first"})))
            .unwrap();
        ledger
            .record_result(scene.project.id, number, "Done")
            .unwrap();
        let next = ledger
            .create_task(
                scene.project.id,
                &cf_ledger::NewTask {
                    from: "chief".into(),
                    after: Some(number),
                    body: "Follow-up".into(),
                    ..Default::default()
                },
            )
            .unwrap()
            .task
            .number;
        ledger
            .accept_task(scene.project.id, number, "chief")
            .unwrap();
        let brief = ledger
            .task(scene.project.id, next)
            .unwrap()
            .unwrap()
            .messages
            .into_iter()
            .find(|m| m.kind == "task")
            .unwrap()
            .id;
        ledger.begin_delivery(brief).unwrap();
        (next, brief)
    };
    let before = ask(&scene, &token, r#"{"body":"Before the report"}"#).await;
    assert_eq!(
        before.0, 409,
        "the ended previous holding still applies: {before:?}"
    );
    scene
        .context
        .ledger
        .borrow_mut()
        .question_brief_reported(brief)
        .unwrap();
    let after = ask(&scene, &token, r#"{"body":"After the report"}"#).await;
    assert_eq!(after.0, 201, "{after:?}");
    assert_eq!(after.1["message"]["task"], next);
    let question = after.1["message"]["id"].as_i64().unwrap();
    let chief = scene.id("chief");
    let answer = {
        let mut ledger = scene.context.ledger.borrow_mut();
        let answer = ledger
            .answer(question, chief, Some(&json!("The complete answer")), None)
            .unwrap()
            .id;
        ledger.cancel_task(scene.project.id, next, "chief").unwrap();
        assert_eq!(
            ledger.message(question).unwrap().unwrap().state,
            "cancelled"
        );
        assert_eq!(ledger.message(answer).unwrap().unwrap().state, "cancelled");
        assert_eq!(
            ledger
                .inbox(chief, 100)
                .unwrap()
                .iter()
                .filter(|m| { m.kind == "note" && m.body.contains("The complete answer") })
                .count(),
            1
        );
        answer
    };
    assert!(scene
        .context
        .ledger
        .borrow()
        .next_delivery_of(session, answer)
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn t1813_a_brief_reported_after_cancellation_refuses_the_window_with_the_task_location() {
    let scene = scene();
    let (number, _, session, first) = first_brief_opening(&scene);
    scene
        .context
        .ledger
        .borrow_mut()
        .question_brief_handed_over(first)
        .unwrap();
    let token = issue_on_open(&scene, session);
    let next = {
        let mut ledger = scene.context.ledger.borrow_mut();
        ledger
            .confirm_delivery(first, Some(&json!({"item":"first"})))
            .unwrap();
        ledger
            .record_result(scene.project.id, number, "Done")
            .unwrap();
        let next = ledger
            .create_task(
                scene.project.id,
                &cf_ledger::NewTask {
                    from: "chief".into(),
                    after: Some(number),
                    body: "Follow-up".into(),
                    ..Default::default()
                },
            )
            .unwrap()
            .task
            .number;
        ledger
            .accept_task(scene.project.id, number, "chief")
            .unwrap();
        let brief = ledger
            .task(scene.project.id, next)
            .unwrap()
            .unwrap()
            .messages
            .into_iter()
            .find(|m| m.kind == "task")
            .unwrap()
            .id;
        ledger.begin_delivery(brief).unwrap();
        ledger.cancel_task(scene.project.id, next, "chief").unwrap();
        ledger.question_brief_reported(brief).unwrap();
        next
    };
    let response = ask(
        &scene,
        &token,
        r#"{"body":"A question from the new brief"}"#,
    )
    .await;
    assert_eq!(response.0, 409, "{response:?}");
    let words = response.1["message"].as_str().unwrap();
    assert!(
        words.contains(&format!("T-{next}")) && words.contains("cancelled"),
        "{words}"
    );
}

#[tokio::test]
async fn t1813_a_resumed_window_can_ask_before_the_host_answers_its_open() {
    let scene = scene();
    let task = session_task_at_work(&scene);
    let session = holder(&scene, task);
    let start = {
        let mut ledger = scene.context.ledger.borrow_mut();
        ledger.begin_question_engine();
        ledger.close_questions_of_ended_engine().unwrap();
        let held = ledger.question_holding(session).unwrap().unwrap();
        assert!(held.resumable);
        cf_engine::seams::QuestionWindowStart {
            last_brief: Some(held.last_brief),
            task_in_hand: Some(held.task_id),
            launch_brief: None,
        }
    };
    // The token is issued just before the pane host opens the window. A call
    // bearing it proves the window ran, even if open has not replied yet.
    let token = scene
        .context
        .credentials
        .issue_for_window(scene.project.id, session, start)
        .unwrap();
    let response = ask(&scene, &token, r#"{"body":"A question as Resume opens"}"#).await;
    assert_eq!(response.0, 201, "{response:?}");
    assert_eq!(response.1["message"]["task"], task);
}

#[tokio::test]
async fn t1813_resumed_window_questions_use_the_resumed_task_holding() {
    let scene = scene();
    let task = session_task_at_work(&scene);
    let session = holder(&scene, task);
    let start = {
        let mut ledger = scene.context.ledger.borrow_mut();
        ledger.begin_question_engine();
        ledger.close_questions_of_ended_engine().unwrap();
        let held = ledger.question_holding(session).unwrap().unwrap();
        assert!(held.resumable);
        let start = cf_engine::seams::QuestionWindowStart {
            last_brief: Some(held.last_brief),
            task_in_hand: Some(held.task_id),
            launch_brief: None,
        };
        assert!(ledger.resume_question_holding(session, task).unwrap());
        start
    };
    let token = scene
        .context
        .credentials
        .issue_for_window(scene.project.id, session, start)
        .unwrap();
    let response = ask(&scene, &token, r#"{"body":"Question after Resume"}"#).await;
    assert_eq!(response.0, 201, "{response:?}");
    assert_eq!(response.1["message"]["task"], task);
}

#[tokio::test]
async fn t1813_resume_window_whose_task_moved_refuses_with_its_location() {
    let scene = scene();
    let task = session_task_at_work(&scene);
    let session = holder(&scene, task);
    let start = {
        let mut ledger = scene.context.ledger.borrow_mut();
        ledger.begin_question_engine();
        ledger.close_questions_of_ended_engine().unwrap();
        let held = ledger.question_holding(session).unwrap().unwrap();
        let start = cf_engine::seams::QuestionWindowStart {
            last_brief: Some(held.last_brief),
            task_in_hand: Some(held.task_id),
            launch_brief: None,
        };
        ledger
            .release_task(scene.project.id, task, "by @human")
            .unwrap();
        assert!(!ledger.resume_question_holding(session, task).unwrap());
        start
    };
    let token = scene
        .context
        .credentials
        .issue_for_window(scene.project.id, session, start)
        .unwrap();
    let response = ask(&scene, &token, r#"{"body":"Question after it moved"}"#).await;
    assert_eq!(response.0, 409, "{response:?}");
    let words = response.1["message"].as_str().unwrap();
    assert!(
        words.contains(&format!("T-{task}")) && words.contains("open"),
        "{words}"
    );
}

#[tokio::test]
async fn window_generation_allows_a_fresh_idle_question_after_cancellation() {
    let scene = scene();
    let task = session_task_at_work(&scene);
    let session = holder(&scene, task);
    let old = issue_on_open(&scene, session);
    scene
        .context
        .ledger
        .borrow_mut()
        .cancel_task(scene.project.id, task, "chief")
        .unwrap();
    let late = ask(&scene, &old, r#"{"body":"Old turn"}"#).await;
    assert_eq!(late.0, 409, "{late:?}");
    assert_eq!(late.1["error"], "task-cancelled");
    let fresh = issue_on_open(&scene, session);
    let asked = ask(&scene, &fresh, r#"{"body":"A taskless question"}"#).await;
    assert_eq!(asked.0, 201, "{asked:?}");
    assert_eq!(asked.1["message"]["task"], json!(null));
}

#[tokio::test]
async fn window_generation_refuses_an_ended_tasks_old_window_and_allows_a_fresh_one() {
    for ending in ["accepted", "failed", "released"] {
        let scene = scene();
        let task = session_task_at_work(&scene);
        let session = holder(&scene, task);
        let old = issue_on_open(&scene, session);
        {
            let mut ledger = scene.context.ledger.borrow_mut();
            match ending {
                "done" | "accepted" => {
                    ledger
                        .record_result(scene.project.id, task, "Done")
                        .unwrap();
                    if ending == "accepted" {
                        ledger.accept_task(scene.project.id, task, "chief").unwrap();
                    }
                }
                "failed" => {
                    ledger
                        .fail_task(scene.project.id, task, "the window failed")
                        .unwrap();
                }
                "released" => {
                    ledger
                        .release_task(scene.project.id, task, "by @human")
                        .unwrap();
                }
                _ => unreachable!(),
            }
        }
        let late = ask(&scene, &old, r#"{"body":"Old turn"}"#).await;
        assert_eq!(late.0, 409, "{ending}: {late:?}");
        let fresh = issue_on_open(&scene, session);
        let asked = ask(&scene, &fresh, r#"{"body":"A taskless question"}"#).await;
        assert_eq!(asked.0, 201, "{ending}: {asked:?}");
        assert_eq!(asked.1["message"]["task"], json!(null));
    }
}

#[tokio::test]
async fn window_generation_tracks_a_brief_that_starts_after_the_window_opened() {
    for delivery in ["confirmed", "interrupted", "refunded"] {
        let scene = scene();
        let first = session_task_at_work(&scene);
        let session = holder(&scene, first);
        let follow = {
            let mut ledger = scene.context.ledger.borrow_mut();
            ledger
                .record_result(scene.project.id, first, "Done")
                .unwrap();
            ledger
                .create_task(
                    scene.project.id,
                    &cf_ledger::NewTask {
                        from: "chief".into(),
                        after: Some(first),
                        body: "Next task".into(),
                        ..Default::default()
                    },
                )
                .unwrap()
                .task
                .number
        };
        let token = issue_on_open(&scene, session);
        {
            let mut ledger = scene.context.ledger.borrow_mut();
            ledger
                .accept_task(scene.project.id, first, "chief")
                .unwrap();
            let brief = ledger
                .task(scene.project.id, follow)
                .unwrap()
                .unwrap()
                .messages
                .into_iter()
                .find(|m| m.kind == "task")
                .unwrap()
                .id;
            ledger.begin_delivery(brief).unwrap();
            if delivery == "confirmed" {
                ledger
                    .confirm_delivery(brief, Some(&json!({ "item": "test" })))
                    .unwrap();
            } else if delivery == "refunded" {
                ledger.retry_delivery(brief, "not ready", true).unwrap();
            }
            ledger
                .cancel_task(scene.project.id, follow, "chief")
                .unwrap();
        }
        let late = ask(&scene, &token, r#"{"body":"Old turn"}"#).await;
        assert_eq!(late.0, 409, "{delivery}: {late:?}");
        assert_eq!(
            late.1["error"],
            if delivery == "confirmed" {
                "task-cancelled"
            } else {
                "task-ended"
            }
        );
        let fresh = issue_on_open(&scene, session);
        let asked = ask(&scene, &fresh, r#"{"body":"A taskless question"}"#).await;
        assert_eq!(asked.0, 201, "{delivery}: {asked:?}");
        assert_eq!(asked.1["message"]["task"], json!(null));
    }
}

#[tokio::test]
async fn window_generation_tracks_a_new_brief_on_the_same_task() {
    let scene = scene();
    let task = session_task_at_work(&scene);
    let session = holder(&scene, task);
    scene
        .context
        .ledger
        .borrow_mut()
        .record_result(scene.project.id, task, "Done")
        .unwrap();
    let token = issue_on_open(&scene, session);
    {
        let mut ledger = scene.context.ledger.borrow_mut();
        let brief = ledger
            .reopen_task(scene.project.id, task, "chief", "Go on")
            .unwrap()
            .message
            .unwrap()
            .id;
        ledger.begin_delivery(brief).unwrap();
        ledger
            .confirm_delivery(brief, Some(&json!({ "item": "test" })))
            .unwrap();
        ledger
            .record_result(scene.project.id, task, "Done again")
            .unwrap();
    }
    let late = ask(&scene, &token, r#"{"body":"Old turn"}"#).await;
    assert_eq!(late.0, 201, "{late:?}");
    assert_eq!(late.1["message"]["task"], task);
    let fresh = issue_on_open(&scene, session);
    let asked = ask(&scene, &fresh, r#"{"body":"Same holding"}"#).await;
    assert_eq!(asked.0, 201, "{asked:?}");
    assert_eq!(asked.1["message"]["task"], task);
}

#[tokio::test]
async fn the_chief_is_sent_to_its_terminal_before_its_body_is_read() {
    let scene = scene();
    for body in [r#"{"body":"Ship?"}"#, "not json", "[]"] {
        let said = ask(&scene, &scene.chief, body).await;
        assert_eq!(
            refused(&said),
            (
                403,
                "ask-in-your-terminal",
                "ask the human here in your terminal: they read and answer you there"
            ),
            "{body}"
        );
    }
    let huge = "x".repeat(3 * 1024 * 1024);
    assert_eq!(ask(&scene, &scene.chief, &huge).await.0, 403);
    assert_eq!(scene.kicks.get(), 0);
}

#[tokio::test]
async fn a_member_s_question_is_about_the_task_it_has_in_progress_and_the_task_waits_for_the_answer(
) {
    let scene = scene();
    let number = working_task(&scene);
    let (status, said) = ask(&scene, &scene.zeus, r#"{"body":"Which format?"}"#).await;
    assert_eq!(status, 201, "{said}");
    let message = said["message"].to_string();
    assert!(
        message.starts_with(&format!(
            r#"{{"id":{},"kind":"question","state":"queued","sender":"zeus","recipient":"chief","task":{number},"preview":"Which format?","questions":null,"choices":null,"createdAt":""#,
            said["message"]["id"]
        )),
        "{message}"
    );
    assert_eq!(
        said.as_object().unwrap().keys().collect::<Vec<_>>(),
        ["message"]
    );
    assert_eq!(state_of(&scene, number), "waiting");
    assert_eq!(scene.kicks.get(), 1);
}

#[tokio::test]
async fn a_window_with_no_task_asks_about_none() {
    let scene = scene();
    let (status, said) = ask(&scene, &scene.zeus, r#"{"body":"Which?"}"#).await;
    assert_eq!(status, 201);
    assert_eq!(said["message"]["task"], json!(null));
    assert_eq!(said["message"]["preview"], "Which?");
}

#[tokio::test]
async fn an_old_windows_ask_that_arrives_after_release_is_refused_not_made_taskless() {
    let scene = scene();
    let number = session_task_at_work(&scene);
    let (old_id, old) = {
        let ledger = scene.context.ledger.borrow();
        let task = ledger.task(scene.project.id, number).unwrap().unwrap().task;
        let old = task.assignee.unwrap();
        let old_id = ledger
            .project(scene.project.id)
            .unwrap()
            .unwrap()
            .participants
            .into_iter()
            .find(|participant| participant.handle == old)
            .unwrap()
            .id;
        (old_id, old)
    };
    // A request can already have its caller while the old window is being stopped.
    let token = scene
        .context
        .credentials
        .issue(scene.project.id, old_id)
        .unwrap();
    let request = request(
        Method::POST,
        "/api/questions",
        Some(&token),
        r#"{"body":"Which format?"}"#,
    );
    let caller = caller_of(&scene.context, &request).unwrap();
    scene
        .context
        .ledger
        .borrow_mut()
        .release_task(scene.project.id, number, "by @human")
        .unwrap();
    let said = said(super::ask(&scene.context, &caller, request).await);
    assert_eq!(said.0, 409, "{said:?}");
    assert_eq!(said.1["error"], "question-moved");
    let words = said.1["message"].as_str().unwrap();
    assert!(words.contains("T-1") && words.contains("open"), "{words}");
    let ledger = scene.context.ledger.borrow();
    let chief = ledger
        .project(scene.project.id)
        .unwrap()
        .unwrap()
        .participants
        .into_iter()
        .find(|p| p.handle == "chief")
        .unwrap()
        .id;
    assert!(ledger
        .inbox(chief, 100)
        .unwrap()
        .iter()
        .all(|m| { m.sender.as_deref() != Some(old.as_str()) || m.kind != "question" }));
    assert!(ledger
        .pending(old_id)
        .unwrap()
        .iter()
        .all(|m| m.kind != "answer"));
}

async fn ask_after_interrupted_brief(refund: bool) {
    let scene = scene();
    let number = open_task(&scene);
    let (old_id, brief) = {
        let mut ledger = scene.context.ledger.borrow_mut();
        let project = ledger.project(scene.project.id).unwrap().unwrap();
        let zeus = project
            .participants
            .iter()
            .find(|p| p.handle == "zeus")
            .unwrap()
            .id;
        let assigned = ledger.assign_task(scene.project.id, number, zeus).unwrap();
        let old = assigned.task.assignee.unwrap();
        let old_id = ledger
            .project(scene.project.id)
            .unwrap()
            .unwrap()
            .participants
            .into_iter()
            .find(|p| p.handle == old)
            .unwrap()
            .id;
        let brief = assigned.message.unwrap().id;
        ledger.begin_delivery(brief).unwrap();
        (old_id, brief)
    };
    let token = scene
        .context
        .credentials
        .issue(scene.project.id, old_id)
        .unwrap();
    let request = request(
        Method::POST,
        "/api/questions",
        Some(&token),
        r#"{"body":"Which format?"}"#,
    );
    let caller = caller_of(&scene.context, &request).unwrap();
    {
        let mut ledger = scene.context.ledger.borrow_mut();
        if refund {
            let retried = ledger
                .retry_delivery(brief, "the window went", true)
                .unwrap();
            assert_eq!((retried.state.as_str(), retried.attempts), ("queued", 0));
        }
        ledger
            .release_task(scene.project.id, number, "by @human")
            .unwrap();
        assert_eq!(ledger.message(brief).unwrap().unwrap().state, "cancelled");
    }
    let said = said(super::ask(&scene.context, &caller, request).await);
    assert_eq!(said.0, 201, "{said:?}");
    assert_eq!(said.1["message"]["task"], json!(null));
}

#[tokio::test]
async fn an_ask_in_flight_is_refused_when_release_interrupts_its_brief_delivery() {
    ask_after_interrupted_brief(false).await;
}

#[tokio::test]
async fn an_ask_in_flight_is_refused_after_its_brief_attempt_is_refunded() {
    ask_after_interrupted_brief(true).await;
}

#[tokio::test]
async fn an_old_windows_ask_that_arrives_after_it_finishes_is_refused_not_made_taskless() {
    for ending in ["accepted", "failed", "cancelled"] {
        let scene = scene();
        let number = session_task_at_work(&scene);
        let old_id = {
            let ledger = scene.context.ledger.borrow();
            let old = ledger
                .task(scene.project.id, number)
                .unwrap()
                .unwrap()
                .task
                .assignee
                .unwrap();
            ledger
                .project(scene.project.id)
                .unwrap()
                .unwrap()
                .participants
                .into_iter()
                .find(|p| p.handle == old)
                .unwrap()
                .id
        };
        let token = scene
            .context
            .credentials
            .issue(scene.project.id, old_id)
            .unwrap();
        let request = request(
            Method::POST,
            "/api/questions",
            Some(&token),
            r#"{"body":"Which format?"}"#,
        );
        let caller = caller_of(&scene.context, &request).unwrap();
        {
            let mut ledger = scene.context.ledger.borrow_mut();
            match ending {
                "done" | "accepted" => {
                    ledger
                        .record_result(scene.project.id, number, "Done")
                        .unwrap();
                    if ending == "accepted" {
                        ledger
                            .accept_task(scene.project.id, number, "chief")
                            .unwrap();
                    }
                }
                "failed" => {
                    ledger
                        .fail_task(scene.project.id, number, "the window failed")
                        .unwrap();
                }
                "cancelled" => {
                    ledger
                        .cancel_task(scene.project.id, number, "chief")
                        .unwrap();
                }
                _ => unreachable!(),
            }
        }
        let said = said(super::ask(&scene.context, &caller, request).await);
        assert_eq!(said.0, 409, "{ending}: {said:?}");
        assert!(
            said.1["message"].as_str().unwrap().contains("T-1"),
            "{ending}: {said:?}"
        );
    }
}

#[tokio::test]
async fn a_window_whose_task_was_cancelled_is_told_so_and_nothing_is_put() {
    let scene = scene();
    let number = working_task(&scene);
    scene
        .context
        .ledger
        .borrow_mut()
        .cancel_task(scene.project.id, number, "human")
        .unwrap();
    let said = ask(&scene, &scene.zeus, r#"{"body":"Which format?"}"#).await;
    assert_eq!(
        refused(&said),
        (
            409,
            "task-cancelled",
            "T-1 is cancelled. Wait for another brief in this window before asking."
        )
    );
    // The body is read first: a bad one is the answer, cancelled or not.
    assert_eq!(
        ask(&scene, &scene.zeus, "not json").await.1["error"],
        "invalid-json"
    );
    assert_eq!(scene.kicks.get(), 0);
}

/// The first poll of the door of question `id`, as zeus's harness makes it.
async fn poll(scene: &Scene, id: i64) -> (u16, Value) {
    let target = format!("/api/questions/{id}?wait=5000");
    api(scene, Method::GET, &target, &scene.zeus, "").await
}

#[tokio::test(start_paused = true)]
async fn a_window_the_pause_has_not_stopped_yet_asks_about_its_paused_task_and_its_door_is_born_shut(
) {
    let scene = scene();
    let number = working_task(&scene);
    scene.pause();
    let (status, said) = ask(&scene, &scene.zeus, r#"{"body":"Which format?"}"#).await;
    assert_eq!(status, 201, "{said}");
    assert_eq!(
        said["message"]["task"], number,
        "it is about the task it was stopped on"
    );
    assert_eq!(state_of(&scene, number), "paused", "asking moves nothing");
    assert_eq!(scene.kicks.get(), 1);
    // The turn that asks is an old one: its door answers at its first poll, without a wait.
    let started = tokio::time::Instant::now();
    let (status, said) = poll(&scene, said["message"]["id"].as_i64().unwrap()).await;
    assert_eq!((status, said["error"].as_str()), (409, Some("door-closed")));
    assert_eq!(started.elapsed(), std::time::Duration::ZERO);
}

#[tokio::test(start_paused = true)]
async fn a_window_whose_task_waits_for_the_words_that_resume_it_asks_about_it_with_its_door_born_shut(
) {
    let scene = scene();
    let number = working_task(&scene);
    scene.pause();
    scene
        .context
        .ledger
        .borrow_mut()
        .resume_task(scene.project.id, number, Some("chief"), "Carry on")
        .unwrap();
    assert_eq!(
        state_of(&scene, number),
        "queued",
        "the words are not pasted yet"
    );
    let (status, said) = ask(&scene, &scene.zeus, r#"{"body":"Which format?"}"#).await;
    assert_eq!(status, 201, "{said}");
    assert_eq!(said["message"]["task"], number);
    assert_eq!(
        state_of(&scene, number),
        "queued",
        "the words that resume it come first"
    );
    let (status, said) = poll(&scene, said["message"]["id"].as_i64().unwrap()).await;
    assert_eq!((status, said["error"].as_str()), (409, Some("door-closed")));
}

#[tokio::test(start_paused = true)]
async fn a_window_at_work_asks_with_its_door_open_and_the_task_waits() {
    let scene = scene();
    let number = working_task(&scene);
    let (_, said) = ask(&scene, &scene.zeus, r#"{"body":"Which format?"}"#).await;
    assert_eq!(state_of(&scene, number), "waiting");
    let started = tokio::time::Instant::now();
    let (status, said) = poll(&scene, said["message"]["id"].as_i64().unwrap()).await;
    assert_eq!((status, said["answer"].clone()), (200, json!(null)));
    assert_eq!(
        started.elapsed(),
        std::time::Duration::from_secs(5),
        "nothing shut it: it waited"
    );
}

#[tokio::test]
async fn words_that_are_no_words_are_refused_and_questions_that_are_none_are_refused_in_theirs() {
    let scene = scene();
    for (body, said) in [
        (
            "{}",
            (
                400,
                "invalid-text",
                "body must be text, not empty, at most 1000000 characters",
            ),
        ),
        (
            r#"{"body":5}"#,
            (
                400,
                "invalid-text",
                "body must be text, not empty, at most 1000000 characters",
            ),
        ),
        (
            r#"{"body":"x","questions":null}"#,
            (400, "bad-questions", "questions: one to 4 questions"),
        ),
        (
            r#"{"body":"x","questions":[]}"#,
            (400, "bad-questions", "questions: one to 4 questions"),
        ),
        (
            r#"{"body":"x","questions":"x"}"#,
            (400, "bad-questions", "questions: one to 4 questions"),
        ),
    ] {
        let answered = ask(&scene, &scene.zeus, body).await;
        assert_eq!(refused(&answered), said, "{body}");
    }
    assert_eq!(scene.kicks.get(), 0);
}

#[tokio::test]
async fn a_question_with_options_is_put_with_them_and_its_words_are_theirs() {
    let scene = scene();
    // What it says in `body` is not read when it asks by questions.
    let body = json!({
        "body": 5,
        "questions": [{
            "question": "Which parser?",
            "header": "Parser",
            "options": [{ "label": "A" }, { "label": "B", "description": "bee" }]
        }]
    })
    .to_string();
    let (status, said) = ask(&scene, &scene.zeus, &body).await;
    assert_eq!(status, 201, "{said}");
    assert_eq!(said["message"]["preview"], "Parser: Which parser?");
    assert_eq!(
        said["message"]["questions"].to_string(),
        r#"[{"question":"Which parser?","header":"Parser","options":[{"label":"A","description":null},{"label":"B","description":"bee"}],"multiple":false}]"#
    );
    assert_eq!(scene.kicks.get(), 1);
}

#[tokio::test]
async fn t1800_finding_01_an_ask_authenticated_on_one_turn_does_not_belong_to_the_follow_up() {
    let scene = scene();
    let task = session_task_at_work(&scene);
    let session = {
        let ledger = scene.context.ledger.borrow();
        let holder = ledger
            .task(scene.project.id, task)
            .unwrap()
            .unwrap()
            .task
            .assignee
            .unwrap();
        ledger
            .project(scene.project.id)
            .unwrap()
            .unwrap()
            .participants
            .into_iter()
            .find(|p| p.handle == holder)
            .unwrap()
    };
    let token = scene
        .context
        .credentials
        .issue(scene.project.id, session.id)
        .unwrap();
    let request = request(
        Method::POST,
        "/api/questions",
        Some(&token),
        r#"{"body":"A late question"}"#,
    );
    let caller = caller_of(&scene.context, &request).unwrap();
    {
        let mut ledger = scene.context.ledger.borrow_mut();
        ledger
            .record_result(scene.project.id, task, "Done")
            .unwrap();
        ledger
            .create_task(
                scene.project.id,
                &cf_ledger::NewTask {
                    from: "chief".into(),
                    after: Some(task),
                    body: "Next task".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        ledger.accept_task(scene.project.id, task, "chief").unwrap();
    }
    let response = said(super::ask(&scene.context, &caller, request).await);
    assert_eq!(response.0, 409, "{response:?}");
    let ledger = scene.context.ledger.borrow();
    assert!(ledger
        .inbox(scene.id("chief"), 100)
        .unwrap()
        .iter()
        .all(|m| m.body != "A late question"));
}

#[tokio::test]
async fn t1800_finding_06_a_fresh_idle_session_can_ask_a_taskless_question() {
    let scene = scene();
    let task = session_task_at_work(&scene);
    let session = {
        let mut ledger = scene.context.ledger.borrow_mut();
        let holder = ledger
            .task(scene.project.id, task)
            .unwrap()
            .unwrap()
            .task
            .assignee
            .unwrap();
        let id = ledger
            .project(scene.project.id)
            .unwrap()
            .unwrap()
            .participants
            .into_iter()
            .find(|p| p.handle == holder)
            .unwrap()
            .id;
        ledger
            .record_result(scene.project.id, task, "Done")
            .unwrap();
        ledger.accept_task(scene.project.id, task, "chief").unwrap();
        id
    };
    let token = issue_on_open(&scene, session);
    let (status, body) = ask(&scene, &token, r#"{"body":"A taskless question"}"#).await;
    assert_eq!(status, 201, "{body}");
    assert_eq!(body["message"]["task"], json!(null));
}

#[tokio::test]
async fn review_01_a_follow_up_not_yet_handed_over_cannot_capture_an_old_turns_question() {
    for begun in [false, true] {
        let scene = scene();
        let task = session_task_at_work(&scene);
        let session = {
            let mut ledger = scene.context.ledger.borrow_mut();
            let holder = ledger
                .task(scene.project.id, task)
                .unwrap()
                .unwrap()
                .task
                .assignee
                .unwrap();
            let id = ledger
                .project(scene.project.id)
                .unwrap()
                .unwrap()
                .participants
                .into_iter()
                .find(|p| p.handle == holder)
                .unwrap()
                .id;
            ledger
                .record_result(scene.project.id, task, "Done")
                .unwrap();
            let follow = ledger
                .create_task(
                    scene.project.id,
                    &cf_ledger::NewTask {
                        from: "chief".into(),
                        after: Some(task),
                        body: "Next task".into(),
                        ..Default::default()
                    },
                )
                .unwrap()
                .task
                .number;
            ledger.accept_task(scene.project.id, task, "chief").unwrap();
            let brief = ledger
                .task(scene.project.id, follow)
                .unwrap()
                .unwrap()
                .messages
                .into_iter()
                .find(|m| m.kind == "task")
                .unwrap()
                .id;
            if begun {
                ledger.begin_delivery(brief).unwrap();
            }
            id
        };
        let token = scene
            .context
            .credentials
            .issue(scene.project.id, session)
            .unwrap();
        let (status, body) = ask(&scene, &token, r#"{"body":"Still from the old turn"}"#).await;
        assert_eq!(status, 409, "begun={begun}: {body}");
    }
}

#[tokio::test]
async fn a_follow_up_can_ask_once_its_brief_is_handed_to_the_window() {
    let scene = scene();
    let first = session_task_at_work(&scene);
    let (token, second) = {
        let mut ledger = scene.context.ledger.borrow_mut();
        let holder = ledger
            .task(scene.project.id, first)
            .unwrap()
            .unwrap()
            .task
            .assignee
            .unwrap();
        let id = ledger
            .project(scene.project.id)
            .unwrap()
            .unwrap()
            .participants
            .into_iter()
            .find(|p| p.handle == holder)
            .unwrap()
            .id;
        // The window token is the same one its preceding task used.
        let token = scene
            .context
            .credentials
            .issue(scene.project.id, id)
            .unwrap();
        ledger
            .record_result(scene.project.id, first, "Done")
            .unwrap();
        let second = ledger
            .create_task(
                scene.project.id,
                &cf_ledger::NewTask {
                    from: "chief".into(),
                    after: Some(first),
                    body: "Next task".into(),
                    ..Default::default()
                },
            )
            .unwrap()
            .task
            .number;
        ledger
            .accept_task(scene.project.id, first, "chief")
            .unwrap();
        let brief = ledger
            .task(scene.project.id, second)
            .unwrap()
            .unwrap()
            .messages
            .into_iter()
            .find(|m| m.kind == "task")
            .unwrap()
            .id;
        ledger.begin_delivery(brief).unwrap();
        assert_eq!(
            ledger.admit(brief, &[]).unwrap(),
            cf_ledger::HandOver::Admitted
        );
        ledger.question_brief_handed_over(brief).unwrap();
        (token, second)
    };
    let (status, body) = ask(&scene, &token, r#"{"body":"About the new task"}"#).await;
    assert_eq!(status, 201, "{body}");
    assert_eq!(body["message"]["task"], second);
}

#[tokio::test]
async fn window_generation_refuses_conflicting_brief_projects_without_writing() {
    for conflict in ["task", "message"] {
        let scene = scene();
        let task = session_task_at_work(&scene);
        let session = holder(&scene, task);
        let token = issue_on_open(&scene, session);
        let chief = scene.id("chief");
        let (brief, expected, before) = {
            let mut ledger = scene.context.ledger.borrow_mut();
            let thread = ledger.task(scene.project.id, task).unwrap().unwrap();
            let original_task = thread.task.id;
            let brief = thread
                .messages
                .into_iter()
                .find(|m| m.kind == "task" && m.recipient_id == session)
                .unwrap()
                .id;
            ledger.cancel_task(scene.project.id, task, "chief").unwrap();
            let other = ledger
                .create_project(&cf_ledger::NewProject {
                    directory: "/work/generation-other".into(),
                    name: "generation-other".into(),
                    chief: cf_ledger::NewChief {
                        harness: "claude-code".into(),
                        model: Some("mybuilder".into()),
                        effort: None,
                        model_name: None,
                    },
                    staff: vec![cf_ledger::NewMember {
                        model: Some("mybuilder".into()),
                        effort: None,
                        model_name: Some("hera".into()),
                        harness: "claude-code".into(),
                        designer: false,
                        roles: vec!["worker".into()],
                        tier: "standard".into(),
                    }],
                    gate: false,
                    wake_up: None,
                    specs: None,
                    staff_rules: None,
                    compaction: None,
                    advisor_tool: false,
                    prompt_suggestion: false,
                    own_hands: false,
                })
                .unwrap();
            let foreign = cf_ledger::NewTask {
                from: "chief".into(),
                body: "Other project".into(),
                pool: Some("worker".into()),
                tier: Some("standard".into()),
                ..Default::default()
            };
            ledger.create_task(other.id, &foreign).unwrap();
            let foreign_task = ledger.create_task(other.id, &foreign).unwrap().task.id;
            let expected = if conflict == "task" {
                ledger
                    .sqlite()
                    .execute(
                        "UPDATE message SET task_id = ?1 WHERE id = ?2",
                        (foreign_task, brief),
                    )
                    .unwrap();
                (foreign_task, scene.project.id)
            } else {
                ledger
                    .sqlite()
                    .execute(
                        "UPDATE message SET project_id = ?1 WHERE id = ?2",
                        (other.id, brief),
                    )
                    .unwrap();
                (original_task, other.id)
            };
            let before = ledger.inbox(chief, 100).unwrap();
            (brief, expected, before)
        };
        let kicks = scene.kicks.get();
        let said = ask(&scene, &token, r#"{"body":"A question"}"#).await;
        assert_eq!(said.0, 409, "{conflict}: {said:?}");
        assert_eq!(said.1["error"], "question-conflict");
        let ledger = scene.context.ledger.borrow();
        assert_eq!(ledger.inbox(chief, 100).unwrap(), before);
        assert_eq!(
            ledger
                .sqlite()
                .query_row(
                    "SELECT task_id, project_id FROM message WHERE id = ?",
                    [brief],
                    |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
                )
                .unwrap(),
            expected
        );
        assert_eq!(scene.kicks.get(), kicks);
    }
}

#[tokio::test]
async fn window_generation_revoked_before_the_body_is_read_cannot_ask() {
    let scene = scene();
    let task = session_task_at_work(&scene);
    let session = holder(&scene, task);
    let old = issue_on_open(&scene, session);
    let request = request(
        Method::POST,
        "/api/questions",
        Some(&old),
        r#"{"body":"From the closed window"}"#,
    );
    let caller = caller_of(&scene.context, &request).unwrap();
    scene.context.credentials.revoke(&old);
    let fresh = issue_on_open(&scene, session);
    let late = said(super::ask(&scene.context, &caller, request).await);
    assert_eq!(late.0, 409, "{late:?}");
    assert_eq!(late.1["error"], "question-window-closed");
    let asked = ask(&scene, &fresh, r#"{"body":"From the new window"}"#).await;
    assert_eq!(asked.0, 201, "{asked:?}");
    assert_eq!(asked.1["message"]["task"], task);
    let chief = scene.id("chief");
    let ledger = scene.context.ledger.borrow();
    assert!(ledger
        .inbox(chief, 100)
        .unwrap()
        .iter()
        .all(|message| message.body != "From the closed window"));
}

#[tokio::test]
async fn t1803_finding_01_queued_reopen_does_not_replace_the_last_delivered_task() {
    let scene = scene();
    let root = session_task_at_work(&scene);
    let session = holder(&scene, root);
    let token = issue_on_open(&scene, session);
    let (earlier, recent, pending) = {
        let mut ledger = scene.context.ledger.borrow_mut();
        ledger
            .record_result(scene.project.id, root, "Root result")
            .unwrap();
        ledger.accept_task(scene.project.id, root, "chief").unwrap();
        let earlier = ledger
            .create_task(
                scene.project.id,
                &cf_ledger::NewTask {
                    from: "chief".into(),
                    after: Some(root),
                    body: "Earlier follow-up".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        let earlier_number = earlier.task.number;
        let earlier_brief = earlier.message.unwrap().id;
        ledger.begin_delivery(earlier_brief).unwrap();
        ledger
            .confirm_delivery(earlier_brief, Some(&json!({"item": "earlier"})))
            .unwrap();
        ledger
            .give_up_task(scene.project.id, earlier_number, "Earlier stopped")
            .unwrap();
        let recent = ledger
            .create_task(
                scene.project.id,
                &cf_ledger::NewTask {
                    from: "chief".into(),
                    after: Some(root),
                    body: "Recent follow-up".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        let recent_number = recent.task.number;
        let recent_brief = recent.message.unwrap().id;
        ledger.begin_delivery(recent_brief).unwrap();
        ledger
            .confirm_delivery(recent_brief, Some(&json!({"item": "recent"})))
            .unwrap();
        ledger
            .record_result(scene.project.id, recent_number, "Result")
            .unwrap();
        ledger
            .accept_task(scene.project.id, recent_number, "chief")
            .unwrap();
        let pending = ledger
            .reopen_task(scene.project.id, earlier_number, "chief", "Reopen")
            .unwrap()
            .message
            .unwrap()
            .id;
        (earlier_number, recent_number, pending)
    };
    assert_eq!(
        scene
            .context
            .ledger
            .borrow()
            .message(pending)
            .unwrap()
            .unwrap()
            .state,
        "queued"
    );
    let reply = ask(&scene, &token, r#"{"body":"Old recent turn asks"}"#).await;
    assert_eq!(reply.0, 409, "T-{recent} was last delivered; the queued reopen T-{earlier} cannot capture its question: {reply:?}");
}

#[tokio::test]
async fn t1813_a_brief_admitted_but_not_yet_written_does_not_capture_the_prior_turns_question() {
    let scene = scene();
    let first = session_task_at_work(&scene);
    let session = holder(&scene, first);
    let token = issue_on_open(&scene, session);
    {
        let mut ledger = scene.context.ledger.borrow_mut();
        ledger
            .record_result(scene.project.id, first, "Done")
            .unwrap();
        ledger
            .accept_task(scene.project.id, first, "chief")
            .unwrap();
        let next = ledger
            .create_task(
                scene.project.id,
                &cf_ledger::NewTask {
                    from: "chief".into(),
                    after: Some(first),
                    body: "Follow-up".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        let brief = next.message.unwrap().id;
        ledger.begin_delivery(brief).unwrap();
        assert_eq!(
            ledger.admit(brief, &[]).unwrap(),
            cf_ledger::HandOver::Admitted
        );
        // The host has yet to receive pane.write_paste, so nothing is delivered.
    }
    let reply = ask(&scene, &token, r#"{"body":"Question from the prior turn"}"#).await;
    assert_eq!(
        reply.0, 409,
        "the unperformed paste must not change the holding: {reply:?}"
    );
}

#[tokio::test]
async fn t1813_a_refunded_brief_does_not_become_the_windows_held_task() {
    let scene = scene();
    let first = session_task_at_work(&scene);
    let session = holder(&scene, first);
    let token = issue_on_open(&scene, session);
    let second = {
        let mut ledger = scene.context.ledger.borrow_mut();
        ledger
            .record_result(scene.project.id, first, "Done")
            .unwrap();
        ledger
            .accept_task(scene.project.id, first, "chief")
            .unwrap();
        let task = ledger
            .create_task(
                scene.project.id,
                &cf_ledger::NewTask {
                    from: "chief".into(),
                    after: Some(first),
                    body: "Follow-up".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        let brief = task.message.unwrap().id;
        ledger.begin_delivery(brief).unwrap();
        assert_eq!(
            ledger.admit(brief, &[]).unwrap(),
            cf_ledger::HandOver::Admitted
        );
        ledger
            .retry_delivery(brief, "The handover was refused", true)
            .unwrap();
        task.task.number
    };
    let reply = ask(&scene, &token, r#"{"body":"Question from the prior turn"}"#).await;
    assert_eq!(
        reply.0, 409,
        "T-{second} was returned to the queue: {reply:?}"
    );
}

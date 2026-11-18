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
    assert!(words.contains("T-1") && words.contains("moved"), "{words}");
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
    assert_eq!(said.0, 409, "{said:?}");
    assert_eq!(said.1["error"], "question-moved");
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
    for ending in ["done", "accepted", "failed", "cancelled"] {
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
            "T-1 is cancelled: nothing more of it goes to @chief"
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
async fn review_01_an_ask_authenticated_on_one_turn_does_not_belong_to_the_follow_up() {
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
async fn review_06_a_fresh_idle_session_can_ask_a_taskless_question() {
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
    let token = scene
        .context
        .credentials
        .issue(scene.project.id, session)
        .unwrap();
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
        (token, second)
    };
    let (status, body) = ask(&scene, &token, r#"{"body":"About the new task"}"#).await;
    assert_eq!(status, 201, "{body}");
    assert_eq!(body["message"]["task"], second);
}

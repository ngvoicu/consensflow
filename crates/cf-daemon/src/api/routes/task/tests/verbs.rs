//! The verbs that move a task (`done`, `accept`, `reopen`, `cancel`, `pause`,
//! `resume`, `tell`): the body before who may, who may, what the ledger is
//! asked and what it says, the wake-ups, and the view of the task a request
//! acts on once its body is in. Where a test says what Node answered, it is
//! what the real API answered the same request (the probe that printed those
//! answers went with Node's API).

use hyper::Method;
use serde_json::json;

use super::{post, refused};
use crate::api::routes::tests::support::{
    open_task, read_while, state_of, through, with_body, working_task,
};
use crate::testing::scene;

#[tokio::test]
async fn the_body_is_read_before_it_is_decided_who_may_do_what() {
    let scene = scene();
    let number = working_task(&scene);
    let huge = "x".repeat(3 * 1024 * 1024);
    for verb in [
        "done", "accept", "pause", "resume", "reopen", "cancel", "tell",
    ] {
        // zeus did not give the task and, but for `done`, is no one to move it.
        for (body, status, code) in [
            ("not json", 400, "invalid-json"),
            ("[]", 400, "invalid-json"),
            (huge.as_str(), 413, "too-large"),
        ] {
            let said = post(&scene, &scene.zeus, &format!("{number}/{verb}"), body).await;
            assert_eq!(
                (said.0, said.1["error"].as_str().unwrap()),
                (status, code),
                "{verb}"
            );
        }
    }
    assert_eq!(scene.kicks.get(), 0);
}

#[tokio::test]
async fn a_window_that_is_no_coordinator_is_told_which_verb_it_may_not() {
    let scene = scene();
    let number = working_task(&scene);
    for verb in ["accept", "pause", "resume", "reopen", "cancel", "tell"] {
        let said = post(&scene, &scene.zeus, &format!("{number}/{verb}"), "{}").await;
        assert_eq!(
            refused(&said),
            (
                403,
                "not-a-coordinator",
                format!("only the chief or @chief may {verb} T-{number}").as_str()
            ),
            "{verb}"
        );
    }
    assert_eq!(state_of(&scene, number), "working");
    assert_eq!(scene.kicks.get(), 0);
}

#[tokio::test]
async fn only_the_assignee_finishes_a_task_and_its_result_is_its_words() {
    let scene = scene();
    let number = working_task(&scene);
    // The chief gave it and is not the one to finish it.
    let said = post(
        &scene,
        &scene.chief,
        &format!("{number}/done"),
        r#"{"body":"Done."}"#,
    )
    .await;
    assert_eq!(
        refused(&said),
        (
            403,
            "not-yours",
            format!("T-{number} is assigned to @zeus").as_str()
        )
    );
    // Words that are no words are the ledger's to refuse, after the check.
    for body in ["{}", r#"{"body":5}"#, r#"{"body":"  "}"#] {
        let said = post(&scene, &scene.zeus, &format!("{number}/done"), body).await;
        assert_eq!(
            refused(&said),
            (
                400,
                "invalid-text",
                "body must be text, not empty, at most 1000000 characters"
            ),
            "{body}"
        );
    }
    assert_eq!(scene.kicks.get(), 0);
    let (status, said) = post(
        &scene,
        &scene.zeus,
        &format!("{number}/done"),
        r#"{"body":"Done."}"#,
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(said["task"]["state"], "done");
    assert_eq!(
        said.as_object().unwrap().keys().collect::<Vec<_>>(),
        ["task"]
    );
    assert_eq!(scene.kicks.get(), 1);
    // A task nobody has is assigned to @null.
    let open = open_task(&scene);
    let said = post(
        &scene,
        &scene.zeus,
        &format!("{open}/done"),
        r#"{"body":"x"}"#,
    )
    .await;
    assert_eq!(
        refused(&said),
        (
            403,
            "not-yours",
            format!("T-{open} is assigned to @null").as_str()
        )
    );
}

#[tokio::test]
async fn the_chief_moves_a_task_with_the_verbs_and_each_wakes_the_dispatcher_once() {
    let scene = scene();
    let number = working_task(&scene);
    let route = |verb: &str| format!("{number}/{verb}");
    let (status, said) = post(&scene, &scene.chief, &route("pause"), "").await;
    assert_eq!(
        (status, said["task"]["state"].clone()),
        (200, json!("paused")),
        "{said}"
    );
    assert_eq!(scene.kicks.get(), 1);
    // The words that resume it are asked for, and are words.
    for body in ["{}", r#"{"body":5}"#, r#"{"body":null}"#] {
        let said = post(&scene, &scene.chief, &route("resume"), body).await;
        assert_eq!(said.1["error"], "invalid-text", "{body}");
    }
    assert_eq!(scene.kicks.get(), 1);
    let (status, said) = post(
        &scene,
        &scene.chief,
        &route("resume"),
        r#"{"body":"Go on"}"#,
    )
    .await;
    assert_eq!(status, 200, "{said}");
    assert_ne!(said["task"]["state"], "paused");
    assert_eq!(scene.kicks.get(), 2);
    let (status, said) = post(&scene, &scene.chief, &route("cancel"), "{}").await;
    assert_eq!(
        (status, said["task"]["state"].clone()),
        (200, json!("cancelled"))
    );
    assert_eq!(scene.kicks.get(), 3);
    // What the ledger refuses is said as it said it, and wakes nothing.
    let said = post(&scene, &scene.chief, &route("accept"), "{}").await;
    assert_eq!(
        refused(&said),
        (
            409,
            "invalid-transition",
            format!("cannot accept T-{number}: it is cancelled").as_str()
        )
    );
    assert_eq!(scene.kicks.get(), 3);
}

#[tokio::test]
async fn a_result_is_accepted_or_the_task_is_given_back_with_the_chief_s_words() {
    let scene = scene();
    let number = working_task(&scene);
    post(
        &scene,
        &scene.zeus,
        &format!("{number}/done"),
        r#"{"body":"Done."}"#,
    )
    .await;
    let said = post(&scene, &scene.chief, &format!("{number}/reopen"), "{}").await;
    assert_eq!(
        said.1["error"], "invalid-text",
        "the words are asked for first"
    );
    let (status, said) = post(
        &scene,
        &scene.chief,
        &format!("{number}/reopen"),
        r#"{"body":"Also the lexer"}"#,
    )
    .await;
    assert_eq!(status, 200, "{said}");
    assert_ne!(said["task"]["state"], "done");
    let (status, said) = post(&scene, &scene.chief, &format!("{number}/accept"), "{}").await;
    // Back with its window, a task is not a result to accept.
    assert_eq!(status, 409, "{said}");
}

#[tokio::test]
async fn a_post_to_the_transcript_is_the_reopen_that_node_s_last_branch_made_of_any_other_word() {
    let scene = scene();
    let number = open_task(&scene);
    // The body is read, and the words are the ledger's first check.
    let said = post(&scene, &scene.chief, &format!("{number}/transcript"), "").await;
    assert_eq!(said.1["error"], "invalid-text");
    let said = post(
        &scene,
        &scene.chief,
        &format!("{number}/transcript"),
        r#"{"body":"again"}"#,
    )
    .await;
    assert_eq!(
        refused(&said),
        (
            409,
            "invalid-transition",
            format!("cannot reopen T-{number}: it is open").as_str()
        )
    );
    // The word in the refusal of a window that may not is its own.
    let said = post(&scene, &scene.zeus, &format!("{number}/transcript"), "{}").await;
    assert_eq!(
        refused(&said),
        (
            403,
            "not-a-coordinator",
            format!("only the chief or @chief may transcript T-{number}").as_str()
        )
    );
}

#[tokio::test]
async fn a_tell_pauses_the_task_for_the_question_it_puts_to_the_window() {
    let scene = scene();
    let number = working_task(&scene);
    let (status, said) = post(
        &scene,
        &scene.chief,
        &format!("{number}/tell"),
        r#"{"body":"Stop: use v2"}"#,
    )
    .await;
    assert_eq!(status, 200, "{said}");
    assert_eq!(
        said.as_object().unwrap().keys().collect::<Vec<_>>(),
        ["message", "task"]
    );
    assert_eq!(said["message"]["kind"], "question");
    assert_eq!(said["message"]["recipient"], "zeus");
    assert_eq!(said["message"]["preview"], "Stop: use v2");
    assert_eq!(said["task"]["state"], "paused", "the task as it is now");
    assert_eq!(scene.kicks.get(), 1);
    // A paused task has a window still, and is told again.
    let (status, _) = post(
        &scene,
        &scene.chief,
        &format!("{number}/tell"),
        r#"{"body":"And the tests"}"#,
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(scene.kicks.get(), 2);
}

#[tokio::test]
async fn a_task_with_no_window_is_not_told_and_says_what_it_is() {
    let scene = scene();
    let open = open_task(&scene);
    let said = post(
        &scene,
        &scene.chief,
        &format!("{open}/tell"),
        r#"{"body":"Hey"}"#,
    )
    .await;
    assert_eq!(
        refused(&said),
        (
            409,
            "no-window",
            format!("T-{open} has no window to tell: it is open").as_str()
        )
    );
    // Not even the words are looked at.
    let said = post(&scene, &scene.chief, &format!("{open}/tell"), "{}").await;
    assert_eq!(said.1["error"], "no-window");
    assert_eq!(scene.kicks.get(), 0);
}

#[tokio::test]
async fn words_that_are_no_words_are_refused_after_the_pause_and_the_pause_goes_with_them() {
    let scene = scene();
    let number = working_task(&scene);
    for body in ["{}", r#"{"body":5}"#, r#"{"body":""}"#] {
        let said = post(&scene, &scene.chief, &format!("{number}/tell"), body).await;
        assert_eq!(
            refused(&said),
            (
                400,
                "invalid-text",
                "body must be text, not empty, at most 1000000 characters"
            ),
            "{body}"
        );
        // The ledger asked for the pause first and undid it with the refusal.
        assert_eq!(state_of(&scene, number), "working", "{body}");
    }
    assert_eq!(scene.kicks.get(), 0);
}

#[tokio::test]
async fn a_task_is_acted_on_as_it_was_read_before_the_body_came_in() {
    // The task is read first, and the body is awaited after: what a tell
    // checks is the task as it was, working with a window; what the ledger
    // does is done to the task as it is, cancelled meanwhile, and it says so.
    let scene = scene();
    let number = working_task(&scene);
    let context = std::rc::Rc::clone(&scene.context);
    let cancelled = read_while(r#"{"body":"Stop"}"#, move || {
        context
            .ledger
            .borrow_mut()
            .cancel_task(1, number, "chief")
            .unwrap();
    });
    let asked = with_body(
        Method::POST,
        &format!("/api/tasks/{number}/tell"),
        &scene.chief,
        cancelled,
    );
    let said = through(&scene, asked).await;
    assert_eq!(
        refused(&said),
        (
            409,
            "invalid-transition",
            format!("cannot pause T-{number}: it is cancelled").as_str()
        ),
        "a task read afresh would have said it has no window"
    );
    assert_eq!(scene.kicks.get(), 0);
}

#[tokio::test]
async fn the_one_who_may_move_a_task_is_decided_on_the_task_as_it_was_read() {
    // zeus is the assignee of the task as it was read, so it may finish it;
    // the task is cancelled while the words come in, and the ledger refuses.
    let scene = scene();
    let number = working_task(&scene);
    let context = std::rc::Rc::clone(&scene.context);
    let body = read_while(r#"{"body":"Done."}"#, move || {
        context
            .ledger
            .borrow_mut()
            .cancel_task(1, number, "chief")
            .unwrap();
    });
    let asked = with_body(
        Method::POST,
        &format!("/api/tasks/{number}/done"),
        &scene.zeus,
        body,
    );
    let said = through(&scene, asked).await;
    assert_eq!(
        refused(&said),
        (
            409,
            "invalid-transition",
            format!("cannot record a result for T-{number}: it is cancelled").as_str()
        )
    );
}

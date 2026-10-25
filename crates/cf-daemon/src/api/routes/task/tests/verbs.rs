//! The verbs that move a task (`done`, `accept`, `reopen`, `edit`, `cancel`,
//! `pause`, `resume`, `tell`): the body before who may, who may, what the ledger is
//! asked and what it says, the wake-ups, and the view of the task a request
//! acts on once its body is in. Where a test says what Node answered, it is
//! what the real API answered the same request (the probe that printed those
//! answers went with Node's API).

use cf_base::refusal::Refusal;
use hyper::Method;
use serde_json::json;

use super::{get, post, refused};
use crate::api::routes::tests::support::{
    open_task, queued_task, read_while, state_of, through, with_body, working_task,
};
use crate::testing::scene;

#[tokio::test]
async fn the_body_is_read_before_it_is_decided_who_may_do_what() {
    let scene = scene();
    let number = working_task(&scene);
    let huge = "x".repeat(3 * 1024 * 1024);
    for verb in [
        "done", "accept", "pause", "resume", "reopen", "edit", "cancel", "finish", "tell",
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
    for verb in [
        "accept", "pause", "resume", "reopen", "edit", "cancel", "finish", "tell",
    ] {
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
async fn the_chief_asks_the_engine_to_finish_a_task_and_what_it_refuses_is_refused_in_its_words() {
    // Only the engine reads the window: it is asked, in the chief's name, and the route
    // answers with the task done, the dispatcher woken for what follows.
    let scene = scene();
    let number = working_task(&scene);
    let (status, said) = post(&scene, &scene.chief, &format!("{number}/finish"), "").await;
    assert_eq!(status, 200, "{said}");
    assert_eq!(
        said.as_object().unwrap().keys().collect::<Vec<_>>(),
        ["task", "jobs", "agents"]
    );
    assert_eq!(said["task"]["number"], number);
    assert_eq!(said["task"]["state"], "done");
    assert_eq!((&said["jobs"], &said["agents"]), (&json!(0), &json!(0)));
    assert_eq!(
        scene.finishings.asked.borrow().clone(),
        [(scene.project.id, number, "chief".to_owned())]
    );
    assert_eq!(scene.kicks.get(), 1);

    // What the engine refuses is said as it said it, with its status, and wakes nothing.
    let words = format!(
        "cannot finish T-{number}: @zeus has written no complete answer since its latest message about it; wait for one, or cancel it"
    );
    *scene.finishings.refusal.borrow_mut() =
        Some(Refusal::with_status("no-answer", words.clone(), 409));
    let said = post(&scene, &scene.chief, &format!("{number}/finish"), "{}").await;
    assert_eq!(refused(&said), (409, "no-answer", words.as_str()));
    assert_eq!(scene.kicks.get(), 1);

    // A task there is none of is the route's to refuse, before the engine is asked.
    let said = post(&scene, &scene.chief, "99/finish", "").await;
    assert_eq!(said.1["error"], "unknown-task");
    assert_eq!(scene.finishings.asked.borrow().len(), 2);

    // The jobs and agents its harness still counted, which finishing ends none of, are
    // counted, for `cf` to say.
    *scene.finishings.refusal.borrow_mut() = None;
    scene.finishings.jobs.set(1);
    scene.finishings.agents.set(2);
    let (status, said) = post(&scene, &scene.chief, &format!("{number}/finish"), "").await;
    assert_eq!(
        (status, &said["jobs"], &said["agents"]),
        (200, &json!(1), &json!(2)),
        "{said}"
    );
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
async fn a_queued_task_is_told_with_no_pause_and_its_words_wait_for_its_window_after_its_brief() {
    let scene = scene();
    let (number, brief) = queued_task(&scene);
    let (status, said) = post(
        &scene,
        &scene.chief,
        &format!("{number}/tell"),
        r#"{"body":"Use the v2 grammar"}"#,
    )
    .await;
    assert_eq!(status, 200, "{said}");
    assert_eq!(
        [
            &said["message"]["kind"],
            &said["message"]["state"],
            &said["message"]["recipient"],
            &said["task"]["state"],
        ],
        [
            &json!("note"),
            &json!("queued"),
            &json!("zeus"),
            &json!("queued")
        ],
        "kept for the window, and nothing paused"
    );
    assert_eq!(scene.kicks.get(), 1);
    let told = said["message"]["id"].as_i64().unwrap();
    let begun = scene
        .context
        .ledger
        .borrow_mut()
        .begin_delivery(brief)
        .unwrap();
    assert_eq!(
        begun.carried.iter().map(|row| row.id).collect::<Vec<_>>(),
        [told],
        "it rides in the brief's paste"
    );

    // No words are none, and nothing is kept for them.
    let (number, _) = queued_task(&scene);
    let said = post(&scene, &scene.chief, &format!("{number}/tell"), "{}").await;
    assert_eq!(said.1["error"], "invalid-text");
    assert_eq!(state_of(&scene, number), "queued");
}

#[tokio::test]
async fn a_tell_goes_the_way_its_task_is_on_once_its_words_are_in() {
    // Queued when it was read; its brief arrives while the words come in: the
    // window has it, and is stopped and asked.
    let scene = scene();
    let (number, brief) = queued_task(&scene);
    let context = std::rc::Rc::clone(&scene.context);
    let arrived = read_while(r#"{"body":"Use the v2 grammar"}"#, move || {
        let mut ledger = context.ledger.borrow_mut();
        ledger.begin_delivery(brief).unwrap();
        ledger
            .confirm_delivery(brief, Some(&json!({ "item": "test" })))
            .unwrap();
    });
    let asked = with_body(
        Method::POST,
        &format!("/api/tasks/{number}/tell"),
        &scene.chief,
        arrived,
    );
    let (status, said) = through(&scene, asked).await;
    assert_eq!(status, 200, "{said}");
    assert_eq!(
        [&said["message"]["kind"], &said["task"]["state"]],
        [&json!("question"), &json!("paused")]
    );

    // At work when it was read; paused and resumed while the words come in: it
    // is queued for its window again, and the words are kept for it.
    let number = working_task(&scene);
    let context = std::rc::Rc::clone(&scene.context);
    let queued_again = read_while(r#"{"body":"And keep the old tests"}"#, move || {
        let mut ledger = context.ledger.borrow_mut();
        ledger.pause_task(1, number, Some("human"), None).unwrap();
        ledger
            .resume_task(1, number, Some("human"), "Go on")
            .unwrap();
    });
    let asked = with_body(
        Method::POST,
        &format!("/api/tasks/{number}/tell"),
        &scene.chief,
        queued_again,
    );
    let (status, said) = through(&scene, asked).await;
    assert_eq!(status, 200, "{said}");
    assert_eq!(
        [&said["message"]["kind"], &said["task"]["state"]],
        [&json!("note"), &json!("queued")]
    );
}

#[tokio::test]
async fn a_cancel_says_which_of_the_callers_words_its_window_never_had_went_with_it() {
    let scene = scene();
    let (number, _) = queued_task(&scene);
    let mut told = Vec::new();
    for words in ["Use the v2 grammar", "And keep the old tests"] {
        let body = json!({ "body": words }).to_string();
        let (_, said) = post(&scene, &scene.chief, &format!("{number}/tell"), &body).await;
        told.push(said["message"]["id"].clone());
    }
    let (status, said) = post(&scene, &scene.chief, &format!("{number}/cancel"), "{}").await;
    assert_eq!(status, 200, "{said}");
    assert_eq!(said["task"]["state"], "cancelled");
    assert_eq!(said["dropped"], json!(told));

    // A cancel that took none says nothing of it.
    let (number, _) = queued_task(&scene);
    let (_, said) = post(&scene, &scene.chief, &format!("{number}/cancel"), "{}").await;
    assert_eq!(said.get("dropped"), None, "{said}");
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

#[tokio::test]
async fn the_chief_replaces_the_brief_of_a_task_no_window_has_and_is_refused_for_one_that_does() {
    let scene = scene();
    let open = open_task(&scene);
    // The new brief is asked for, and is words.
    for body in ["{}", r#"{"body":5}"#, r#"{"body":"  "}"#] {
        let said = post(&scene, &scene.chief, &format!("{open}/edit"), body).await;
        assert_eq!(said.1["error"], "invalid-text", "{body}");
    }
    assert_eq!(scene.kicks.get(), 0);

    let (status, said) = post(
        &scene,
        &scene.chief,
        &format!("{open}/edit"),
        r#"{"body":"A new brief\nin two lines"}"#,
    )
    .await;
    assert_eq!(status, 200, "{said}");
    assert_eq!(said["task"]["title"], "A new brief");
    assert_eq!(said["task"]["state"], "open");
    assert_eq!(scene.kicks.get(), 1);
    let (_, read) = get(&scene, &scene.chief, &open.to_string()).await;
    assert_eq!(read["task"]["body"], "A new brief\nin two lines");

    // A task its window has is told, never edited; nothing wakes.
    let working = working_task(&scene);
    let kicks = scene.kicks.get();
    let said = post(
        &scene,
        &scene.chief,
        &format!("{working}/edit"),
        r#"{"body":"Another"}"#,
    )
    .await;
    assert_eq!(
        refused(&said),
        (
            409,
            "already-given",
            format!("cannot edit T-{working}: it was given to @zeus, which has the brief; tell its window: cf tell T-{working} \"…\"").as_str()
        )
    );
    assert_eq!(scene.kicks.get(), kicks);
}

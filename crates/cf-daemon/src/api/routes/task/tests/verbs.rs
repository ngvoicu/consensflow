//! The verbs that move a task (`done`, `accept`, `reopen`, `edit`, `cancel`,
//! `pause`, `resume`, `tell`): the body before who may, who may, what the ledger is
//! asked and what it says, the wake-ups, and the view of the task a request
//! acts on once its body is in. Where a test says what Node answered, it is
//! what the real API answered the same request (the probe that printed those
//! answers went with Node's API).

use cf_base::refusal::Refusal;
use hyper::Method;
use serde_json::json;

use super::{api, get, post, refused};
use crate::api::routes::tests::support::{
    open_task, queued_task, read_while, state_of, through, with_body, working_task,
};
use crate::testing::scene;

#[tokio::test]
async fn own_hands_leaves_existing_tasks_and_answers_in_their_ordinary_flow() {
    let scene = scene();
    // Both tasks and the question existed before the person changed the switch.
    let working = working_task(&scene);
    let open = open_task(&scene);
    scene
        .context
        .ledger
        .borrow_mut()
        .set_own_hands(scene.project.id, true)
        .unwrap();

    let answer = json!({"question": scene.question.id, "body": "Yes"}).to_string();
    let (status, said) = api(&scene, Method::POST, "/api/answers", &scene.chief, &answer).await;
    assert_eq!(status, 201, "{said}");
    let mut told = None;
    for (number, verb, body, state) in [
        (open, "edit", r#"{"body":"An edited brief"}"#, "open"),
        (open, "cancel", "{}", "cancelled"),
        (
            working,
            "tell",
            r#"{"body":"Please check this too"}"#,
            "working",
        ),
        (working, "pause", "{}", "paused"),
        (working, "resume", r#"{"body":"Go on"}"#, "working"),
    ] {
        let (status, said) = post(&scene, &scene.chief, &format!("{number}/{verb}"), body).await;
        assert_eq!(status, 200, "{verb}: {said}");
        if verb == "resume" {
            assert_ne!(said["task"]["state"], "paused", "{verb}: {said}");
        } else {
            assert_eq!(said["task"]["state"], state, "{verb}: {said}");
        }
        if verb == "tell" {
            assert_eq!(said["message"]["kind"], "note", "{said}");
            told = Some(said["message"]["id"].clone());
        }
        if verb == "pause" {
            assert_eq!(
                said["toldWithdrawn"],
                json!([told.take().unwrap()]),
                "{said}"
            );
        }
    }
    let (status, said) = post(&scene, &scene.chief, &format!("{working}/cancel"), "{}").await;
    assert_eq!(status, 200, "{said}");

    let other = crate::testing::scene();
    let finished = working_task(&other);
    other
        .context
        .ledger
        .borrow_mut()
        .set_own_hands(other.project.id, true)
        .unwrap();
    let (status, said) = post(
        &other,
        &other.zeus,
        &format!("{finished}/done"),
        r#"{"body":"Done"}"#,
    )
    .await;
    assert_eq!(status, 200, "{said}");
    let (status, said) = post(
        &other,
        &other.chief,
        &format!("{finished}/reopen"),
        r#"{"body":"One more thing"}"#,
    )
    .await;
    assert_eq!(status, 200, "{said}");
    let (status, said) = post(&other, &other.chief, &format!("{finished}/cancel"), "{}").await;
    assert_eq!(status, 200, "{said}");

    let accepted = crate::testing::scene();
    let finished = working_task(&accepted);
    accepted
        .context
        .ledger
        .borrow_mut()
        .set_own_hands(accepted.project.id, true)
        .unwrap();
    let (status, said) = post(
        &accepted,
        &accepted.zeus,
        &format!("{finished}/done"),
        r#"{"body":"Done"}"#,
    )
    .await;
    assert_eq!(status, 200, "{said}");
    let (status, said) = post(
        &accepted,
        &accepted.chief,
        &format!("{finished}/accept"),
        "{}",
    )
    .await;
    assert_eq!(status, 200, "{said}");
    assert_eq!(said["task"]["state"], "accepted");
}

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
async fn the_chief_accepts_a_failed_task_on_the_words_of_what_it_checked_and_they_are_its_result() {
    // T-1111: a designer's session died after it had saved both files, and `cf task accept`
    // was refused. The ledger decides on the task as it is when the verb comes: a failed one
    // takes the words, a done one is accepted as it was.
    let scene = scene();
    let number = working_task(&scene);
    scene
        .context
        .ledger
        .borrow_mut()
        .fail_task(scene.project.id, number, "Selected model is at capacity")
        .unwrap();
    let route = format!("{number}/accept");

    // Without words it is refused in the words that say what to type, and wakes nothing.
    for body in ["{}", r#"{"body":"  "}"#] {
        let said = post(&scene, &scene.chief, &route, body).await;
        assert_eq!(
            refused(&said),
            (
                400,
                "checked-needed",
                format!(
                    "T-{number} failed: accepting it takes the words of what you checked, which are its result: cf task accept T-{number} \"what I checked\""
                )
                .as_str()
            ),
            "{body}"
        );
    }
    assert_eq!(state_of(&scene, number), "failed");
    assert_eq!(scene.kicks.get(), 0);

    // A window that is no coordinator may not, with or without words.
    let said = post(&scene, &scene.zeus, &route, r#"{"body":"It is fine"}"#).await;
    assert_eq!(said.0, 403, "{said:?}");
    assert_eq!(state_of(&scene, number), "failed");

    let (status, said) = post(
        &scene,
        &scene.chief,
        &route,
        r#"{"body":"Both files are saved and whole"}"#,
    )
    .await;
    assert_eq!(
        (status, said["task"]["state"].clone()),
        (200, json!("accepted")),
        "{said}"
    );
    assert_eq!(scene.kicks.get(), 1, "what waits for it is woken");
    let (status, thread) = get(&scene, &scene.chief, &number.to_string()).await;
    assert_eq!(status, 200);
    let results: Vec<_> = thread["task"]["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|message| message["kind"] == "result")
        .collect();
    assert_eq!(results.len(), 1, "{thread}");
    assert_eq!(results[0]["body"], "Both files are saved and whole");
    assert_eq!(results[0]["sender"], "chief");
}

#[tokio::test]
async fn a_done_tasks_accept_is_as_it_was_whatever_words_come_with_it() {
    let scene = scene();
    let number = working_task(&scene);
    let (status, _) = post(
        &scene,
        &scene.zeus,
        &format!("{number}/done"),
        r#"{"body":"Done."}"#,
    )
    .await;
    assert_eq!(status, 200);
    let (status, said) = post(
        &scene,
        &scene.chief,
        &format!("{number}/accept"),
        r#"{"body":"Looks right"}"#,
    )
    .await;
    assert_eq!(
        (status, said["task"]["state"].clone()),
        (200, json!("accepted")),
        "{said}"
    );
    let (_, thread) = get(&scene, &scene.chief, &number.to_string()).await;
    let results: Vec<_> = thread["task"]["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|message| message["kind"] == "result")
        .map(|message| message["body"].clone())
        .collect();
    assert_eq!(
        results,
        [json!("Done.")],
        "the worker's result stays the result"
    );
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
async fn a_tell_to_a_task_at_work_keeps_its_words_for_the_window_and_stops_nothing() {
    let scene = scene();
    let number = working_task(&scene);
    let (status, said) = post(
        &scene,
        &scene.chief,
        &format!("{number}/tell"),
        r#"{"body":"Use v2"}"#,
    )
    .await;
    assert_eq!(status, 200, "{said}");
    assert_eq!(
        said.as_object().unwrap().keys().collect::<Vec<_>>(),
        ["message", "task"]
    );
    assert_eq!(said["message"]["kind"], "note");
    assert_eq!(said["message"]["recipient"], "zeus");
    assert_eq!(said["message"]["preview"], "Use v2");
    assert_eq!(said["task"]["state"], "working", "the task is not stopped");
    assert_eq!(scene.kicks.get(), 1);
    // Told again, it is kept again.
    let (status, said) = post(
        &scene,
        &scene.chief,
        &format!("{number}/tell"),
        r#"{"body":"And the tests"}"#,
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(said["task"]["state"], "working");
    assert_eq!(scene.kicks.get(), 2);
}

#[tokio::test]
async fn a_pause_with_words_pauses_the_task_for_the_question_it_puts_to_the_window() {
    let scene = scene();
    let number = working_task(&scene);
    let (status, said) = post(
        &scene,
        &scene.chief,
        &format!("{number}/pause"),
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
    // A paused task has a window still, and is asked again at once.
    let (status, said) = post(
        &scene,
        &scene.chief,
        &format!("{number}/pause"),
        r#"{"body":"And the tests"}"#,
    )
    .await;
    assert_eq!(status, 200, "{said}");
    assert_eq!(said["message"]["kind"], "question");
    assert_eq!(scene.kicks.get(), 2);
}

#[tokio::test]
async fn a_pause_with_or_without_words_names_the_tell_it_withdraws() {
    for words in [r#"{"body":"Stop: use v2"}"#, "{}"] {
        let scene = scene();
        let number = working_task(&scene);
        let (_, told) = post(
            &scene,
            &scene.chief,
            &format!("{number}/tell"),
            r#"{"body":"Use v2"}"#,
        )
        .await;
        let id = told["message"]["id"].as_i64().expect("the tell's id");
        let (status, said) = post(&scene, &scene.chief, &format!("{number}/pause"), words).await;
        assert_eq!(status, 200, "{said}");
        assert_eq!(said["task"]["state"], "paused", "{words}");
        assert_eq!(said["toldWithdrawn"], json!([id]), "{words}: {said}");
    }
}

#[tokio::test]
async fn a_pause_of_a_queued_task_names_the_tell_it_withdraws_and_not_the_brief() {
    let scene = scene();
    let (number, brief) = queued_task(&scene);
    let (_, told) = post(
        &scene,
        &scene.chief,
        &format!("{number}/tell"),
        r#"{"body":"Use v2"}"#,
    )
    .await;
    let id = told["message"]["id"].as_i64().expect("the tell's id");
    let (status, said) = post(&scene, &scene.chief, &format!("{number}/pause"), "{}").await;
    assert_eq!(status, 200, "{said}");
    assert_eq!(said["toldWithdrawn"], json!([id]), "{said}");
    let ledger = scene.context.ledger.borrow();
    for cancelled in [id, brief] {
        assert_eq!(
            ledger.message(cancelled).unwrap().unwrap().state,
            "cancelled"
        );
    }
}

#[tokio::test]
async fn a_pause_with_words_is_refused_for_a_queued_or_an_open_task_and_names_the_tell() {
    let scene = scene();
    let (queued, _) = queued_task(&scene);
    let open = open_task(&scene);
    for (number, state) in [(queued, "queued"), (open, "open")] {
        let said = post(
            &scene,
            &scene.chief,
            &format!("{number}/pause"),
            r#"{"body":"Stop"}"#,
        )
        .await;
        assert_eq!(
            refused(&said),
            (
                409,
                "no-turn-to-stop",
                format!(
                    "T-{number} is {state}: its window has no turn to stop. To put words to it: cf tell T-{number} \"…\""
                )
                .as_str()
            )
        );
        assert_eq!(state_of(&scene, number), state);
    }
    assert_eq!(scene.kicks.get(), 0);
}

#[tokio::test]
async fn a_task_that_is_over_is_not_told_and_says_what_it_is() {
    let scene = scene();
    let (number, _) = queued_task(&scene);
    scene
        .context
        .ledger
        .borrow_mut()
        .cancel_task(1, number, "chief")
        .unwrap();
    let said = post(
        &scene,
        &scene.chief,
        &format!("{number}/tell"),
        r#"{"body":"Hey"}"#,
    )
    .await;
    assert_eq!(
        refused(&said),
        (
            409,
            "no-window",
            format!("T-{number} has no window to tell: it is cancelled").as_str()
        )
    );
    // Not even the words are looked at.
    let said = post(&scene, &scene.chief, &format!("{number}/tell"), "{}").await;
    assert_eq!(said.1["error"], "no-window");
    assert_eq!(scene.kicks.get(), 0);
}

#[tokio::test]
async fn an_open_task_is_told_with_no_pause_and_its_words_wait_in_its_brief_for_the_window_that_takes_it(
) {
    let scene = scene();
    let open = open_task(&scene);
    let (status, said) = post(
        &scene,
        &scene.chief,
        &format!("{open}/tell"),
        r#"{"body":"Use the v2 grammar"}"#,
    )
    .await;
    assert_eq!(status, 200, "{said}");
    assert_eq!(
        said.as_object().unwrap().keys().collect::<Vec<_>>(),
        ["task"],
        "no message: no window has the words yet"
    );
    assert_eq!(said["task"]["state"], "open", "nothing is paused or queued");
    assert_eq!(state_of(&scene, open), "open");
    let (_, thread) = get(&scene, &scene.chief, &format!("{open}")).await;
    assert_eq!(
        thread["task"]["body"],
        "Lexer\n\nTold by @chief before a window had it: Use the v2 grammar"
    );
    // The window the daemon gives it is given the words with the brief.
    let moved = scene
        .context
        .ledger
        .borrow_mut()
        .assign_task(scene.project.id, open, scene.id("zeus"))
        .unwrap();
    assert_eq!(
        moved.message.unwrap().body,
        "Lexer\n\nTold by @chief before a window had it: Use the v2 grammar"
    );

    // No words are none, and nothing is kept for them.
    let open = open_task(&scene);
    let said = post(&scene, &scene.chief, &format!("{open}/tell"), "{}").await;
    assert_eq!(said.1["error"], "invalid-text");
    let (_, thread) = get(&scene, &scene.chief, &format!("{open}")).await;
    assert_eq!(thread["task"]["body"], "Lexer");
}

#[tokio::test]
async fn an_open_task_a_window_is_given_while_its_words_come_in_is_told_as_a_queued_one() {
    // Open when it was read; the daemon gives it to a window while the words come in:
    // it is queued for it, and the words are kept for the window, after its brief.
    let scene = scene();
    let open = open_task(&scene);
    let member = scene.id("zeus");
    let context = std::rc::Rc::clone(&scene.context);
    let queued = read_while(r#"{"body":"Use the v2 grammar"}"#, move || {
        context
            .ledger
            .borrow_mut()
            .assign_task(1, open, member)
            .unwrap();
    });
    let asked = with_body(
        Method::POST,
        &format!("/api/tasks/{open}/tell"),
        &scene.chief,
        queued,
    );
    let (status, said) = through(&scene, asked).await;
    assert_eq!(status, 200, "{said}");
    assert_eq!(
        [&said["message"]["kind"], &said["task"]["state"]],
        [&json!("note"), &json!("queued")]
    );
}

#[tokio::test]
async fn a_queued_task_taken_back_to_the_board_while_its_words_come_in_is_told_as_an_open_one() {
    // Queued for its window when it was read; its member runs out of quota while the words
    // come in, and the task is back on the board for another: no window has it, and the
    // words are added to its brief.
    let scene = scene();
    let number = open_task(&scene);
    let member = scene.id("zeus");
    scene
        .context
        .ledger
        .borrow_mut()
        .assign_task(1, number, member)
        .unwrap();
    assert_eq!(state_of(&scene, number), "queued");
    let context = std::rc::Rc::clone(&scene.context);
    let released = read_while(r#"{"body":"Use the v2 grammar"}"#, move || {
        context
            .ledger
            .borrow_mut()
            .release_task(1, number, "its member ran out of quota")
            .unwrap();
    });
    let asked = with_body(
        Method::POST,
        &format!("/api/tasks/{number}/tell"),
        &scene.chief,
        released,
    );
    let (status, said) = through(&scene, asked).await;
    assert_eq!(status, 200, "{said}");
    assert_eq!(said["task"]["state"], "open");
    assert_eq!(said.get("message"), None, "no window has it to be told");
    let (_, thread) = get(&scene, &scene.chief, &format!("{number}")).await;
    assert!(
        thread["task"]["body"]
            .as_str()
            .unwrap()
            .ends_with("Told by @chief before a window had it: Use the v2 grammar"),
        "{thread}"
    );
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
    // window has it, and the words are kept for it at rest; nothing is stopped.
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
        [&json!("note"), &json!("working")]
    );

    // At work when it was read; paused and resumed while the words come in: it
    // is queued for its window again, and the words are kept for it.
    let scene = self::scene();
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
async fn a_tell_of_a_task_shown_working_for_the_words_that_resumed_it_is_kept_behind_them() {
    // T-1153's F5: at work (or waiting for an answer) on the turn before the words that
    // resumed it, the task is queued for them still. A pause here would withdraw them.
    let scene = scene();
    let number = working_task(&scene);
    let words = {
        let mut ledger = scene.context.ledger.borrow_mut();
        ledger.pause_task(1, number, Some("chief"), None).unwrap();
        let resumed = ledger
            .resume_task(1, number, Some("chief"), "Go on, or say you finished")
            .unwrap();
        ledger.work_while_resumed(1, number).unwrap();
        resumed.message.unwrap().id
    };
    assert_eq!(state_of(&scene, number), "working");
    let (status, said) = post(
        &scene,
        &scene.chief,
        &format!("{number}/tell"),
        r#"{"body":"Also check the lexer"}"#,
    )
    .await;
    assert_eq!(status, 200, "{said}");
    assert_eq!(
        [&said["message"]["kind"], &said["task"]["state"]],
        [&json!("note"), &json!("working")],
        "kept for the window, the task not stopped"
    );
    let words = scene
        .context
        .ledger
        .borrow()
        .message(words)
        .unwrap()
        .unwrap();
    assert_eq!(words.state, "queued", "the resume's words stand");

    // Once the words are in, a tell is kept for the window at rest as before: nothing is
    // stopped.
    {
        let mut ledger = scene.context.ledger.borrow_mut();
        ledger.begin_delivery(words.id).unwrap();
        ledger
            .confirm_delivery(words.id, Some(&json!({ "item": "words" })))
            .unwrap();
    }
    let (status, said) = post(
        &scene,
        &scene.chief,
        &format!("{number}/tell"),
        r#"{"body":"And the parser"}"#,
    )
    .await;
    assert_eq!(status, 200, "{said}");
    assert_eq!(
        [&said["message"]["kind"], &said["task"]["state"]],
        [&json!("note"), &json!("working")]
    );
}

#[tokio::test]
async fn a_pause_with_words_of_a_task_shown_working_for_the_words_that_resumed_it_keeps_them_behind_them(
) {
    // T-1153's F5: at work on the turn before the words that resumed it, the task is queued for
    // them still. A pause here would withdraw them: the words are kept behind them, nothing
    // is stopped.
    let scene = scene();
    let number = working_task(&scene);
    let words = {
        let mut ledger = scene.context.ledger.borrow_mut();
        ledger.pause_task(1, number, Some("chief"), None).unwrap();
        let resumed = ledger
            .resume_task(1, number, Some("chief"), "Go on, or say you finished")
            .unwrap();
        ledger.work_while_resumed(1, number).unwrap();
        resumed.message.unwrap().id
    };
    let (status, said) = post(
        &scene,
        &scene.chief,
        &format!("{number}/pause"),
        r#"{"body":"Also check the lexer"}"#,
    )
    .await;
    assert_eq!(status, 200, "{said}");
    assert_eq!(
        [&said["message"]["kind"], &said["task"]["state"]],
        [&json!("note"), &json!("working")],
        "kept for the window, the task not stopped"
    );
    let words = scene
        .context
        .ledger
        .borrow()
        .message(words)
        .unwrap()
        .unwrap();
    assert_eq!(words.state, "queued", "the resume's words stand");
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
            format!("cannot keep words for the window of T-{number}: it is cancelled").as_str()
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

/// A light worker joins the scene's staff, which has zeus, a standard one.
fn hera_joins(scene: &crate::testing::Scene) {
    scene
        .context
        .ledger
        .borrow_mut()
        .add_member(
            scene.project.id,
            &cf_ledger::NewMember {
                model: None,
                effort: None,
                model_name: Some("hera".to_owned()),
                harness: "claude-code".to_owned(),
                designer: false,
                roles: vec!["worker".to_owned()],
                tier: "light".to_owned(),
            },
        )
        .unwrap();
}

#[tokio::test]
async fn the_chief_changes_the_tier_of_a_task_no_window_has_with_the_brief_or_without_it() {
    let scene = scene();
    hera_joins(&scene);
    let open = open_task(&scene);
    // The tier alone: its brief stays.
    let (status, said) = post(
        &scene,
        &scene.chief,
        &format!("{open}/edit"),
        r#"{"tier":"light"}"#,
    )
    .await;
    assert_eq!(status, 200, "{said}");
    assert_eq!(said["task"]["tier"], "light");
    assert_eq!(said["task"]["title"], "Lexer");
    assert!(said.get("asked").is_none(), "{said}");
    assert_eq!(
        scene.kicks.get(),
        1,
        "the board is woken: a member may be free for it"
    );
    // The brief too, in the same command.
    let (status, said) = post(
        &scene,
        &scene.chief,
        &format!("{open}/edit"),
        r#"{"tier":"standard","body":"Lexer, the new way"}"#,
    )
    .await;
    assert_eq!(status, 200, "{said}");
    assert_eq!(
        (
            said["task"]["tier"].as_str(),
            said["task"]["title"].as_str()
        ),
        (Some("standard"), Some("Lexer, the new way"))
    );
}

#[tokio::test]
async fn a_tier_nobody_holds_is_the_nearest_and_the_answer_says_which_and_whose() {
    let scene = scene();
    hera_joins(&scene);
    let open = open_task(&scene);
    let (status, said) = post(
        &scene,
        &scene.chief,
        &format!("{open}/edit"),
        r#"{"tier":"critical"}"#,
    )
    .await;
    // The task has no purpose, which critical work names.
    assert_eq!(refused(&(status, said)).1, "purpose-required");
    let (status, said) = post(
        &scene,
        &scene.chief,
        &format!("{open}/edit"),
        r#"{"tier":"complex"}"#,
    )
    .await;
    assert_eq!(status, 200, "{said}");
    assert_eq!(said["task"]["tier"], "standard");
    assert_eq!(said["asked"], "complex");
    assert_eq!(said["heldBy"], json!(["zeus"]));
}

#[tokio::test]
async fn a_tier_that_is_no_tier_and_a_brief_that_is_no_words_are_refused_before_anything_changes() {
    let scene = scene();
    let open = open_task(&scene);
    for (body, code) in [
        (r#"{"tier":"huge"}"#, "invalid-tier"),
        (r#"{"tier":5}"#, "invalid-tier"),
        (r#"{"tier":"light","body":5}"#, "invalid-text"),
        (r#"{"tier":"light","body":"  "}"#, "invalid-text"),
        (r#"{"tier":null}"#, "invalid-text"),
    ] {
        let said = post(&scene, &scene.chief, &format!("{open}/edit"), body).await;
        assert_eq!(said.1["error"], code, "{body}: {said:?}");
    }
    assert_eq!(scene.kicks.get(), 0);
    let (_, read) = get(&scene, &scene.chief, &open.to_string()).await;
    assert_eq!(
        (read["task"]["tier"].as_str(), read["task"]["body"].as_str()),
        (Some("standard"), Some("Lexer"))
    );
}

#[tokio::test]
async fn the_chief_changes_what_a_task_needs_and_what_waits_for_it_by_a_list_or_by_adding_and_removing(
) {
    let scene = scene();
    let (a, b, c) = (open_task(&scene), open_task(&scene), open_task(&scene));
    let edit = format!("{c}/edit");
    let (status, said) = post(
        &scene,
        &scene.chief,
        &edit,
        &format!(r#"{{"needs":[{a},{b}]}}"#),
    )
    .await;
    assert_eq!(status, 200, "{said}");
    assert_eq!(said["task"]["blockedBy"], json!([a, b]));
    let (status, said) = post(
        &scene,
        &scene.chief,
        &edit,
        &format!(r#"{{"needs":{{"remove":[{a}]}}}}"#),
    )
    .await;
    assert_eq!(status, 200, "{said}");
    assert_eq!(said["task"]["blockedBy"], json!([b]));
    // What waits for it, said back.
    let (status, said) = post(
        &scene,
        &scene.chief,
        &format!("{a}/edit"),
        &format!(r#"{{"before":{{"add":[{b}]}}}}"#),
    )
    .await;
    assert_eq!(status, 200, "{said}");
    assert_eq!(said["waiting"], json!([b]));
    assert_eq!(scene.kicks.get(), 3);
}

#[tokio::test]
async fn a_list_that_is_none_a_task_not_there_and_a_circle_are_refused_in_the_ledgers_words() {
    let scene = scene();
    let (a, b) = (open_task(&scene), open_task(&scene));
    post(
        &scene,
        &scene.chief,
        &format!("{a}/edit"),
        &format!(r#"{{"needs":[{b}]}}"#),
    )
    .await;
    for (route, body, status, code) in [
        (a, r#"{"needs":"T-2"}"#.to_owned(), 400, "invalid-needs"),
        (
            a,
            r#"{"before":{"add":"T-2"}}"#.to_owned(),
            400,
            "invalid-needs",
        ),
        (a, r#"{"needs":[0]}"#.to_owned(), 400, "invalid-needs"),
        (a, r#"{"needs":[99]}"#.to_owned(), 404, "unknown-task"),
        (b, format!(r#"{{"needs":[{a}]}}"#), 409, "circular-needs"),
    ] {
        let said = post(&scene, &scene.chief, &format!("{route}/edit"), &body).await;
        assert_eq!(
            (said.0, said.1["error"].as_str()),
            (status, Some(code)),
            "{body}"
        );
    }
}

/// T-1118: the title is the brief's first line, and a reopen or a resume adds words but
/// keeps it. `title` sets it with any of the three, and `edit` takes it alone, in any
/// state, the brief as it is.
#[tokio::test]
async fn the_chief_gives_a_task_a_new_title_alone_with_edit_in_any_state_and_the_brief_stays() {
    let scene = scene();
    let working = working_task(&scene);
    let kicks = scene.kicks.get();
    let (status, said) = post(
        &scene,
        &scene.chief,
        &format!("{working}/edit"),
        r#"{"title":"Parser, and the lexer too"}"#,
    )
    .await;
    assert_eq!(status, 200, "{said}");
    assert_eq!(said["task"]["title"], "Parser, and the lexer too");
    assert_eq!(said["task"]["state"], "working", "the state is as it was");
    assert_eq!(scene.kicks.get(), kicks + 1);
    let (_, read) = get(&scene, &scene.chief, &working.to_string()).await;
    assert_eq!(read["task"]["title"], "Parser, and the lexer too");
    assert_eq!(read["task"]["body"], "Parser", "its brief is as it was");

    // A task that is over is retitled too.
    post(
        &scene,
        &scene.zeus,
        &format!("{working}/done"),
        r#"{"body":"Done."}"#,
    )
    .await;
    post(&scene, &scene.chief, &format!("{working}/accept"), "{}").await;
    let (status, said) = post(
        &scene,
        &scene.chief,
        &format!("{working}/edit"),
        r#"{"title":"Parser, the lexer and the docs"}"#,
    )
    .await;
    assert_eq!(status, 200, "{said}");
    assert_eq!(said["task"]["title"], "Parser, the lexer and the docs");
    assert_eq!(said["task"]["state"], "accepted");
}

#[tokio::test]
async fn a_new_brief_with_a_title_is_titled_so_and_without_one_by_its_first_line() {
    let scene = scene();
    let open = open_task(&scene);
    let (status, said) = post(
        &scene,
        &scene.chief,
        &format!("{open}/edit"),
        r#"{"body":"A new brief\nin two lines","title":"The new scope"}"#,
    )
    .await;
    assert_eq!(status, 200, "{said}");
    assert_eq!(said["task"]["title"], "The new scope");
    let (_, read) = get(&scene, &scene.chief, &open.to_string()).await;
    assert_eq!(read["task"]["body"], "A new brief\nin two lines");

    let (_, said) = post(
        &scene,
        &scene.chief,
        &format!("{open}/edit"),
        r#"{"body":"Another brief\nin two lines"}"#,
    )
    .await;
    assert_eq!(
        said["task"]["title"], "Another brief",
        "the first line, as it was"
    );

    // A brief a window has is not replaced, and its title is not changed by the refusal.
    let working = working_task(&scene);
    let said = post(
        &scene,
        &scene.chief,
        &format!("{working}/edit"),
        r#"{"body":"Another","title":"Not this"}"#,
    )
    .await;
    assert_eq!(said.1["error"], "already-given");
    let (_, read) = get(&scene, &scene.chief, &working.to_string()).await;
    assert_eq!(read["task"]["title"], "Parser");
}

#[tokio::test]
async fn a_reopen_and_a_resume_take_a_title_and_keep_the_brief() {
    let scene = scene();
    let number = working_task(&scene);
    post(
        &scene,
        &scene.zeus,
        &format!("{number}/done"),
        r#"{"body":"Done."}"#,
    )
    .await;
    let (status, said) = post(
        &scene,
        &scene.chief,
        &format!("{number}/reopen"),
        r#"{"body":"Also the lexer","title":"Parser and lexer"}"#,
    )
    .await;
    assert_eq!(status, 200, "{said}");
    assert_ne!(said["task"]["state"], "done");
    assert_eq!(said["task"]["title"], "Parser and lexer");

    post(&scene, &scene.chief, &format!("{number}/pause"), "{}").await;
    let (status, said) = post(
        &scene,
        &scene.chief,
        &format!("{number}/resume"),
        r#"{"body":"Go on, with the docs too","title":"Parser, lexer and docs"}"#,
    )
    .await;
    assert_eq!(status, 200, "{said}");
    assert_eq!(said["task"]["title"], "Parser, lexer and docs");
    let (_, read) = get(&scene, &scene.chief, &number.to_string()).await;
    assert_eq!(read["task"]["body"], "Parser", "the brief is as it was");
}

#[tokio::test]
async fn a_title_that_says_nothing_is_refused_and_moves_nothing() {
    let scene = scene();
    let number = working_task(&scene);
    post(
        &scene,
        &scene.zeus,
        &format!("{number}/done"),
        r#"{"body":"Done."}"#,
    )
    .await;
    let kicks = scene.kicks.get();
    for title in [r#""""#, r#""  ""#, "5", "null", "[]"] {
        let said = post(
            &scene,
            &scene.chief,
            &format!("{number}/reopen"),
            &format!(r#"{{"body":"More","title":{title}}}"#),
        )
        .await;
        assert_eq!(said.1["error"], "invalid-text", "{title}");
        assert_eq!(state_of(&scene, number), "done", "{title}: nothing moved");
    }
    let said = post(
        &scene,
        &scene.chief,
        &format!("{number}/edit"),
        r#"{"title":"  "}"#,
    )
    .await;
    assert_eq!(said.1["error"], "invalid-text");
    assert_eq!(scene.kicks.get(), kicks);

    // And a window that is no coordinator does not retitle.
    let said = post(
        &scene,
        &scene.zeus,
        &format!("{number}/edit"),
        r#"{"title":"Mine"}"#,
    )
    .await;
    assert_eq!(said.1["error"], "not-a-coordinator");
}

#[tokio::test]
async fn an_accept_says_how_many_jobs_and_agents_the_finish_counted_running() {
    // T-691: the close of the window after the accept ended a parity run the worker had left
    // running, and nothing said so.
    let scene = scene();
    let number = working_task(&scene);
    scene
        .context
        .ledger
        .borrow_mut()
        .record_finished(scene.project.id, number, "Parser done", (1, 2))
        .unwrap();
    let said = post(&scene, &scene.chief, &format!("{number}/accept"), "{}").await;
    assert_eq!(said.0, 200, "{:?}", said.1);
    assert_eq!(said.1["running"], json!({ "jobs": 1, "agents": 2 }));
    assert_eq!(said.1["task"]["state"], "accepted");
}

#[tokio::test]
async fn a_pause_says_what_the_engine_holds_its_task_in_the_background_for() {
    // RS-25: no note is typed of the work a pause leaves running; the pause says it at once.
    let scene = scene();
    let number = working_task(&scene);
    let assignee = scene
        .context
        .ledger
        .borrow()
        .task(scene.project.id, number)
        .unwrap()
        .unwrap()
        .task
        .assignee
        .unwrap();
    let id = scene
        .context
        .ledger
        .borrow()
        .project(scene.project.id)
        .unwrap()
        .unwrap()
        .participants
        .into_iter()
        .find(|participant| participant.handle == assignee)
        .unwrap()
        .id;
    // Another task's hold of the same window says nothing of this one.
    scene
        .finishings
        .held
        .borrow_mut()
        .push((id, number + 1, 4, 4));
    let said = post(&scene, &scene.chief, &format!("{number}/pause"), "{}").await;
    assert_eq!(said.0, 200, "{:?}", said.1);
    assert!(said.1.get("running").is_none(), "{:?}", said.1);
    post(
        &scene,
        &scene.chief,
        &format!("{number}/resume"),
        r#"{"body":"Go on"}"#,
    )
    .await;

    scene.finishings.held.borrow_mut().clear();
    scene.finishings.held.borrow_mut().push((id, number, 1, 3));
    let said = post(&scene, &scene.chief, &format!("{number}/pause"), "{}").await;
    assert_eq!(said.0, 200, "{:?}", said.1);
    assert_eq!(said.1["running"], json!({ "jobs": 1, "agents": 3 }));
    assert_eq!(said.1["task"]["state"], "paused");
}

#[tokio::test]
async fn an_accept_of_a_result_that_left_nothing_running_says_nothing_of_it() {
    let scene = scene();
    // The worker's own result waits for its work to end, so there is nothing to say.
    let given = working_task(&scene);
    let done = post(
        &scene,
        &scene.zeus,
        &format!("{given}/done"),
        r#"{"body":"Done."}"#,
    )
    .await;
    assert_eq!(done.0, 200, "{:?}", done.1);
    let said = post(&scene, &scene.chief, &format!("{given}/accept"), "{}").await;
    assert_eq!(said.0, 200, "{:?}", said.1);
    assert!(said.1.get("running").is_none(), "{:?}", said.1);
}

/// Edit member renames a member and its windows (HMD-12), a window at work too, and the window
/// goes on: its token names its participant by id, so `cf` keeps answering to it under the new
/// name. It reads its task, finishes it, and what the chief answered it before the rename still
/// reaches it; the task is its own under the new name, and the old name is nobody's.
#[tokio::test]
async fn a_window_at_work_keeps_running_cf_with_its_token_after_its_member_is_renamed() {
    let scene = scene();
    let project = scene.project.id;
    let zeus = scene.id("zeus");
    // The chief's task for the standard workers goes to a window of zeus, which takes it up.
    let created = scene
        .context
        .ledger
        .borrow_mut()
        .create_task(
            project,
            &cf_ledger::NewTask {
                from: "chief".to_owned(),
                pool: Some("worker".to_owned()),
                tier: Some("standard".to_owned()),
                body: "Parser".to_owned(),
                ..cf_ledger::NewTask::default()
            },
        )
        .unwrap();
    let number = created.task.number;
    let moved = scene
        .context
        .ledger
        .borrow_mut()
        .assign_task(project, number, zeus)
        .unwrap();
    let window = moved.task.assignee.clone().expect("a window has the task");
    assert!(window.starts_with("zeus-"), "{window}");
    {
        let mut ledger = scene.context.ledger.borrow_mut();
        let brief = moved.message.as_ref().expect("its brief").id;
        ledger.begin_delivery(brief).unwrap();
        ledger
            .confirm_delivery(brief, Some(&json!({ "item": "test" })))
            .unwrap();
    }
    let window_id = scene
        .context
        .ledger
        .borrow()
        .project(project)
        .unwrap()
        .unwrap()
        .participants
        .iter()
        .find(|participant| participant.handle == window)
        .unwrap()
        .id;
    // Its token, as the engine issues it when the window opens; a question it asked on the
    // task, and the chief's answer, which waits for it.
    let token = scene.context.credentials.issue(project, window_id);
    let asked = cf_ledger::NewQuestion {
        from: Some(window.clone()),
        to: "chief".to_owned(),
        body: Some("Which?".to_owned()),
        task: Some(number),
        ..cf_ledger::NewQuestion::default()
    };
    let question = scene
        .context
        .ledger
        .borrow_mut()
        .ask(project, &asked)
        .unwrap();
    scene
        .context
        .ledger
        .borrow_mut()
        .answer(
            question.id,
            scene.id("chief"),
            Some(&json!("Use the first.")),
            None,
        )
        .unwrap();
    // The human edits zeus to another model while the window works.
    let edited = scene
        .context
        .ledger
        .borrow_mut()
        .edit_member(
            project,
            "zeus",
            &cf_ledger::MemberEdit {
                harness: "claude-code".to_owned(),
                model: Some("claude-opus-5-5".to_owned()),
                effort: Some("high".to_owned()),
                model_name: Some("claude-opus-5-5".to_owned()),
                tier: "standard".to_owned(),
            },
        )
        .unwrap();
    assert_eq!(edited.handle, "claude-opus-5-5-high");
    let renamed = format!(
        "claude-opus-5-5-high-{}",
        window.strip_prefix("zeus-").unwrap()
    );
    // The same token: who the window is now, and the task, its own under the new name.
    let (status, said) = api_whoami(&scene, &token).await;
    assert_eq!(status, 200, "{said}");
    assert_eq!(said["participant"]["handle"], renamed.as_str());
    assert_eq!(said["task"]["number"], number);
    assert_eq!(said["task"]["assignee"], renamed.as_str());
    let (status, said) = get(&scene, &token, &number.to_string()).await;
    assert_eq!(status, 200, "{said}");
    assert_eq!(said["task"]["assignee"], renamed.as_str());
    // What the chief answered before reaches it, by its id.
    assert_eq!(
        scene
            .context
            .ledger
            .borrow()
            .next_delivery(window_id)
            .unwrap()
            .map(|message| message.body),
        Some("Use the first.".to_owned())
    );
    // It finishes its task with the token it had.
    let (status, said) = post(
        &scene,
        &token,
        &format!("{number}/done"),
        r#"{"body":"Done."}"#,
    )
    .await;
    assert_eq!(status, 200, "{said}");
    assert_eq!(said["task"]["state"], "done");
    assert_eq!(said["task"]["assignee"], renamed.as_str());
    // The old name is no one's: the board holds no participant by it.
    let names: Vec<String> = scene
        .context
        .ledger
        .borrow()
        .project(project)
        .unwrap()
        .unwrap()
        .participants
        .into_iter()
        .map(|participant| participant.handle)
        .collect();
    assert!(
        !names.iter().any(|name| name == "zeus" || *name == window),
        "{names:?}"
    );
}

async fn api_whoami(scene: &crate::testing::Scene, token: &str) -> (u16, serde_json::Value) {
    crate::api::routes::tests::support::api(scene, Method::GET, "/api/whoami", token, "").await
}

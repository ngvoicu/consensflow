//! `GET /api/tasks` and `POST /api/tasks`: the board as summaries, and a task
//! given. What the words and the order of the checks are is Node's: where a
//! test says what Node answered, it is what the real API answered the same
//! request: `node tests/goldens/daemon/probes/api-corners.mjs` prints it again.

use std::rc::Rc;

use hyper::Method;
use serde_json::{json, Value};

use crate::api::routes::tests::support::{
    api, finished_session_task, open_task, read_while, through, with_body, working_task,
};
use crate::testing::{scene, Scene};

async fn post(scene: &Scene, token: &str, body: &str) -> (u16, Value) {
    api(scene, Method::POST, "/api/tasks", token, body).await
}

/// The refusal a request to give a task is: its status, its code, its words.
fn refused(answered: &(u16, Value)) -> (u16, &str, &str) {
    (
        answered.0,
        answered.1["error"].as_str().unwrap_or("-"),
        answered.1["message"].as_str().unwrap_or("-"),
    )
}

#[tokio::test]
async fn the_board_is_the_open_tasks_and_every_lane_as_summaries() {
    let scene = scene();
    let open = open_task(&scene);
    let working = working_task(&scene);
    let (status, said) = api(&scene, Method::GET, "/api/tasks", &scene.chief, "").await;
    assert_eq!(status, 200);
    assert_eq!(
        said["open"]
            .as_array()
            .unwrap()
            .iter()
            .map(|task| task["number"].as_i64().unwrap())
            .collect::<Vec<_>>(),
        [open]
    );
    let lanes: Vec<(&str, &str, usize)> = said["lanes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|lane| {
            (
                lane["handle"].as_str().unwrap(),
                lane["role"].as_str().unwrap(),
                lane["tasks"].as_array().unwrap().len(),
            )
        })
        .collect();
    assert_eq!(
        lanes,
        [
            ("human", "human", 0),
            ("chief", "chief", 0),
            ("zeus", "worker", 1)
        ]
    );
    let lane = said["lanes"][2]["tasks"][0].to_string();
    assert!(
        lane.starts_with(&format!(
            r#"{{"number":{working},"title":"Parser","state":"working","requester":"chief","assignee":"zeus","pool":null,"tier":null,"needs":[],"blockedBy":[],"updatedAt":""#
        )),
        "{lane}"
    );
    assert_eq!(
        said.as_object().unwrap().keys().collect::<Vec<_>>(),
        ["open", "lanes"]
    );
    assert_eq!(scene.kicks.get(), 0, "a read wakes nothing");
}

#[tokio::test]
async fn a_member_is_refused_before_its_body_is_read() {
    let scene = scene();
    let huge = "x".repeat(3 * 1024 * 1024);
    for body in [
        "not json",
        "[1]",
        huge.as_str(),
        r#"{"tier":"standard","body":"Do it"}"#,
    ] {
        let said = post(&scene, &scene.zeus, body).await;
        assert_eq!(
            refused(&said),
            (
                403,
                "not-a-coordinator",
                "members do not hand out tasks: ask your chief instead (cf ask)"
            )
        );
    }
    assert_eq!(scene.kicks.get(), 0);
}

#[tokio::test]
async fn the_chief_s_body_is_read_next_and_is_one_json_object() {
    let scene = scene();
    for (body, status, code) in [
        ("not json", 400, "invalid-json"),
        ("[]", 400, "invalid-json"),
        ("null", 400, "invalid-json"),
        (&"x".repeat(3 * 1024 * 1024), 413, "too-large"),
        // Nothing is `{}`: and a task with no words is refused by the ledger.
        ("", 400, "invalid-text"),
    ] {
        let said = post(&scene, &scene.chief, body).await;
        assert_eq!((said.0, said.1["error"].as_str().unwrap()), (status, code));
    }
    assert_eq!(scene.kicks.get(), 0);
}

#[tokio::test]
async fn a_task_for_a_tier_opens_on_the_board_and_wakes_the_dispatcher_once() {
    let scene = scene();
    let (status, said) = post(
        &scene,
        &scene.chief,
        r#"{"tier":"standard","body":"Write the parser"}"#,
    )
    .await;
    assert_eq!(status, 201);
    let written = said.to_string();
    assert!(
        written.starts_with(
            r#"{"task":{"number":1,"title":"Write the parser","state":"open","requester":"chief","assignee":null,"pool":"worker","tier":"standard","needs":[],"blockedBy":[],"updatedAt":""#
        ),
        "{written}"
    );
    assert!(
        written.ends_with(r#""},"message":null,"gated":false}"#),
        "{written}"
    );
    assert_eq!(scene.kicks.get(), 1);
}

#[tokio::test]
async fn whether_the_brief_waits_for_the_human_is_read_as_the_board_is_now() {
    let scene = scene();
    scene
        .context
        .ledger
        .borrow_mut()
        .set_gate(scene.project.id, true)
        .unwrap();
    let (status, said) = post(&scene, &scene.chief, r#"{"tier":"standard","body":"Held"}"#).await;
    assert_eq!(status, 201);
    assert_eq!(said["gated"], true);
}

#[tokio::test]
async fn the_gate_is_read_again_after_the_body_came_in_and_the_task_was_given() {
    // Unlike who may give a task, which is decided on the window as it was
    // read, whether the brief waits is the project's as it is by then.
    let scene = scene();
    let context = Rc::clone(&scene.context);
    let body = read_while(r#"{"tier":"standard","body":"Held"}"#, move || {
        context.ledger.borrow_mut().set_gate(1, true).unwrap();
    });
    let asked = with_body(Method::POST, "/api/tasks", &scene.chief, body);
    let (status, said) = through(&scene, asked).await;
    assert_eq!(
        (status, said["gated"].clone()),
        (201, json!(true)),
        "{said}"
    );
}

#[tokio::test]
async fn a_task_for_the_chief_itself_is_queued_with_its_message_named() {
    let scene = scene();
    let (status, said) = post(
        &scene,
        &scene.chief,
        r#"{"self":true,"body":"Plan the release"}"#,
    )
    .await;
    assert_eq!(status, 201, "{said}");
    assert_eq!(said["task"]["assignee"], "chief");
    assert_eq!(said["task"]["state"], "queued");
    assert!(
        said["message"].is_i64(),
        "the brief that takes it to its window"
    );
    // Only a flag that is `true` is one.
    let (status, said) = post(
        &scene,
        &scene.chief,
        r#"{"self":"true","tier":"standard","body":"Not for itself"}"#,
    )
    .await;
    assert_eq!(
        (status, said["task"]["assignee"].clone()),
        (201, json!(null))
    );
}

#[tokio::test]
async fn the_kind_of_staff_asked_for_is_the_first_flag_that_is_true_design_advice_review() {
    let scene = scene();
    for (body, said) in [
        (
            r#"{"design":true,"advice":true,"review":true,"body":"x","tier":"standard"}"#,
            "no image designer is on the staff: ask the human for one, in your terminal",
        ),
        (
            r#"{"advice":true,"review":true,"body":"x","tier":"standard"}"#,
            "no advisor is on the staff: ask the human for one, in your terminal",
        ),
        (
            r#"{"review":true,"body":"x","tier":"standard"}"#,
            "no reviewer is on the staff: ask the human for one, in your terminal",
        ),
    ] {
        let answered = post(&scene, &scene.chief, body).await;
        assert_eq!(
            refused(&answered),
            (409, "no-member-of-tier", said),
            "{body}"
        );
    }
    // A designer asks for no tier, and what it says of one is not read.
    let answered = post(
        &scene,
        &scene.chief,
        r#"{"design":true,"body":"x","tier":"junk"}"#,
    )
    .await;
    assert_eq!(answered.1["error"], "no-member-of-tier");
    // Only `true` is a flag.
    let (status, _) = post(
        &scene,
        &scene.chief,
        r#"{"design":"true","body":"x","tier":"standard"}"#,
    )
    .await;
    assert_eq!(status, 201);
    assert_eq!(scene.kicks.get(), 1);
}

#[tokio::test]
async fn a_tier_nobody_holds_goes_to_the_nearest_and_says_which_was_asked_between_the_message_and_the_gate(
) {
    let scene = scene();
    let (status, said) = post(
        &scene,
        &scene.chief,
        r#"{"tier":"critical","purpose":"hard-problem","body":"Why is it slow?"}"#,
    )
    .await;
    assert_eq!(status, 201, "{said}");
    assert_eq!(said["task"]["tier"], "standard");
    assert_eq!(
        said.as_object().unwrap().keys().collect::<Vec<_>>(),
        ["task", "message", "asked", "gated"]
    );
    assert_eq!(said["asked"], "critical");
}

#[tokio::test]
async fn what_is_wrong_with_the_task_is_said_in_the_order_the_ledger_checks_it() {
    let scene = scene();
    for (body, said) in [
        // The words first, then the lists, then the tier.
        (
            r#"{"needs":"T-1","tier":"huge"}"#,
            (400, "invalid-text", "body must be text, not empty, at most 1000000 characters"),
        ),
        (
            r#"{"body":"x","needs":"T-1","before":[0],"tier":"huge"}"#,
            (400, "invalid-needs", "needs is a list of task numbers (T-3, T-4)"),
        ),
        (
            r#"{"body":"x","before":[0],"tier":"huge"}"#,
            (400, "invalid-needs", "before is a list of task numbers (T-3, T-4)"),
        ),
        (
            r#"{"body":7,"tier":"standard"}"#,
            (400, "invalid-text", "body must be text, not empty, at most 1000000 characters"),
        ),
        (
            r#"{"body":"x"}"#,
            (400, "invalid-tier", "a tier is critical, complex, standard, light, not undefined"),
        ),
        (
            r#"{"body":"x","tier":5}"#,
            (400, "invalid-tier", "a tier is critical, complex, standard, light, not 5"),
        ),
        (
            r#"{"body":"x","tier":null}"#,
            (400, "invalid-tier", "a tier is critical, complex, standard, light, not null"),
        ),
        (
            r#"{"body":"x","tier":"critical"}"#,
            (
                400,
                "purpose-required",
                "critical work names its purpose: critical-review, architecture, hard-problem, important-question",
            ),
        ),
    ] {
        let answered = post(&scene, &scene.chief, body).await;
        assert_eq!(refused(&answered), said, "{body}");
    }
    assert_eq!(
        scene.kicks.get(),
        0,
        "a task that was refused wakes nothing"
    );
}

/// What `Number(body.after)` reads, and the follow-up of a task that is none:
/// what Node answered for each (its ledger looked for the task after it had
/// checked the words and the lists, and none of these was one).
#[tokio::test]
async fn a_follow_up_is_read_as_a_number_and_what_is_no_task_number_names_no_task() {
    let scene = scene();
    for (after, said) in [
        (r#""abc""#, "no task T-NaN in project 1"),
        ("2.5", "no task T-2.5 in project 1"),
        ("null", "no task T-0 in project 1"),
        ("true", "no task T-1 in project 1"),
        ("{}", "no task T-NaN in project 1"),
        ("[3]", "no task T-3 in project 1"),
        (r#""0x10""#, "no task T-16 in project 1"),
        ("1e21", "no task T-1e+21 in project 1"),
        ("-3", "no task T--3 in project 1"),
        (r#""""#, "no task T-0 in project 1"),
        ("7.0", "no task T-7 in project 1"),
    ] {
        let body = format!(r#"{{"after":{after},"body":"x"}}"#);
        let answered = post(&scene, &scene.chief, &body).await;
        assert_eq!(refused(&answered), (404, "unknown-task", said), "{body}");
    }
    assert_eq!(scene.kicks.get(), 0);
}

#[tokio::test]
async fn a_follow_up_checks_its_words_and_its_lists_before_it_looks_for_the_task_and_never_its_tier(
) {
    let scene = scene();
    for (body, said) in [
        (
            r#"{"after":"abc"}"#,
            (
                400,
                "invalid-text",
                "body must be text, not empty, at most 1000000 characters",
            ),
        ),
        (
            r#"{"after":"abc","body":"x","needs":"T-1"}"#,
            (
                400,
                "invalid-needs",
                "needs is a list of task numbers (T-3, T-4)",
            ),
        ),
        (
            r#"{"after":"abc","body":"x","before":[0]}"#,
            (
                400,
                "invalid-needs",
                "before is a list of task numbers (T-3, T-4)",
            ),
        ),
        // A follow-up goes back to a window: what it says of a tier is not read.
        (
            r#"{"after":2.5,"body":"x","tier":"huge"}"#,
            (404, "unknown-task", "no task T-2.5 in project 1"),
        ),
        // And it is a follow-up before it is a task for the chief itself.
        (
            r#"{"after":7,"self":true,"body":"x"}"#,
            (404, "unknown-task", "no task T-7 in project 1"),
        ),
    ] {
        let answered = post(&scene, &scene.chief, body).await;
        assert_eq!(refused(&answered), said, "{body}");
    }
}

#[tokio::test]
async fn a_number_that_cannot_be_read_as_one_fails_the_request_as_javascript_threw() {
    let scene = scene();
    for after in [r#"{"toString":1}"#, r#"[{"toString":1}]"#] {
        let (status, said) = post(
            &scene,
            &scene.chief,
            &format!(r#"{{"after":{after},"body":"x"}}"#),
        )
        .await;
        assert_eq!(status, 500, "{after}");
        assert_eq!(
            said,
            json!({ "error": "internal", "message": "Cannot convert object to primitive value" })
        );
    }
    // Before anything is asked of the ledger: not even the words are read.
    assert_eq!(scene.kicks.get(), 0);
}

#[tokio::test]
async fn a_follow_up_goes_back_to_the_session_that_did_the_task_whatever_the_spelling_of_its_number(
) {
    // Each on a board of its own: the window is busy with the first.
    for after in ["1", r#""1""#, "1.0", r#""0x1""#, "[1]", "true"] {
        let scene = scene();
        assert_eq!(finished_session_task(&scene), 1);
        let body = format!(r#"{{"after":{after},"body":"Now the lexer"}}"#);
        let (status, said) = post(&scene, &scene.chief, &body).await;
        assert_eq!(status, 201, "{after}: {said}");
        assert_eq!(said["task"]["state"], "queued", "{after}");
        assert!(
            said["task"]["assignee"]
                .as_str()
                .unwrap()
                .starts_with("zeus-"),
            "{after}"
        );
        assert_eq!(scene.kicks.get(), 1);
    }
}

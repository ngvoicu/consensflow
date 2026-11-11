//! `GET /api/tasks` and `POST /api/tasks`: the board as summaries, and a task
//! given. What the words and the order of the checks are is Node's: where a
//! test says what Node answered, it is what the real API answered the same
//! request (the probe that printed those answers went with Node's API).

use std::rc::Rc;

use hyper::Method;
use serde_json::{json, Value};

use crate::api::routes::task::GATED;
use crate::api::routes::tests::support::{
    api, finished_session_task, gated_brief_of, open_task, project_is, read_while,
    session_task_at_work, through, with_body, working_task,
};
use crate::api::waits::WAITS;
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
    // A lane says the model and effort it runs on where its name does not (harness-models,
    // HMD-12): the chief's, whose name is its role; the human's runs on none, and zeus's
    // name holds its model.
    let runs_on: Vec<&Value> = said["lanes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|lane| &lane["runsOn"])
        .collect();
    assert_eq!(runs_on, [&Value::Null, &json!("mybuilder"), &Value::Null]);
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

/// A task is for one role (the review of T-1441, T-1453): one that asks for two or more is
/// refused in words that name each, where the first of the table's order took it and the rest
/// went unsaid. One asked alone is that role's, as before.
#[tokio::test]
async fn a_task_that_asks_for_two_roles_is_refused_naming_each_and_one_alone_is_its_roles() {
    let scene = scene();
    for (body, said) in [
        (
            r#"{"design":true,"advice":true,"review":true,"test":true,"body":"x","tier":"standard"}"#,
            "a task is for one role, and this one asks for an advisor, a reviewer, a tester and an image designer: add a task for each",
        ),
        (
            r#"{"advice":true,"review":true,"body":"x","tier":"standard"}"#,
            "a task is for one role, and this one asks for an advisor and a reviewer: add a task for each",
        ),
        (
            r#"{"frontend":true,"architect":true,"body":"x"}"#,
            "a task is for one role, and this one asks for a frontend designer and an architect: add a task for each",
        ),
        (
            r#"{"research":true,"integrate":true,"after":1,"self":true,"body":"x"}"#,
            "a task is for one role, and this one asks for a researcher and an integrator: add a task for each",
        ),
    ] {
        let answered = post(&scene, &scene.chief, body).await;
        assert_eq!(refused(&answered), (400, "two-roles", said), "{body}");
    }
    for (body, said) in [
        (
            r#"{"advice":true,"body":"x","tier":"standard"}"#,
            "no advisor is on the staff: ask the human for one, in your terminal",
        ),
        (
            r#"{"review":true,"body":"x","tier":"standard"}"#,
            "no reviewer is on the staff: ask the human for one, in your terminal",
        ),
        (
            r#"{"test":true,"body":"x","tier":"standard"}"#,
            "no tester is on the staff: ask the human for one, in your terminal",
        ),
        (
            r#"{"design":true,"body":"x"}"#,
            "no image designer is on the staff: ask the human for one, in your terminal",
        ),
    ] {
        let answered = post(&scene, &scene.chief, body).await;
        assert_eq!(
            refused(&answered),
            (409, "no-member-of-tier", said),
            "{body}"
        );
    }
    // Only `true` is a flag.
    let (status, _) = post(
        &scene,
        &scene.chief,
        r#"{"design":"true","body":"x","tier":"standard"}"#,
    )
    .await;
    assert_eq!(status, 201);
    assert_eq!(scene.kicks.get(), 1);
    let (status, said) = post(
        &scene,
        &scene.chief,
        r#"{"test":"true","body":"x","tier":"standard"}"#,
    )
    .await;
    assert_eq!(
        (status, said["task"]["pool"].as_str()),
        (201, Some("worker")),
        "a test is asked for with true, and with no other word"
    );
}

/// A role is one row of the roles' table: a sixth, put in it for this test alone, is asked
/// for by its own field, with no tier where its row has none; a staff with nobody in it
/// refuses it by the role's name.
#[tokio::test]
async fn a_role_in_the_table_alone_is_asked_for_by_its_field_and_refused_by_its_name() {
    let _scout = cf_ledger::roles::seam::put(&cf_ledger::roles::seam::WITH_A_SCOUT);
    let scene = scene();
    let answered = post(&scene, &scene.chief, r#"{"scout":true,"body":"x"}"#).await;
    assert_eq!(
        refused(&answered),
        (
            409,
            "no-member-of-tier",
            "no field scout is on the staff: ask the human for one, in your terminal"
        )
    );
    // Asked beside others, it is named among them in the table's order, and none is taken
    // (T-1453).
    for (body, said) in [
        (
            r#"{"test":true,"scout":true,"advice":true,"body":"x","tier":"standard"}"#,
            "a task is for one role, and this one asks for an advisor, a field scout and a tester: add a task for each",
        ),
        (
            r#"{"design":true,"scout":true,"body":"x"}"#,
            "a task is for one role, and this one asks for a field scout and an image designer: add a task for each",
        ),
    ] {
        let answered = post(&scene, &scene.chief, body).await;
        assert_eq!(refused(&answered), (400, "two-roles", said), "{body}");
    }
}

#[tokio::test]
async fn a_test_opens_on_the_board_for_the_testers_of_its_tier_and_wakes_the_dispatcher_once() {
    let scene = scene();
    scene
        .context
        .ledger
        .borrow_mut()
        .add_member(
            scene.project.id,
            &cf_ledger::NewMember {
                model: None,
                effort: None,
                model_name: Some("tess".to_owned()),
                harness: "claude-code".to_owned(),
                designer: false,
                roles: vec!["tester".to_owned()],
                tier: "standard".to_owned(),
            },
        )
        .unwrap();
    let (status, said) = post(
        &scene,
        &scene.chief,
        r#"{"test":true,"tier":"standard","body":"Run the suite on T-1"}"#,
    )
    .await;
    assert_eq!(status, 201, "{said}");
    assert_eq!(
        (
            said["task"]["pool"].as_str(),
            said["task"]["tier"].as_str(),
            said["task"]["state"].as_str(),
            said["task"]["assignee"].as_str(),
        ),
        (Some("tester"), Some("standard"), Some("open"), None),
        "it waits on the board for a tester"
    );
    assert_eq!(scene.kicks.get(), 1);
    // A member does not give one: only the chief hands out tasks.
    let answered = post(
        &scene,
        &scene.zeus,
        r#"{"test":true,"tier":"standard","body":"x"}"#,
    )
    .await;
    assert_eq!(refused(&answered).1, "not-a-coordinator");
}

/// The four roles of spec new-roles (FD-04): each is asked for by its field, with no tier, and a
/// staff with none of it refuses it by the role's name; once a member of it is on the staff
/// the task waits on the board for its role's pool, with no tier. A tier said beside it is
/// refused in the role's name.
#[tokio::test]
async fn each_new_role_is_asked_for_by_its_field_and_refused_by_its_name_until_one_is_on_the_staff()
{
    let scene = scene();
    for (field, pool, named, whose) in [
        (
            "frontend",
            "frontend",
            "frontend designer",
            "a frontend designer's",
        ),
        ("architect", "architect", "architect", "an architect's"),
        ("research", "researcher", "researcher", "a researcher's"),
        ("integrate", "integrator", "integrator", "an integrator's"),
    ] {
        // A tier said beside it is refused in the role's name (T-1453), not dropped.
        let tiered = format!(r#"{{"{field}":true,"body":"x","tier":"standard"}}"#);
        let answered = post(&scene, &scene.chief, &tiered).await;
        assert_eq!(
            refused(&answered),
            (
                400,
                "no-tier-for-role",
                format!("{whose} task has no tier: leave out --tier").as_str()
            ),
            "{tiered}"
        );
        let body = format!(r#"{{"{field}":true,"body":"x"}}"#);
        let answered = post(&scene, &scene.chief, &body).await;
        assert_eq!(
            refused(&answered),
            (
                409,
                "no-member-of-tier",
                format!("no {named} is on the staff: ask the human for one, in your terminal")
                    .as_str()
            ),
            "{body}"
        );
        scene
            .context
            .ledger
            .borrow_mut()
            .add_member(
                scene.project.id,
                &cf_ledger::NewMember {
                    model: None,
                    effort: None,
                    model_name: Some(format!("{pool}-agent")),
                    harness: "claude-code".to_owned(),
                    designer: false,
                    roles: vec![pool.to_owned()],
                    tier: "light".to_owned(),
                },
            )
            .unwrap();
        let (status, said) = post(&scene, &scene.chief, &body).await;
        assert_eq!(status, 201, "{said}");
        assert_eq!(
            (
                said["task"]["pool"].as_str(),
                said["task"]["tier"].as_str(),
                said["task"]["state"].as_str(),
            ),
            (Some(pool), None, Some("open")),
            "{body}"
        );
    }
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
        ["task", "message", "asked", "heldBy", "gated"]
    );
    assert_eq!(said["asked"], "critical");
    assert_eq!(said["heldBy"], json!(["zeus"]), "whose tier it went to");
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
        // A follow-up goes back to a window, whose member keeps its tier: a tier asked with it
        // is refused before the task it follows is looked for (T-1453).
        (
            r#"{"after":2.5,"body":"x","tier":"huge"}"#,
            (
                400,
                "follow-up-takes-no-tier",
                "a follow-up goes to the window of the task it follows, whose member keeps its own tier: leave out --tier",
            ),
        ),
        // And it is no task for the chief itself: the two are refused together.
        (
            r#"{"after":7,"self":true,"body":"x"}"#,
            (
                400,
                "self-and-after",
                "a --self task is your own step, and --after gives the next step to another task's window: leave out one of them",
            ),
        ),
    ] {
        let answered = post(&scene, &scene.chief, body).await;
        assert_eq!(refused(&answered), said, "{body}");
    }
}

/// What `cf task add` asked beside a role, a tier or `--self` went unsaid (the review of
/// T-1441, T-1453): `--self` won over a role and a tier, a role with no tier let its tier and
/// its purpose go, and a follow-up any tier. Each is refused now in words that name what was
/// asked, for the old five roles' switches as for the four new ones, before anything is
/// looked for.
#[tokio::test]
async fn a_role_or_a_tier_asked_beside_self_or_a_follow_up_or_a_tier_for_a_role_with_none_is_refused_in_its_words(
) {
    let scene = scene();
    for (body, said) in [
        (
            r#"{"research":true,"self":true,"needs":[1],"body":"x"}"#,
            (
                400,
                "self-takes-no-role",
                "a --self task is your own step: leave out --research",
            ),
        ),
        (
            r#"{"review":true,"self":true,"needs":[1],"body":"x"}"#,
            (
                400,
                "self-takes-no-role",
                "a --self task is your own step: leave out --review",
            ),
        ),
        (
            r#"{"self":true,"tier":"standard","needs":[1],"body":"x"}"#,
            (
                400,
                "self-takes-no-tier",
                "a --self task is your own step: leave out --tier",
            ),
        ),
        (
            r#"{"self":true,"purpose":"design","needs":[1],"body":"x"}"#,
            (
                400,
                "self-takes-no-tier",
                "a --self task is your own step: leave out --purpose",
            ),
        ),
        (
            r#"{"frontend":true,"tier":"critical","purpose":"design","body":"x"}"#,
            (
                400,
                "no-tier-for-role",
                "a frontend designer's task has no tier: leave out --tier",
            ),
        ),
        (
            r#"{"design":true,"tier":"standard","body":"x"}"#,
            (
                400,
                "no-tier-for-role",
                "an image designer's task has no tier: leave out --tier",
            ),
        ),
        (
            r#"{"integrate":true,"purpose":"design","body":"x"}"#,
            (
                400,
                "no-tier-for-role",
                "an integrator's task has no tier, and so no purpose: leave out --purpose",
            ),
        ),
        (
            r#"{"after":1,"tier":"complex","body":"x"}"#,
            (
                400,
                "follow-up-takes-no-tier",
                "a follow-up goes to the window of the task it follows, whose member keeps its own tier: leave out --tier",
            ),
        ),
        (
            r#"{"after":1,"advice":true,"purpose":"design","tier":"critical","body":"x"}"#,
            (
                400,
                "follow-up-takes-no-tier",
                "a follow-up goes to the window of the task it follows, whose member keeps its own tier: leave out --tier",
            ),
        ),
    ] {
        let answered = post(&scene, &scene.chief, body).await;
        assert_eq!(refused(&answered), said, "{body}");
    }
}

/// A role asked for with `--after T-n` (the review of T-1441, T-1453): `cf` sent the follow-up
/// alone and the ledger gave it to T-n's window, whatever role that window was for, even with
/// nobody on the staff in the role asked. It is taken when T-n's window is of the role asked
/// and refused otherwise, by the ledger, in words that name both roles; one that asks for no
/// role still continues the window, as before.
#[tokio::test]
async fn a_follow_up_for_a_role_is_taken_by_a_window_of_that_role_alone_and_refused_naming_both() {
    let scene = scene();
    let finished = finished_session_task(&scene);
    scene
        .context
        .ledger
        .borrow_mut()
        .accept_task(scene.project.id, finished, "chief")
        .unwrap();
    for (switch, asked) in [
        ("research", "a researcher's"),
        ("advice", "an advisor's"),
        ("review", "a reviewer's"),
        ("integrate", "an integrator's"),
    ] {
        let body = format!(r#"{{"after":{finished},"{switch}":true,"body":"x"}}"#);
        let answered = post(&scene, &scene.chief, &body).await;
        assert_eq!(
            refused(&answered),
            (
                409,
                "follow-up-of-another-role",
                format!("T-{finished} is a worker's task; {asked} task starts in a new window: leave out --after").as_str()
            ),
            "{body}"
        );
    }
    assert_eq!(
        scene.kicks.get(),
        0,
        "nothing was given for the refused ones"
    );
    let body = format!(r#"{{"after":{finished},"body":"Now the lexer"}}"#);
    let (status, said) = post(&scene, &scene.chief, &body).await;
    assert_eq!(status, 201, "{said}");
    assert!(said["task"]["assignee"]
        .as_str()
        .is_some_and(|window| window.starts_with("zeus-")));
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
        // The chief has decided it: a follow-up of a task still undecided waits (T-1118).
        scene
            .context
            .ledger
            .borrow_mut()
            .accept_task(scene.project.id, 1, "chief")
            .unwrap();
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

#[tokio::test]
async fn a_follow_up_of_a_task_still_at_work_waits_for_it_and_is_not_queued_for_the_window() {
    let scene = scene();
    let working = session_task_at_work(&scene);
    let body = format!(r#"{{"after":{working},"body":"Then the lexer"}}"#);
    let (status, said) = post(&scene, &scene.chief, &body).await;
    assert_eq!(status, 201, "{said}");
    assert_eq!(said["task"]["state"], "open");
    assert_eq!(said["task"]["blockedBy"], json!([working]));
    assert!(
        said["task"]["assignee"]
            .as_str()
            .unwrap()
            .starts_with("zeus-"),
        "{said}"
    );
    assert!(
        said["message"].is_null(),
        "nothing is queued for the window"
    );

    // A follow-up of the follow-up waits for it in turn.
    let next = said["task"]["number"].as_i64().unwrap();
    let body = format!(r#"{{"after":{next},"body":"And the docs"}}"#);
    let (status, said) = post(&scene, &scene.chief, &body).await;
    assert_eq!(
        (status, said["task"]["blockedBy"].clone()),
        (201, json!([next]))
    );
}

/// T-1118: a follow-up of a task whose result is in and not accepted waits for it, as one
/// of a task at work does; it was queued for the window at once, before the chief decided.
#[tokio::test]
async fn a_follow_up_of_a_task_with_its_result_in_waits_for_it_and_is_not_queued_for_the_window() {
    let scene = scene();
    let finished = finished_session_task(&scene);
    let body = format!(r#"{{"after":{finished},"body":"Now the lexer"}}"#);
    let (status, said) = post(&scene, &scene.chief, &body).await;
    assert_eq!(status, 201, "{said}");
    assert_eq!(said["task"]["state"], "open");
    assert_eq!(said["task"]["blockedBy"], json!([finished]));
    assert!(
        said["task"]["assignee"]
            .as_str()
            .unwrap()
            .starts_with("zeus-"),
        "{said}"
    );
    assert!(
        said["message"].is_null(),
        "nothing is queued for the window"
    );

    // Accepted, it goes to the window.
    scene
        .context
        .ledger
        .borrow_mut()
        .accept_task(scene.project.id, finished, "chief")
        .unwrap();
    let next = said["task"]["number"].as_i64().unwrap();
    let (status, said) = api(
        &scene,
        Method::GET,
        &format!("/api/tasks/{next}"),
        &scene.chief,
        "",
    )
    .await;
    assert_eq!(status, 200, "{said}");
    assert_eq!(said["task"]["state"], "queued");
}

/// The title of task `number` as `token`'s window lists the board: on its lane, or
/// among the open tasks.
async fn listed_title(scene: &Scene, token: &str, number: i64) -> Value {
    let (status, said) = api(scene, Method::GET, "/api/tasks", token, "").await;
    assert_eq!(status, 200, "{said}");
    let lanes = said["lanes"].as_array().unwrap().iter();
    lanes
        .flat_map(|lane| lane["tasks"].as_array().unwrap().iter())
        .chain(said["open"].as_array().unwrap().iter())
        .find(|card| card["number"] == number)
        .map(|card| card["title"].clone())
        .unwrap_or_else(|| panic!("T-{number} is on no list: {said}"))
}

/// Calliope's review of the merge (T-408, P3-1): a card's title is its brief's
/// first line, so `cf task list` gave a line of a brief that waits at the human's
/// gate to every window. Each window that did not give the task lists it saying it
/// waits; the chief, who wrote it, lists its title.
#[tokio::test]
async fn a_card_whose_brief_waits_at_the_gate_is_listed_saying_so_but_to_its_writer() {
    let scene = scene();
    let (session, _) = gated_brief_of(&scene, "Lexer\nTokenize every file under src");
    for token in [&scene.zeus, &session] {
        assert_eq!(listed_title(&scene, token, 1).await, GATED);
    }
    assert_eq!(listed_title(&scene, &scene.chief, 1).await, "Lexer");
}

/// The same, for a brief that waits for the Resume: while the project is closed,
/// its window lists it saying it waits, and lists its title once it is open.
#[tokio::test]
async fn while_the_project_is_closed_a_card_whose_brief_no_window_was_handed_is_listed_saying_it_waits(
) {
    let scene = scene();
    let created = scene
        .context
        .ledger
        .borrow_mut()
        .create_task(
            scene.project.id,
            &cf_ledger::NewTask {
                from: "chief".to_owned(),
                to: Some("zeus".to_owned()),
                body: "Lexer\nTokenize every file under src".to_owned(),
                ..cf_ledger::NewTask::default()
            },
        )
        .unwrap();
    let number = created.task.number;
    project_is(&scene, "suspended");
    assert_eq!(listed_title(&scene, &scene.zeus, number).await, WAITS);
    assert_eq!(listed_title(&scene, &scene.chief, number).await, "Lexer");
    project_is(&scene, "open");
    assert_eq!(listed_title(&scene, &scene.zeus, number).await, "Lexer");
}

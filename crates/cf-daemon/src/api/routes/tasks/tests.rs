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
async fn own_hands_leaves_every_kind_of_board_task_available() {
    let scene = scene();
    for (role, model, designer) in [
        ("advisor", "advisor-model", false),
        ("reviewer", "reviewer-model", false),
        ("tester", "tester-model", false),
        ("designer", "image-model", true),
        ("frontend", "frontend-model", false),
        ("architect", "architect-model", false),
        ("researcher", "research-model", false),
        ("integrator", "integrator-model", false),
    ] {
        scene
            .context
            .ledger
            .borrow_mut()
            .add_member(
                scene.project.id,
                &cf_ledger::NewMember {
                    model: None,
                    effort: None,
                    model_name: Some(model.to_owned()),
                    harness: if designer { "codex" } else { "claude-code" }.to_owned(),
                    designer,
                    roles: vec![role.to_owned()],
                    tier: "standard".to_owned(),
                },
            )
            .unwrap();
    }
    let finished = finished_session_task(&scene);
    scene
        .context
        .ledger
        .borrow_mut()
        .accept_task(scene.project.id, finished, "chief")
        .unwrap();
    scene
        .context
        .ledger
        .borrow_mut()
        .set_own_hands(scene.project.id, true)
        .unwrap();
    for body in [
        json!({"tier":"standard", "body":"worker"}),
        json!({"tier":"standard", "advice":true, "body":"advice"}),
        json!({"tier":"standard", "review":true, "body":"review"}),
        json!({"tier":"standard", "test":true, "body":"test"}),
        json!({"design":true, "body":"image"}),
        json!({"frontend":true, "tier":"standard", "body":"frontend"}),
        json!({"architect":true, "tier":"standard", "body":"architecture"}),
        json!({"research":true, "tier":"standard", "body":"research"}),
        json!({"integrate":true, "tier":"standard", "body":"integrate"}),
        json!({"after":finished, "body":"follow up"}),
        json!({"self":true, "body":"own later step"}),
    ] {
        let (status, said) = post(&scene, &scene.chief, &body.to_string()).await;
        assert_eq!(status, 201, "{body}: {said}");
    }
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
    // A task is never refused for want of a member: it waits on the board for one.
    for body in [
        r#"{"advice":true,"body":"x","tier":"standard"}"#,
        r#"{"review":true,"body":"x","tier":"standard"}"#,
        r#"{"test":true,"body":"x","tier":"standard"}"#,
        r#"{"design":true,"body":"x"}"#,
    ] {
        let (status, said) = post(&scene, &scene.chief, body).await;
        assert_eq!(status, 201, "{body}: {said}");
        assert_eq!(said["task"]["state"], "open", "{body}");
    }
    // Only `true` is a flag.
    let (status, _) = post(
        &scene,
        &scene.chief,
        r#"{"design":"true","body":"x","tier":"standard"}"#,
    )
    .await;
    assert_eq!(status, 201);
    assert_eq!(scene.kicks.get(), 5);
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
    // The SQLite CHECK of `task.pool` holds the table's roles alone.
    scene
        .context
        .ledger
        .borrow()
        .sqlite()
        .execute_batch("PRAGMA ignore_check_constraints = ON")
        .unwrap();
    // A staff with nobody in the role takes the task, which waits for one.
    let (status, said) = post(&scene, &scene.chief, r#"{"scout":true,"body":"x"}"#).await;
    assert_eq!(status, 201, "{said}");
    assert_eq!(said["task"]["state"], "open");
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

/// The four roles of spec new-roles (FD-04) have tiers now (the staff as slots): each is asked
/// for by its field and a tier, a staff with nobody in it takes the task and it waits for one,
/// critical work asks for its purpose in each of them (every role has a Critical slot), and a
/// tier nobody holds goes to the nearest one somebody does.
#[tokio::test]
async fn each_new_role_is_asked_for_by_its_field_and_a_tier_and_waits_until_one_is_on_the_staff() {
    let scene = scene();
    for (field, pool) in [
        ("frontend", "frontend"),
        ("architect", "architect"),
        ("research", "researcher"),
        ("integrate", "integrator"),
    ] {
        // No tier is asked for the tier.
        let body = format!(r#"{{"{field}":true,"body":"x"}}"#);
        let answered = post(&scene, &scene.chief, &body).await;
        assert_eq!(
            refused(&answered),
            (
                400,
                "invalid-tier",
                "a tier is critical, complex, standard, light, not undefined"
            ),
            "{body}"
        );
        // Critical: every role names a purpose there.
        let critical = format!(r#"{{"{field}":true,"body":"x","tier":"critical"}}"#);
        let answered = post(&scene, &scene.chief, &critical).await;
        assert_eq!(refused(&answered).1, "purpose-required", "{critical}");
        // A purpose comes with Critical only.
        let purposed =
            format!(r#"{{"{field}":true,"body":"x","tier":"light","purpose":"architecture"}}"#);
        let answered = post(&scene, &scene.chief, &purposed).await;
        assert_eq!(
            refused(&answered),
            (
                400,
                "purpose-without-critical",
                "a purpose comes with --tier critical"
            ),
            "{purposed}"
        );
        // With nobody in the role the task waits for one: it is never refused for want of it.
        let body = format!(r#"{{"{field}":true,"body":"x","tier":"standard"}}"#);
        let (status, said) = post(&scene, &scene.chief, &body).await;
        assert_eq!(status, 201, "{said}");
        assert_eq!(
            (
                said["task"]["pool"].as_str(),
                said["task"]["tier"].as_str(),
                said["task"]["state"].as_str()
            ),
            (Some(pool), Some("standard"), Some("open")),
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
        // A tier nobody holds goes to the nearest one somebody does, and says which was asked.
        let (status, said) = post(&scene, &scene.chief, &body).await;
        assert_eq!(status, 201, "{said}");
        assert_eq!(
            (
                said["task"]["pool"].as_str(),
                said["task"]["tier"].as_str(),
                said["asked"].as_str()
            ),
            (Some(pool), Some("light"), Some("standard")),
            "{body}"
        );
        assert_eq!(said["heldBy"], json!([format!("{pool}-agent")]));
    }
}

#[tokio::test]
async fn a_tier_nobody_holds_goes_to_the_nearest_and_says_which_was_asked_between_the_message_and_the_gate(
) {
    let scene = scene();
    let (status, said) = post(
        &scene,
        &scene.chief,
        r#"{"tier":"light","body":"Why is it slow?"}"#,
    )
    .await;
    assert_eq!(status, 201, "{said}");
    assert_eq!(said["task"]["tier"], "standard");
    assert_eq!(
        said.as_object().unwrap().keys().collect::<Vec<_>>(),
        ["task", "message", "asked", "heldBy", "gated"]
    );
    assert_eq!(said["asked"], "light");
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
        (
            r#"{"advice":true,"body":"x","tier":"critical"}"#,
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

/// What `cf task add` asked beside a role, a tier or `--self` went unsaid (the review of
/// T-1441, T-1453): `--self` won over a role and a tier, a role with no tier let its tier and
/// its purpose go. Each is refused now in words that name what was
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
            r#"{"frontend":true,"tier":"critical","purpose":"design","body":"x"}"#,
            (
                400,
                "purpose-required",
                "critical work names its purpose: critical-review, architecture, hard-problem, important-question",
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
            r#"{"design":true,"tier":"standard","purpose":"design","body":"x"}"#,
            (
                400,
                "no-tier-for-role",
                "an image designer's task has no tier: leave out --tier and --purpose",
            ),
        ),
        (
            r#"{"design":true,"purpose":"design","body":"x"}"#,
            (
                400,
                "no-tier-for-role",
                "an image designer's task has no tier, and so no purpose: leave out --purpose",
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

// STC-01: exercise the public task route so the ledger transaction, name
// resolution and retry contract are all observed through the same boundary as cf.
fn stc_plan() -> Value {
    json!({ "plan": [
        { "name": "first", "tier": "standard", "body": "First\n" },
        { "name": "second", "tier": "standard", "body": "Second\n", "needs": ["first"] },
        { "name": "third", "tier": "standard", "body": "Third\n" }
    ] })
}

async fn stc_post(scene: &Scene, body: &Value) -> (u16, Value) {
    post(scene, &scene.chief, &body.to_string()).await
}

/// Each section's answer carries the tier it went to, kept with the key: a Complex request that
/// fell to Standard reads Standard, with the tier asked and whose, and the key sent again after
/// the holder moved gives the answer it first gave.
#[tokio::test]
async fn a_plans_answer_carries_each_sections_tier_and_a_replay_gives_it_again_after_the_holder_moved(
) {
    let scene = scene();
    let plan = json!({ "key": "one", "plan": [
        { "name": "build", "tier": "complex", "body": "Build\n" },
        { "name": "polish", "tier": "standard", "body": "Polish\n" }
    ] });
    let (status, said) = stc_post(&scene, &plan).await;
    assert_eq!(status, 201, "{said}");
    let first = said["tasks"].clone();
    assert_eq!(first[0]["tier"], "standard", "{said}");
    assert_eq!(first[0]["asked"], "complex");
    assert_eq!(first[0]["heldBy"], json!(["zeus"]));
    assert_eq!(first[1]["tier"], "standard");
    assert!(
        first[1].get("asked").is_none(),
        "a section that did not fall asks nothing"
    );
    // The holder moves to another tier; the same file and key answer as they first did.
    scene
        .context
        .ledger
        .borrow_mut()
        .edit_member(
            scene.project.id,
            "zeus",
            &cf_ledger::MemberEdit {
                harness: "claude-code".to_owned(),
                model: Some("claude-sonnet-5".to_owned()),
                effort: None,
                model_name: Some("zeus".to_owned()),
                tier: "light".to_owned(),
            },
        )
        .unwrap();
    let (status, again) = stc_post(&scene, &plan).await;
    assert_eq!(status, 200, "{again}");
    assert_eq!(
        again["tasks"], first,
        "the original destination, not a recomputed one"
    );
}

/// A plan an earlier version took under a key replays its saved answer, though its section had
/// no tier (a researcher's): the key is looked up before the section is read for its tier, and
/// the same file under a fresh key is refused for it.
#[tokio::test]
async fn a_keyed_plan_an_earlier_version_took_replays_its_saved_answer_and_a_fresh_key_refuses_it()
{
    let scene = scene();
    let plan = json!({ "key": "old", "plan": [
        { "name": "lookup", "research": true, "body": "Look it up\n" }
    ] });
    let (status, said) = stc_post(&scene, &json!({ "key": "fresh", "plan": plan["plan"] })).await;
    assert_eq!(status, 400, "{said}");
    assert_eq!(said["error"], "invalid-tier");
    // What the earlier version kept for this file under its key: the ledger's own fingerprint
    // of the sections and of the file, and the answer as `[name, number]`.
    let task = cf_ledger::NewTask {
        from: "chief".to_owned(),
        pool: Some("researcher".to_owned()),
        body: "Look it up\n".to_owned(),
        ..cf_ledger::NewTask::default()
    };
    let sections = [cf_ledger::PlanTask {
        name: "lookup".to_owned(),
        task,
        needs: Vec::new(),
        before: Vec::new(),
    }];
    let source = serde_json::to_string(&json!({ "plan": plan["plan"], "source": null })).unwrap();
    let fingerprint = {
        use sha2::{Digest, Sha256};
        let raw = format!("{:x}", Sha256::digest(source.as_bytes()));
        let _ = raw;
        let route = format!("{:x}", Sha256::digest(source.as_bytes()));
        format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&(&sections[..], route)).unwrap())
        )
    };
    scene
        .context
        .ledger
        .borrow()
        .sqlite()
        .execute(
            "INSERT INTO task_plan_key (project_id, key, fingerprint, tasks) VALUES (?, 'old', ?, '[[\"lookup\", 7]]')",
            rusqlite::params![scene.project.id, fingerprint],
        )
        .unwrap();
    let (status, replayed) = stc_post(&scene, &plan).await;
    assert_eq!(status, 200, "{replayed}");
    assert_eq!(replayed["tasks"][0]["task"]["number"], 7, "{replayed}");
    assert_eq!(scene.kicks.get(), 0, "no second task and no wake-up");
    // A changed file under the old key is a different plan.
    let changed = json!({ "key": "old", "plan": [
        { "name": "lookup", "research": true, "body": "Another\n" }
    ] });
    let (status, said) = stc_post(&scene, &changed).await;
    assert_ne!(status, 200, "{said}");
}

/// Keeps a plan under a key as an earlier version did: the ledger's own fingerprint of its
/// sections and of the file, and the answer as `[name, number]`.
fn keep_plan_under_key(scene: &Scene, key: &str, plan: &Value, task: cf_ledger::NewTask) {
    use sha2::{Digest, Sha256};
    let sections = [cf_ledger::PlanTask {
        name: "only".to_owned(),
        task,
        needs: Vec::new(),
        before: Vec::new(),
    }];
    let source = serde_json::to_string(&json!({ "plan": plan["plan"], "source": null })).unwrap();
    let route = format!("{:x}", Sha256::digest(source.as_bytes()));
    let fingerprint = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&(&sections[..], route)).unwrap())
    );
    scene
        .context
        .ledger
        .borrow()
        .sqlite()
        .execute(
            "INSERT INTO task_plan_key (project_id, key, fingerprint, tasks) VALUES (?, ?, ?, '[[\"only\", 7]]')",
            rusqlite::params![scene.project.id, key, fingerprint],
        )
        .unwrap();
}

/// A plan an earlier version took that the staff now refuses (a purpose at a tier below Critical,
/// on advice or a review) replays its saved answer: the key is looked up
/// before any role, tier or purpose is checked, a changed file under it is `key-used`, and the
/// same file under a fresh key is refused for what it asks.
#[tokio::test]
async fn a_keyed_plan_an_earlier_version_took_with_a_tier_the_staff_now_refuses_still_replays() {
    for (section, pool, purpose, tier, refusal) in [
        (
            json!({ "name": "only", "advice": true, "tier": "light",
                    "purpose": "important-question", "body": "Ask\n" }),
            "advisor",
            "important-question",
            "light",
            "purpose-without-critical",
        ),
        (
            json!({ "name": "only", "review": true, "tier": "standard",
                    "purpose": "critical-review", "body": "Check\n" }),
            "reviewer",
            "critical-review",
            "standard",
            "purpose-without-critical",
        ),
    ] {
        let scene = scene();
        let plan = json!({ "key": "old", "plan": [section.clone()] });
        let fresh = json!({ "key": "fresh", "plan": [section.clone()] });
        let (status, said) = stc_post(&scene, &fresh).await;
        assert_eq!(status, 400, "{said}");
        assert_eq!(said["error"], refusal, "{said}");
        let task = cf_ledger::NewTask {
            from: "chief".to_owned(),
            pool: Some(pool.to_owned()),
            tier: Some(tier.to_owned()),
            purpose: Some(purpose.to_owned()),
            body: section["body"].as_str().unwrap().to_owned(),
            ..cf_ledger::NewTask::default()
        };
        keep_plan_under_key(&scene, "old", &plan, task);
        let (status, replayed) = stc_post(&scene, &plan).await;
        assert_eq!(status, 200, "{refusal}: {replayed}");
        assert_eq!(replayed["tasks"][0]["task"]["number"], 7, "{replayed}");
        assert_eq!(scene.kicks.get(), 0, "no second task and no wake-up");
        let mut changed = plan.clone();
        changed["plan"][0]["body"] = json!("Another\n");
        let (status, said) = stc_post(&scene, &changed).await;
        assert_eq!(status, 409, "{refusal}: {said}");
        assert_eq!(said["error"], "key-used", "{said}");
    }
}

#[tokio::test]
async fn stc_three_sections_resolve_a_plan_name() {
    let scene = scene();
    let (status, said) = stc_post(&scene, &stc_plan()).await;
    assert_eq!(status, 201, "{said}");
    let tasks = said["tasks"].as_array().unwrap();
    assert_eq!(tasks.len(), 3);
    let first = tasks[0]["task"]["number"].as_i64().unwrap();
    let second = tasks[1]["task"]["number"].as_i64().unwrap();
    let thread = scene
        .context
        .ledger
        .borrow()
        .task(scene.project.id, second)
        .unwrap()
        .unwrap();
    assert_eq!(thread.task.blocked_by, vec![first]);
}

#[tokio::test]
async fn stc_a_refusal_in_the_second_section_rolls_back_the_first() {
    let scene = scene();
    let mut plan = stc_plan();
    plan["plan"][1]["needs"] = json!([999]);
    let (status, said) = stc_post(&scene, &plan).await;
    assert_ne!(status, 201, "{said}");
    assert!(
        said["message"].as_str().unwrap().contains("second"),
        "{said}"
    );
    assert!(scene
        .context
        .ledger
        .borrow()
        .board(scene.project.id)
        .unwrap()
        .open
        .is_empty());
}

#[tokio::test]
async fn stc_a_repeated_key_returns_the_first_tasks() {
    let scene = scene();
    let mut plan = stc_plan();
    plan["key"] = json!("retry-one");
    let first = stc_post(&scene, &plan).await;
    let again = stc_post(&scene, &plan).await;
    assert_eq!(first.0, 201, "{}", first.1);
    assert_eq!(again.0, 200, "{}", again.1);
    assert_eq!(first.1["tasks"], again.1["tasks"]);
    assert_eq!(
        scene
            .context
            .ledger
            .borrow()
            .board(scene.project.id)
            .unwrap()
            .open
            .len(),
        3
    );
}

#[tokio::test]
async fn stc_a_different_plan_under_a_used_key_is_refused() {
    let scene = scene();
    let mut plan = stc_plan();
    plan["key"] = json!("retry-two");
    let first = stc_post(&scene, &plan).await;
    assert_eq!(first.0, 201, "{}", first.1);
    plan["plan"][2]["body"] = json!("Different\n");
    let again = stc_post(&scene, &plan).await;
    assert_ne!(again.0, 201, "{}", again.1);
    assert!(
        again.1["message"].as_str().unwrap().contains("key"),
        "{}",
        again.1
    );
    assert_eq!(
        scene
            .context
            .ledger
            .borrow()
            .board(scene.project.id)
            .unwrap()
            .open
            .len(),
        3
    );
}

#[tokio::test]
async fn stc_accept_is_all_or_none_when_a_later_task_cannot_be_accepted() {
    let scene = scene();
    let done = finished_session_task(&scene);
    let open = open_task(&scene);
    let (status, said) = stc_post(
        &scene,
        &json!({"action":"accept", "tasks":[done,open], "body":"checked"}),
    )
    .await;
    assert_ne!(status, 200, "{said}");
    assert!(
        said["message"]
            .as_str()
            .unwrap()
            .contains(&format!("T-{open}")),
        "{said}"
    );
    assert_eq!(
        scene
            .context
            .ledger
            .borrow()
            .task(scene.project.id, done)
            .unwrap()
            .unwrap()
            .task
            .state,
        "done"
    );
}

#[tokio::test]
async fn stc_cancel_moves_several_together() {
    let scene = scene();
    let a = open_task(&scene);
    let b = open_task(&scene);
    let (status, said) = stc_post(
        &scene,
        &json!({"action":"cancel", "tasks":[a,b], "body":"called off"}),
    )
    .await;
    assert_eq!(status, 200, "{said}");
    for number in [a, b] {
        assert_eq!(
            scene
                .context
                .ledger
                .borrow()
                .task(scene.project.id, number)
                .unwrap()
                .unwrap()
                .task
                .state,
            "cancelled"
        );
    }
}

#[tokio::test]
async fn stc_cancel_can_take_both_ends_of_a_dependency_in_either_order() {
    let scene = scene();
    let (status, made) = stc_post(&scene, &stc_plan()).await;
    assert_eq!(status, 201, "{made}");
    let first = made["tasks"][0]["task"]["number"].as_i64().unwrap();
    let second = made["tasks"][1]["task"]["number"].as_i64().unwrap();
    let (status, said) =
        stc_post(&scene, &json!({"action":"cancel", "tasks":[first,second]})).await;
    assert_eq!(status, 200, "{said}");
    let ledger = scene.context.ledger.borrow();
    assert_eq!(
        ledger
            .task(scene.project.id, first)
            .unwrap()
            .unwrap()
            .task
            .state,
        "cancelled"
    );
    assert_eq!(
        ledger
            .task(scene.project.id, second)
            .unwrap()
            .unwrap()
            .task
            .state,
        "cancelled"
    );
}

#[tokio::test]
async fn stc_a_nonpositive_decision_names_the_bad_task_and_changes_nothing() {
    let scene = scene();
    let open = open_task(&scene);
    let (status, said) = stc_post(&scene, &json!({"action":"cancel", "tasks":[open,0]})).await;
    assert_ne!(status, 200, "{said}");
    assert!(said["message"].as_str().unwrap().contains("T-0"), "{said}");
    assert_eq!(
        scene
            .context
            .ledger
            .borrow()
            .task(scene.project.id, open)
            .unwrap()
            .unwrap()
            .task
            .state,
        "open"
    );
}

#[tokio::test]
async fn stc_cancel_refusal_rolls_back_earlier_cancels() {
    let scene = scene();
    let a = open_task(&scene);
    let (status, said) = stc_post(
        &scene,
        &json!({"action":"cancel", "tasks":[a,999], "body":"called off"}),
    )
    .await;
    assert_ne!(status, 200, "{said}");
    assert!(
        said["message"].as_str().unwrap().contains("T-999"),
        "{said}"
    );
    assert_eq!(
        scene
            .context
            .ledger
            .borrow()
            .task(scene.project.id, a)
            .unwrap()
            .unwrap()
            .task
            .state,
        "open"
    );
}

#[tokio::test]
async fn stc_a_refusal_during_creation_rolls_back_an_already_inserted_section() {
    let scene = scene();
    let mut plan = stc_plan();
    plan["plan"][1]["tier"] = json!("huge");
    let (status, said) = stc_post(&scene, &plan).await;
    assert_ne!(status, 201, "{said}");
    assert!(
        said["message"].as_str().unwrap().contains("second"),
        "{said}"
    );
    assert!(scene
        .context
        .ledger
        .borrow()
        .board(scene.project.id)
        .unwrap()
        .open
        .is_empty());
}

#[tokio::test]
async fn stc_forward_names_before_unknown_duplicate_and_circles() {
    let scene = scene();
    let mut plan = stc_plan();
    plan["plan"][0]["before"] = json!(["third"]);
    plan["plan"][1]["needs"] = json!(["third"]);
    let (status, said) = stc_post(&scene, &plan).await;
    assert_eq!(status, 201, "{said}");
    let tasks = said["tasks"].as_array().unwrap();
    let number = |at: usize| tasks[at]["task"]["number"].as_i64().unwrap();
    {
        let ledger = scene.context.ledger.borrow();
        assert_eq!(
            ledger
                .task(scene.project.id, number(2))
                .unwrap()
                .unwrap()
                .task
                .blocked_by,
            [number(0)]
        );
        assert_eq!(
            ledger
                .task(scene.project.id, number(1))
                .unwrap()
                .unwrap()
                .task
                .blocked_by,
            [number(2)]
        );
    }
    for (edited, section) in [
        (
            json!({"plan":[{"name":"a", "tier":"standard", "body":"A", "needs":["missing"]}]}),
            "a",
        ),
        (
            json!({"plan":[{"name":"a", "tier":"standard", "body":"A"},{"name":"a", "tier":"standard", "body":"B"}]}),
            "a",
        ),
        (
            json!({"plan":[{"name":"a", "tier":"standard", "body":"A", "needs":["b"]},{"name":"b", "tier":"standard", "body":"B", "needs":["a"]}]}),
            "section",
        ),
    ] {
        let (status, said) = stc_post(&scene, &edited).await;
        assert_ne!(status, 201, "{said}");
        assert!(
            said["message"].as_str().unwrap().contains(section),
            "{said}"
        );
    }
    assert_eq!(
        scene
            .context
            .ledger
            .borrow()
            .board(scene.project.id)
            .unwrap()
            .open
            .len(),
        3
    );
}

#[tokio::test]
async fn stc_a_numeric_reference_cannot_name_a_task_created_by_the_same_plan() {
    let scene = scene();
    let mut plan = stc_plan();
    plan["plan"][1]["needs"] = json!([1]);
    let (status, said) = stc_post(&scene, &plan).await;
    assert_ne!(status, 201, "{said}");
    assert!(
        said["message"].as_str().unwrap().contains("second"),
        "{said}"
    );
    assert!(scene
        .context
        .ledger
        .borrow()
        .board(scene.project.id)
        .unwrap()
        .open
        .is_empty());
}

#[tokio::test]
async fn stc_cancel_later_invalid_state_rolls_back_a_first_cancel() {
    let scene = scene();
    let done = finished_session_task(&scene);
    scene
        .context
        .ledger
        .borrow_mut()
        .accept_task(scene.project.id, done, "chief")
        .unwrap();
    let open = open_task(&scene);
    let (status, said) = stc_post(&scene, &json!({"action":"cancel", "tasks":[open,done]})).await;
    assert_ne!(status, 200, "{said}");
    assert!(
        said["message"]
            .as_str()
            .unwrap()
            .contains(&format!("T-{done}")),
        "{said}"
    );
    assert_eq!(
        scene
            .context
            .ledger
            .borrow()
            .task(scene.project.id, open)
            .unwrap()
            .unwrap()
            .task
            .state,
        "open"
    );
}

#[tokio::test]
async fn stc_a_member_cannot_use_the_batch_door() {
    let scene = scene();
    let open = open_task(&scene);
    let (status, _) = post(
        &scene,
        &scene.zeus,
        &json!({"action":"cancel", "tasks":[open]}).to_string(),
    )
    .await;
    assert_eq!(status, 403);
    assert_eq!(
        scene
            .context
            .ledger
            .borrow()
            .task(scene.project.id, open)
            .unwrap()
            .unwrap()
            .task
            .state,
        "open"
    );
}

#[tokio::test]
async fn stc_accept_moves_several_done_tasks_together() {
    let scene = scene();
    let first = finished_session_task(&scene);
    let second = finished_session_task(&scene);
    let (status, said) = stc_post(
        &scene,
        &json!({"action":"accept", "tasks":[first,second], "body":"checked"}),
    )
    .await;
    assert_eq!(status, 200, "{said}");
    for number in [first, second] {
        assert_eq!(
            scene
                .context
                .ledger
                .borrow()
                .task(scene.project.id, number)
                .unwrap()
                .unwrap()
                .task
                .state,
            "accepted"
        );
    }
}

#[tokio::test]
async fn stc_a_key_refuses_a_changed_source_even_when_the_sections_are_the_same() {
    let scene = scene();
    let mut plan = stc_plan();
    plan["key"] = json!("source-boundaries");
    plan["source"] = json!("plan-without-blank-boundaries");
    let first = stc_post(&scene, &plan).await;
    assert_eq!(first.0, 201, "{}", first.1);
    let repeated = stc_post(&scene, &plan).await;
    assert_eq!(repeated.0, 200, "{}", repeated.1);
    assert_eq!(repeated.1["tasks"], first.1["tasks"]);
    plan["source"] = json!("plan-with-blank-boundaries");
    let different = stc_post(&scene, &plan).await;
    assert_eq!(different.0, 409, "{}", different.1);
    assert_eq!(different.1["error"], "key-used");
    assert_eq!(
        scene
            .context
            .ledger
            .borrow()
            .board(scene.project.id)
            .unwrap()
            .open
            .len(),
        3
    );
}

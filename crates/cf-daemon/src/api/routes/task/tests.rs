//! One task: reading it and its transcript; the order of the checks (the task
//! before the verb) and the numbers read as JavaScript reads them. The verbs
//! that move it are [`verbs`]. Where a test says what Node answered, it is what
//! the real API answered the same request (the probe that printed those answers
//! went with Node's API).

mod verbs;

use hyper::Method;
use serde_json::{json, Value};

use super::{items_of, whole};
use crate::api::routes::tests::support::{
    answered_question, api, gated_brief, gated_brief_of, open_task, state_of, working_question,
    working_task,
};
use crate::testing::{scene, Scene};

fn refused(answered: &(u16, Value)) -> (u16, &str, &str) {
    (
        answered.0,
        answered.1["error"].as_str().unwrap_or("-"),
        answered.1["message"].as_str().unwrap_or("-"),
    )
}

async fn get(scene: &Scene, token: &str, route: &str) -> (u16, Value) {
    api(
        scene,
        Method::GET,
        &format!("/api/tasks/{route}"),
        token,
        "",
    )
    .await
}

async fn post(scene: &Scene, token: &str, route: &str, body: &str) -> (u16, Value) {
    api(
        scene,
        Method::POST,
        &format!("/api/tasks/{route}"),
        token,
        body,
    )
    .await
}

/// What the zeus' window did in task `number`: `count` items.
fn written_by_zeus(scene: &Scene, count: usize) {
    let mut ledger = scene.context.ledger.borrow_mut();
    let project = ledger.project(scene.project.id).unwrap().unwrap();
    let zeus = project
        .participants
        .iter()
        .find(|p| p.handle == "zeus")
        .unwrap();
    let conversation = ledger.start_conversation(zeus.id, "claude-code").unwrap();
    let items: Vec<Value> = (1..=count)
        .map(|at| json!({ "id": format!("i{at}"), "role": "assistant", "text": format!("item {at}"), "complete": true }))
        .collect();
    ledger.copy_transcript(conversation.id, &items, 0).unwrap();
}

#[tokio::test]
async fn an_unknown_task_is_404_before_anything_else_is_asked_whatever_the_route() {
    let scene = scene();
    let huge = "x".repeat(3 * 1024 * 1024);
    for (method, route, body) in [
        (Method::GET, "9", ""),
        (Method::GET, "9/transcript", ""),
        (Method::GET, "9/accept", ""),
        (Method::POST, "9/pause", "not json"),
        (Method::POST, "9/done", huge.as_str()),
        (Method::DELETE, "9", ""),
    ] {
        let said = api(
            &scene,
            method.clone(),
            &format!("/api/tasks/{route}"),
            &scene.zeus,
            body,
        )
        .await;
        assert_eq!(
            refused(&said),
            (404, "unknown-task", "no task T-9 in this project"),
            "{method} {route}"
        );
    }
    assert_eq!(scene.kicks.get(), 0);
}

#[tokio::test]
async fn the_number_is_quoted_as_javascript_writes_it_and_found_as_it_reads_it() {
    let scene = scene();
    for (digits, said) in [
        ("007", "T-7"),
        ("99999999999999999999", "T-100000000000000000000"),
        ("1000000000000000000000", "T-1e+21"),
        ("9007199254740993", "T-9007199254740992"),
        ("0", "T-0"),
    ] {
        let answered = get(&scene, &scene.chief, digits).await;
        assert_eq!(
            refused(&answered),
            (
                404,
                "unknown-task",
                format!("no task {said} in this project").as_str()
            ),
            "{digits}"
        );
    }
    let number = open_task(&scene);
    for digits in [
        format!("{number}"),
        format!("000{number}"),
        format!("{number:0>20}"),
    ] {
        let (status, said) = get(&scene, &scene.chief, &digits).await;
        assert_eq!(
            (status, said["task"]["number"].as_i64()),
            (200, Some(number)),
            "{digits}"
        );
    }
}

#[test]
fn a_number_names_a_task_only_if_it_is_a_whole_number_a_double_holds_exactly() {
    for (number, found) in [
        (0.0, Some(0)),
        (7.0, Some(7)),
        (9_007_199_254_740_991.0, Some(9_007_199_254_740_991)),
        (9_007_199_254_740_992.0, None),
        (1e21, None),
        (2.5, None),
        (-3.0, Some(-3)),
        (f64::NAN, None),
        (f64::INFINITY, None),
        (f64::NEG_INFINITY, None),
    ] {
        assert_eq!(whole(number), found, "{number}");
    }
}

/// The keys of a task as the ledger has it, its thread last.
const TASK_KEYS: [&str; 20] = [
    "id",
    "projectId",
    "number",
    "title",
    "body",
    "state",
    "requester",
    "assignee",
    "pool",
    "tier",
    "purpose",
    "session",
    "needs",
    "blockedBy",
    "heldUntil",
    "pausedAt",
    "deletedAt",
    "createdAt",
    "updatedAt",
    "messages",
];

#[tokio::test]
async fn a_task_is_read_with_its_thread_less_what_waits_for_the_human() {
    let scene = scene();
    let (session, brief) = gated_brief(&scene);
    drop(session);
    let (status, said) = get(&scene, &scene.chief, "1").await;
    assert_eq!(status, 200);
    let task = said["task"].as_object().unwrap();
    assert_eq!(
        task.keys().map(String::as_str).collect::<Vec<_>>(),
        TASK_KEYS,
        "the task as the ledger has it, its thread last"
    );
    assert_eq!(
        task["messages"],
        json!([]),
        "the brief {brief} is gated and waits unseen"
    );
    // The ledger has it.
    let thread = scene
        .context
        .ledger
        .borrow()
        .task(scene.project.id, 1)
        .unwrap()
        .unwrap();
    assert_eq!(thread.messages.len(), 1);
    assert_eq!(thread.messages[0].state, "gated");
}

/// Every window reads the task whose brief waits at the gate: its words are in
/// no view of it, and every key is where it was. The task's title stays: it is
/// the card's label, which every window lists and the human sees on the board
/// (a brief of one line is its own title).
#[tokio::test]
async fn a_brief_that_waits_at_the_gate_is_in_no_agents_view_of_the_task() {
    let scene = scene();
    let scope = "Tokenize every file under src and print the tokens";
    let (session, _) = gated_brief_of(&scene, &format!("Lexer\n{scope}"));
    let held = scene
        .context
        .ledger
        .borrow()
        .task(scene.project.id, 1)
        .unwrap()
        .unwrap()
        .task;
    assert_eq!(
        held.body,
        format!("Lexer\n{scope}"),
        "the ledger, which the human and the board read, has it"
    );
    // The human reads the task through the page, from the ledger: the brief is
    // theirs to see, and it is held for them in the thread.
    let seen = scene
        .context
        .ledger
        .borrow()
        .task_that_fits(scene.project.id, 1)
        .unwrap()
        .unwrap();
    let seen = serde_json::to_value(&seen).unwrap();
    assert_eq!(seen["body"], format!("Lexer\n{scope}"));
    assert_eq!(seen["messages"][0]["state"], "gated");
    for token in [&scene.chief, &scene.zeus, &session] {
        let (status, said) = get(&scene, token, "1").await;
        assert_eq!(status, 200);
        let task = said["task"].as_object().unwrap();
        assert_eq!(task["body"], "", "the brief is not given");
        assert_eq!(task["title"], "Lexer", "the card's label is");
        assert_eq!(
            task.keys().map(String::as_str).collect::<Vec<_>>(),
            TASK_KEYS
        );
        assert!(
            !said.to_string().contains(scope),
            "no field of the view holds the brief: {said}"
        );
    }
}

#[tokio::test]
async fn a_brief_is_given_once_it_has_passed_the_gate_and_a_later_message_held_there_does_not_take_it_back(
) {
    let scene = scene();
    let (session, brief) = gated_brief(&scene);
    let (_, said) = get(&scene, &scene.chief, "1").await;
    assert_eq!(said["task"]["body"], "");
    // The human passes it on, and the window receives it.
    {
        let mut ledger = scene.context.ledger.borrow_mut();
        ledger.approve_message(brief, "human").unwrap();
        ledger.begin_delivery(brief).unwrap();
        ledger
            .confirm_delivery(brief, Some(&json!({ "item": "test" })))
            .unwrap();
    }
    let (_, said) = get(&scene, &session, "1").await;
    assert_eq!(said["task"]["body"], "Lexer");
    // The chief stops it and sends it on with words of its own, which wait for
    // approval in their turn: the brief is its window's already.
    let resumed = {
        let mut ledger = scene.context.ledger.borrow_mut();
        ledger
            .pause_task(scene.project.id, 1, Some("chief"), None)
            .unwrap();
        ledger
            .resume_task(scene.project.id, 1, Some("chief"), "Mind the tests")
            .unwrap()
    };
    assert_eq!(
        resumed.message.map(|message| message.state).as_deref(),
        Some("gated")
    );
    let (_, said) = get(&scene, &scene.chief, "1").await;
    assert_eq!(said["task"]["body"], "Lexer");
}

/// Astraeus's sequence: a worker asks, the chief answers, and the worker runs
/// `cf task get T-1 --transcript`: the thread first, then the transcript, which
/// a worker may not read. Nothing it did consumed the answer.
#[tokio::test]
async fn a_worker_that_reads_its_task_and_is_refused_its_transcript_still_has_its_answer_to_be_pasted(
) {
    let scene = scene();
    let answer = answered_question(&scene, "JSON");
    assert_eq!(state_of(&scene, 1), "waiting");
    let (status, said) = get(&scene, &scene.zeus, "1").await;
    assert_eq!(status, 200);
    assert!(said["task"]["messages"]
        .as_array()
        .unwrap()
        .iter()
        .any(|message| message["id"] == answer.id && message["body"] == "H: JSON"));
    let refusal = get(&scene, &scene.zeus, "1/transcript").await;
    assert_eq!(refused(&refusal).0, 403);
    let still = scene.message(answer.id);
    assert_eq!(still.state, "queued");
    assert_eq!(still.receipt, Value::Null);
    assert_eq!(state_of(&scene, 1), "waiting");
    assert_eq!(scene.logged("message.read"), 0);
    assert_eq!(scene.kicks.get(), 0);
    assert_eq!(scene.next_for_zeus(), Some("H: JSON".to_owned()));
}

#[tokio::test]
async fn a_thread_read_by_anyone_changes_nothing() {
    let scene = scene();
    let answer = answered_question(&scene, "JSON");
    // The chief gave the task and zeus has it: neither read receives the answer.
    for token in [&scene.chief, &scene.zeus, &scene.zeus] {
        let (status, _) = get(&scene, token, "1").await;
        assert_eq!(status, 200);
    }
    let (status, said) = get(&scene, &scene.chief, "1/transcript").await;
    assert_eq!(status, 200, "{said}");
    assert_eq!(scene.message(answer.id).state, "queued");
    assert_eq!(state_of(&scene, 1), "waiting");
    assert_eq!(scene.kicks.get(), 0);
}

#[tokio::test]
async fn a_gated_answer_is_left_out_of_the_thread_and_is_not_received() {
    let scene = scene();
    let question = working_question(&scene);
    scene
        .context
        .ledger
        .borrow_mut()
        .set_gate(scene.project.id, true)
        .unwrap();
    let answer = scene.choose(question.id, "red");
    assert_eq!(answer.state, "gated");
    let (status, said) = get(&scene, &scene.zeus, "1").await;
    assert_eq!(status, 200);
    let ids: Vec<&Value> = said["task"]["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|message| &message["id"])
        .collect();
    assert!(!ids.contains(&&json!(answer.id)), "{ids:?}");
    assert_eq!(scene.message(answer.id).state, "gated");
    assert_eq!(scene.kicks.get(), 0);
}

#[tokio::test]
async fn only_the_chief_and_the_one_who_gave_the_task_read_what_its_window_did() {
    let scene = scene();
    let number = working_task(&scene);
    written_by_zeus(&scene, 3);
    let (status, said) = get(&scene, &scene.chief, &format!("{number}/transcript")).await;
    assert_eq!(status, 200);
    assert_eq!(said["total"], 3);
    assert_eq!(said["items"].as_array().unwrap().len(), 3);
    assert_eq!(
        said.as_object().unwrap().keys().collect::<Vec<_>>(),
        ["total", "items"]
    );
    let refusal = get(&scene, &scene.zeus, &format!("{number}/transcript")).await;
    assert_eq!(
        refused(&refusal),
        (
            403,
            "not-a-coordinator",
            "only the chief or @chief may read what T-1's window did"
        )
    );
}

#[test]
fn how_many_items_were_asked_for_is_read_as_number_reads_it_between_1_and_50_and_10_for_none() {
    for (asked, items) in [
        (None, 10),
        (Some(""), 10),
        (Some("0"), 10),
        (Some("abc"), 10),
        (Some("NaN"), 10),
        (Some("1_0"), 10),
        (Some("2,5"), 10),
        (Some("3abc"), 10),
        (Some("1"), 1),
        (Some("3"), 3),
        (Some("1e1"), 10),
        (Some("0x2"), 2),
        (Some(" 3 "), 3),
        (Some("+3"), 3),
        (Some("-5"), 1),
        (Some("-Infinity"), 1),
        (Some("99"), 50),
        (Some("Infinity"), 50),
        // A fraction is read as `rows.slice(rows.length - last)` read it: the
        // next whole number of items, as Node gave them.
        (Some("0.5"), 1),
        (Some(".5"), 1),
        (Some("1.2"), 2),
        (Some("2.5"), 3),
        (Some("2."), 2),
        (Some("-2.5"), 1),
        (Some("49.5"), 50),
    ] {
        assert_eq!(items_of(asked), items, "{asked:?}");
    }
}

#[tokio::test]
async fn a_transcript_gives_the_last_items_asked_for_and_the_next_whole_item_for_a_fraction() {
    let scene = scene();
    let number = working_task(&scene);
    written_by_zeus(&scene, 5);
    // What Node gave for each of these, of five items.
    for (last, ids) in [
        ("", "i1,i2,i3,i4,i5"),
        ("1", "i5"),
        ("1.2", "i4,i5"),
        ("2.5", "i3,i4,i5"),
        ("0.5", "i5"),
        ("4.5", "i1,i2,i3,i4,i5"),
        ("5.5", "i1,i2,i3,i4,i5"),
        ("7", "i1,i2,i3,i4,i5"),
        ("0x2", "i4,i5"),
        ("-Infinity", "i5"),
        ("-2.5", "i5"),
        (".5", "i5"),
        ("2.", "i4,i5"),
    ] {
        let route = format!("{number}/transcript?last={last}");
        let (status, said) = get(&scene, &scene.chief, &route).await;
        assert_eq!(status, 200, "{last}");
        let got: Vec<&str> = said["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["id"].as_str().unwrap())
            .collect();
        assert_eq!(got.join(","), ids, "last={last}");
    }
}

#[tokio::test]
async fn a_verb_with_the_wrong_method_or_no_verb_is_no_such_task_command_of_a_task_that_is_there() {
    let scene = scene();
    let number = open_task(&scene);
    for (method, route) in [
        (Method::GET, format!("{number}/tell")),
        (Method::GET, format!("{number}/accept")),
        (Method::POST, format!("{number}")),
        (Method::DELETE, format!("{number}")),
        (Method::PUT, format!("{number}/done")),
        (Method::POST, "0001".to_owned()),
    ] {
        let said = api(
            &scene,
            method.clone(),
            &format!("/api/tasks/{route}"),
            &scene.chief,
            "",
        )
        .await;
        assert_eq!(
            refused(&said),
            (404, "unknown-route", "no such task command"),
            "{method} {route}"
        );
    }
    assert_eq!(scene.kicks.get(), 0);
}

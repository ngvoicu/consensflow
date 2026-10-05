//! One task: reading it and its transcript; the order of the checks (the task
//! before the verb) and the numbers read as JavaScript reads them. The verbs
//! that move it are [`verbs`]. Where a test says what Node answered, it is what
//! the real API answered the same request: `node
//! tests/goldens/daemon/probes/api-corners.mjs` prints it again.

mod verbs;

use hyper::Method;
use serde_json::{json, Value};

use super::{items_of, whole};
use crate::api::routes::tests::support::{api, gated_brief, open_task, working_task};
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

#[tokio::test]
async fn a_task_is_read_whole_with_its_thread_less_what_waits_for_the_human() {
    let scene = scene();
    let (session, brief) = gated_brief(&scene);
    drop(session);
    let (status, said) = get(&scene, &scene.chief, "1").await;
    assert_eq!(status, 200);
    let task = said["task"].as_object().unwrap();
    assert_eq!(
        task.keys().map(String::as_str).collect::<Vec<_>>(),
        [
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
            "messages"
        ],
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

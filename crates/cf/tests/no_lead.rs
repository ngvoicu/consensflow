//! What `cf` does in a project with no lead, pinned before leads are built (the leads
//! design, step 0): a member's words are never read for a name, a chief's unnamed forms
//! are refused in today's words, and `cf task list` prints an all-finished human lane as
//! it does now. A later step must leave every one of these as it is.

// The tests' own helpers: a failure in one is the test's.
#![allow(clippy::expect_used)]

mod common;

use cf_board::scripted::{reply, scripted, ScriptedApi};
use common::cf;
use serde_json::{json, Value};

const CF_WORD: &str = "'C:/Program Files/ConsensFlow/cf.exe'";

fn run(api: &ScriptedApi, args: &[&str]) -> (Option<i32>, String, String) {
    let result = cf(
        args,
        &[
            ("CONSENSFLOW_URL", &api.url),
            ("CONSENSFLOW_TOKEN", "tok"),
            ("CONSENSFLOW_CF", CF_WORD),
        ],
        "",
    );
    (
        result.status.code(),
        String::from_utf8_lossy(&result.stdout).into_owned(),
        String::from_utf8_lossy(&result.stderr).into_owned(),
    )
}

fn requests(api: &ScriptedApi) -> Vec<(String, String, Option<Value>)> {
    api.received()
        .into_iter()
        .map(|r| {
            let body = r.json();
            (r.method, r.path, body)
        })
        .collect()
}

fn who(role: &str) -> Value {
    json!({ "participant": { "handle": "window", "role": role }, "task": { "number": 3 } })
}

#[test]
fn a_member_s_ask_with_at_chief_in_its_words_puts_them_as_written_with_no_name_read() {
    let api = scripted(vec![reply(
        201,
        json!({ "message": { "id": 7, "recipient": "chief" } }),
    )]);
    let (code, out, err) = run(&api, &["ask", "@chief", "which branch?"]);
    assert_eq!((code, err.as_str()), (Some(0), ""));
    assert!(out.starts_with("m-7 asked @chief. "), "{out}");
    let sent = requests(&api);
    assert_eq!(sent.len(), 1, "no lookup of who the caller is: {sent:?}");
    assert_eq!(
        (sent[0].0.as_str(), sent[0].1.as_str()),
        ("POST", "/api/questions")
    );
    let body = sent[0].2.clone().expect("a body");
    assert_eq!(body["body"], "@chief which branch?");
    let mut keys: Vec<_> = body
        .as_object()
        .expect("an object")
        .keys()
        .cloned()
        .collect();
    keys.sort();
    assert_eq!(
        keys,
        ["askedAt", "body"],
        "nothing but the words and the moment"
    );
}

#[test]
fn a_member_s_tell_with_at_chief_in_its_words_sends_them_as_written_to_whoever_gave_its_task() {
    let api = scripted(vec![
        reply(200, who("worker")),
        reply(
            201,
            json!({ "message": { "id": 12, "recipient": "human", "kind": "note", "task": 4 } }),
        ),
    ]);
    let (code, out, err) = run(&api, &["tell", "@chief", "the tests passed"]);
    assert_eq!((code, err.as_str()), (Some(0), ""));
    assert_eq!(out, "m-12 noted to @human; nothing waits on it.\n");
    assert_eq!(
        requests(&api),
        [
            ("GET".into(), "/api/whoami".into(), None),
            (
                "POST".into(),
                "/api/notes".into(),
                Some(json!({ "body": "@chief the tests passed" }))
            )
        ]
    );
}

#[test]
fn a_chief_s_ask_is_refused_by_the_board_in_the_words_that_send_it_to_its_terminal() {
    let api = scripted(vec![reply(
        403,
        json!({
            "error": "ask-in-your-terminal",
            "message": "ask the human here in your terminal: they read and answer you there",
        }),
    )]);
    let (code, out, err) = run(&api, &["ask", "Ship it?"]);
    assert_eq!(code, Some(1));
    assert_eq!(out, "");
    assert_eq!(
        err,
        "cf: ask the human here in your terminal: they read and answer you there\n"
    );
    assert_eq!(requests(&api).len(), 1);
}

/// A lane of `role` named `handle`, its tasks as `(number, state)`.
fn lane(handle: &str, role: &str, tasks: &[(i64, &str)]) -> Value {
    json!({
        "handle": handle, "role": role,
        "tasks": tasks.iter().map(|(number, state)| json!({
            "number": number, "state": state, "assignee": handle,
            "requester": "chief", "title": format!("Task {number}"),
            "blockedBy": [], "pool": null, "tier": null,
        })).collect::<Vec<_>>(),
    })
}

#[test]
fn a_lane_whose_tasks_are_all_over_is_an_idle_session_the_human_s_included_and_the_chief_s_never() {
    let board = json!({ "open": [], "lanes": [
        lane("chief", "chief", &[(1, "accepted")]),
        lane("human", "human", &[(2, "accepted"), (3, "cancelled")]),
        lane("zeus", "worker", &[(4, "accepted")]),
    ]});
    let api = scripted(vec![reply(200, board.clone())]);
    let (code, out, _) = run(&api, &["task", "list"]);
    assert_eq!(code, Some(0));
    assert_eq!(
        out,
        "@chief (chief)\nT-1 [accepted] @chief ← @chief: Task 1\nand 2 idle sessions (`'C:/Program Files/ConsensFlow/cf.exe' task list --all`)\n"
    );
    // --all lists them as the board always did.
    let api = scripted(vec![reply(200, board)]);
    let (_, out, _) = run(&api, &["task", "list", "--all"]);
    assert_eq!(
        out,
        "@chief (chief)\nT-1 [accepted] @chief ← @chief: Task 1\n@human (human)\nT-2 [accepted] @human ← @chief: Task 2\nT-3 [cancelled] @human ← @chief: Task 3\n@zeus (worker)\nT-4 [accepted] @zeus ← @chief: Task 4\n"
    );
}

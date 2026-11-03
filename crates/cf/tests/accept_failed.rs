//! `cf task accept` of a failed task (T-1111): the words after the task are what the chief
//! checked, and go to the board as the task's `body`, as the words of `cf task reopen` do.
//! A done task is accepted with none, as it always was. What the board refuses (a failed
//! task accepted with no words) is said as the board says it. The commands are run as a
//! window runs them, against an API that writes down what it is asked.

// The tests' own helpers: a failure in one is the test's.
#![allow(clippy::expect_used)]

mod common;

use cf_board::scripted::{reply, scripted, ScriptedApi};
use common::cf;
use serde_json::{json, Value};

/// `cf args` as a window runs it against `api`, with `input` on its standard input: its
/// exit code, what it printed and what it said on the error output.
fn window(api: &ScriptedApi, args: &[&str], input: &str) -> (Option<i32>, String, String) {
    let ran = cf(
        args,
        &[("CONSENSFLOW_URL", &api.url), ("CONSENSFLOW_TOKEN", "tok")],
        input,
    );
    (
        ran.status.code(),
        String::from_utf8_lossy(&ran.stdout).into_owned(),
        String::from_utf8_lossy(&ran.stderr).into_owned(),
    )
}

/// The requests the board was asked: path and body.
fn asked(api: &ScriptedApi) -> Vec<(String, Option<Value>)> {
    api.received()
        .into_iter()
        .map(|received| {
            let body = received.json();
            (received.path, body)
        })
        .collect()
}

/// T-3 as the board gives it back once it is accepted.
fn accepted() -> Value {
    json!({
        "task": {
            "number": 3, "state": "accepted", "assignee": "zeus-amber-pine",
            "requester": "chief", "title": "Parser", "blockedBy": [],
            "pool": "worker", "tier": "standard",
        }
    })
}

#[test]
fn the_words_after_the_task_go_to_the_board_as_what_the_chief_checked() {
    let api = scripted(vec![reply(200, accepted())]);
    let (code, printed, said) = window(
        &api,
        &["task", "accept", "T-3", "Both files are saved and whole"],
        "",
    );
    assert_eq!((code, said.as_str()), (Some(0), ""));
    assert_eq!(
        printed,
        "T-3 [accepted] @zeus-amber-pine ← @chief: Parser\n"
    );
    assert_eq!(
        asked(&api),
        [(
            "/api/tasks/3/accept".to_owned(),
            Some(json!({ "body": "Both files are saved and whole" }))
        )]
    );

    // Taken from the standard input as any text is, when it is asked for.
    let api = scripted(vec![reply(200, accepted())]);
    let (code, _, _) = window(&api, &["task", "accept", "T-3", "-"], "Read both files\n");
    assert_eq!(code, Some(0));
    assert_eq!(
        asked(&api),
        [(
            "/api/tasks/3/accept".to_owned(),
            Some(json!({ "body": "Read both files" }))
        )]
    );
}

#[test]
fn a_task_accepted_with_no_words_asks_the_board_as_it_always_did() {
    let api = scripted(vec![reply(200, accepted())]);
    let (code, printed, _) = window(&api, &["task", "accept", "T-3"], "");
    assert_eq!(code, Some(0));
    assert_eq!(
        printed,
        "T-3 [accepted] @zeus-amber-pine ← @chief: Parser\n"
    );
    assert_eq!(
        asked(&api),
        [("/api/tasks/3/accept".to_owned(), Some(json!({})))]
    );
}

#[test]
fn what_the_board_refuses_of_a_failed_task_is_said_as_it_says_it() {
    let words = "T-3 failed: accepting it takes the words of what you checked, which are its result: cf task accept T-3 \"what I checked\"";
    let api = scripted(vec![reply(
        400,
        json!({ "error": "checked-needed", "message": words }),
    )]);
    let (code, printed, said) = window(&api, &["task", "accept", "T-3"], "");
    assert_eq!((code, printed.as_str()), (Some(1), ""));
    assert!(said.contains(words), "{said}");
}

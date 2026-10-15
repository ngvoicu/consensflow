//! `cf tell` to a task still queued, its window not up yet: the board keeps the
//! words for the window, and `cf` says so, and that nothing is paused (the field
//! report of 2026-10-15, where the board refused it). A cancel that took told
//! words with it says which. The commands are run as a window runs them, against
//! an API that writes down what it is asked.

// The tests' own helpers: a failure in one is the test's.
#![allow(clippy::expect_used)]

mod common;

use cf_board::scripted::{reply, scripted, ScriptedApi};
use common::cf;
use serde_json::{json, Value};

/// `cf args` as a window runs it against `api`: its exit code and what it printed.
fn window(api: &ScriptedApi, args: &[&str]) -> (Option<i32>, String) {
    let ran = cf(
        args,
        &[("CONSENSFLOW_URL", &api.url), ("CONSENSFLOW_TOKEN", "tok")],
        "",
    );
    (
        ran.status.code(),
        String::from_utf8_lossy(&ran.stdout).into_owned(),
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

/// T-3 as the board gives a task back: in `state`, at @zeus-amber-pine.
fn task(state: &str) -> Value {
    json!({
        "number": 3, "state": state, "assignee": "zeus-amber-pine", "requester": "chief",
        "title": "Parser", "blockedBy": [], "pool": "worker", "tier": "standard",
    })
}

/// What the board answers a tell with: the message that carries the words, of
/// `kind`, and the task as it is now, in `state`.
fn told(kind: &str, state: &str) -> Value {
    json!({
        "message": {
            "id": 12, "kind": kind, "state": "queued", "sender": "chief",
            "recipient": "zeus-amber-pine", "task": 3, "preview": "Use the v2 grammar",
            "questions": null, "choices": null, "createdAt": "2026-10-15T09:00:00.000Z",
        },
        "task": task(state),
    })
}

#[test]
fn a_tell_to_a_queued_task_says_the_words_wait_for_its_window_and_nothing_is_paused() {
    let api = scripted(vec![reply(200, told("note", "queued"))]);
    let (code, printed) = window(&api, &["tell", "T-3", "Use the v2 grammar"]);
    assert_eq!(code, Some(0));
    assert_eq!(
        printed,
        "T-3 is queued: m-12 waits for @zeus-amber-pine's window and goes in after its brief. Nothing is paused, and nothing waits for an answer.\n"
    );
    assert_eq!(
        asked(&api),
        [(
            "/api/tasks/3/tell".to_owned(),
            Some(json!({ "body": "Use the v2 grammar" }))
        )]
    );
}

#[test]
fn a_tell_to_a_task_in_its_window_says_it_is_paused_as_it_always_did() {
    let api = scripted(vec![reply(200, told("question", "paused"))]);
    let (code, printed) = window(&api, &["tell", "T-3", "Stop: use v2"]);
    assert_eq!(code, Some(0));
    assert_eq!(
        printed,
        "T-3 is paused and m-12 put to @zeus-amber-pine; its answer arrives as a message. Then: cf task resume T-3 \"…\"\n"
    );
}

#[test]
fn a_cancel_says_which_of_the_words_told_the_window_went_with_it() {
    let api = scripted(vec![reply(
        200,
        json!({ "task": task("cancelled"), "dropped": [12, 13] }),
    )]);
    let (code, printed) = window(&api, &["task", "cancel", "T-3"]);
    assert_eq!(code, Some(0));
    assert_eq!(
        printed,
        "T-3 [cancelled] @zeus-amber-pine ← @chief: Parser\nWhat you told its window, m-12, m-13, went with it: it never had them.\n"
    );

    let api = scripted(vec![reply(
        200,
        json!({ "task": task("cancelled"), "dropped": [12] }),
    )]);
    let (_, printed) = window(&api, &["task", "cancel", "T-3"]);
    assert!(
        printed.ends_with("\nWhat you told its window, m-12, went with it: it never had it.\n"),
        "{printed}"
    );

    // A cancel that took none says what it always said.
    let api = scripted(vec![reply(200, json!({ "task": task("cancelled") }))]);
    let (_, printed) = window(&api, &["task", "cancel", "T-3"]);
    assert_eq!(
        printed,
        "T-3 [cancelled] @zeus-amber-pine ← @chief: Parser\n"
    );
}

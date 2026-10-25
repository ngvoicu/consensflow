//! `cf task done` on the chief's own step (`cf task add --self`): the board
//! accepts it with the chief's words, and `cf` says nothing waits for its
//! decision (the field report of 2026-10-15, B4). A task done for whoever else
//! asked for it says its line, as it always did.

// The tests' own helpers: a failure in one is the test's.
#![allow(clippy::expect_used)]

mod common;

use cf_board::scripted::{reply, scripted, ScriptedApi};
use common::cf;
use serde_json::json;

/// What `cf args` printed as a window runs it against `api`, and its exit code.
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

#[test]
fn the_chiefs_own_step_done_says_it_is_accepted_and_another_task_says_its_line() {
    let own = json!({ "task": {
        "number": 7, "state": "accepted", "assignee": "chief", "requester": "chief",
        "title": "Check the parser", "blockedBy": [], "pool": null, "tier": null,
    } });
    let api = scripted(vec![reply(200, own)]);
    let (code, printed) = window(&api, &["task", "done", "T-7", "Checked: it holds"]);
    assert_eq!(code, Some(0));
    assert_eq!(
        printed,
        "T-7 is done and accepted: it was your own step, so nothing waits for your decision, and what needed it goes on.\n"
    );

    let given = json!({ "task": {
        "number": 8, "state": "done", "assignee": "chief", "requester": "human",
        "title": "Plan the week", "blockedBy": [], "pool": null, "tier": null,
    } });
    let api = scripted(vec![reply(200, given)]);
    let (_, printed) = window(&api, &["task", "done", "T-8", "Planned"]);
    assert_eq!(printed, "T-8 [done] @chief ← @human: Plan the week\n");
}

/// The reply of the board to `cf task done T-7` on the chief's own step, which
/// it accepted, with what it names of the messages.
fn own_step_reply(named: serde_json::Value) -> String {
    let mut said = json!({ "task": {
        "number": 7, "state": "accepted", "assignee": "chief", "requester": "chief",
        "title": "Check the parser", "blockedBy": [], "pool": null, "tier": null,
    } });
    for (key, messages) in named.as_object().expect("an object") {
        said[key] = messages.clone();
    }
    let api = scripted(vec![reply(200, said)]);
    let (code, printed) = window(&api, &["task", "done", "T-7", "Checked: it holds"]);
    assert_eq!(code, Some(0));
    printed
}

const ACCEPTED: &str = "T-7 is done and accepted: it was your own step, so nothing waits for your decision, and what needed it goes on.";

#[test]
fn what_was_withdrawn_with_the_brief_is_named_with_its_kind() {
    assert_eq!(
        own_step_reply(json!({ "withdrawn": [{ "id": 3091, "kind": "task" }] })),
        format!("{ACCEPTED} Still on its way to you, and now withdrawn: the brief m-3091.\n")
    );
    assert_eq!(
        own_step_reply(json!({ "withdrawn": [
            { "id": 3091, "kind": "task" },
            { "id": 3095, "kind": "note" },
            { "id": 3096, "kind": "answer" },
        ] })),
        format!(
            "{ACCEPTED} Still on its way to you, and now withdrawn: the brief m-3091, a note m-3095 and an answer m-3096.\n"
        )
    );
}

#[test]
fn a_brief_that_was_being_typed_in_is_said_to_land_once() {
    assert_eq!(
        own_step_reply(json!({ "landing": [{ "id": 3092, "kind": "task" }] })),
        format!(
            "{ACCEPTED} Being typed into your window as you finished: the brief m-3092; it may still land once, and is not sent again.\n"
        )
    );
}

#[test]
fn a_step_with_nothing_on_its_way_says_only_that_it_is_accepted() {
    assert_eq!(own_step_reply(json!({})), format!("{ACCEPTED}\n"));
}

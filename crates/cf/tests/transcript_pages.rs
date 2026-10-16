//! `cf task get T-n --transcript --page N` and `--find "…"`: what a window did,
//! read past its newest items, fifty to a page from the newest, searched in any
//! case (the field report of 2026-10-15, B7: `--last 400` never reached the
//! window that overwrote a file). Each page says where it is among them and how
//! to read the one before. The commands are run as a window runs them, against an
//! API that writes down what it is asked.

// The tests' own helpers: a failure in one is the test's.
#![allow(clippy::expect_used)]

mod common;

use cf_board::scripted::{reply, scripted, ScriptedApi};
use common::cf;
use serde_json::{json, Value};

/// What `cf args` did as a window runs it against `api`: its exit code, what it
/// printed and what it said on the error output.
fn window(api: &ScriptedApi, args: &[&str]) -> (Option<i32>, String, String) {
    let ran = cf(
        args,
        &[("CONSENSFLOW_URL", &api.url), ("CONSENSFLOW_TOKEN", "tok")],
        "",
    );
    (
        ran.status.code(),
        String::from_utf8_lossy(&ran.stdout).into_owned(),
        String::from_utf8_lossy(&ran.stderr).into_owned(),
    )
}

/// The paths the board was asked.
fn paths(api: &ScriptedApi) -> Vec<String> {
    api.received()
        .into_iter()
        .map(|received| received.path)
        .collect()
}

/// T-3 as the board gives it, done.
fn task() -> Value {
    json!({ "task": {
        "number": 3, "state": "done", "assignee": "zeus", "requester": "chief",
        "title": "Parser", "blockedBy": [], "pool": "worker", "tier": "standard",
        "deletedAt": null, "messages": [],
    } })
}

/// An item of the window's copy.
fn item(id: &str, role: &str, text: &str) -> Value {
    json!({ "id": id, "conversation": 1, "role": role, "text": text, "complete": true, "at": null })
}

#[test]
fn a_page_says_where_it_is_among_them_and_how_to_read_the_one_before() {
    let page = json!({ "total": 400, "page": 2, "pages": 8,
        "items": [item("i301", "assistant", "item 301"), item("i302", "tool", "item 302")] });
    let api = scripted(vec![reply(200, task()), reply(200, page)]);
    let (code, printed, _) = window(&api, &["task", "get", "T-3", "--transcript", "--page", "2"]);
    assert_eq!(code, Some(0));
    assert_eq!(
        paths(&api),
        ["/api/tasks/3", "/api/tasks/3/transcript?page=2"]
    );
    assert_eq!(
        printed,
        "T-3 [done] @zeus ← @chief: Parser\n\nWhat its window did, page 2 of 8: items 301 to 350 of 400.\n\n[The agent]\nitem 301\n\n[Tool output]\nitem 302\n\nOlder: cf task get T-3 --transcript --page 3\n"
    );
}

#[test]
fn a_search_says_how_many_items_say_its_words_and_shows_a_long_one_around_them() {
    let long = format!(
        "{}git checkout -- src/parser.rs{}",
        "x".repeat(900),
        "y".repeat(900)
    );
    let found = json!({ "total": 400, "found": 2, "page": 1, "pages": 1,
        "items": [item("i7", "user", "run git checkout"), item("i9", "tool", &long)] });
    let api = scripted(vec![reply(200, task()), reply(200, found)]);
    let (code, printed, _) = window(
        &api,
        &[
            "task",
            "get",
            "T-3",
            "--transcript",
            "--find",
            "git checkout",
        ],
    );
    assert_eq!(code, Some(0));
    assert_eq!(
        paths(&api)[1],
        "/api/tasks/3/transcript?page=1&find=git+checkout"
    );
    assert!(
        printed.contains(
            "What its window did that says \"git checkout\", page 1 of 1: 2 of its 400 items say it.\n\n[Sent to the window]\nrun git checkout\n\n[Tool output]\n…"
        ),
        "{printed}"
    );
    assert!(
        printed.contains("git checkout -- src/parser.rs"),
        "{printed}"
    );
    assert!(
        printed.ends_with("…\n\nThis is the oldest page.\n"),
        "{printed}"
    );

    // Words nothing says.
    let none = json!({ "total": 400, "found": 0, "page": 0, "pages": 0, "items": [] });
    let api = scripted(vec![reply(200, task()), reply(200, none)]);
    let (_, printed, _) = window(
        &api,
        &["task", "get", "T-3", "--transcript", "--find", "overwrote"],
    );
    assert_eq!(
        printed,
        "T-3 [done] @zeus ← @chief: Parser\n\nNothing its window did says \"overwrote\".\n"
    );
}

#[test]
fn the_last_items_and_a_page_are_one_or_the_other_and_a_page_is_a_number_from_one() {
    for (args, said) in [
        (
            vec!["--last", "5", "--page", "2"],
            "cf: --last gives the newest items, and --page and --find the rest: give one or the other\n",
        ),
        (
            vec!["--find", "x", "--last", "5"],
            "cf: --last gives the newest items, and --page and --find the rest: give one or the other\n",
        ),
        (vec!["--page", "0"], "cf: --page takes a page number: 1, 2, …\n"),
        (vec!["--page", "two"], "cf: --page takes a page number: 1, 2, …\n"),
        (vec!["--find", " "], "cf: --find takes the words to look for\n"),
    ] {
        let api = scripted(Vec::new());
        let words: Vec<&str> = ["task", "get", "T-3", "--transcript"]
            .into_iter()
            .chain(args.iter().copied())
            .collect();
        let (code, printed, error) = window(&api, &words);
        assert_eq!((code, printed.as_str(), error.as_str()), (Some(2), "", said), "{args:?}");
        assert!(api.received().is_empty(), "nothing is asked: {args:?}");
    }
}

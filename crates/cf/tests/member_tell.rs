//! A member tells its task's requester without asking, while the chief's tell
//! still names and stops a task. The caller is the window the board identifies.

#![allow(clippy::expect_used)]

mod common;

use cf_board::scripted::{reply, scripted, ScriptedApi};
use common::cf;
use serde_json::{json, Value};

/// The same shell word the caller gives the CLI and expects in its guidance.
const CF_WORD: &str = "'C:/Program Files/ConsensFlow/cf.exe'";

fn who(role: &str, task: Value) -> Value {
    json!({ "participant": { "handle": "window", "role": role }, "task": task })
}

fn noted(to: &str, task: Value) -> Value {
    json!({ "message": { "id": 12, "recipient": to, "kind": "note", "task": task } })
}

fn run(api: &ScriptedApi, args: &[&str], input: &str) -> (Option<i32>, String, String) {
    let result = cf(
        args,
        &[
            ("CONSENSFLOW_URL", &api.url),
            ("CONSENSFLOW_TOKEN", "tok"),
            ("CONSENSFLOW_CF", CF_WORD),
        ],
        input,
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

fn lookup() -> (String, String, Option<Value>) {
    ("GET".into(), "/api/whoami".into(), None)
}

fn post(body: Value) -> (String, String, Option<Value>) {
    ("POST".into(), "/api/notes".into(), Some(body))
}

#[test]
fn each_member_tells_the_requester_by_the_existing_note_route() {
    for role in [
        "worker",
        "frontend",
        "architect",
        "researcher",
        "advisor",
        "reviewer",
        "tester",
        "integrator",
        "designer",
    ] {
        for (task, recipient) in [
            (json!(null), "chief"),
            (json!({"number": 3}), "chief"),
            (json!({"number": 4}), "human"),
        ] {
            let api = scripted(vec![
                reply(200, who(role, task.clone())),
                reply(201, noted(recipient, task)),
            ]);
            let (status, out, err) = run(&api, &["tell", "The premise is wrong."], "");
            assert_eq!(
                (status, out.as_str(), err.as_str()),
                (
                    Some(0),
                    format!("m-12 noted to @{recipient}; nothing waits on it.\n").as_str(),
                    ""
                ),
                "{role}"
            );
            assert_eq!(
                requests(&api),
                [lookup(), post(json!({ "body": "The premise is wrong." }))],
                "{role}"
            );
        }
    }
}

#[test]
fn a_member_tell_reads_stdin_and_files_and_can_quote_a_task_number_as_text() {
    let folder = tempfile::tempdir().expect("folder");
    let path = folder.path().join("words.md");
    std::fs::write(&path, "Unicode: 🍀\n").expect("file");
    let path = path.to_str().expect("path");
    for (args, input, body) in [
        (vec!["tell", "-"], "First\nSecond\n", "First\nSecond"),
        (vec!["tell", "--file", path], "", "Unicode: 🍀"),
        (
            vec!["tell", "--", "T-3", "is missing"],
            "",
            "T-3 is missing",
        ),
    ] {
        let api = scripted(vec![
            reply(200, who("worker", json!(null))),
            reply(201, noted("chief", json!(null))),
        ]);
        let (status, _, err) = run(&api, &args, input);
        assert_eq!((status, err.as_str()), (Some(0), ""), "{args:?}");
        assert_eq!(
            requests(&api),
            [lookup(), post(json!({"body": body}))],
            "{args:?}"
        );
    }
}

#[test]
fn a_member_cannot_use_plain_note_and_is_shown_tell() {
    let api = scripted(vec![reply(200, who("worker", json!({"number": 3})))]);
    let (status, out, err) = run(&api, &["note", "A warning"], "");
    assert_eq!((status, out.as_str()), (Some(2), ""));
    assert_eq!(err, format!("cf: members use {CF_WORD} tell \"…\"\n"));
    assert_eq!(err.lines().count(), 1);
    assert_eq!(requests(&api), [lookup()]);
}

#[test]
fn a_member_is_shown_tell_even_when_plain_note_has_no_words() {
    let api = scripted(vec![reply(200, who("worker", json!({"number": 3})))]);
    let (status, out, err) = run(&api, &["note"], "");
    assert_eq!((status, out.as_str()), (Some(2), ""));
    assert_eq!(err, format!("cf: members use {CF_WORD} tell \"…\"\n"));
    assert_eq!(err.lines().count(), 1);
    assert_eq!(requests(&api), [lookup()]);
}

#[test]
fn a_members_one_word_tell_can_itself_be_a_task_number() {
    for text in ["3", "T-3", "t-3"] {
        let api = scripted(vec![
            reply(200, who("worker", json!(null))),
            reply(201, noted("chief", json!(null))),
        ]);
        let (status, _, err) = run(&api, &["tell", text], "");
        assert_eq!((status, err.as_str()), (Some(0), ""), "{text}");
        assert_eq!(requests(&api), [lookup(), post(json!({"body": text}))]);
    }
}

#[test]
fn note_human_keeps_the_old_route_and_its_refusal() {
    for (status, answer, expected) in [
        (201, noted("human", json!(3)), Some(0)),
        (
            403,
            json!({"error": "not-the-chief", "message": "only the chief notes the human"}),
            Some(1),
        ),
    ] {
        let api = scripted(vec![reply(status, answer)]);
        let (actual, _, _) = run(&api, &["note", "--human", "Hello"], "");
        assert_eq!(actual, expected);
        assert_eq!(
            requests(&api),
            [post(json!({"body": "Hello", "to": "human"}))]
        );
    }
}

#[test]
fn a_chiefs_plain_note_is_unchanged() {
    let api = scripted(vec![
        reply(200, who("chief", json!(null))),
        reply(201, noted("human", json!(null))),
    ]);
    assert_eq!(run(&api, &["note", "Hello"], "").0, Some(0));
    assert_eq!(requests(&api), [lookup(), post(json!({"body": "Hello"}))]);
}

#[test]
fn a_chief_without_a_task_number_and_a_member_with_one_are_told_their_forms() {
    let api = scripted(vec![reply(200, who("chief", json!(null)))]);
    let (status, _, err) = run(&api, &["tell", "A warning"], "");
    assert_eq!(status, Some(2));
    assert_eq!(
        err,
        format!("cf: the chief names the task: {CF_WORD} tell T-3 \"…\"\n")
    );
    assert_eq!(err.lines().count(), 1);
    assert_eq!(requests(&api), [lookup()]);

    for number in ["T-3", "t-3", "3"] {
        let api = scripted(vec![reply(200, who("worker", json!({"number": 3})))]);
        let (status, _, err) = run(&api, &["tell", number, "A warning"], "");
        assert_eq!(status, Some(2), "{number}");
        assert_eq!(
            err,
            format!("cf: members tell without a task number: {CF_WORD} tell \"…\"\n"),
            "{number}"
        );
        assert_eq!(err.lines().count(), 1);
        assert_eq!(requests(&api), [lookup()]);
    }
}

#[test]
fn an_unreadable_caller_never_sends_to_either_route() {
    for identity in [
        json!({}),
        json!({"participant": {"role": null}}),
        json!({"participant": {"role": "unknown"}}),
    ] {
        let api = scripted(vec![reply(200, identity)]);
        let (status, _, _) = run(&api, &["tell", "A warning"], "");
        assert_eq!(status, Some(1));
        assert_eq!(requests(&api), [lookup()]);
    }
}

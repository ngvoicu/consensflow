//! The words of the commands that give a window work read from a file (`--file
//! <path>`), which no shell cuts (the field report of 2026-10-15, B6: a heredoc a
//! shell cut sent the board a brief cut at about 8.1k characters). The file is
//! read whole, its final line ending dropped, and posted as it is, backticks and
//! `$( )` and all; the reply says how many characters went and how they end. A
//! file beside words of their own is a command written wrong, and one that cannot
//! be read fails it. The commands are run as a window runs them, against an API
//! that writes down what it is asked.

// The tests' own helpers: a failure in one is the test's.
#![allow(clippy::expect_used)]

mod common;

use std::fs;
use std::path::Path;

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

/// The bodies the board was posted, by path.
fn posted(api: &ScriptedApi) -> Vec<(String, Option<Value>)> {
    api.received()
        .into_iter()
        .filter(|received| received.method == "POST")
        .map(|received| {
            let body = received.json();
            (received.path, body)
        })
        .collect()
}

/// T-3 as the board gives it back, in `state`.
fn task(state: &str) -> Value {
    json!({
        "number": 3, "state": state, "assignee": "zeus", "requester": "chief",
        "title": "Parser", "blockedBy": [], "pool": "worker", "tier": "standard",
    })
}

/// A file in `folder` named `name`, holding `text`: its path.
fn file(folder: &Path, name: &str, text: &str) -> String {
    let path = folder.join(name);
    fs::write(&path, text).expect("the file is written");
    path.to_string_lossy().into_owned()
}

#[test]
fn a_brief_from_a_file_is_posted_whole_as_written_and_the_reply_says_how_it_ends() {
    let folder = tempfile::tempdir().expect("a folder");
    let brief = format!(
        "{}\nRun `cargo test` and $(nothing else).\n```rust\nfn main() {{}}\n```",
        "Write the parser. ".repeat(600)
    );
    let path = file(folder.path(), "brief.md", &format!("{brief}\n"));
    let api = scripted(vec![reply(
        201,
        json!({ "task": task("open"), "message": null, "gated": false }),
    )]);
    let (code, printed, said) = window(
        &api,
        &["task", "add", "--tier", "standard", "--file", &path],
    );
    assert_eq!((code, said.as_str()), (Some(0), ""));
    assert_eq!(
        posted(&api),
        [(
            "/api/tasks".to_owned(),
            Some(json!({ "tier": "standard", "body": brief }))
        )],
        "whole, its final line ending the file's"
    );
    assert!(
        printed.ends_with(
            "\nT-3: sent 10,863 characters, ending \"…and $(nothing else). ```rust fn main() {} ```\".\n"
        ),
        "{printed}"
    );
}

#[test]
fn the_words_that_resume_reopen_or_tell_come_from_a_file_too_and_a_windows_line_ending_is_dropped()
{
    let folder = tempfile::tempdir().expect("a folder");
    let path = file(folder.path(), "words.md", "Use the v2 grammar.\r\n");
    let told = json!({
        "message": { "id": 12, "kind": "note", "state": "queued", "sender": "chief",
            "recipient": "zeus", "task": 3, "preview": "Use the v2 grammar.",
            "questions": null, "choices": null, "createdAt": "2026-10-16T09:00:00.000Z" },
        "task": task("queued"),
    });
    for (args, path_asked, answer) in [
        (
            vec!["task", "resume", "T-3", "--file", &path],
            "/api/tasks/3/resume",
            json!({ "task": task("queued") }),
        ),
        (
            vec!["task", "reopen", "T-3", "--file", &path],
            "/api/tasks/3/reopen",
            json!({ "task": task("queued") }),
        ),
        (
            vec!["tell", "T-3", "--file", &path],
            "/api/tasks/3/tell",
            told,
        ),
    ] {
        let api = scripted(common::replies_for(&args, answer));
        let (code, printed, _) = window(&api, &args);
        assert_eq!(code, Some(0), "{args:?}");
        assert_eq!(
            posted(&api),
            [(
                path_asked.to_owned(),
                Some(json!({ "body": "Use the v2 grammar." }))
            )],
            "{args:?}"
        );
        assert!(
            printed.ends_with("\nT-3: sent 19 characters, ending \"Use the v2 grammar.\".\n"),
            "{args:?}: {printed}"
        );
    }
}

#[test]
fn a_task_done_takes_its_result_from_a_file_as_the_report_ran_it() {
    // The command the report ran (2026-11-02): `cf task done T-1110 --file
    // /tmp/t1110-result.md` stored "--file /tmp/t1110-result.md" as the result.
    let folder = tempfile::tempdir().expect("a folder");
    let path = file(
        folder.path(),
        "result.md",
        "Done: the flags of a command hold.\n",
    );
    let api = scripted(vec![reply(200, json!({ "task": task("working") }))]);
    let (code, _, said) = window(&api, &["task", "done", "T-3", "--file", &path]);
    assert_eq!((code, said.as_str()), (Some(0), ""));
    assert_eq!(
        posted(&api),
        [(
            "/api/tasks/3/done".to_owned(),
            Some(json!({ "body": "Done: the flags of a command hold." }))
        )],
        "the file's words, not the flag's"
    );
}

#[test]
fn every_command_that_takes_words_takes_a_file_of_them() {
    let folder = tempfile::tempdir().expect("a folder");
    let path = file(folder.path(), "words.md", "The words of the file.\n");
    let moved = || json!({ "task": task("working") });
    let answered = |id: u64| json!({ "message": { "id": id, "recipient": "zeus" } });
    for (args, asked, answer, body) in [
        (
            vec!["task", "done", "T-3", "--file", &path],
            "/api/tasks/3/done",
            moved(),
            json!({ "body": "The words of the file." }),
        ),
        (
            vec!["task", "accept", "T-3", "--file", &path],
            "/api/tasks/3/accept",
            moved(),
            json!({ "body": "The words of the file." }),
        ),
        (
            vec!["task", "cancel", "T-3", "--file", &path],
            "/api/tasks/3/cancel",
            moved(),
            json!({ "body": "The words of the file." }),
        ),
        (
            vec!["task", "pause", "T-3", "--file", &path],
            "/api/tasks/3/pause",
            moved(),
            json!({ "body": "The words of the file." }),
        ),
        (
            vec!["task", "finish", "T-3", "--file", &path],
            "/api/tasks/3/finish",
            moved(),
            json!({ "body": "The words of the file." }),
        ),
        (
            vec!["note", "--file", &path],
            "/api/notes",
            answered(7),
            json!({ "body": "The words of the file." }),
        ),
        (
            vec!["note", "--human", "--file", &path],
            "/api/notes",
            json!({ "message": { "id": 7, "recipient": "human" } }),
            json!({ "body": "The words of the file.", "to": "human" }),
        ),
        (
            vec!["ask", "--file", &path],
            "/api/questions",
            json!({ "message": { "id": 7, "recipient": "chief" } }),
            json!({ "body": "The words of the file." }),
        ),
        (
            vec!["answer", "m-9", "--file", &path],
            "/api/answers",
            answered(7),
            json!({ "question": 9, "body": "The words of the file." }),
        ),
    ] {
        let api = scripted(common::replies_for(&args, answer));
        let (code, _, said) = window(&api, &args);
        assert_eq!((code, said.as_str()), (Some(0), ""), "{args:?}");
        assert_eq!(posted(&api), [(asked.to_owned(), Some(body))], "{args:?}");
    }
}

#[test]
fn a_flag_the_command_does_not_know_in_front_of_the_words_is_refused_not_kept() {
    for args in [
        vec!["task", "done", "T-3", "--bogus", "hi"],
        vec!["task", "add", "--tier", "light", "--bogus", "hi"],
        vec!["tell", "T-3", "--bogus", "hi"],
        vec!["task", "resume", "T-3", "--bogus", "hi"],
        vec!["note", "--bogus", "hi"],
        vec!["ask", "--bogus", "hi"],
        vec!["answer", "m-9", "--bogus", "hi"],
    ] {
        let api = scripted(common::replies_for(&args, json!({})));
        let (code, printed, said) = window(&api, &args);
        assert_eq!(
            (code, printed.as_str(), said),
            (
                Some(2),
                "",
                "cf: unknown flag \"--bogus\": write it after -- to keep it as words\n".to_owned()
            ),
            "{args:?}"
        );
        let asked: Vec<_> = api.received().into_iter().map(|r| r.path).collect();
        let expected = if ["tell", "note"].contains(&args[0]) {
            vec!["/api/whoami"]
        } else {
            vec![]
        };
        assert_eq!(asked, expected, "{args:?}");
    }
    // A flag inside the words is text; one the words begin with is kept by `--`.
    for (args, body) in [
        (vec!["note", "hi", "--bogus"], "hi --bogus"),
        (vec!["note", "--", "--bogus"], "--bogus"),
    ] {
        let api = scripted(common::replies_for(
            &args,
            json!({ "message": { "id": 7, "recipient": "zeus" } }),
        ));
        let (code, _, said) = window(&api, &args);
        assert_eq!((code, said.as_str()), (Some(0), ""), "{args:?}");
        assert_eq!(
            posted(&api),
            [("/api/notes".to_owned(), Some(json!({ "body": body })))],
            "{args:?}"
        );
    }
}

#[test]
fn a_file_beside_words_of_their_own_is_written_wrong_and_one_that_cannot_be_read_fails() {
    let folder = tempfile::tempdir().expect("a folder");
    let path = file(folder.path(), "brief.md", "Write the parser.");
    for args in [
        vec![
            "task", "add", "--tier", "standard", "--file", &path, "and more",
        ],
        vec!["task", "add", "--tier", "standard", "--file", &path, "-"],
        vec!["tell", "T-3", "Stop", "--file", &path],
    ] {
        let api = scripted(common::replies_for(&args, json!({})));
        let (code, printed, said) = window(&api, &args);
        assert_eq!((code, printed.as_str()), (Some(2), ""), "{args:?}");
        assert!(
            said.starts_with("cf: give the words or --file ") && said.ends_with(", not both\n"),
            "{said}"
        );
        let asked: Vec<_> = api.received().into_iter().map(|r| r.path).collect();
        let expected = if args[0] == "tell" {
            vec!["/api/whoami"]
        } else {
            vec![]
        };
        assert_eq!(asked, expected, "nothing is posted: {args:?}");
    }

    let missing = folder.path().join("nowhere.md");
    let missing = missing.to_string_lossy();
    let api = scripted(Vec::new());
    let (code, _, said) = window(&api, &["task", "resume", "T-3", "--file", &missing]);
    assert_eq!(code, Some(1));
    assert!(
        said.starts_with(&format!("cf: cannot read {missing} (")),
        "{said}"
    );
    assert!(api.received().is_empty());
}

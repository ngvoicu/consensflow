//! Every command of the board answers `--help` and `-h` with its usage, exit
//! code 0, and asks the board nothing. A window's agent that writes `cf note
//! --help` used to post a note saying "--help", and `cf task get --help` was
//! refused as no task. The word is text, as any `--word` a command does not know
//! is, unless it stands where only a flag could: it is the only word the command
//! was given that is no flag, after the number the command takes first, with no
//! `--` ahead of it. The standalone verbs are `help_standalone.rs`'s.
//!
//! The commands are run as a window runs them, against an API that writes down
//! what it is asked.

// The tests' own helpers: a failure in one is the test's.
#![allow(clippy::expect_used)]

mod common;

use cf_board::scripted::{reply, scripted, ScriptedApi};
use common::cf;
use serde_json::{json, Value};

/// An API that answers `{}` to the few requests a wrong command would make.
fn api() -> ScriptedApi {
    scripted((0..4).map(|_| reply(200, json!({}))).collect())
}

/// `cf args` as a window runs it against `api`: its exit code, what it
/// printed and said on the error output.
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

/// What the requests were: method, path and body as JSON.
fn asked(api: &ScriptedApi) -> Vec<(String, String, Option<Value>)> {
    api.received()
        .into_iter()
        .map(|received| {
            let body = received.json();
            (received.method, received.path, body)
        })
        .collect()
}

/// The usage the board command named by `path` answers with.
fn usage_of(path: &[&str]) -> &'static str {
    BOARD
        .iter()
        .find(|(named, _)| *named == path)
        .map(|(_, usage)| *usage)
        .expect("a command of the board")
}

/// What `cf help` prints inside a window: all the board's usage.
fn board_usage() -> String {
    window(&api(), &["help"]).1
}

/// Each board command's words (the ones that name it), and the lines of the
/// usage it answers with.
const BOARD: [(&[&str], &str); 17] = [
    (
        &["task", "add"],
        concat!(
            "  cf task add --tier <critical|complex|standard|light> \"…\"\n",
            "                                    work for a worker of that tier; ConsensFlow picks\n",
            "                                    the member (--purpose for critical work)\n",
            "  cf task add --advice --tier <tier> \"…\"\n",
            "                                    a question for an advisor of that tier: findings and\n",
            "                                    recommendations back, no file changed\n",
            "  cf task add --review --tier <tier> \"…\"\n",
            "                                    a review for a reviewer of that tier: say what to\n",
            "                                    review; findings back, no file changed\n",
            "  cf task add --design \"…\"          an image from the image designer: what to draw, what\n",
            "                                    to use as reference, where to save it\n",
            "  cf task add --after T-3 \"…\"       a follow-up for the window that did T-3, which\n",
            "                                    keeps its context; only when that context matters\n",
            "  cf task add --self --needs T-3 \"…\" your own later step: its brief comes back to you when T-3 is accepted\n",
            "  … --needs T-3,T-4                 the task waits on the board until T-3 and T-4 are accepted\n",
            "  … --before T-9,T-10               T-9 and T-10 (still on the board) wait for this task\n",
        ),
    ),
    (
        &["task", "list"],
        "  cf task list                      the board: what waits for a member, then every lane\n",
    ),
    (
        &["task", "get"],
        concat!(
            "  cf task get T-3                   one task and its whole thread\n",
            "  cf task get T-3 --transcript      what its window did so far (the last 10 items; --last 30 for more)\n",
        ),
    ),
    (
        &["task", "done"],
        "  cf task done T-3 \"…\"              finish a task assigned to you (the chief)\n",
    ),
    (
        &["task", "accept"],
        "  cf task accept|cancel T-3         move a task you asked for\n",
    ),
    (
        &["task", "cancel"],
        "  cf task accept|cancel T-3         move a task you asked for\n",
    ),
    (
        &["task", "reopen"],
        "  cf task reopen T-3 \"…\"            send a finished or failed task back with a follow-up\n",
    ),
    (
        &["task", "pause"],
        "  cf task pause T-3                 stop a worker's task: the agent stops, its window and work wait\n",
    ),
    (
        &["task", "resume"],
        "  cf task resume T-3 \"…\"            go on with it: the same window, with your words\n",
    ),
    (
        &["tell"],
        concat!(
            "  cf tell T-3 \"…\"                   stop T-3 and put this to its window: its answer arrives as\n",
            "                                    a message; then cf task resume T-3 \"…\"\n",
        ),
    ),
    (
        &["inbox"],
        "  cf inbox [read m-12]              what is waiting for you, or one message in full\n",
    ),
    (
        &["ask"],
        concat!(
            "  cf ask \"…\"                        a question to the chief (a member's; the chief asks the\n",
            "                                    human in its own terminal)\n",
        ),
    ),
    (
        &["note"],
        "  cf note \"…\" [--human]             something they should know; nothing waits on it\n",
    ),
    (
        &["answer"],
        "  cf answer m-12 \"…\"                answer a question put to you\n",
    ),
    (
        &["staff"],
        "  cf staff                           the members: roles and tiers\n",
    ),
    (
        &["whoami"],
        "  cf whoami                         your project, role and current task\n",
    ),
    (
        &["history"],
        concat!(
            "  cf history [--page 2] [--find \"…\"] [--tools]\n",
            "                                    the chief's: what the human and the chiefs before you\n",
            "                                    said, newest page first (after the chief was switched)\n",
        ),
    ),
];

#[test]
fn every_board_command_answers_help_with_its_usage_and_asks_the_board_nothing() {
    for (path, usage) in BOARD {
        for word in ["--help", "-h"] {
            let api = api();
            let mut args = path.to_vec();
            args.push(word);
            assert_eq!(
                window(&api, &args),
                (Some(0), usage.to_owned(), String::new()),
                "{args:?}"
            );
            assert!(api.received().is_empty(), "{args:?}: it asked the board");
        }
    }
}

#[test]
fn the_word_may_stand_after_the_number_a_command_takes_first_or_beside_its_flags() {
    let cases: [(&[&str], &[&str]); 12] = [
        (&["task", "get", "T-3", "--help"], &["task", "get"]),
        (
            &["task", "get", "T-3", "--transcript", "-h"],
            &["task", "get"],
        ),
        (&["task", "get", "--transcript", "--help"], &["task", "get"]),
        (&["task", "done", "T-3", "-h"], &["task", "done"]),
        (&["task", "accept", "T-3", "--help"], &["task", "accept"]),
        (&["task", "resume", "T-3", "-h"], &["task", "resume"]),
        (
            &["task", "add", "--tier", "light", "--help"],
            &["task", "add"],
        ),
        (
            &["task", "add", "--help", "--tier", "light"],
            &["task", "add"],
        ),
        (&["note", "--human", "--help"], &["note"]),
        (&["tell", "T-3", "-h"], &["tell"]),
        (&["answer", "m-12", "--help"], &["answer"]),
        (&["inbox", "read", "--help"], &["inbox"]),
    ];
    for (args, path) in cases {
        let api = api();
        assert_eq!(
            window(&api, args),
            (Some(0), usage_of(path).to_owned(), String::new()),
            "{args:?}"
        );
        assert!(api.received().is_empty(), "{args:?}: it asked the board");
    }
    // The list of the inbox, and the read of one message, are one command to ask help of.
    let api = api();
    assert_eq!(window(&api, &["inbox", "read", "m-12", "-h"]).0, Some(0));
    assert!(api.received().is_empty());
}

#[test]
fn json_asked_for_is_the_usage_in_a_key() {
    let api = api();
    let (code, out, err) = window(&api, &["note", "--help", "--json"]);
    assert_eq!((code, err.as_str()), (Some(0), ""));
    let said: Value = serde_json::from_str(&out).expect("JSON");
    assert_eq!(said, json!({ "usage": usage_of(&["note"]).trim_end() }));
    assert!(api.received().is_empty());
}

#[test]
fn help_and_its_spellings_ahead_of_a_command_are_the_whole_usage_still() {
    let usage = board_usage();
    assert!(usage.starts_with("cf inside a ConsensFlow window"));
    for args in [&["--help"][..], &["-h"], &["help"], &[]] {
        assert_eq!(
            window(&api(), args),
            (Some(0), usage.clone(), String::new())
        );
    }
    let tasks = window(&api(), &["task", "--help"]).1;
    for args in [&["task", "-h"][..], &["task", "help"]] {
        assert_eq!(window(&api(), args).1, tasks, "{args:?}");
    }
}

#[test]
fn every_line_of_the_usage_is_a_commands_to_answer_help_with() {
    let usage = board_usage();
    let wanted: Vec<&str> = BOARD.iter().flat_map(|(_, lines)| lines.lines()).collect();
    // The commands, from the first to the blank line that ends them.
    let commands: Vec<&str> = usage
        .lines()
        .skip_while(|line| !line.starts_with("  cf "))
        .take_while(|line| !line.is_empty())
        .collect();
    assert!(commands.len() > 20, "{commands:?}");
    for line in &commands {
        assert!(
            wanted.contains(line),
            "no command answers --help with {line:?}"
        );
    }
    // And each line of the table is one of the usage's: none is made up.
    let mut said = wanted.clone();
    said.sort_unstable();
    said.dedup();
    assert_eq!(said.len(), commands.len());
}

#[test]
fn a_text_that_holds_the_word_among_others_or_after_a_double_dash_is_text() {
    // Each posts what it was given, and the board is asked as it is for any text.
    let cases: [(&[&str], &str, Value); 12] = [
        (
            &["note", "--", "--help"],
            "/api/notes",
            json!({ "body": "--help" }),
        ),
        (
            &["note", "--human", "--", "-h"],
            "/api/notes",
            json!({ "body": "-h", "to": "human" }),
        ),
        (
            &["note", "see", "--help"],
            "/api/notes",
            json!({ "body": "see --help" }),
        ),
        (
            &["note", "--help", "me"],
            "/api/notes",
            json!({ "body": "--help me" }),
        ),
        (
            &["note", "run cf --help"],
            "/api/notes",
            json!({ "body": "run cf --help" }),
        ),
        (
            &["ask", "--", "--help"],
            "/api/questions",
            json!({ "body": "--help" }),
        ),
        (
            &["tell", "T-3", "--", "--help"],
            "/api/tasks/3/tell",
            json!({ "body": "--help" }),
        ),
        (
            &["answer", "m-12", "--", "-h"],
            "/api/answers",
            json!({ "question": 12, "body": "-h" }),
        ),
        (
            &["task", "done", "T-3", "--", "--help"],
            "/api/tasks/3/done",
            json!({ "body": "--help" }),
        ),
        (
            &["task", "reopen", "T-3", "see", "--help"],
            "/api/tasks/3/reopen",
            json!({ "body": "see --help" }),
        ),
        (
            &["task", "add", "--tier", "light", "--", "--help"],
            "/api/tasks",
            json!({ "tier": "light", "body": "--help" }),
        ),
        (
            &["task", "add", "--tier", "light", "fix", "-h", "now"],
            "/api/tasks",
            json!({ "tier": "light", "body": "fix -h now" }),
        ),
    ];
    for (args, path, body) in cases {
        let api = api();
        window(&api, args);
        assert_eq!(
            asked(&api),
            [("POST".to_owned(), path.to_owned(), Some(body))],
            "{args:?}"
        );
    }
}

#[test]
fn the_word_a_flag_takes_is_its_value() {
    let api = api();
    window(&api, &["history", "--find", "--help"]);
    assert_eq!(
        asked(&api)
            .first()
            .map(|(method, path, _)| (method.as_str(), path.as_str())),
        Some(("GET", "/api/history?find=--help"))
    );
}

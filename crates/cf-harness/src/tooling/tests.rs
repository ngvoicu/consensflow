//! The answers the evals and the live tools are given, each asked as the
//! test binary is: a question as JSON, an answer as JSON.

use std::fs;
use std::path::Path;

use serde_json::{json, Value};

use super::*;

const LAUNCH: &str = "00000000-0000-4000-8000-000000000001";

/// The question `op`, with `fields` besides.
fn ask(op: &str, fields: Value) -> Value {
    let mut question = json!({ "op": op });
    for (name, value) in fields.as_object().into_iter().flatten() {
        question[name] = value.clone();
    }
    question
}

/// A Claude transcript of the session `s1` in a config folder of its own: the
/// user's turn, the assistant's answer, and, when `ended`, the record that says
/// the turn took its time and is over.
fn claude_folder(ended: bool) -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    let base = |record: Value| {
        let mut record = record;
        record["sessionId"] = json!("s1");
        record["isSidechain"] = json!(false);
        record
    };
    let mut records = vec![
        base(json!({
            "type": "user", "uuid": "u1", "parentUuid": null,
            "message": { "role": "user", "content": "Hello" },
        })),
        base(json!({
            "type": "assistant", "uuid": "a1", "parentUuid": "u1",
            "message": {
                "id": "m1", "role": "assistant",
                "content": [{ "type": "text", "text": "Hi" }],
                "stop_reason": if ended { json!("end_turn") } else { Value::Null },
            },
        })),
    ];
    if ended {
        records.push(base(json!({
            "type": "system", "subtype": "turn_duration", "uuid": "d1", "parentUuid": "a1",
            "durationMs": 5, "messageCount": 2,
        })));
    }
    let lines: Vec<String> = records.iter().map(Value::to_string).collect();
    fs::create_dir_all(root.path().join("projects")).unwrap();
    fs::write(
        root.path().join("projects").join("s1.jsonl"),
        format!("{}\n", lines.join("\n")),
    )
    .unwrap();
    root
}

/// A look at the session `s1` in the Claude config folder `root`.
fn look_at(root: &Path) -> Result<Value, String> {
    answer(&ask(
        "records",
        json!({
            "kind": "claude-code",
            "session": "s1",
            "env": { "CLAUDE_CONFIG_DIR": root.to_str().unwrap(), "HOME": root.to_str().unwrap() },
        }),
    ))
}

#[test]
fn a_record_is_read_whole_and_a_turn_that_ended_is_settled() {
    let root = claude_folder(true);
    let looked = look_at(root.path()).unwrap();
    assert_eq!(looked["settled"], json!(true));
    let items: Vec<(&str, &str, &str)> = looked["reading"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| {
            (
                item["id"].as_str().unwrap(),
                item["role"].as_str().unwrap(),
                item["text"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(items, [("u1", "user", "Hello"), ("m1", "assistant", "Hi")]);
    assert_eq!(
        looked["reading"]["settlement"],
        json!({ "state": "settled" })
    );
}

#[test]
fn a_turn_still_at_work_is_not_settled() {
    let root = claude_folder(false);
    let looked = look_at(root.path()).unwrap();
    assert_eq!(looked["settled"], json!(false));
    assert_eq!(
        looked["reading"]["settlement"],
        json!({ "state": "in-flight" })
    );
}

#[test]
fn a_conversation_the_harness_kept_no_record_of_reads_as_unknown_and_settled() {
    let root = tempfile::tempdir().unwrap();
    let looked = look_at(root.path()).unwrap();
    assert_eq!(looked["reading"]["unknown"], json!(true));
    assert!(looked["reading"]["reason"].is_string());
    // Nothing is in flight in a conversation with no record: the dispatcher may send to it.
    assert_eq!(looked["settled"], json!(true));
}

#[test]
fn a_question_that_cannot_be_answered_says_why() {
    let failed = |question: Value| answer(&question).unwrap_err();
    assert!(failed(json!({})).contains("names no op"));
    assert!(failed(json!({ "op": "nothing" })).contains("asks for nothing"));
    let env = json!({ "HOME": "/h" });
    assert_eq!(
        failed(json!({ "op": "records", "kind": "claude", "session": "s", "env": env })),
        "no harness runs as claude"
    );
    assert!(failed(json!({ "op": "records", "kind": "codex", "session": "s" })).contains("env"));
    assert!(failed(json!({ "op": "records", "kind": "codex", "env": env })).contains("session"));
    assert!(failed(ask("window_text", json!({}))).contains("names no text"));
}

#[test]
fn a_variable_the_caller_removed_is_not_in_the_environment_a_record_is_read_in() {
    let root = claude_folder(true);
    // CLAUDE_CONFIG_DIR given null is gone: the transcript is then looked for under HOME,
    // which holds none, so the folder that has it is not read.
    let nowhere = tempfile::tempdir().unwrap();
    let looked = answer(&ask(
        "records",
        json!({
            "kind": "claude-code",
            "session": "s1",
            "env": { "CLAUDE_CONFIG_DIR": null, "HOME": nowhere.path().to_str().unwrap() },
        }),
    ))
    .unwrap();
    assert_eq!(looked["reading"]["unknown"], json!(true));
    assert!(root.path().exists());
}

#[test]
fn a_window_starts_as_the_harness_opens_one_on_a_new_conversation() {
    let start = |agent: Value, session: Value, seed: Value| {
        answer(&ask(
            "start",
            json!({ "agent": agent, "session": session, "seed": seed }),
        ))
        .unwrap()
    };
    assert_eq!(
        start(
            json!({ "kind": "claude-code", "model": "claude-haiku-5-5", "effort": "low" }),
            json!("s-1"),
            Value::Null,
        ),
        json!({
            "command": "claude",
            "args": [
                "--session-id", "s-1", "--model", "claude-haiku-5-5", "--effort", "low",
                "--permission-mode", "bypassPermissions",
            ],
            "env": {},
            "dropEnv": ["ANTHROPIC_API_KEY"],
        })
    );
    // Devin takes its first message in another way than as an argument, and the
    // effort joined to its model; Codex names no session of its own to start on.
    assert_eq!(
        start(
            json!({ "kind": "devin", "model": "swe-2", "effort": "medium" }),
            Value::Null,
            json!("Go on"),
        ),
        json!({
            "command": "devin",
            "args": [
                "--model", "swe-2-medium", "--permission-mode", "dangerous",
                "--respect-workspace-trust", "false",
            ],
            "prompt": "Go on",
            "env": {},
            "dropEnv": [],
        })
    );
    assert_eq!(
        start(json!({ "kind": "codex" }), json!("ignored"), Value::Null)["args"],
        json!(["--dangerously-bypass-approvals-and-sandbox"])
    );
    // Claude needs a conversation id of ConsensFlow's to start on, and a kind the build does not run has no window.
    assert_eq!(
        start(json!({ "kind": "claude-code" }), Value::Null, Value::Null),
        Value::Null
    );
    assert_eq!(
        start(json!({ "kind": "kimi" }), json!("s"), Value::Null),
        Value::Null
    );
}

#[test]
fn text_is_given_as_a_window_and_the_console_take_it() {
    let said = |op: &str, text: &str| answer(&ask(op, json!({ "text": text }))).unwrap();
    // A CR before a newline goes; a control character is shown as its picture.
    assert_eq!(said("window_text", "a\r\nb\u{1b}c"), json!("a\nb\u{241b}c"));
    // What the console would drop is spelled in ASCII.
    assert_eq!(said("console_text", "a — b → €"), json!("a -- b -> EUR"));
}

#[test]
fn a_turn_is_interrupted_as_the_harness_says() {
    let keys = |kind: &str| answer(&ask("interrupt", json!({ "kind": kind }))).unwrap();
    // Escape once, unless the harness says more.
    assert_eq!(
        keys("claude-code"),
        json!({ "presses": 1, "closeAfterMs": null })
    );
    // Devin's rewind opens at a turn just ended: a second Escape, and one more a second later.
    assert_eq!(keys("devin"), json!({ "presses": 2, "closeAfterMs": 1000 }));
    for kind in ["codex", "pi", "opencode"] {
        assert_eq!(keys(kind)["presses"], json!(1), "{kind}");
    }
}

#[test]
fn a_claude_window_gets_the_settings_file_of_its_launch() {
    let home = tempfile::tempdir().unwrap();
    let settings = |board_questions: bool| {
        let question = ask(
            "claude_settings",
            json!({
                "env": { "CONSENSFLOW_HOME": home.path().to_str().unwrap() },
                "launch": LAUNCH,
                "boardQuestions": board_questions,
            }),
        );
        let args = answer(&question).unwrap();
        let file = args[1].as_str().unwrap().to_owned();
        assert_eq!(args[0], json!("--settings"));
        serde_json::from_str::<Value>(&fs::read_to_string(file).unwrap()).unwrap()
    };
    let members = settings(true);
    assert_eq!(
        members["permissions"]["defaultMode"],
        json!("bypassPermissions")
    );
    assert_eq!(members["skipDangerousModePermissionPrompt"], json!(true));
    assert_eq!(
        members["hooks"]["PreToolUse"][0]["matcher"],
        json!("AskUserQuestion")
    );
    // A question that is not put to the board is Claude's own: the hook is not there.
    assert_eq!(settings(false)["hooks"]["PreToolUse"], json!([]));
    assert!(home
        .path()
        .join("integrations")
        .join("claude")
        .join(LAUNCH)
        .join("settings.json")
        .is_file());
    // The id of a launch is a uuid, and the file is written nowhere else.
    let refused = answer(&ask(
        "claude_settings",
        json!({ "env": { "CONSENSFLOW_HOME": "/nowhere" }, "launch": "../live" }),
    ));
    assert!(refused.unwrap_err().contains("not a launch id"));
}

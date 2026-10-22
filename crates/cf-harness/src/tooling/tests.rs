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
fn a_window_resumes_the_conversation_the_harness_keeps_when_the_question_says_so() {
    let resumed = |agent: Value, session: Value| {
        answer(&ask(
            "start",
            json!({ "agent": agent, "session": session, "seed": null, "resume": true }),
        ))
        .unwrap()
    };
    assert_eq!(
        resumed(
            json!({ "kind": "claude-code", "model": "claude-haiku-5-5", "effort": "low" }),
            json!("s-1"),
        )["args"],
        json!([
            "--resume",
            "s-1",
            "--model",
            "claude-haiku-5-5",
            "--effort",
            "low",
            "--permission-mode",
            "bypassPermissions",
        ])
    );
    assert_eq!(
        resumed(json!({ "kind": "codex" }), json!("t-1"))["args"],
        json!([
            "resume",
            "t-1",
            "--dangerously-bypass-approvals-and-sandbox"
        ])
    );
    // A conversation to resume has to be named.
    assert_eq!(
        resumed(json!({ "kind": "claude-code" }), Value::Null),
        Value::Null
    );
}

#[test]
fn a_devin_windows_wire_says_its_mode_and_the_command_the_daemon_gives_it_before_it_is_ready() {
    let folder = tempfile::tempdir().unwrap();
    let wire = folder.path().join("wire.jsonl");
    let ready = || answer(&ask("devin_ready", json!({ "wire": wire }))).unwrap();
    // A log not written yet says nothing, and the window is given nothing.
    assert_eq!(
        ready(),
        json!({ "shown": null, "mode": null, "command": null })
    );
    let configured = json!({ "sessionId": "mild-coin", "update": {
        "sessionUpdate": "config_option_update",
        "configOptions": [{ "id": "mode", "currentValue": "normal" }] } });
    fs::write(&wire, format!("{configured}\n")).unwrap();
    assert_eq!(
        ready(),
        json!({ "shown": "mild-coin", "mode": "normal", "command": "/bypass" })
    );
    let changed = json!({ "sessionId": "mild-coin",
        "update": { "sessionUpdate": "current_mode_update", "currentModeId": "bypass" } });
    fs::write(&wire, format!("{configured}\n{changed}\n")).unwrap();
    assert_eq!(
        ready(),
        json!({ "shown": "mild-coin", "mode": "bypass", "command": null })
    );
    assert!(
        answer(&ask("devin_ready", json!({}))).is_err(),
        "it names a wire"
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
fn the_key_toward_a_yes_is_the_one_the_engine_reads_in_a_harnesss_question_about_its_folder() {
    let read = |kind: &str, screen: &[&str]| {
        answer(&ask("trust", json!({ "kind": kind, "screen": screen }))).unwrap()
    };
    let claude = [
        " Quick safety check",
        " ❯ No, exit",
        "   Yes, I trust this folder",
    ];
    assert_eq!(
        read("claude-code", &claude),
        json!({ "asks": true, "key": "down", "prompt": false })
    );
    // Its cursor drawn otherwise: asked, and no key known.
    assert_eq!(
        read(
            "claude-code",
            &[
                " Quick safety check",
                " > No, exit",
                "   Yes, I trust this folder"
            ]
        ),
        json!({ "asks": true, "key": null, "prompt": false })
    );
    let codex = [
        "  Trust this folder? Codex can read, edit, and run files here.",
        "› 1. Trust and continue",
        "  2. Back to Agent Command Center",
    ];
    // A harness that never asks as ConsensFlow opens it reads none, whatever the
    // screen shows: Codex's own question is its window's opened alone.
    let none = json!({ "asks": false, "key": null, "prompt": false });
    assert_eq!(read("codex", &codex), none);
    for kind in ["pi", "opencode", "devin"] {
        assert_eq!(read(kind, &claude), none, "{kind}");
    }
    let home = tempfile::tempdir().unwrap();
    let directory = home.path().join("project");
    std::fs::create_dir(&directory).unwrap();
    let asked = answer(&ask(
        "trust",
        json!({ "kind": "claude-code", "screen": claude, "directory": directory }),
    ))
    .unwrap();
    let real = std::fs::canonicalize(&directory).unwrap();
    let real = real.to_string_lossy();
    assert_eq!(
        asked["trusts"],
        json!({ "folder": real.strip_prefix(r"\\?\").unwrap_or(&real), "inside": true })
    );
    let prompt = [
        "❯ Try \"fix lint errors\"",
        "  ⏵⏵ bypass permissions on (shift+tab to cycle)",
    ];
    assert_eq!(
        read("claude-code", &prompt),
        json!({ "asks": false, "key": null, "prompt": true })
    );
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

/// The argv the window `argv` opens on, as the daemon would open it, with `env`.
fn program_of(argv: &[&str], env: Value) -> Result<Vec<String>, String> {
    let words = answer(&ask("program", json!({ "argv": argv, "env": env })))?;
    Ok(words
        .as_array()
        .unwrap()
        .iter()
        .map(|word| word.as_str().unwrap().to_owned())
        .collect())
}

#[test]
fn a_program_that_is_no_script_opens_a_window_as_it_is() {
    let argv = [
        "/home/me/.local/bin/claude",
        "--model",
        "a b",
        "--session-id",
        "s-1",
    ];
    assert_eq!(program_of(&argv, json!({})).unwrap(), argv);
    assert_eq!(program_of(&[], json!({})).unwrap(), Vec::<String>::new());
}

#[test]
fn an_npm_shim_opens_a_window_as_the_node_and_the_script_it_runs() {
    // The pane host starts a file with each argument quoted the way programs read
    // them, which cmd.exe does not: the shim is read for what it runs, on any platform.
    let folder = tempfile::tempdir().unwrap();
    let script = folder
        .path()
        .join("node_modules")
        .join("claude")
        .join("cli.js");
    fs::create_dir_all(script.parent().unwrap()).unwrap();
    fs::write(&script, "").unwrap();
    let node = folder.path().join("node.exe");
    fs::write(&node, "").unwrap();
    let shim = folder.path().join("claude.cmd");
    fs::write(
        &shim,
        "@ECHO off\r\nSETLOCAL\r\n\"%_prog%\" \"%dp0%\\node_modules\\claude\\cli.js\" %*\r\n",
    )
    .unwrap();
    let shim = shim.to_string_lossy().into_owned();
    assert_eq!(
        program_of(&[&shim, "--model", "a b"], json!({})).unwrap(),
        [
            node.to_string_lossy().into_owned(),
            script.to_string_lossy().into_owned(),
            "--model".to_owned(),
            "a b".to_owned()
        ]
    );
}

#[test]
fn a_script_of_another_shape_cannot_open_a_window_and_the_error_says_so() {
    let folder = tempfile::tempdir().unwrap();
    let script = folder.path().join("codex.cmd");
    fs::write(&script, "@echo off\r\necho hello\r\n").unwrap();
    let script = script.to_string_lossy().into_owned();
    let refused = program_of(&[&script], json!({})).unwrap_err();
    assert!(
        refused.contains("is not an npm shim, and only cmd.exe could run it in a window"),
        "{refused}"
    );
    assert!(answer(&ask("program", json!({ "env": {} })))
        .unwrap_err()
        .contains("names no argv"));
    assert!(
        answer(&ask("program", json!({ "argv": ["a", 7], "env": {} })))
            .unwrap_err()
            .contains("is not text")
    );
}

#[test]
fn the_newest_session_of_a_folder_is_asked_of_the_store_the_environment_names() {
    let root = tempfile::tempdir().unwrap();
    let store = root.path().join("opencode.db");
    let db = rusqlite::Connection::open(&store).unwrap();
    db.execute_batch(
        "CREATE TABLE session (id TEXT PRIMARY KEY, directory TEXT, parent_id TEXT, time_created INTEGER);
         INSERT INTO session VALUES ('mine', '/work/a', NULL, 5000);",
    )
    .unwrap();
    drop(db);
    let asked = |kind: &str, workspace: &str, since: Value| {
        answer(&ask(
            "newest_session",
            json!({
                "kind": kind, "workspace": workspace, "since": since,
                "env": { "OPENCODE_DB": store.to_str().unwrap() },
            }),
        ))
    };
    assert_eq!(
        asked("opencode", "/work/a", json!(1000)).unwrap(),
        json!("mine")
    );
    // Nothing yet is null, which is not a failure.
    assert_eq!(
        asked("opencode", "/work/a", json!(6000)).unwrap(),
        Value::Null
    );
    assert_eq!(asked("opencode", "/work/b", json!(0)).unwrap(), Value::Null);
    // Claude and Pi are opened on an id they are given.
    let refused = asked("claude-code", "/work/a", json!(0)).unwrap_err();
    assert!(
        refused.contains("opens on a session id of its own choosing"),
        "{refused}"
    );
    assert!(asked("kimi", "/work/a", json!(0))
        .unwrap_err()
        .contains("no harness runs as kimi"));
    assert!(asked("opencode", "/work/a", Value::Null)
        .unwrap_err()
        .contains("names no since"));
}

#[test]
fn the_specmint_variant_a_harness_has_installed_is_asked_of_its_adapter_in_the_home_the_environment_names(
) {
    let home = tempfile::tempdir().unwrap();
    let skills = home
        .path()
        .join(".agents")
        .join("skills")
        .join("specmint-tdd");
    fs::create_dir_all(&skills).unwrap();
    fs::write(skills.join("SKILL.md"), "---\nname: specmint-tdd\n---\n").unwrap();
    let env = json!({ "HOME": home.path().to_str().unwrap(), "CONSENSFLOW_HOME": home.path().join("cf").to_str().unwrap() });
    let asked = |kind: &str, variant: &str| {
        answer(&ask(
            "specmint",
            json!({ "kind": kind, "variant": variant, "env": env }),
        ))
    };
    // Codex, OpenCode, Pi and Devin read the agents' folder: it has the variant there.
    for kind in ["codex", "opencode", "pi", "devin"] {
        assert_eq!(
            asked(kind, "tdd").unwrap(),
            json!({ "installed": true, "skill": skills.join("SKILL.md").to_str().unwrap() }),
            "{kind}"
        );
        // Another variant is no copy of it: ConsensFlow would write its own.
        let other = asked(kind, "core").unwrap();
        assert_eq!(other["installed"], false, "{kind}");
        assert!(
            other["skill"].as_str().unwrap().ends_with("SKILL.md"),
            "{kind}"
        );
        assert!(
            other["skill"].as_str().unwrap().contains("specmint-core"),
            "{kind}"
        );
    }
    // Claude Code does not read it: it has none of its own installed.
    assert_eq!(asked("claude-code", "tdd").unwrap()["installed"], false);
    assert!(asked("claude-code", "full")
        .unwrap_err()
        .contains("variant is none of"));
    assert!(asked("kimi", "core")
        .unwrap_err()
        .contains("no harness runs as kimi"));
}

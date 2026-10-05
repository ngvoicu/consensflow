//! How a Devin window is launched, and the role it is given (`tests/adapter-devin.test.mjs`, and the Devin cases
//! of `tests/role-skills.test.mjs`).

use std::fs;
use std::path::Path;

use cf_base::path;
use cf_harness::contract::{Agent, Prepared};
use cf_harness::testing::called;
use serde_json::{json, Value};

use super::fixtures::*;

#[test]
fn launches_devin_on_its_own_config_in_full_permission_mode_with_the_task_in_a_prompt_file() {
    let home = Home::new();
    let plan = prepare(&home.adapter(), &Request::default()).unwrap();
    let root = home.folder();
    assert_eq!(plan.native_session, None, "Devin names the session itself");
    let mut argv = vec![home.executable.clone()];
    argv.extend(words(&["--config", &path::join(&[&root, "config.json"])]));
    argv.extend(words(&[
        "--model",
        "swe-1-6-slow",
        "--permission-mode",
        "dangerous",
        "--respect-workspace-trust",
        "false",
        "--prompt-file",
        &path::join(&[&root, "prompt.txt"]),
    ]));
    assert_eq!(plan.argv, argv);
    assert_eq!(
        fs::read_to_string(path::join(&[&root, "prompt.txt"])).unwrap(),
        TASK
    );
    assert_eq!(
        plan.env
            .iter()
            .find(|(name, _)| name == "CHISEL_PURE_ACP_WIRE_LOG"),
        Some(&(
            "CHISEL_PURE_ACP_WIRE_LOG".to_owned(),
            path::join(&[&root, "wire.jsonl"])
        ))
    );
    let config: Value =
        serde_json::from_str(&fs::read_to_string(path::join(&[&root, "config.json"])).unwrap())
            .unwrap();
    let mut hooks: Vec<&String> = config["hooks"].as_object().unwrap().keys().collect();
    hooks.sort();
    assert_eq!(hooks, ["PreToolUse", "SessionStart"]);
    assert_eq!(
        config["hooks"]["PreToolUse"][0]["matcher"],
        "ask_user_question"
    );
    assert_eq!(config["auto_update"], false);
}

#[test]
fn leaves_the_chiefs_question_tool_to_devins_own_dialog_where_the_human_answers_it() {
    let home = Home::new();
    prepare(&home.adapter(), &Request::chief()).unwrap();
    let config: Value = serde_json::from_str(
        &fs::read_to_string(path::join(&[&home.folder(), "config.json"])).unwrap(),
    )
    .unwrap();
    assert_eq!(config["hooks"]["PreToolUse"], json!([]));
}

#[test]
fn asks_devin_its_version_once_not_at_every_launch_and_refuses_one_too_old() {
    let home = Home::new();
    home.says("devin 3000.11.3 (9c803229faa4)");
    let adapter = home.adapter();
    let other = |n: u8| format!("3d4e5f60-7182-4394-8dbe-2f3a4b5c6d7{n}");
    let question = format!("{} --version", called(Path::new(&home.executable)));
    prepare_as(&adapter, &Request::default(), &other(1)).unwrap();
    prepare_as(&adapter, &Request::default(), &other(2)).unwrap();
    assert_eq!(
        home.asked(),
        std::slice::from_ref(&question),
        "one question for an unchanged Devin"
    );
    // An update gives the file another size and time: it is asked again.
    fs::write(&home.executable, "#!/bin/sh\n# an older Devin\nexit 0\n").unwrap();
    home.says("devin 3000.9.1 (00000000)");
    let Err(refused) = prepare_as(&adapter, &Request::default(), &other(3)) else {
        panic!("a Devin too old");
    };
    assert!(refused.contains("3000.10.21 or newer"), "{refused}");
    assert_eq!(home.asked(), std::slice::from_ref(&question));
}

#[test]
fn launches_a_catalog_agent_on_devins_own_id_the_family_with_its_level() {
    let home = Home::new();
    let request = Request {
        agent: Some(Agent {
            model: Some("claude-opus-5-5"),
            effort: Some("max"),
            ..Agent::default()
        }),
        ..Request::default()
    };
    let plan = prepare(&home.adapter(), &request).unwrap();
    let at = plan.argv.iter().position(|arg| arg == "--model").unwrap();
    assert_eq!(plan.argv[at + 1], "claude-opus-5-5-max");
}

#[test]
fn resumes_the_session_it_has() {
    let home = Home::new();
    let plan = prepare(&home.adapter(), &Request::resumed("mild-coin")).unwrap();
    assert_eq!(plan.native_session.as_deref(), Some("mild-coin"));
    assert_eq!(
        plan.argv[3..],
        words(&[
            "--resume",
            "mild-coin",
            "--model",
            "swe-1-6-slow",
            "--permission-mode",
            "dangerous",
            "--respect-workspace-trust",
            "false",
        ])
    );
}

#[test]
fn tells_devin_on_windows_to_name_files_the_way_its_file_tools_write_them_and_nowhere_else() {
    let role = "# ConsensFlow worker\n\nRole text for the test.";
    let role_of = |home: &Home| {
        let plan = prepare(&home.adapter(), &Request::default()).unwrap();
        let (_, file) = plan
            .env
            .iter()
            .find(|(name, _)| name == "CF_DEVIN_ROLE_FILE")
            .unwrap();
        fs::read_to_string(file).unwrap()
    };
    let windows = role_of(&Home::windows());
    assert!(windows.starts_with(role));
    assert!(windows.contains("never /c/\u{2026} paths"), "{windows}");
    // A Windows machine is Windows whatever its environment says.
    let elsewhere = role_of(&Home::new());
    assert_eq!(
        elsewhere,
        if cfg!(windows) {
            windows
        } else {
            role.to_owned()
        }
    );
}

#[test]
fn every_role_enters_devin_with_its_whole_text_already_loaded() {
    let staff = "| saved-worker | worker | Complex work |";
    for name in ["chief", "advisor", "worker", "reviewer", "designer"] {
        let home = Home::new();
        let content = if name == "chief" {
            format!("# ConsensFlow chief\n\n{staff}\n\nPreserve \"quotes\", `backticks`, $HOME\nand newlines.")
        } else {
            format!(
                "# ConsensFlow {name}\n\nPreserve \"quotes\", `backticks`, $HOME\nand newlines."
            )
        };
        let request = Request {
            role: name,
            instructions: content.clone(),
            ..Request::default()
        };
        let plan = prepare(&home.adapter(), &request).unwrap();
        let (_, file) = plan
            .env
            .iter()
            .find(|(var, _)| var == "CF_DEVIN_ROLE_FILE")
            .unwrap();
        assert!(
            file.replace('\\', "/")
                .ends_with(&format!("/consensflow-{name}/SKILL.md")),
            "{file}"
        );
        let loaded = fs::read_to_string(file).unwrap();
        assert!(loaded.contains(&content), "the whole role text is loaded");
        assert_eq!(loaded.contains(staff), name == "chief", "{name}");
    }
}

#[test]
fn gives_each_launch_a_role_file_of_its_own_one_chief_never_reads_another_projects_staff() {
    let home = Home::new();
    let adapter = home.adapter();
    let first = prepare_as(
        &adapter,
        &Request {
            instructions: "| saved-worker | worker |".to_owned(),
            ..Request::chief()
        },
        LAUNCH,
    )
    .unwrap();
    let second = prepare_as(
        &adapter,
        &Request {
            instructions: "no staff".to_owned(),
            ..Request::chief()
        },
        "3d4e5f60-7182-4394-8dbe-2f3a4b5c6d7e",
    )
    .unwrap();
    let file = |plan: &Prepared| {
        plan.env
            .iter()
            .find(|(name, _)| name == "CF_DEVIN_ROLE_FILE")
            .unwrap()
            .1
            .clone()
    };
    assert!(file(&first).starts_with(&home.folder()), "{}", file(&first));
    let (one, two) = (
        fs::read_to_string(file(&first)).unwrap(),
        fs::read_to_string(file(&second)).unwrap(),
    );
    assert!(one.starts_with("| saved-worker | worker |"));
    assert!(!two.contains("saved-worker"));
}

#[test]
fn refuses_a_window_without_its_role_text_and_writes_it_in_a_private_file() {
    let home = Home::new();
    let empty = Request {
        instructions: String::new(),
        ..Request::default()
    };
    // On Windows too: the note on naming files there is added to a role
    // text, never made one (parity:launch on zeewin, 2026-10-05).
    let refused = prepare(&home.adapter(), &empty);
    assert_eq!(
        refused.err().as_deref(),
        Some("the worker window needs its role text")
    );
    let home = Home::new();
    let plan = prepare(&home.adapter(), &Request::default()).unwrap();
    let (_, file) = plan
        .env
        .iter()
        .find(|(name, _)| name == "CF_DEVIN_ROLE_FILE")
        .unwrap();
    // Windows has no POSIX modes; its files answer 0o666 whatever the writer asked.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(file).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    #[cfg(not(unix))]
    let _ = file;
}

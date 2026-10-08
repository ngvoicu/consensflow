//! What Devin's window runs with, as the cases of Node's Devin adapter suite
//! held that reach no window: the version asked of it, the config of its
//! launch's own, the owner's config read as Devin reads it, and the prompt
//! file. Node's tests took any filename-safe word for a launch, and so does
//! this.

use std::fs;
use std::path::PathBuf;
use std::rc::Rc;

use cf_base::env::Env;
use serde_json::{json, Value};
use tempfile::TempDir;

use super::*;
use crate::seams::Bundle;
use crate::testing::{called, fake_executable, finished, Fakes, ScriptedProcesses};

const LAUNCH: &str = "launch-1";

/// A home of its own: ConsensFlow's folder, Devin's config folder and a
/// stand-in `devin` on PATH that says its version.
struct Home {
    dir: TempDir,
    fakes: Fakes,
    services: Services,
    executable: String,
}

impl Home {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_string_lossy().into_owned();
        let at = |folder: &str| path::join(&[&root, folder]);
        let env = Env::from_vars([
            ("HOME", at("home")),
            ("CONSENSFLOW_HOME", at("consensflow")),
            // Devin's config folder, on Windows (`%APPDATA%`) as elsewhere.
            ("XDG_CONFIG_HOME", at("config")),
            ("APPDATA", at("config")),
            ("PATH", at("bin")),
        ]);
        fs::create_dir_all(dir.path().join("bin")).unwrap();
        let executable = fake_executable(&dir.path().join("bin").join("devin"));
        let fakes = Fakes::new(&env);
        let services = fakes.services(&env, dir.path());
        let home = Self {
            dir,
            fakes,
            services,
            executable: executable.to_string_lossy().into_owned(),
        };
        home.says("devin 3000.11.3");
        home
    }

    /// What the stand-in `devin` says to `--version`, its line end with it.
    fn says(&self, version: &str) {
        self.fakes.processes.every_answer(
            &called(Path::new(&self.executable)),
            Ok(format!("{version}\n")),
        );
    }

    /// The owner's config, written where Devin keeps it.
    fn owner(&self, text: &str) {
        let folder = self.dir.path().join("config").join("devin");
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("config.json"), text).unwrap();
    }

    /// The folder a launch's own files are in.
    fn folder(&self, launch: &str) -> PathBuf {
        self.dir
            .path()
            .join("consensflow")
            .join("integrations")
            .join("devin")
            .join(launch)
    }

    fn launch(&self, launch: &str, board_questions: bool) -> Result<Integration, String> {
        finished(Box::pin(integration(
            &self.services,
            launch,
            &self.executable,
            board_questions,
        )))
    }

    /// The config written for a launch.
    fn written(&self, launch: &str) -> Value {
        let text = fs::read_to_string(self.folder(launch).join("config.json")).unwrap();
        serde_json::from_str(&text).unwrap()
    }
}

#[test]
fn a_launch_is_a_folder_of_one_to_two_hundred_safe_characters() {
    for launch in [
        "launch-1",
        "a",
        "A_b-C",
        "2c3d4e5f-6071-4283-9cad-1e2f3a4b5c6d",
        &"x".repeat(200),
    ] {
        assert!(valid(launch), "{launch}");
    }
    for launch in [
        "",
        "..",
        "a/b",
        "a b",
        "a.b",
        "caf\u{e9}",
        "a\n",
        &"x".repeat(201),
    ] {
        assert!(!valid(launch), "{launch:?}");
    }
}

#[test]
fn a_launch_that_names_no_safe_folder_is_refused_before_anything_is_asked_or_written() {
    let home = Home::new();
    let Err(refused) = home.launch("../elsewhere", true) else {
        panic!("a launch outside its folder");
    };
    assert_eq!(refused, "invalid Devin launch");
    assert!(home.fakes.processes.take_ran().is_empty());
    assert!(!home.dir.path().join("consensflow").exists());
}

#[test]
fn a_shell_word_is_quoted_with_each_quote_of_its_own_closed_around() {
    assert_eq!(quote("/a b/cf"), "'/a b/cf'");
    assert_eq!(quote("it's"), r"'it'\''s'");
    assert_eq!(quote("''"), r"''\'''\'''");
    assert_eq!(quote(""), "''");
}

#[test]
fn the_window_launches_on_a_config_of_its_own_with_hooks_that_log_each_turn() {
    let home = Home::new();
    let integration = home.launch(LAUNCH, true).unwrap();
    let root = home.folder(LAUNCH);
    let config = root.join("config.json").to_string_lossy().into_owned();
    assert_eq!(integration.args, ["--config", config.as_str()]);
    let wire = root.join("wire.jsonl").to_string_lossy().into_owned();
    assert_eq!(
        integration.env,
        [("CHISEL_PURE_ACP_WIRE_LOG".to_owned(), wire.clone())]
    );
    assert_eq!(integration.wire, wire);
    let written = home.written(LAUNCH);
    let hooks = written["hooks"].as_object().unwrap();
    let mut names: Vec<&String> = hooks.keys().collect();
    names.sort();
    assert_eq!(names, ["PreToolUse", "SessionStart"]);
    assert_eq!(
        written["hooks"]["PreToolUse"][0]["matcher"],
        "ask_user_question"
    );
    assert_eq!(written["auto_update"], false);
}

#[test]
fn a_session_starts_with_its_role_text_from_the_bundles_own_cf_named_in_full_and_quoted() {
    let home = Home::new();
    let mut services = home.services.clone();
    services.bundle = Bundle {
        pane_cf: "/a b/it's/bin/cf".to_owned(),
        ..services.bundle.clone()
    };
    finished(Box::pin(integration(
        &services,
        LAUNCH,
        &home.executable,
        true,
    )))
    .unwrap();
    assert_eq!(
        home.written(LAUNCH)["hooks"]["SessionStart"],
        json!([{ "matcher": "", "hooks": [{
            "type": "command",
            "command": r"'/a b/it'\''s/bin/cf' hook devin-session",
            "timeout": 5,
        }] }])
    );
}

#[test]
fn a_members_questions_are_answered_from_the_board_for_an_hour_and_the_chiefs_in_its_window() {
    let home = Home::new();
    home.launch("member", true).unwrap();
    home.launch("chief", false).unwrap();
    assert_eq!(
        home.written("member")["hooks"]["PreToolUse"],
        json!([{
            "matcher": "ask_user_question",
            "hooks": [{ "type": "command", "command": "cf hook devin", "timeout": 3600 }],
        }])
    );
    assert_eq!(home.written("chief")["hooks"]["PreToolUse"], json!([]));
}

#[test]
fn the_owners_config_is_kept_its_hooks_first_and_ours_after_and_its_updates_turned_off() {
    let home = Home::new();
    let owner = json!({
        "theme": "dark",
        "auto_update": true,
        "hooks": {
            "SessionStart": [{ "matcher": "startup", "hooks": [] }],
            "PreToolUse": [{ "matcher": "shell", "hooks": [] }],
            "Stop": [],
        },
    });
    home.owner(&owner.to_string());
    home.launch(LAUNCH, true).unwrap();
    let written = home.written(LAUNCH);
    let keys: Vec<&String> = written.as_object().unwrap().keys().collect();
    assert_eq!(keys, ["theme", "auto_update", "hooks"]);
    assert_eq!(written["auto_update"], false);
    let hooks = &written["hooks"];
    let events: Vec<&String> = hooks.as_object().unwrap().keys().collect();
    assert_eq!(events, ["SessionStart", "PreToolUse", "Stop"]);
    assert_eq!(hooks["SessionStart"][0], owner["hooks"]["SessionStart"][0]);
    assert_eq!(hooks["SessionStart"].as_array().unwrap().len(), 2);
    assert_eq!(hooks["PreToolUse"][0], owner["hooks"]["PreToolUse"][0]);
    assert_eq!(hooks["PreToolUse"][1]["matcher"], "ask_user_question");
    assert_eq!(hooks["Stop"], json!([]));
    // Devin's own config is never touched: only a copy is written.
    let original = fs::read_to_string(home.dir.path().join("config/devin/config.json")).unwrap();
    assert_eq!(original, owner.to_string());
}

#[test]
fn a_hook_list_that_is_null_is_one_with_nothing_in_it_and_hooks_that_are_null_are_none() {
    let home = Home::new();
    home.owner(r#"{"hooks": {"SessionStart": null, "PreToolUse": null}}"#);
    home.launch("one", true).unwrap();
    assert_eq!(
        home.written("one")["hooks"]["SessionStart"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    home.owner(r#"{"hooks": null}"#);
    home.launch("two", false).unwrap();
    assert_eq!(home.written("two")["hooks"]["PreToolUse"], json!([]));
}

#[test]
fn a_hook_list_that_is_no_list_is_refused_after_the_launchs_folder_was_made() {
    for owner in [
        r#"{"hooks": {"SessionStart": "x"}}"#,
        r#"{"hooks": {"PreToolUse": {}}}"#,
        r#"{"hooks": {"SessionStart": false}}"#,
        r#"{"hooks": {"PreToolUse": 0}}"#,
    ] {
        let home = Home::new();
        home.owner(owner);
        assert_eq!(
            home.launch(LAUNCH, true).err().as_deref(),
            Some("Invalid native Devin hook configuration"),
            "{owner}"
        );
        assert!(home.folder(LAUNCH).is_dir(), "{owner}");
        assert!(!home.folder(LAUNCH).join("config.json").exists(), "{owner}");
    }
}

#[test]
fn hooks_that_are_a_zero_a_flag_or_empty_text_are_refused_in_a_sentence_of_this_modules_own() {
    for owner in [r#"{"hooks": 0}"#, r#"{"hooks": false}"#, r#"{"hooks": ""}"#] {
        let home = Home::new();
        home.owner(owner);
        assert_eq!(
            home.launch(LAUNCH, true).err().as_deref(),
            Some("the hooks of the native Devin configuration cannot take a hook"),
            "{owner}"
        );
        assert!(home.folder(LAUNCH).is_dir(), "the folder was made first");
    }
}

#[test]
fn an_owner_config_that_cannot_be_used_is_refused_before_anything_is_written() {
    for owner in [
        "{not json",
        "",
        "[]",
        r#""devin""#,
        "7",
        "null",
        "true",
        r#"{"hooks": []}"#,
        r#"{"hooks": "x"}"#,
        r#"{"hooks": 5}"#,
        r#"{"hooks": true}"#,
        "\u{feff}{}",
        r#"{"hooks": {"SessionStart": ["#,
        "{} /* open",
    ] {
        let home = Home::new();
        home.owner(owner);
        assert_eq!(
            home.launch(LAUNCH, true).err().as_deref(),
            Some("Cannot read native Devin configuration; the original was preserved"),
            "{owner:?}"
        );
        assert!(!home.dir.path().join("consensflow").exists(), "{owner:?}");
    }
}

#[test]
fn a_config_that_is_not_there_is_none_and_one_that_cannot_be_read_says_why_as_node_does() {
    let home = Home::new();
    assert!(home.launch(LAUNCH, true).is_ok(), "no owner config at all");
    let home = Home::new();
    let config = home
        .dir
        .path()
        .join("config")
        .join("devin")
        .join("config.json");
    fs::create_dir_all(&config).unwrap();
    assert_eq!(
        home.launch(LAUNCH, true).err(),
        Some(format!(
            "EISDIR: illegal operation on a directory, read '{}'",
            config.display()
        ))
    );
}

/// A file where Devin's config folder should be: Unix says it is no folder;
/// what Windows says is the platform's own, held by its goldens.
#[cfg(unix)]
#[test]
fn a_config_folder_that_is_a_file_is_refused_in_nodes_words() {
    let home = Home::new();
    fs::create_dir_all(home.dir.path().join("config")).unwrap();
    fs::write(home.dir.path().join("config/devin"), "x").unwrap();
    let config = home.dir.path().join("config/devin/config.json");
    assert_eq!(
        home.launch(LAUNCH, true).err(),
        Some(format!(
            "ENOTDIR: not a directory, open '{}'",
            config.display()
        ))
    );
}

#[test]
fn an_environment_that_names_no_home_has_no_config_folder_to_read() {
    let home = Home::new();
    let services = Services {
        env: Env::from_vars([("CONSENSFLOW_HOME", "/nowhere")]),
        ..home.services.clone()
    };
    let refused = finished(Box::pin(integration(
        &services,
        LAUNCH,
        &home.executable,
        true,
    )));
    assert_eq!(refused.err().as_deref(), Some("missing home in env"));
}

#[test]
fn a_config_is_written_in_javascripts_key_order_and_numbers() {
    let home = Home::new();
    home.owner(r#"{"b": 1, "10": 2, "a": 3, "2": 4, "big": 12345678901234567890, "small": 1.50, "exp": 1E3}"#);
    home.launch(LAUNCH, true).unwrap();
    let text = fs::read_to_string(home.folder(LAUNCH).join("config.json")).unwrap();
    assert!(
        text.starts_with(r#"{"2":4,"10":2,"b":1,"a":3,"big":12345678901234567000,"small":1.5,"exp":1000,"hooks":"#),
        "{text}"
    );
}

#[test]
fn a_config_that_is_there_already_refuses_the_launch_in_nodes_words_and_is_left_alone() {
    let home = Home::new();
    home.launch(LAUNCH, true).unwrap();
    let config = home.folder(LAUNCH).join("config.json");
    let before = fs::read_to_string(&config).unwrap();
    assert_eq!(
        home.launch(LAUNCH, false).err(),
        Some(format!(
            "EEXIST: file already exists, open '{}'",
            config.display()
        ))
    );
    assert_eq!(fs::read_to_string(&config).unwrap(), before);
}

#[cfg(unix)]
#[test]
fn the_launchs_folder_and_its_files_are_the_users_alone() {
    use std::os::unix::fs::PermissionsExt;
    let home = Home::new();
    let integration = home.launch(LAUNCH, true).unwrap();
    let mode = |file: &Path| fs::metadata(file).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&home.folder(LAUNCH)), 0o700);
    assert_eq!(mode(&home.folder(LAUNCH).join("config.json")), 0o600);
    let invocation = Invocation {
        command: "devin",
        args: Vec::new(),
        prompt: Some("Write the parser".to_owned()),
        drop_env: &[],
    };
    prepare_prompt(invocation, &integration).unwrap();
    assert_eq!(mode(&home.folder(LAUNCH).join("prompt.txt")), 0o600);
}

#[test]
fn a_devin_older_than_the_minimum_is_refused_and_one_updated_since_is_asked_again() {
    let home = Home::new();
    home.launch("one", true).unwrap();
    home.launch("two", true).unwrap();
    assert_eq!(
        home.fakes.processes.take_ran().len(),
        1,
        "one question for an unchanged Devin"
    );
    // An update gives the file another size and time: it is asked again.
    fs::write(&home.executable, "#!/bin/sh\n# an older Devin\nexit 0\n").unwrap();
    home.says("devin 3000.9.1 (00000000)");
    assert_eq!(
        home.launch("three", true).err().as_deref(),
        Some("Devin 3000.10.21 or newer is required for complete worker replies. Update Devin before opening this pane.")
    );
    assert_eq!(home.fakes.processes.take_ran().len(), 1);
    assert!(!home.folder("three").exists(), "nothing was written");
}

#[test]
fn a_devin_that_cannot_be_asked_says_why() {
    let home = Home::new();
    let services = Services {
        processes: Rc::new(ScriptedProcesses::default()),
        ..home.services.clone()
    };
    let refused = finished(Box::pin(integration(
        &services,
        LAUNCH,
        &home.executable,
        true,
    )));
    let name = called(Path::new(&home.executable));
    assert_eq!(
        refused.err(),
        Some(format!("spawn {name} --version ENOENT"))
    );
    let refused = finished(Box::pin(integration(
        &home.services,
        LAUNCH,
        "/nowhere/devin",
        true,
    )));
    let refused = refused.err().unwrap();
    assert!(
        refused.starts_with("ENOENT: no such file or directory, realpath"),
        "{refused}"
    );
}

#[test]
fn the_first_message_goes_in_a_file_beside_the_wire_log_named_by_its_flag() {
    let home = Home::new();
    let integration = home.launch(LAUNCH, true).unwrap();
    let invocation = Invocation {
        command: "devin",
        args: vec!["--model".to_owned(), "m".to_owned()],
        prompt: Some("[ConsensFlow m-1] Write the parser".to_owned()),
        drop_env: &[],
    };
    let prepared = prepare_prompt(invocation, &integration).unwrap();
    let file = home.folder(LAUNCH).join("prompt.txt");
    assert_eq!(
        prepared.args,
        [
            "--model",
            "m",
            "--prompt-file",
            file.to_string_lossy().as_ref()
        ]
    );
    assert_eq!(prepared.prompt, None);
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "[ConsensFlow m-1] Write the parser"
    );
}

#[test]
fn a_window_with_no_first_message_has_no_prompt_file_and_one_there_already_refuses_the_launch() {
    let home = Home::new();
    let integration = home.launch(LAUNCH, true).unwrap();
    let invocation = |prompt: Option<&str>| Invocation {
        command: "devin",
        args: vec!["--model".to_owned(), "m".to_owned()],
        prompt: prompt.map(str::to_owned),
        drop_env: &[],
    };
    let prepared = prepare_prompt(invocation(None), &integration).unwrap();
    assert_eq!(prepared.args, ["--model", "m"]);
    assert!(!home.folder(LAUNCH).join("prompt.txt").exists());
    prepare_prompt(invocation(Some("first")), &integration).unwrap();
    let file = home.folder(LAUNCH).join("prompt.txt");
    assert_eq!(
        prepare_prompt(invocation(Some("again")), &integration).err(),
        Some(format!(
            "EEXIST: file already exists, open '{}'",
            file.display()
        ))
    );
    assert_eq!(fs::read_to_string(&file).unwrap(), "first");
}

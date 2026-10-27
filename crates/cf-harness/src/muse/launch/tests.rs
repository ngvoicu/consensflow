use std::fs;

use super::*;

/// A machine's Muse install: the launcher and, beside it, `.muse-version` naming
/// `line` and the binaries in `binaries`.
fn install(dir: &Path, line: &str, binaries: &[&str]) -> String {
    let bin = dir.join("bin");
    fs::create_dir_all(&bin).unwrap();
    fs::write(bin.join("muse"), "#!/bin/sh\n").unwrap();
    fs::write(bin.join(".muse-version"), format!("{line}\n")).unwrap();
    for binary in binaries {
        fs::write(bin.join(binary), "binary").unwrap();
    }
    bin.join("muse").to_string_lossy().into_owned()
}

fn unix() -> Env {
    Env::from_vars([("HOME", "/h")])
}

#[test]
fn a_window_runs_the_binary_its_launcher_names_and_never_the_launcher_where_it_is_there() {
    let dir = tempfile::tempdir().unwrap();
    let launcher = install(
        dir.path(),
        "1.4.13-R6996.1",
        &["muse-bin-1.4.13-R6996.1", "muse-bin-1.4.12-R6000"],
    );
    let binary = fs::canonicalize(dir.path().join("bin/muse-bin-1.4.13-R6996.1")).unwrap();
    assert_eq!(
        pinned(&launcher, &unix()).unwrap(),
        binary.to_string_lossy()
    );
    // A version line naming a binary that is not there: the launcher is run itself.
    let other = tempfile::tempdir().unwrap();
    let alone = install(other.path(), "1.4.14-R7000", &[]);
    assert_eq!(pinned(&alone, &unix()).unwrap(), alone);
    // No version file: the program found.
    let bare = tempfile::tempdir().unwrap();
    let program = bare.path().join("muse");
    fs::write(&program, "#!/bin/sh\n").unwrap();
    let program = program.to_string_lossy().into_owned();
    assert_eq!(pinned(&program, &unix()).unwrap(), program);
}

#[test]
fn an_older_muse_than_consensflow_reads_is_refused_in_the_words_of_the_version_floor() {
    let dir = tempfile::tempdir().unwrap();
    let launcher = install(dir.path(), "1.4.12-R6000", &["muse-bin-1.4.12-R6000"]);
    assert_eq!(
        pinned(&launcher, &unix()).unwrap_err(),
        "Muse Code 1.4.13 or newer is required. Update Muse Code before opening a pane."
    );
}

/// An environment whose Muse config folder is in `dir`.
fn configured(dir: &Path) -> Env {
    Env::from_vars([("HOME", "/h"), ("XDG_CONFIG_HOME", dir.to_str().unwrap())])
}

fn ours() -> Value {
    added_hook("/app/cf", "/launch/role/SKILL.md", "/launch/sessions.jsonl")
}

#[test]
fn the_owner_s_hooks_run_with_consensflow_s_after_them_and_their_file_is_never_written() {
    let dir = tempfile::tempdir().unwrap();
    let settings = dir.path().join("muse/settings.json");
    fs::create_dir_all(settings.parent().unwrap()).unwrap();
    let owner = json!({
        "schema_version": 1,
        "model": "muse-spark-1.3",
        "hooks": {
            "SessionStart": [{ "matcher": "", "hooks": [{ "type": "command", "command": "owner-start" }] }],
            "UserPromptSubmit": [{ "matcher": "", "hooks": [{ "type": "command", "command": "owner-submit" }] }],
        },
    });
    let text = serde_json::to_string_pretty(&owner).unwrap();
    fs::write(&settings, &text).unwrap();
    let hooks = hooks(&configured(dir.path()), ours()).unwrap();
    assert_eq!(
        hooks["UserPromptSubmit"],
        owner["hooks"]["UserPromptSubmit"]
    );
    let starts = hooks["SessionStart"].as_array().unwrap();
    assert_eq!(starts.len(), 2);
    assert_eq!(
        starts[0], owner["hooks"]["SessionStart"][0],
        "the owner's first"
    );
    assert_eq!(starts[1], ours());
    assert_eq!(
        fs::read_to_string(&settings).unwrap(),
        text,
        "the owner's file as it was"
    );
}

#[test]
fn with_no_settings_or_no_hooks_there_is_consensflow_s_alone_and_a_settings_file_muse_cannot_read_is_refused(
) {
    let dir = tempfile::tempdir().unwrap();
    let env = configured(dir.path());
    assert_eq!(
        hooks(&env, ours()).unwrap(),
        json!({ "SessionStart": [ours()] })
    );
    let settings = dir.path().join("muse/settings.json");
    fs::create_dir_all(settings.parent().unwrap()).unwrap();
    fs::write(&settings, r#"{"schema_version":1,"hooks":null}"#).unwrap();
    assert_eq!(
        hooks(&env, ours()).unwrap(),
        json!({ "SessionStart": [ours()] })
    );
    fs::write(&settings, "{not json").unwrap();
    assert!(hooks(&env, ours()).unwrap_err().contains("not JSON"));
    fs::write(&settings, r#"{"hooks":[1]}"#).unwrap();
    assert!(hooks(&env, ours())
        .unwrap_err()
        .contains("cannot take ConsensFlow's"));
    fs::write(&settings, r#"{"hooks":{"SessionStart":"x"}}"#).unwrap();
    assert!(hooks(&env, ours())
        .unwrap_err()
        .contains("cannot take ConsensFlow's"));
}

#[test]
fn the_hook_names_its_files_on_its_command_line_quoted_for_the_hook_s_shell() {
    let hook = added_hook(
        "/Apps/Consens Flow/cf",
        "/l/it's/SKILL.md",
        "/l/sessions.jsonl",
    );
    assert_eq!(
        hook["hooks"][0]["command"],
        r"'/Apps/Consens Flow/cf' hook muse-session '/l/it'\''s/SKILL.md' '/l/sessions.jsonl'"
    );
    assert_eq!(hook["matcher"], "");
}

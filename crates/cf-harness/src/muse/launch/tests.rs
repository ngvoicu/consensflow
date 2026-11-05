use std::fs;

use super::*;

/// A machine's Muse install: the launcher and, beside it, `.muse-version` naming
/// `line` and the binaries in `binaries`.
fn install(dir: &Path, line: &str, binaries: &[&str]) -> String {
    install_as(dir, "muse", line, binaries)
}

/// [`install`], its launcher named `launcher` (Windows' is `muse.cmd`).
fn install_as(dir: &Path, launcher: &str, line: &str, binaries: &[&str]) -> String {
    let bin = dir.join("bin");
    fs::create_dir_all(&bin).unwrap();
    fs::write(bin.join(launcher), "#!/bin/sh\n").unwrap();
    fs::write(bin.join(".muse-version"), format!("{line}\n")).unwrap();
    for binary in binaries {
        fs::write(bin.join(binary), "binary").unwrap();
    }
    bin.join(launcher).to_string_lossy().into_owned()
}

/// The environment of this machine's kind: Windows' says so.
fn here() -> Env {
    if cfg!(windows) {
        Env::from_vars([("HOME", "/h"), ("OS", "Windows_NT")])
    } else {
        Env::from_vars([("HOME", "/h")])
    }
}

/// A binary's file name on this machine: `.exe` on Windows.
fn binary_name(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_owned()
    }
}

#[cfg(unix)]
#[test]
fn a_launcher_that_is_a_link_runs_the_binary_beside_the_file_it_names() {
    let dir = tempfile::tempdir().unwrap();
    let launcher = install(dir.path(), "1.4.13-R6996.1", &["muse-bin-1.4.13-R6996.1"]);
    let link = dir.path().join("muse");
    std::os::unix::fs::symlink(&launcher, &link).unwrap();
    let binary = fs::canonicalize(dir.path().join("bin/muse-bin-1.4.13-R6996.1")).unwrap();
    assert_eq!(
        pinned(&link.to_string_lossy(), &here()).unwrap(),
        binary.to_string_lossy()
    );
}

#[test]
fn a_window_runs_the_binary_its_launcher_names_and_never_the_launcher_where_it_is_there() {
    let dir = tempfile::tempdir().unwrap();
    let launcher = install(
        dir.path(),
        "1.4.13-R6996.1",
        &[
            &binary_name("muse-bin-1.4.13-R6996.1"),
            "muse-bin-1.4.12-R6000",
        ],
    );
    let binary = dir
        .path()
        .join("bin")
        .join(binary_name("muse-bin-1.4.13-R6996.1"));
    assert_eq!(
        pinned(&launcher, &here()).unwrap(),
        binary.to_string_lossy()
    );
    // A version line naming a binary that is not there: the launcher is run itself.
    let other = tempfile::tempdir().unwrap();
    let alone = install(other.path(), "1.4.14-R7000", &[]);
    assert_eq!(pinned(&alone, &here()).unwrap(), alone);
    // No version file: the program found.
    let bare = tempfile::tempdir().unwrap();
    let program = bare.path().join("muse");
    fs::write(&program, "#!/bin/sh\n").unwrap();
    let program = program.to_string_lossy().into_owned();
    assert_eq!(pinned(&program, &here()).unwrap(), program);
}

/// Windows' launcher is `muse.cmd`, which `cmd.exe` runs: the binary beside it is run,
/// and where there is none the window is refused in plain words, never opened through
/// `cmd.exe`, which would mangle the hooks' JSON (Calliope, T-824 P2-4).
#[test]
fn a_batch_file_launcher_with_no_binary_beside_it_is_refused_in_plain_words_and_never_run() {
    let windows = Env::from_vars([("USERPROFILE", r"C:\Users\a"), ("OS", "Windows_NT")]);
    let dir = tempfile::tempdir().unwrap();
    let launcher = install_as(
        dir.path(),
        "muse.cmd",
        "1.4.13-R6996.1",
        &["muse-bin-1.4.13-R6996.1.exe"],
    );
    let binary = dir.path().join("bin").join("muse-bin-1.4.13-R6996.1.exe");
    assert_eq!(
        pinned(&launcher, &windows).unwrap(),
        binary.to_string_lossy()
    );
    for (named, line) in [("muse.cmd", Some("1.4.14-R7309.1")), ("MUSE.BAT", None)] {
        let other = tempfile::tempdir().unwrap();
        let launcher = match line {
            Some(line) => install_as(other.path(), named, line, &[]),
            None => {
                let program = other.path().join(named);
                fs::write(&program, "@echo off\r\n").unwrap();
                program.to_string_lossy().into_owned()
            }
        };
        let refused = pinned(&launcher, &windows).unwrap_err();
        assert!(
            refused.contains("is a batch file") && refused.contains(&launcher),
            "{refused}"
        );
    }
}

#[test]
fn an_older_muse_than_consensflow_reads_is_refused_in_the_words_of_the_version_floor() {
    let dir = tempfile::tempdir().unwrap();
    let launcher = install(dir.path(), "1.4.12-R6000", &["muse-bin-1.4.12-R6000"]);
    assert_eq!(
        pinned(&launcher, &here()).unwrap_err(),
        "Muse Code 1.4.13 or newer is required. Update Muse Code before opening a pane."
    );
}

/// An environment whose Muse config folder is in `dir`.
fn configured(dir: &Path) -> Env {
    Env::from_vars([("HOME", "/h"), ("XDG_CONFIG_HOME", dir.to_str().unwrap())])
}

fn ours() -> Value {
    added_hooks(
        "/app/cf",
        "/launch/role/SKILL.md",
        "/launch/sessions.jsonl",
        false,
        1,
    )
    .unwrap()
}

#[test]
fn consensflow_s_hooks_run_before_the_owner_s_so_their_blocks_are_the_first_and_the_owner_s_file_is_never_written(
) {
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
    // Ours first (T-1246): Muse numbers the blocks of the hooks that printed in this order, ours
    // always print, so part k of a role is block k whatever the owner's hooks print.
    assert_eq!(starts[0], ours(), "ConsensFlow's first");
    assert_eq!(
        starts[1], owner["hooks"]["SessionStart"][0],
        "the owner's after"
    );
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
    let hook = added_hooks(
        "/Apps/Consens Flow/cf",
        "/l/it's/SKILL.md",
        "/l/sessions.jsonl",
        false,
        1,
    )
    .unwrap();
    assert_eq!(
        hook["hooks"][0]["command"],
        r"'/Apps/Consens Flow/cf' hook muse-session '/l/it'\''s/SKILL.md' '/l/sessions.jsonl' '0'"
    );
    assert_eq!(hook["matcher"], "");
}

/// Muse runs a hook's `commandWindows` on Windows in cmd.exe, through Rust's quoting, so
/// every `"` of it reaches cmd.exe as `\"` (T-792's probe on zeewin2, after Calliope's
/// T-824 P2-4 took it for PowerShell's): the line has no quote, each word a path cmd.exe
/// takes as it is (its short form where it holds a blank), and a launch whose path has
/// none is refused in plain words. Off Windows there is no Windows form.
#[test]
fn on_windows_the_hook_is_a_cmd_line_of_words_with_no_quote_and_a_path_cmd_cannot_take_is_refused()
{
    let hook = added_hooks(
        r"C:\Apps\ConsensFlow\cf.exe",
        r"C:\Users\Zoë\l\it's\SKILL.md",
        r"C:\Users\Zoë\l\sessions.jsonl",
        true,
        1,
    )
    .unwrap();
    assert_eq!(
        hook["hooks"][0]["commandWindows"],
        r"C:\Apps\ConsensFlow\cf.exe hook muse-session C:\Users\Zoë\l\it's\SKILL.md C:\Users\Zoë\l\sessions.jsonl 0"
    );
    assert_eq!(hook["hooks"][0]["timeout"], 10);
    let off = added_hooks("/a/cf", "/l/SKILL.md", "/l/sessions.jsonl", false, 1).unwrap();
    assert!(off["hooks"][0].get("commandWindows").is_none(), "{off}");
    for unsayable in [r"Z:\no such folder\cf.exe", r"Z:\a&b\cf.exe"] {
        let refused =
            added_hooks(unsayable, r"C:\l\SKILL.md", r"C:\l\s.jsonl", true, 1).unwrap_err();
        assert!(
            refused.contains("cmd.exe") && refused.contains(unsayable),
            "{refused}"
        );
    }
}

/// A role is longer than a hook may say (16,384 bytes, T-1246), so each part of it has a hook
/// of its own, in the one group, named by its number; and the number is on Windows' line too.
#[test]
fn a_role_in_parts_has_a_hook_for_each_numbered_from_0_on_both_lines() {
    let group = added_hooks("/app/cf", "/l/SKILL.md", "/l/s.jsonl", false, 3).unwrap();
    let hooks = group["hooks"].as_array().unwrap();
    assert_eq!(group["matcher"], "");
    assert_eq!(hooks.len(), 3);
    for (part, hook) in hooks.iter().enumerate() {
        assert_eq!(
            hook["command"],
            format!("'/app/cf' hook muse-session '/l/SKILL.md' '/l/s.jsonl' '{part}'")
        );
        assert_eq!(hook["timeout"], 10);
    }
    let group = added_hooks(r"C:\cf.exe", r"C:\l\SKILL.md", r"C:\l\s.jsonl", true, 2).unwrap();
    let lines: Vec<_> = group["hooks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|hook| hook["commandWindows"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(
        lines,
        [
            r"C:\cf.exe hook muse-session C:\l\SKILL.md C:\l\s.jsonl 0",
            r"C:\cf.exe hook muse-session C:\l\SKILL.md C:\l\s.jsonl 1"
        ]
    );
}

/// Which hook says what, in the launch's configuration: each of ours is one `cf hook
/// muse-session` of its number, and `count` of them are given, first.
#[test]
fn a_launch_gives_count_hooks_of_ours_first_whatever_the_owner_has() {
    let dir = tempfile::tempdir().unwrap();
    let launcher = install(dir.path(), "1.4.14-R7309.1", &["muse-bin-1.4.14-R7309.1"]);
    let settings = dir.path().join("muse/settings.json");
    fs::create_dir_all(settings.parent().unwrap()).unwrap();
    fs::write(
        &settings,
        r#"{"hooks":{"SessionStart":[{"matcher":"","hooks":[{"type":"command","command":"a"},{"type":"command","command":"b"}]},{"command":"c"}]}}"#,
    )
    .unwrap();
    let env = Env::from_vars([
        ("HOME", "/h"),
        ("XDG_CONFIG_HOME", dir.path().to_str().unwrap()),
        (
            "CONSENSFLOW_HOME",
            dir.path().join("consensflow").to_str().unwrap(),
        ),
    ]);
    let hooks_of = |count: usize| {
        let given = integration(
            &env,
            &LaunchId::new("11111111-1111-4111-8111-111111111111").unwrap(),
            &launcher,
            "/app/cf",
            "/l/SKILL.md",
            count,
        )
        .unwrap();
        let hooks: Value =
            serde_json::from_str(given.args[1].strip_prefix("hooks=").unwrap()).unwrap();
        let starts = hooks["SessionStart"].as_array().unwrap().clone();
        assert_eq!(starts.len(), 3, "ours and the owner's two entries");
        (starts[0]["hooks"].as_array().unwrap().len(), starts)
    };
    for count in [1, 2, 5, 20] {
        let (ours, starts) = hooks_of(count);
        assert_eq!(ours, count);
        assert_eq!(starts[1]["hooks"][0]["command"], "a", "the owner's follow");
        assert_eq!(starts[2]["command"], "c");
    }
    assert_eq!(
        hooks_of(500).0,
        crate::muse::MOST_PARTS,
        "never past the most hooks Muse was measured to take"
    );
}

/// A Windows process takes a command line of at most 32,767 characters, and the hooks are on
/// Muse's (`-c hooks=<json>`): a hook for each part of a role, each with the window's `cf`,
/// the role's file and the sessions' file on both of its lines, backslashes doubled in the
/// JSON. The most parts, with paths as long as a launch's are, leave the line room.
#[test]
fn the_most_parts_on_windows_paths_as_long_as_a_launch_has_fit_a_command_line() {
    let cf = r"C:\Users\someone-with-a-long-name\AppData\Local\ConsensFlow\cli\bin\cf.exe";
    let launch = r"C:\Users\someone-with-a-long-name\AppData\Roaming\consensflow\integrations\muse\11111111-1111-4111-8111-111111111111";
    let role = format!(r"{launch}\role\.claude\skills\consensflow-chief\SKILL.md");
    let said = format!(r"{launch}\sessions.jsonl");
    let group = added_hooks(cf, &role, &said, true, crate::muse::MOST_PARTS).unwrap();
    let argument = format!(
        "hooks={}",
        js::stringify(&json!({ "SessionStart": [group] }))
    );
    // What else is on the line: the program, the model, the flags, a long folder or two.
    let rest = 4_000;
    assert!(
        argument.len() + rest < 32_767,
        "{} hooks of {} characters leave {} for the rest of the line",
        crate::muse::MOST_PARTS,
        argument.len(),
        32_767 - argument.len()
    );
}

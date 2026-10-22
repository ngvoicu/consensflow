//! `windows`, `windows-matrix` and `scrub-transcript` as a person starts them: the
//! xtask program, on a checkout made for the case (a git repository in a
//! temporary folder, named to xtask the way `cargo run` names its own), with the
//! fake child for the machine's `scp` and `ssh`. What a run does with its words
//! and its streams and its status is the unit tests'; here are the words it
//! refuses and what it starts for the rest.
#![allow(clippy::disallowed_methods, clippy::expect_used, clippy::unwrap_used)]

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use cf_base::env::Env;
use common::{arg_line, arg_lines, checkout, err, out, programs, xtask};

/// A checkout: a git repository whose files are added to its index (no commit is
/// made: none is needed), laid out as xtask finds its own (`tools/xtask`).
fn tree(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("Cargo.toml"), "").unwrap();
    fs::create_dir_all(dir.path().join("tools").join("xtask")).unwrap();
    for (name, text) in files {
        let file = dir.path().join(name);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, text).unwrap();
    }
    for words in [&["init", "-q"][..], &["add", "-A"]] {
        let ran = Command::new("git")
            .args(words)
            .current_dir(dir.path())
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .output()
            .unwrap();
        assert!(ran.status.success(), "git {words:?}: {}", err(&ran));
    }
    dir
}

/// The PATH this test runs with, behind a folder of programs of its own: git and
/// tar are the machine's, and `scp` and `ssh` are what that folder says.
fn path_with(fakes: &Path) -> PathBuf {
    let own = Env::from_process();
    let rest = std::env::split_paths(own.os("PATH").unwrap()).collect::<Vec<_>>();
    std::env::join_paths(std::iter::once(fakes.to_path_buf()).chain(rest))
        .unwrap()
        .into()
}

/// `args` run as a developer would, in the checkout `tree` is, with `fakes` first on the PATH.
fn on(tree: &tempfile::TempDir, fakes: &Path, vars: &[(&str, &str)], args: &[&str]) -> Output {
    let manifest = tree.path().join("tools").join("xtask");
    let manifest = manifest.to_str().unwrap();
    let mut all = vec![("CARGO_MANIFEST_DIR", manifest)];
    all.extend_from_slice(vars);
    xtask(tree.path(), &path_with(fakes), &all, args)
}

/// What the script that goes to the machine is, as the JavaScript sent it for a
/// run of `cargo xtask test integration` (`tests/golden/windows`), where the
/// mark `␍` stands for the carriage return that git would not keep in a file.
fn golden(name: &str) -> String {
    let path = checkout()
        .join("tools")
        .join("xtask")
        .join("tests")
        .join("golden")
        .join("windows")
        .join(format!("{name}.ps1"));
    fs::read_to_string(path).unwrap().replace('\u{240d}', "\r")
}

/// The script a line of arguments hands PowerShell, decoded: `arg` is how the
/// fake child wrote the argument that holds it.
fn script_in(arg: &str) -> String {
    let encoded = arg
        .strip_prefix("arg: \"powershell -NoProfile -NonInteractive -EncodedCommand ")
        .and_then(|rest| rest.strip_suffix('"'))
        .unwrap_or_else(|| panic!("not a powershell line: {arg}"));
    let bytes = STANDARD.decode(encoded).unwrap();
    let units: Vec<u16> = bytes
        .chunks(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    String::from_utf16(&units).unwrap()
}

#[test]
fn the_commands_refuse_the_words_they_do_not_take_and_start_nothing() {
    let nothing = tempfile::tempdir().unwrap();
    for (args, said) in [
        (
            vec!["windows"],
            "xtask: windows takes --host <ssh host> and the command to run there after a --: \
             cargo xtask windows --host <ssh host> [--build] [--env NAME=VALUE] -- <command>\n",
        ),
        (
            vec!["windows", "--host", "zeewin"],
            "xtask: windows takes --host <ssh host> and the command to run there after a --: \
             cargo xtask windows --host <ssh host> [--build] [--env NAME=VALUE] -- <command>\n",
        ),
        (
            vec!["windows", "--host", "zeewin", "--nope", "--", "x"],
            "xtask: unknown option: --nope\n",
        ),
        (
            vec!["windows", "--host", "zeewin", "--dir", "..", "--", "x"],
            "xtask: --dir is a folder name: ..\n",
        ),
        (
            vec!["windows", "--host", "zeewin", "--env", "NOVALUE", "--", "x"],
            "xtask: --env is NAME=VALUE, a variable's name and what to give it: NOVALUE\n",
        ),
        (
            vec!["windows-matrix", "--host", "zeewin"],
            "xtask: windows-matrix takes --host <ssh host>, a --scenario <name> and a --pair \
             <chief>:<staff> (each as many times as there are): cargo xtask windows-matrix \
             --host <ssh host> [--env NAME=VALUE] --scenario <name>... --pair <chief>:<staff>...\n",
        ),
        (
            vec![
                "windows-matrix",
                "--host",
                "zeewin",
                "--scenario",
                "round-trip",
                "--pair",
                "devin",
            ],
            "xtask: a pair is chief:staff, not devin\n",
        ),
        (
            vec!["scrub-transcript", "a.jsonl"],
            "xtask: scrub-transcript takes the transcript to scrub and the fixture to make: \
             <transcript.jsonl> <fixture.jsonl>\n",
        ),
    ] {
        let ran = xtask(&checkout(), nothing.path(), &[], &args);
        assert_eq!(ran.status.code(), Some(2), "{args:?}");
        assert_eq!(out(&ran), "", "{args:?}");
        assert_eq!(
            err(&ran),
            format!("{said}see `cargo xtask --help`\n"),
            "{args:?}"
        );
    }
}

#[test]
fn the_commands_say_so_in_their_help_that_they_run_in_rust() {
    let nothing = tempfile::tempdir().unwrap();
    for command in ["windows", "windows-matrix", "scrub-transcript"] {
        let ran = xtask(&checkout(), nothing.path(), &[], &[command, "--help"]);
        assert!(ran.status.success(), "{command}: {}", err(&ran));
        let help = out(&ran);
        assert!(
            help.starts_with(&format!("Usage: cargo xtask {command}")),
            "{help}"
        );
        assert!(help.contains("It runs in Rust."), "{help}");
        assert!(!help.contains("node "), "{help}");
        assert_eq!(err(&ran), "");
    }
}

#[test]
fn a_run_sends_the_tree_as_it_is_and_has_the_machine_run_the_script_the_javascript_sent() {
    let fakes = programs(&["scp", "ssh"]);
    let tree = tree(&[("keep.txt", "kept"), ("gone.txt", "deleted since")]);
    // Tracked, and gone: the copy that broke on it.
    fs::remove_file(tree.path().join("gone.txt")).unwrap();
    let ran = on(
        &tree,
        fakes.path(),
        &[
            ("FAKE_CHILD_STDERR", "said"),
            ("FAKE_CHILD_LAST", "== exit 0"),
        ],
        &[
            "windows",
            "--host",
            "zeewin",
            "--",
            "cargo",
            "xtask",
            "test",
            "integration",
        ],
    );
    assert!(ran.status.success(), "{}", err(&ran));
    let report = out(&ran);
    let programs: Vec<&str> = report.split("cwd: ").skip(1).collect();
    assert_eq!(programs.len(), 2, "scp, then ssh: {report}");

    let scp = arg_lines(programs[0]);
    assert_eq!(scp.len(), 3, "{scp:?}");
    assert_eq!(scp[0], arg_line("-q"));
    assert!(scp[1].ends_with("tree.tgz\""), "{}", scp[1]);
    assert_eq!(scp[2], arg_line("zeewin:consensflow-build.tgz"));

    let ssh = arg_lines(programs[1]);
    assert_eq!(ssh.len(), 2, "{ssh:?}");
    assert_eq!(ssh[0], arg_line("zeewin"));
    assert!(script_in(ssh[1]) == golden("xtask-test-integration"));
    // What each said on its error stream: scp's on the terminal's, ssh's relayed.
    assert_eq!(err(&ran).matches("said").count(), 2, "{}", err(&ran));
}

#[test]
fn a_run_answers_what_the_machine_says_of_the_command_not_what_ssh_ends_with() {
    let fakes = programs(&["scp", "ssh"]);
    let tree = tree(&[("keep.txt", "kept")]);
    // ssh ends with 0 (the fake child's way), and the line says the command ended with 7.
    let ran = on(
        &tree,
        fakes.path(),
        &[("FAKE_CHILD_LAST", "== exit 7")],
        &["windows", "--host", "zeewin", "--", "cmd", "/c", "exit 7"],
    );
    assert_eq!(ran.status.code(), Some(7), "{}", err(&ran));
    assert!(out(&ran).ends_with("== exit 7\n"), "{}", out(&ran));
    assert_eq!(err(&ran), "");
}

#[test]
fn a_run_whose_machine_did_not_say_how_the_command_ended_answers_1_though_ssh_said_0() {
    let fakes = programs(&["scp", "ssh"]);
    let tree = tree(&[("keep.txt", "kept")]);
    let ran = on(
        &tree,
        fakes.path(),
        &[],
        &["windows", "--host", "zeewin", "--", "cmd", "/c", "exit 0"],
    );
    assert_eq!(ran.status.code(), Some(1), "{}", err(&ran));
    assert_eq!(
        err(&ran),
        "windows: no `== exit N` line ended the output, and ssh's status 0 is not taken for a command's without it: answering 1\n"
    );
}

#[test]
fn a_run_starts_no_ssh_when_the_tree_could_not_be_sent() {
    let fakes = programs(&["scp", "ssh"]);
    let tree = tree(&[("keep.txt", "kept")]);
    let ran = on(
        &tree,
        fakes.path(),
        &[("FAKE_CHILD_EXIT", "3")],
        &["windows", "--host", "zeewin", "--", "x"],
    );
    assert_eq!(ran.status.code(), Some(1));
    let said = err(&ran);
    assert!(said.starts_with("windows: `scp -q "), "{said}");
    assert!(
        said.ends_with(":consensflow-build.tgz` ended with status 3\n"),
        "{said}"
    );
    // The one that was asked is the one that said what it was asked.
    assert_eq!(out(&ran).matches("cwd: ").count(), 1, "{}", out(&ran));
}

#[test]
fn a_run_asks_for_the_folder_the_build_and_the_variables_it_was_given() {
    let fakes = programs(&["scp", "ssh"]);
    let tree = tree(&[("keep.txt", "kept")]);
    let ran = on(
        &tree,
        fakes.path(),
        &[("FAKE_CHILD_LAST", "== exit 0")],
        &[
            "windows",
            "--host",
            "zeewin",
            "--build",
            "--dir",
            "build-2",
            "--in",
            "app/src-tauri",
            "--env",
            "CONSENSFLOW_TEST_DAEMON=native",
            "--",
            "cargo",
            "test",
        ],
    );
    assert!(ran.status.success(), "{}", err(&ran));
    let report = out(&ran);
    let programs: Vec<&str> = report.split("cwd: ").skip(1).collect();
    assert_eq!(arg_lines(programs[0])[2], arg_line("zeewin:build-2.tgz"));
    let script = script_in(arg_lines(programs[1])[1]);
    assert!(
        script.contains("$dir = Join-Path $env:USERPROFILE 'build-2'\n"),
        "{script}"
    );
    assert!(script.contains("Set-Location (Join-Path $dir 'app/src-tauri')\n"));
    assert!(script.contains("'npm ci --no-audit --no-fund', "));
    assert!(script.contains("$env:CONSENSFLOW_TEST_DAEMON = 'native'\n"));
    assert!(script.contains("'cargo test'"));
}

#[test]
fn a_matrix_runs_each_pair_on_the_machine_builds_it_once_and_keeps_a_log_of_each() {
    let fakes = programs(&["scp", "ssh"]);
    let tree = tree(&[("keep.txt", "kept")]);
    let ran = on(
        &tree,
        fakes.path(),
        &[("FAKE_CHILD_LAST", "== exit 0")],
        &[
            "windows-matrix",
            "--host",
            "zeewin",
            "--build",
            "--scenario",
            "round-trip",
            "--pair",
            "devin:devin",
            "--pair",
            "claude:devin,pi",
            "--claude-model",
            "claude-sonnet-5",
        ],
    );
    // The fake child says what it was asked and no check of an eval: no verdict.
    assert_eq!(ran.status.code(), Some(1), "{}", err(&ran));
    let said = out(&ran);
    let logs = tree.path().join("evals").join("reports").join("windows");
    let folder = fs::read_dir(&logs).unwrap().next().unwrap().unwrap().path();
    for name in ["round-trip devin:devin…", "round-trip claude:devin,pi…"] {
        assert!(
            said.contains(&format!("{name}\n  no verdict (exit 0): see ")),
            "{said}"
        );
    }
    assert!(
        said.contains(&format!("\nlogs: {}\n", folder.display())),
        "{said}"
    );

    // What each run said is its log's: the fake's report of ssh, with the script it was given.
    let script_of = |log: &str| {
        let text = fs::read_to_string(folder.join(log)).unwrap();
        let line = arg_lines(&text)
            .into_iter()
            .find(|line| line.starts_with("arg: \"powershell"))
            .unwrap_or_else(|| panic!("no script in {text}"))
            .to_owned();
        script_in(&line)
    };
    let first = script_of("round-trip-devin-devin.log");
    let second = script_of("round-trip-claude-devin,pi.log");
    assert!(
        first.contains(
            "'cargo xtask eval --scenario round-trip --chief devin --staff devin --timeout-min 30'"
        ),
        "{first}"
    );
    assert!(
        second.contains("'cargo xtask eval --scenario round-trip --chief claude --staff devin,pi --timeout-min 30 --model claude-sonnet-5'"),
        "{second}"
    );
    // Built once: before the first run, and not before the second.
    assert!(first.contains("'npm ci --no-audit --no-fund', "), "{first}");
    assert!(second.contains("foreach ($step in @())"), "{second}");
}

/// A program of a folder that is this script (the system's shell runs it).
#[cfg(unix)]
fn script(folder: &Path, name: &str, body: &str) {
    use std::os::unix::fs::PermissionsExt;

    let path = folder.join(name);
    fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
}

/// What zeewin does: the command's own status is said in the last line, and ssh
/// ends with 1 for any failure, the command's own and the lock's refusal alike.
#[cfg(unix)]
#[test]
fn a_run_answers_the_status_the_machine_says_though_its_ssh_ends_with_1() {
    for (said, answer) in [(7, 7), (75, 75), (0, 0)] {
        let fakes = tempfile::tempdir().unwrap();
        script(fakes.path(), "scp", "exit 0");
        let flattened = i32::from(said != 0);
        // The first ask is the run; the next, for the reports it left, is a listing of none,
        // which the machine's PowerShell ends with 0 whatever the run did.
        script(
            fakes.path(),
            "ssh",
            &format!(
                "n=$(cat \"$0.count\" 2>/dev/null || echo 0); echo $((n+1)) > \"$0.count\"; \
                 if [ \"$n\" -gt 0 ]; then exit 0; fi; \
                 echo \"in the run\"; echo \"== exit {said}\"; exit {flattened}"
            ),
        );
        let tree = tree(&[("keep.txt", "kept")]);
        let ran = on(
            &tree,
            fakes.path(),
            &[],
            &["windows", "--host", "zeewin", "--", "cmd", "/c", "x"],
        );
        assert_eq!(ran.status.code(), Some(answer), "said {said}");
        assert_eq!(out(&ran), format!("in the run\n== exit {said}\n"));
        assert_eq!(err(&ran), "");
    }
}

/// What a stop does to a run, as a person causes it: Ctrl-C (the signal it is)
/// sent to xtask while ssh holds the run, and the program ended there.
#[cfg(unix)]
#[test]
fn a_run_that_is_stopped_ends_ssh_and_asks_the_machine_to_end_the_run_and_answers_130() {
    use std::process::Stdio;
    use std::thread::sleep;
    use std::time::{Duration, Instant};

    use nix::sys::signal::{kill, Signal};
    use nix::unistd::Pid;

    let fakes = tempfile::tempdir().unwrap();
    let log = fakes.path().join("log");
    let first = fakes.path().join("first");
    script(fakes.path(), "scp", "exit 0");
    // The first ssh is the run: it goes on until it is ended. The second is the
    // machine being asked to end it.
    script(
        fakes.path(),
        "ssh",
        r#"printf '%s\t%s\n' "$$" "$2" >> "$CF_LOG"; if [ -e "$CF_FIRST" ]; then exit 0; fi; : > "$CF_FIRST"; exec sleep 120"#,
    );
    let tree = tree(&[("keep.txt", "kept")]);
    let manifest = tree.path().join("tools").join("xtask");
    let mut xtask = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .current_dir(tree.path())
        .env("PATH", path_with(fakes.path()))
        .env("CARGO_MANIFEST_DIR", manifest)
        .env("CF_LOG", &log)
        .env("CF_FIRST", &first)
        .args(["windows", "--host", "zeewin", "--", "sleep", "1"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let lines = |log: &Path| -> Vec<String> {
        fs::read_to_string(log)
            .map(|text| text.lines().map(str::to_owned).collect())
            .unwrap_or_default()
    };
    let until = Instant::now() + Duration::from_secs(120);
    while lines(&log).is_empty() {
        assert!(Instant::now() < until, "the run never reached ssh");
        sleep(Duration::from_millis(50));
    }

    // Only xtask is told: nothing signals ssh but xtask itself.
    kill(
        Pid::from_raw(i32::try_from(xtask.id()).unwrap()),
        Signal::SIGINT,
    )
    .unwrap();
    let status = loop {
        if let Some(status) = xtask.try_wait().unwrap() {
            break status;
        }
        if Instant::now() > until {
            xtask.kill().unwrap();
            panic!("xtask did not end after it was stopped");
        }
        sleep(Duration::from_millis(50));
    };
    assert_eq!(status.code(), Some(130));

    let logged = lines(&log);
    assert_eq!(logged.len(), 2, "{logged:?}");
    let (held, ends): (Vec<&str>, Vec<&str>) = (
        logged[0].split('\t').collect(),
        logged[1].split('\t').collect(),
    );
    // The run's ssh is gone, and the second ssh carried the script that ends the run.
    assert!(!cf_process::alive(held[0].parse().unwrap()), "{logged:?}");
    assert_eq!(
        script_in(&format!("arg: \"{}\"", ends[1])),
        "\n$ProgressPreference = 'SilentlyContinue'\n\
         $running = Join-Path (Join-Path $env:USERPROFILE 'consensflow-build') '.windows-run.pid'\n\
         if (Test-Path $running) { taskkill /PID (Get-Content $running) /T /F | Out-Null; Remove-Item -LiteralPath $running -Force }\n"
    );
}

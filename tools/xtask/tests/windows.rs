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
use std::process::Output;

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use cf_base::env::Env;
use common::{arg_line, arg_lines, checkout, err, out, programs, xtask};
use sha2::{Digest, Sha256};

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
        let ran = common::command("git")
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

/// What a machine says of the tree it unpacked once it has it, for `tree`: the line
/// `== tree <identity>`, the identity being the commit the checkout is at (none: the
/// trees of these tests have no commit) and the first 16 hex digits of a SHA-256 over
/// the files that go over, in git's order, each as its name's length (8 bytes,
/// little-endian), its name, `file`, and its bytes. It is how the documentation of
/// `windows` says a tree is told from another, written out here once more on purpose.
fn tree_line(tree: &tempfile::TempDir) -> String {
    let listing = common::command("git")
        .args(["ls-files", "-co", "--exclude-standard", "-z"])
        .current_dir(tree.path())
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .unwrap();
    assert!(listing.status.success(), "{}", err(&listing));
    let mut hash = Sha256::new();
    for name in String::from_utf8(listing.stdout)
        .unwrap()
        .split('\0')
        .filter(|name| !name.is_empty())
    {
        let path = tree.path().join(name);
        if fs::symlink_metadata(&path).is_err() {
            continue;
        }
        hash.update((name.len() as u64).to_le_bytes());
        hash.update(name.as_bytes());
        hash.update(b"file");
        hash.update(fs::read(&path).unwrap());
    }
    let digest = hash.finalize();
    let content: String = digest[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("== tree none {content}")
}

/// The run script (`.windows-run.ps1`) in the tarball that the fake `scp` kept as number
/// `n` (counting from 1), the file a run is asked for, in `keep`.
fn run_script_kept(keep: &Path, n: usize) -> String {
    let ran = common::command("tar")
        .args(["-xzOf", &format!("{n}.tgz"), ".windows-run.ps1"])
        .current_dir(keep)
        .output()
        .unwrap();
    assert!(ran.status.success(), "tar: {}", err(&ran));
    out(&ran)
}

/// The run's own name in the `host:folder.<id>.tgz` its scp was asked to send to.
fn id_in_scp(destination: &str) -> String {
    let name = destination
        .strip_prefix("arg: \"zeewin:")
        .and_then(|rest| rest.strip_suffix(".tgz\""))
        .unwrap_or_else(|| panic!("not a tarball for the machine: {destination}"));
    let (_, id) = name
        .split_once('.')
        .unwrap_or_else(|| panic!("no run name in {name}"));
    assert_eq!(id.len(), 17, "{id}");
    id.to_owned()
}

/// The run's own name in the script that unpacks its tarball.
fn id_in_script(script: &str) -> String {
    let at =
        script.find("'consensflow-build.").expect("a tarball named") + "'consensflow-build.".len();
    script[at..at + 17].to_owned()
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
            "xtask: windows takes the command to run there after a --: cargo xtask windows \
             [--machine zeewin2|zeewin | --host <ssh host>] [--build] [--env NAME=VALUE] -- <command>\n",
        ),
        (
            vec!["windows", "--host", "zeewin"],
            "xtask: windows takes the command to run there after a --: cargo xtask windows \
             [--machine zeewin2|zeewin | --host <ssh host>] [--build] [--env NAME=VALUE] -- <command>\n",
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
            "xtask: windows-matrix takes a --scenario <name> and a --pair <chief>:<staff> (each \
             as many times as there are): cargo xtask windows-matrix [--machine zeewin2|zeewin | \
             --host <ssh host>] [--env NAME=VALUE] --scenario <name>... --pair <chief>:<staff>...\n",
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
    let said_tree = tree_line(&tree);
    let kept = tempfile::tempdir().unwrap();
    let ran = on(
        &tree,
        fakes.path(),
        &[
            ("FAKE_CHILD_STDERR", "said"),
            ("FAKE_CHILD_FIRST", &said_tree),
            ("FAKE_CHILD_KEEP", kept.path().to_str().unwrap()),
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
    // The tarball is sent under the run's own name: no other run sends to it.
    let id = id_in_scp(scp[2]);
    assert_eq!(
        scp[2],
        arg_line(format!("zeewin:consensflow-build.{id}.tgz"))
    );

    let ssh = arg_lines(programs[1]);
    assert_eq!(ssh.len(), 2, "{ssh:?}");
    assert_eq!(ssh[0], arg_line("zeewin"));
    let script = script_in(ssh[1]);
    assert_eq!(
        id_in_script(&script),
        id,
        "the script unpacks the tarball that was sent"
    );
    // The script the template makes for a run of that name is the one that was recorded.
    assert!(script.replace(&id, "0000abcd-12345678") == golden("xtask-test-integration"));
    // What the run is asked for went in the tarball, as it was recorded.
    assert!(run_script_kept(kept.path(), 1) == golden("xtask-test-integration.run"));
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
        said.contains(" zeewin:consensflow-build.")
            && said.ends_with(".tgz` ended with status 3\n"),
        "{said}"
    );
    // The one that was asked is the one that said what it was asked.
    assert_eq!(out(&ran).matches("cwd: ").count(), 1, "{}", out(&ran));
}

/// A run on the laptop while poker-lab's job holds it, as a person starts it (`--machine zeewin2`,
/// or `--host zeewin2`, which once skipped every guard, T-610): zeelin is asked, says `held`, and the
/// run ends with 75 having started nothing else. The ssh and scp are scripts that write down
/// what they are given.
#[cfg(unix)]
#[test]
fn a_run_on_zeewin2_while_poker_lab_holds_it_ends_with_75_having_sent_nothing_by_either_name() {
    use std::os::unix::fs::PermissionsExt;

    for on_zeewin2 in [["--machine", "zeewin2"], ["--host", "zeewin2"]] {
        let fakes = tempfile::tempdir().unwrap();
        let log = fakes.path().join("log");
        for (name, answer) in [
            ("ssh", "case \"$*\" in *zeelin*) echo held ;; esac"),
            ("scp", ""),
        ] {
            let program = fakes.path().join(name);
            fs::write(
                &program,
                format!(
                    "#!/bin/sh\nprintf '{name} %s\\n' \"$*\" >> '{}'\n{answer}\n",
                    log.display()
                ),
            )
            .unwrap();
            fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
        }
        let tree = tree(&[("keep.txt", "kept")]);
        let mut words = vec!["windows"];
        words.extend(on_zeewin2);
        words.extend(["--", "cargo", "test"]);
        let ran = on(&tree, fakes.path(), &[], &words);
        assert_eq!(ran.status.code(), Some(75), "{on_zeewin2:?}: {}", err(&ran));
        assert_eq!(
            out(&ran),
            "zeewin2 is busy: poker-lab's job holds it (~/.compile-lock/owner on zeelin), and no run goes while it does.\n",
            "{on_zeewin2:?}"
        );
        let asked = fs::read_to_string(&log).unwrap();
        assert_eq!(asked.lines().count(), 1, "{on_zeewin2:?}: {asked}");
        assert!(asked.starts_with("ssh -o ConnectTimeout=5 "), "{asked}");
        assert!(asked.contains(" zeelin "), "{asked}");
    }
}

#[test]
fn a_run_asks_for_the_folder_the_build_and_the_variables_it_was_given() {
    let fakes = programs(&["scp", "ssh"]);
    let tree = tree(&[("keep.txt", "kept")]);
    let kept = tempfile::tempdir().unwrap();
    let ran = on(
        &tree,
        fakes.path(),
        &[
            ("FAKE_CHILD_FIRST", &tree_line(&tree)),
            ("FAKE_CHILD_KEEP", kept.path().to_str().unwrap()),
            ("FAKE_CHILD_LAST", "== exit 0"),
        ],
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
    let id = id_in_scp(arg_lines(programs[0])[2]);
    assert_eq!(
        arg_lines(programs[0])[2],
        arg_line(format!("zeewin:build-2.{id}.tgz"))
    );
    // The folder is on the command line, and nothing the run is asked for is.
    let script = script_in(arg_lines(programs[1])[1]);
    assert!(
        script.contains("$dir = Join-Path $env:USERPROFILE 'build-2'\n"),
        "{script}"
    );
    for asked in [
        "app/src-tauri",
        "npm ci",
        "CONSENSFLOW_TEST_DAEMON",
        "cargo test",
    ] {
        assert!(!script.contains(asked), "{asked}: {script}");
    }
    // It is in the tarball, as the run script.
    let asked = run_script_kept(kept.path(), 1);
    assert!(asked.contains("Set-Location (Join-Path $dir 'app/src-tauri')\n"));
    assert!(asked.contains("'npm ci --no-audit --no-fund', "));
    assert!(asked.contains("$env:CONSENSFLOW_TEST_DAEMON = 'native'\n"));
    assert!(asked.contains("'cargo test'"));
}

#[test]
fn a_matrix_runs_each_pair_on_the_machine_builds_it_once_and_keeps_a_log_of_each() {
    let fakes = programs(&["scp", "ssh"]);
    // The logs of the runs are written into the checkout, as ignored files are.
    let tree = tree(&[("keep.txt", "kept"), (".gitignore", "evals/reports\n")]);
    let kept = tempfile::tempdir().unwrap();
    let ran = on(
        &tree,
        fakes.path(),
        &[
            ("FAKE_CHILD_FIRST", &tree_line(&tree)),
            ("FAKE_CHILD_KEEP", kept.path().to_str().unwrap()),
            ("FAKE_CHILD_LAST", "== exit 0"),
        ],
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

    // What each run is asked for is in the tarball it sent, the first run's first.
    let first = run_script_kept(kept.path(), 1);
    let second = run_script_kept(kept.path(), 2);
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
/// ends with 1 for any failure, the command's own and the lock's refusal alike. The
/// tree it unpacked is said before the command's words, but for the refusal, which
/// unpacks nothing.
#[cfg(unix)]
#[test]
fn a_run_answers_the_status_the_machine_says_though_its_ssh_ends_with_1() {
    for (said, answer) in [(7, 7), (75, 75), (0, 0)] {
        let fakes = tempfile::tempdir().unwrap();
        let kept = fakes.path().join("tree.tgz");
        script(fakes.path(), "scp", r#"cp "$2" "$CF_KEPT""#);
        let flattened = i32::from(said != 0);
        // The first ask is the run; the next, for the reports it left, is a listing of none,
        // which the machine's PowerShell ends with 0 whatever the run did.
        script(
            fakes.path(),
            "ssh",
            &format!(
                "n=$(cat \"$0.count\" 2>/dev/null || echo 0); echo $((n+1)) > \"$0.count\"; \
                 if [ \"$n\" -gt 0 ]; then exit 0; fi; \
                 echo \"in the run\"; if [ {said} -ne 75 ]; then echo \"== tree $(tar -xzOf \"$CF_KEPT\" .windows-tree)\"; fi; echo \"== exit {said}\"; exit {flattened}"
            ),
        );
        let tree = tree(&[("keep.txt", "kept")]);
        let ran = on(
            &tree,
            fakes.path(),
            &[("CF_KEPT", kept.to_str().unwrap())],
            &["windows", "--host", "zeewin", "--", "cmd", "/c", "x"],
        );
        assert_eq!(ran.status.code(), Some(answer), "said {said}");
        let said_tree = if said == 75 {
            String::new()
        } else {
            format!("{}\n", tree_line(&tree))
        };
        assert_eq!(
            out(&ran),
            format!("in the run\n{said_tree}== exit {said}\n")
        );
        assert_eq!(err(&ran), "");
    }
}

/// A machine that says another tree than the one a run sent has not run it: the run is
/// ended there, fails whatever the command went on to say, and says both trees.
#[cfg(unix)]
#[test]
fn a_run_told_another_tree_than_it_sent_fails_loud_and_is_ended_there() {
    let fakes = tempfile::tempdir().unwrap();
    let log = fakes.path().join("log");
    script(fakes.path(), "scp", "exit 0");
    // The first ssh is the run, which says another tree and a pass; the second is the
    // machine being asked to end it.
    script(
        fakes.path(),
        "ssh",
        r#"printf '%s\n' "$$" >> "$CF_LOG"; if [ -e "$CF_FIRST" ]; then exit 0; fi; : > "$CF_FIRST"; echo "== tree none 0000000000000000"; echo "== exit 0"; exit 0"#,
    );
    let tree = tree(&[("keep.txt", "kept")]);
    let first = fakes.path().join("first");
    let ran = on(
        &tree,
        fakes.path(),
        &[
            ("CF_LOG", log.to_str().unwrap()),
            ("CF_FIRST", first.to_str().unwrap()),
        ],
        &["windows", "--host", "zeewin", "--", "cmd", "/c", "x"],
    );
    assert_eq!(ran.status.code(), Some(1), "{}", err(&ran));
    let said = err(&ran);
    assert!(
        said.starts_with("windows: the machine ran another tree than the one this run sent"),
        "{said}"
    );
    assert!(
        said.contains("it said `== tree none 0000000000000000`"),
        "{said}"
    );
    assert!(
        said.contains(&format!("this run sent `{}`", tree_line(&tree))),
        "{said}"
    );
    // The machine was asked to end it: a second ssh.
    assert_eq!(fs::read_to_string(&log).unwrap().lines().count(), 2);
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
    let mut xtask = common::command(env!("CARGO_BIN_EXE_xtask"))
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
    let stop = script_in(&format!("arg: \"{}\"", ends[1]));
    // It ends the run its record names: this run's, by the name its tarball has, which
    // is the one it sent and the one its script unpacks, and it takes that tarball away.
    let id = id_in_script(&script_in(&format!("arg: \"{}\"", held[1])));
    assert_eq!(
        stop,
        format!(
            "\n$ProgressPreference = 'SilentlyContinue'\n\
             $dir = Join-Path $env:USERPROFILE 'consensflow-build'\n\
             $running = Join-Path $dir '.windows-run.pid'\n\
             $owner = Join-Path $dir '.windows-run.id'\n\
             $of = @(Get-Content $owner -ErrorAction SilentlyContinue)\n\
             if ($of[0] -eq '{id}') {{ taskkill /PID ($of[1]) /T /F 2>&1 | Out-Null; Remove-Item -LiteralPath $owner -Force; if ((Get-Content $running -ErrorAction SilentlyContinue) -eq $of[1]) {{ Remove-Item -LiteralPath $running -Force }} }}\n\
             Remove-Item -LiteralPath (Join-Path $env:USERPROFILE 'consensflow-build.{id}.tgz') -Force -ErrorAction SilentlyContinue\n\
             exit 0\n"
        )
    );
}

/// The Windows machine the live case below runs on: `CF_WINDOWS_MACHINE` (zeewin2 or zeewin,
/// as `--machine` names them), else the ssh host `CF_WINDOWS_HOST` (a name of a machine there
/// is that machine, as `--host` reads it), else zeewin.
#[cfg(unix)]
fn live_machine() -> Option<String> {
    std::env::var("CF_WINDOWS_MACHINE").ok()
}

/// The ssh host of the machine the live case runs on.
#[cfg(unix)]
fn live_host() -> String {
    live_machine()
        .or_else(|| std::env::var("CF_WINDOWS_HOST").ok())
        .unwrap_or_else(|| "zeewin".into())
}

/// The words that name the live case's machine to the runner.
#[cfg(unix)]
fn live_on() -> [String; 2] {
    match live_machine() {
        Some(machine) => ["--machine".into(), machine],
        None => ["--host".into(), live_host()],
    }
}

/// A checkout for a run on the real machine, committed, with a marker only it holds and
/// a batch file that says it and then holds the machine's lock for about 20 seconds.
/// Its commit is the second answer.
#[cfg(unix)]
fn live_tree(marker: &str) -> (tempfile::TempDir, String) {
    let dir = tree(&[
        ("marker.txt", marker),
        (
            "hold.cmd",
            "@echo off\r\ntype marker.txt\r\nping -n 20 127.0.0.1 >nul\r\n",
        ),
    ]);
    let git = |words: &[&str]| {
        let ran = common::command("git")
            .args(words)
            .current_dir(dir.path())
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .output()
            .unwrap();
        assert!(ran.status.success(), "git {words:?}: {}", err(&ran));
        out(&ran).trim().to_owned()
    };
    git(&[
        "-c",
        "user.name=windows-test",
        "-c",
        "user.email=windows-test@example.invalid",
        "-c",
        "commit.gpgsign=false",
        "commit",
        "-q",
        "-m",
        marker,
    ]);
    let commit = git(&["rev-parse", "HEAD"]);
    (dir, commit)
}

/// `args` run as `cargo xtask windows` is by a worker's tree (what `windows-from-tree.sh`
/// does: the session's variables and cmux's wrappers stay out), against the machine, in
/// the checkout `tree`: the xtask that was built for the tests, or `CF_WINDOWS_XTASK`
/// where a case wants another to be tried.
#[cfg(unix)]
fn live_start(tree: &tempfile::TempDir, folder: &str, command: &[&str]) -> std::process::Child {
    use std::process::Stdio;

    let own = Env::from_process();
    let path: Vec<PathBuf> = std::env::split_paths(own.os("PATH").unwrap())
        .filter(|part| !part.to_string_lossy().to_lowercase().contains("cmux"))
        .collect();
    let program = std::env::var("CF_WINDOWS_XTASK")
        .unwrap_or_else(|_| env!("CARGO_BIN_EXE_xtask").to_owned());
    let mut start = common::command(program);
    start.env_clear();
    for (name, value) in own.iter() {
        let name = name.to_string_lossy();
        let session = ["CLAUDE", "CMUX", "AI_AGENT"]
            .iter()
            .any(|prefix| name.starts_with(prefix));
        if !session && name != "PATH" && !name.starts_with("GIT_") {
            start.env(&*name, value);
        }
    }
    start
        .env("PATH", std::env::join_paths(path).unwrap())
        .env(
            "CARGO_MANIFEST_DIR",
            tree.path().join("tools").join("xtask"),
        )
        .current_dir(tree.path())
        .arg("windows")
        .args(live_on())
        .args(["--dir", folder])
        // Twelve variables of a hundred characters: more than the command line of ssh could
        // hold with the build, which they ride in the tarball instead of.
        .args((0..12).flat_map(|at| {
            [
                "--env".to_owned(),
                format!("CF_LIVE_{at}={}", "v".repeat(100)),
            ]
        }))
        .arg("--")
        .args(command)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap()
}

/// `script` run on the machine, as the shell of its ssh server runs it (PowerShell).
#[cfg(unix)]
fn on_the_machine(script: &str) -> String {
    let ran = common::command("ssh")
        .arg(live_host())
        .arg(script)
        .output()
        .unwrap();
    assert!(ran.status.success(), "ssh: {}{}", out(&ran), err(&ran));
    out(&ran)
}

/// The case's own folder and what is left in the profile of it, taken away when the case
/// ends, however it ends.
#[cfg(unix)]
struct Folder(String);

#[cfg(unix)]
impl Drop for Folder {
    fn drop(&mut self) {
        let folder = &self.0;
        let _ = common::command("ssh")
            .arg(live_host())
            .arg(format!(
                "Remove-Item -Recurse -Force -ErrorAction SilentlyContinue (Join-Path $env:USERPROFILE '{folder}'), (Join-Path $env:USERPROFILE '{folder}.*tgz')"
            ))
            .output();
    }
}

/// The real machine, one run at a time: two runs started together from two trees, as two
/// workers' are. Each must run its own tree or be refused by the lock, and what a run
/// that is refused sent must not be what the run that holds the lock unpacks, nor stay
/// there. The folder is this case's own, so that its lock is its own: workers' runs in
/// the default folder neither wait for it nor are waited for by it, and what it proves of
/// the lock and the tarballs is the same code. Run it with
/// `cargo test -p xtask --test windows -- --ignored two_runs_started_together`.
#[cfg(unix)]
#[test]
#[ignore = "runs on the Windows machine over ssh (zeewin, or CF_WINDOWS_MACHINE, or CF_WINDOWS_HOST), for about a minute"]
fn two_runs_started_together_from_two_trees_each_run_their_own_tree_or_are_refused() {
    let folder = format!("cf-test-two-runs-{}", std::process::id());
    let _cleanup = Folder(folder.clone());
    let (a, a_commit) = live_tree("marker of tree A");
    let (b, b_commit) = live_tree("marker of tree B");
    // Together, to the millisecond: both are started before either has sent a word, so that
    // their scripts ask the machine for its lock at the moments their uploads end.
    let (first, second) = (
        live_start(&a, &folder, &["hold.cmd"]),
        live_start(&b, &folder, &["hold.cmd"]),
    );
    let (first, second) = (
        first.wait_with_output().unwrap(),
        second.wait_with_output().unwrap(),
    );
    // Whatever was sent there, said before it is removed.
    let left = on_the_machine(&format!(
        "Get-ChildItem $env:USERPROFILE -Filter '{folder}*.tgz' | ForEach-Object {{ $_.Name }}"
    ));

    for (name, run) in [("first", &first), ("second", &second)] {
        println!(
            "--- {name}: status {:?}\n{}{}",
            run.status.code(),
            out(run),
            err(run)
        );
    }
    println!("--- tarballs left on the machine: {:?}", left.trim());

    let statuses = (first.status.code(), second.status.code());
    let ((won, won_marker, won_commit), (lost, lost_marker, lost_tree, lost_commit)) =
        match statuses {
            (Some(0), Some(75)) => (
                (&first, "marker of tree A", &a_commit),
                (&second, "marker of tree B", &b, &b_commit),
            ),
            (Some(75), Some(0)) => (
                (&second, "marker of tree B", &b_commit),
                (&first, "marker of tree A", &a, &a_commit),
            ),
            other => panic!(
                "one run holds the lock and the other is refused with 75, not {other:?}\n--- first\n{}{}\n--- second\n{}{}",
                out(&first),
                err(&first),
                out(&second),
                err(&second)
            ),
        };
    // The run that held the lock ran its own tree, and said so.
    let said = out(won);
    assert!(said.contains(won_marker), "{said}");
    // The twelve variables got there: the last one is said by the run script before its command.
    assert!(said.contains("== env CF_LIVE_11="), "{said}");
    assert!(
        !said.contains(lost_marker),
        "the run that held the lock ran the other run's tree: {said}"
    );
    assert!(
        said.lines().any(|line| line
            .trim_end()
            .starts_with(&format!("== tree {won_commit} "))),
        "{said}"
    );
    assert!(!said.contains(lost_commit.as_str()), "{said}");
    // The one that was refused said so, ran nothing and said no tree.
    let refused = out(lost);
    assert!(
        refused.contains("A run is still going on this machine"),
        "{refused}"
    );
    assert!(!refused.contains("== tree "), "{refused}");
    assert!(!refused.contains(lost_marker), "{refused}");
    // And what either sent is gone, the refused one's too.
    assert_eq!(left.trim(), "", "tarballs left on the machine");

    // Once the lock is free, the run that was refused runs its own tree. Its command ends
    // with a marker that has no line end (`type` of the file), which the run must still
    // answer as a pass: the line it ends by is on a line of its own.
    let again = live_start(lost_tree, &folder, &["type", "marker.txt"])
        .wait_with_output()
        .unwrap();
    let said = out(&again);
    assert_eq!(again.status.code(), Some(0), "{said}{}", err(&again));
    assert!(
        said.lines().any(|line| line
            .trim_end()
            .starts_with(&format!("== tree {lost_commit} "))),
        "{said}"
    );
    assert!(said.contains(lost_marker), "{said}");
}

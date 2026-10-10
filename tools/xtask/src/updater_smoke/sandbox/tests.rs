use super::*;

fn made() -> (tempfile::TempDir, Sandbox) {
    let parent = tempfile::tempdir().unwrap();
    let sandbox = Sandbox::make(parent.path()).unwrap();
    (parent, sandbox)
}

fn text<'a>(env: &'a Env, name: &str) -> &'a str {
    env.text(name)
        .unwrap_or_else(|| panic!("{name} is not set"))
}

#[test]
fn a_machine_has_every_folder_a_case_needs_under_a_folder_of_its_own() {
    let parent = tempfile::tempdir().unwrap();
    let (first, second) = (
        Sandbox::make(parent.path()).unwrap(),
        Sandbox::make(parent.path()).unwrap(),
    );
    assert_ne!(
        first.root, second.root,
        "every case has a machine of its own"
    );
    // Where the system keeps a folder behind a link, the machine is where it really is.
    assert_eq!(first.root, fs::canonicalize(&first.root).unwrap());
    assert!(first
        .root
        .starts_with(fs::canonicalize(parent.path()).unwrap()));
    assert!(first
        .root
        .file_name()
        .unwrap()
        .to_string_lossy()
        .starts_with("cf-updater-smoke-"));
    for folder in [
        &first.apps,
        &first.home,
        &first.state,
        &first.other,
        &first.workspace,
        &first.workspace.join(".consensflow-updater-second"),
        &first.bin,
        &first.pids,
        &first.tls,
        &first.probe,
    ] {
        assert!(folder.is_dir(), "{}", folder.display());
        assert!(folder.starts_with(&first.root));
    }
    assert_eq!(first.copy, first.apps.join("ConsensFlow.app"));
    assert!(
        !first.copy.exists(),
        "the installed app is put there by the case"
    );
    // The parent is made when it is not there.
    let deeper = parent.path().join("a").join("b");
    assert!(Sandbox::make(&deeper)
        .unwrap()
        .root
        .starts_with(fs::canonicalize(&deeper).unwrap()));
}

#[test]
fn a_terminal_on_the_machine_has_its_path_and_homes_and_the_login_shell_is_not_there() {
    let (_parent, sandbox) = made();
    let env = sandbox.terminal_env(&sandbox.other);
    assert_eq!(
        text(&env, "PATH"),
        format!("{}:/usr/bin:/bin:/usr/sbin:/sbin", sandbox.bin.display())
    );
    assert_eq!(text(&env, "HOME"), sandbox.home.to_string_lossy());
    assert_eq!(text(&env, "TMPDIR"), sandbox.root.to_string_lossy());
    assert_eq!(
        text(&env, "CONSENSFLOW_HOME"),
        sandbox.other.to_string_lossy()
    );
    assert_eq!(
        text(&env, "CLAUDE_CONFIG_DIR"),
        sandbox.home.join(".claude").to_string_lossy()
    );
    assert_eq!(
        text(&env, "CODEX_HOME"),
        sandbox.home.join(".codex").to_string_lossy()
    );
    assert_eq!(
        text(&env, "XDG_CONFIG_HOME"),
        sandbox.home.join(".config").to_string_lossy()
    );
    assert_eq!(
        text(&env, "PI_CODING_AGENT_DIR"),
        sandbox.home.join(".pi").join("agent").to_string_lossy()
    );
    assert!(
        env.os("SHELL").is_none(),
        "the app would ask a shell for its PATH"
    );
    assert_eq!(
        env.iter().count(),
        8,
        "and nothing of the machine it runs on"
    );
}

#[test]
fn the_app_has_a_terminals_environment_in_its_own_home_and_the_self_tests_words() {
    let (_parent, sandbox) = made();
    let certificate = sandbox.tls.join("root.pem");
    let key = sandbox.tls.join("updater.key.pub");
    let env = sandbox.app_env(&SelfTest {
        feed: "https://127.0.0.1:4443/feed",
        certificate: &certificate,
        public_key_file: &key,
        expected: "3.0.0-alpha.83",
        deadline: Duration::from_millis(190_000),
    });
    assert_eq!(
        text(&env, "CONSENSFLOW_HOME"),
        sandbox.state.to_string_lossy()
    );
    assert_eq!(text(&env, "HOME"), sandbox.home.to_string_lossy());
    assert_eq!(text(&env, "CONSENSFLOW_SELFTEST"), "1");
    assert_eq!(
        text(&env, "CONSENSFLOW_SELFTEST_DIR"),
        sandbox.workspace.to_string_lossy()
    );
    assert_eq!(
        text(&env, "CONSENSFLOW_SELFTEST_UPDATER_EXPECTED"),
        "3.0.0-alpha.83"
    );
    assert_eq!(
        text(&env, "CONSENSFLOW_SELFTEST_UPDATER_URL"),
        "https://127.0.0.1:4443/feed"
    );
    assert_eq!(
        text(&env, "CONSENSFLOW_SELFTEST_UPDATER_CERT"),
        certificate.to_string_lossy()
    );
    assert_eq!(
        text(&env, "CONSENSFLOW_SELFTEST_UPDATER_KEY"),
        key.to_string_lossy()
    );
    assert_eq!(text(&env, "CONSENSFLOW_SELFTEST_DEADLINE_MS"), "190000");
    assert!(env.os("SHELL").is_none());
    assert_eq!(env.iter().count(), 8 + 7);
}

#[test]
fn the_pids_the_stand_ins_wrote_are_the_files_of_one_each_and_nothing_that_is_not_a_pid() {
    let (_parent, sandbox) = made();
    assert_eq!(sandbox.recorded_pids().unwrap(), Vec::<u32>::new());
    for (name, text) in [
        ("claude-1.pid", "4242\n"),
        ("pi-2.pid", "77"),
        ("codex-3.pid", ""),
        ("opencode-4.pid", "not a pid"),
        ("zero.pid", "0"),
        ("negative.pid", "-5"),
        ("other.txt", "999"),
    ] {
        fs::write(sandbox.pids.join(name), text).unwrap();
    }
    let mut pids = sandbox.recorded_pids().unwrap();
    pids.sort_unstable();
    assert_eq!(pids, [77, 4242]);
}

#[cfg(unix)]
mod on_unix {
    use std::io::{BufRead, BufReader};
    use std::os::unix::fs::{FileTypeExt, PermissionsExt};
    use std::process::Stdio;

    use super::*;
    use crate::process::{self, Invocation};
    use crate::updater_smoke::processes::{gone, until};

    #[test]
    fn a_stand_in_for_each_harness_is_a_program_on_the_machines_path_that_says_its_version() {
        let (_parent, sandbox) = made();
        let env = sandbox.terminal_env(&sandbox.state);
        for (name, version) in [
            ("claude", "2.1.266"),
            ("codex", "0.0.0"),
            ("pi", "0.0.0"),
            ("opencode", "0.0.0"),
        ] {
            let file = sandbox.bin.join(name);
            assert_eq!(
                fs::metadata(&file).unwrap().permissions().mode() & 0o111,
                0o111
            );
            let ran = process::capture(
                &Invocation::new(name, &sandbox.probe).arg("--version"),
                &env,
            )
            .unwrap();
            assert_eq!(
                (ran.code, ran.stdout.as_str()),
                (0, format!("{version}\n").as_str())
            );
        }
    }

    #[test]
    fn a_stand_in_is_alive_records_its_pid_and_reads_its_input_until_it_ends() {
        let (_parent, sandbox) = made();
        let env = sandbox.terminal_env(&sandbox.state);
        let mut child = process::spawn(
            &Invocation::new("codex", &sandbox.probe),
            &env,
            Stdio::piped(),
            false,
        )
        .unwrap();
        let pid = child.id();
        let mut out = BufReader::new(child.stdout.take().unwrap());
        let mut line = String::new();
        out.read_line(&mut line).unwrap();
        assert_eq!(line, format!("CFUPDATER-ALIVE {pid}\n"));
        assert_eq!(sandbox.recorded_pids().unwrap(), [pid]);
        assert_eq!(
            fs::read_to_string(sandbox.pids.join(format!("codex-{pid}.pid"))).unwrap(),
            format!("{pid}\n")
        );
        // It reads for ever: until its input ends.
        until("the stand-in runs", Duration::from_secs(60), || {
            Ok((!gone(pid)).then_some(()))
        })
        .unwrap();
        drop(child.stdin.take());
        assert!(child.wait().unwrap().success());
    }

    #[test]
    fn a_fifo_is_made_in_a_folder_of_its_own_each_time() {
        let parent = tempfile::tempdir().unwrap();
        let first = make_fifo(parent.path()).unwrap();
        let second = make_fifo(parent.path()).unwrap();
        assert_ne!(first, second);
        for fifo in [&first, &second] {
            assert!(fs::metadata(fifo).unwrap().file_type().is_fifo());
            assert!(fifo.starts_with(parent.path()));
            assert_eq!(fifo.file_name().unwrap(), "input.fifo");
        }
        assert_ne!(first.parent(), second.parent());
    }
}

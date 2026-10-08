//! The supervisor against Codex programs that are shell scripts: what it does
//! when Codex cannot be started, or never comes up, with nothing real to talk
//! to. Whole windows, with a TUI and a server that speaks, are the process
//! tests (`tests/integration/codex-session.test.mjs`).

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;
use std::time::Instant;

use sha2::{Digest, Sha256};

use super::*;
use crate::broker::tests::fixture::{Behaviour, FakeCodex};

const BRIDGE: &str =
    r#"{"launchId":"launch-1","port":0,"token":"private-launch-token-1234567890"}"#;

/// A home for one test and the environment that names it and a broker.
struct Home {
    dir: tempfile::TempDir,
    env: Env,
}

impl Home {
    fn new() -> Self {
        Self::bridged(Some(BRIDGE))
    }

    /// A short path, so a socket in it fits.
    fn bridged(bridge: Option<&str>) -> Self {
        let dir = tempfile::Builder::new()
            .prefix("cf-sup-")
            .tempdir_in("/tmp")
            .unwrap();
        let mut vars = vec![("CONSENSFLOW_HOME", dir.path().to_str().unwrap().to_string())];
        vars.extend(bridge.map(|bridge| ("CF_CODEX_SESSION_BRIDGE", bridge.to_string())));
        let env = Env::from_vars(vars);
        Self { dir, env }
    }

    /// A Codex that is this script.
    fn codex(&self, body: &str) -> PathBuf {
        let script = self.dir.path().join("codex");
        fs::write(&script, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        script
    }

    fn supervise(
        &self,
        program: impl Into<OsString>,
        args: &[&str],
        startup: Duration,
    ) -> Result<i32, SessionError> {
        let mut all = vec![program.into()];
        all.extend(args.iter().map(OsString::from));
        let setup = Setup {
            startup,
            endpoint: Endpoint::open,
        };
        crate::block_on_local(supervise_with(&self.env, &all, setup)).unwrap()
    }

    /// What the window left in its socket folder.
    fn left_behind(&self) -> Vec<String> {
        fs::read_dir(self.dir.path().join("tmp"))
            .map(|entries| {
                entries
                    .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default()
    }
}

#[test]
fn says_why_codexs_server_could_not_start_with_what_it_wrote_and_leaves_nothing_behind() {
    let home = Home::new();
    let args = home.dir.path().join("args");
    let codex = home.codex(&format!(
        "printf '%s\\n' \"$@\" > '{}'\necho 'not logged in' >&2\nexit 1",
        args.display()
    ));
    let failed = home
        .supervise(
            codex,
            &["-m", "gpt-5.6-luna", BYPASS],
            Duration::from_secs(5),
        )
        .unwrap_err();
    assert_eq!(
        failed.to_string(),
        "Codex server could not start: not logged in\n"
    );
    assert!(home.left_behind().is_empty(), "{:?}", home.left_behind());
    // The server was given the backend's arguments, ConsensFlow's own
    // variables in its shell policy, and where to listen.
    let given = fs::read_to_string(&args).unwrap();
    let given: Vec<_> = given.lines().collect();
    let home_policy = format!(
        "shell_environment_policy.set.CONSENSFLOW_HOME={:?}",
        home.dir.path().to_str().unwrap()
    );
    assert_eq!(
        &given[..8],
        [
            "-c",
            "model=\"gpt-5.6-luna\"",
            "-c",
            "approval_policy=\"never\"",
            "-c",
            "sandbox_mode=\"danger-full-access\"",
            "-c",
            home_policy.as_str(),
        ]
    );
    assert_eq!(&given[8..10], ["app-server", "--listen"]);
    assert!(
        given[10].starts_with(&format!(
            "unix://{}/tmp/codex-",
            home.dir.path().to_str().unwrap()
        )) && given[10].ends_with("/native.sock"),
        "{}",
        given[10]
    );
    assert_eq!(given.len(), 11);
}

#[test]
fn a_program_that_is_not_there_is_said_by_its_path_and_the_systems_words() {
    let home = Home::new();
    let missing = home.dir.path().join("gone").join("codex");
    let failed = home
        .supervise(&missing, &[], Duration::from_secs(5))
        .unwrap_err();
    assert_eq!(
        failed.to_string(),
        format!(
            "Codex server could not start: {}: No such file or directory (os error 2)",
            missing.display()
        )
    );
    assert!(home.left_behind().is_empty());
}

#[test]
fn a_codex_that_is_an_npm_shim_with_no_node_to_run_on_is_refused_saying_what_to_do() {
    // npm's shim for a global package: its last line runs a node, which is the
    // one beside it or on the PATH, and neither is here (the environment has no PATH).
    let home = Home::new();
    let script = ["node_modules", "@openai", "codex", "bin", "codex.js"]
        .iter()
        .fold(home.dir.path().to_path_buf(), |path, part| path.join(part));
    fs::create_dir_all(script.parent().unwrap()).unwrap();
    fs::write(&script, "").unwrap();
    let shim = home.dir.path().join("codex.cmd");
    fs::write(
        &shim,
        "@ECHO off\r\n\"%_prog%\"  \"%dp0%\\node_modules\\@openai\\codex\\bin\\codex.js\" %*\r\n",
    )
    .unwrap();
    let failed = home
        .supervise(&shim, &[], Duration::from_secs(5))
        .unwrap_err();
    assert!(matches!(failed, SessionError::Unrunnable(_)), "{failed:?}");
    let said = failed.to_string();
    assert!(said.starts_with(&shim.display().to_string()), "{said}");
    assert!(
        said.contains("Make the harness's Node visible to ConsensFlow")
            && said.contains("install the harness's own build"),
        "{said}"
    );
    assert!(home.left_behind().is_empty());
}

#[test]
fn a_bridge_that_is_missing_or_not_what_a_broker_needs_is_refused_before_anything_starts() {
    for bridge in [
        None,
        Some("{}"),
        Some("not json"),
        Some(r#"{"launchId":"launch-1","port":0,"token":"short"}"#),
        Some(r#"{"launchId":"launch/1","port":0,"token":"private-launch-token-1234567890"}"#),
        Some(r#"{"launchId":"launch-1","port":70000,"token":"private-launch-token-1234567890"}"#),
    ] {
        let home = Home::bridged(bridge);
        let started = home.dir.path().join("started");
        let codex = home.codex(&format!("echo started > '{}'", started.display()));
        let failed = home
            .supervise(codex, &[], Duration::from_secs(5))
            .unwrap_err();
        assert_eq!(
            failed.to_string(),
            "Invalid Codex broker configuration",
            "{bridge:?}"
        );
        assert!(!started.exists(), "Codex started under {bridge:?}");
        assert!(
            !home.dir.path().join("tmp").exists(),
            "nothing was made for {bridge:?}"
        );
    }
}

#[test]
fn a_flag_that_wants_a_value_and_has_none_is_refused_before_anything_starts() {
    let home = Home::new();
    let started = home.dir.path().join("started");
    let codex = home.codex(&format!("echo started > '{}'", started.display()));
    for (given, said) in [
        ("-c", "Missing Codex -c value"),
        ("--model", "Missing Codex model"),
    ] {
        let failed = home
            .supervise(&codex, &["resume", given], Duration::from_secs(5))
            .unwrap_err();
        assert_eq!(failed.to_string(), said);
    }
    assert!(!started.exists());
}

#[test]
fn no_program_named_is_said() {
    let home = Home::new();
    let setup = Setup {
        startup: Duration::from_secs(1),
        endpoint: Endpoint::open,
    };
    let failed = crate::block_on_local(supervise_with(&home.env, &[], setup))
        .unwrap()
        .unwrap_err();
    assert_eq!(failed.to_string(), "no Codex program was named");
}

/// Codex as one script for both its programs: the server runs `server_first`,
/// says where it listens, on loopback, as it does on Windows, and runs until
/// it is ended; the TUI says what it was given and ends with 7.
fn codex_on_loopback(home: &Home, port: u16, server_first: &str) -> PathBuf {
    let dir = home.dir.path().display();
    home.codex(&format!(
        "case \" $* \" in\n\
         *\" app-server \"*)\n\
         {server_first}\n\
         printf '%s\\n' \"$@\" > '{dir}/server-args'\n\
         echo 'codex app-server (WebSockets)' >&2\n\
         echo '  listening on: ws://127.0.0.1:{port}' >&2\n\
         exec sleep 60 ;;\n\
         *)\n\
         printf '%s\\n' \"$@\" > '{dir}/tui-args'\n\
         printf '%s' \"$CF_CODEX_TUI_TOKEN\" > '{dir}/tui-token'\n\
         exit 7 ;;\n\
         esac"
    ))
}

#[test]
fn a_window_on_loopback_as_windows_runs_one_hands_the_server_its_tokens_hash_and_connects_with_it()
{
    let home = Home::new();
    let (code, headers, requests) = crate::block_on_local(async {
        let codex = FakeCodex::start(Behaviour::Ready).await;
        let program = codex_on_loopback(&home, codex.address.port(), "");
        let setup = Setup {
            startup: Duration::from_secs(10),
            endpoint: |_| Endpoint::loopback(),
        };
        let args = [
            program.into_os_string(),
            "--model".into(),
            "gpt-5.6-luna".into(),
        ];
        let code = supervise_with(&home.env, &args, setup).await;
        (code, codex.headers(0), codex.requests())
    })
    .unwrap();
    assert_eq!(code.unwrap(), 7, "the TUI's code is the window's");

    // The server is given the hash of the window's own token, to listen on loopback for it.
    let given = fs::read_to_string(home.dir.path().join("server-args")).unwrap();
    let given: Vec<_> = given.lines().collect();
    let hash = given.last().copied().unwrap();
    assert_eq!(hash.len(), 64);
    assert!(hash.bytes().all(|byte| byte.is_ascii_hexdigit()), "{hash}");
    let policy = format!(
        "shell_environment_policy.set.CONSENSFLOW_HOME={:?}",
        home.dir.path().to_str().unwrap()
    );
    assert_eq!(
        given[..given.len() - 1],
        [
            "-c",
            "model=\"gpt-5.6-luna\"",
            "-c",
            policy.as_str(),
            "app-server",
            "--listen",
            "ws://127.0.0.1:0",
            "--ws-auth",
            "capability-token",
            "--ws-token-sha256",
        ]
    );

    // The broker's own connection carried the token that hash is of: the hash
    // of its text, as it was sent.
    let bearer = headers
        .iter()
        .find(|(name, _)| name == "authorization")
        .map(|(_, value)| value.as_str())
        .unwrap();
    let token = bearer.strip_prefix("Bearer ").unwrap();
    let digest: String = Sha256::digest(token)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    assert_eq!(digest, hash);
    assert_eq!(requests[0]["method"], "initialize");

    // The TUI came to the broker, its token in its environment and nowhere else.
    let tui = fs::read_to_string(home.dir.path().join("tui-args")).unwrap();
    let tui: Vec<_> = tui.lines().collect();
    assert!(
        tui[1].starts_with("ws://127.0.0.1:") && tui[1] != "ws://127.0.0.1:0",
        "{tui:?}"
    );
    assert_eq!(
        [tui[0], tui[2], tui[3], tui[4], tui[5]],
        [
            "--remote",
            "--remote-auth-token-env",
            "CF_CODEX_TUI_TOKEN",
            "--model",
            "gpt-5.6-luna"
        ]
    );
    let tui_token = fs::read_to_string(home.dir.path().join("tui-token")).unwrap();
    assert_eq!(tui_token, "private-launch-token-1234567890");
    assert!(
        !home.dir.path().join("tmp").exists(),
        "no socket folder on loopback"
    );
}

#[test]
#[allow(clippy::disallowed_methods)] // The test asks the system whether a process is there.
fn a_server_that_does_not_end_when_asked_is_killed_after_a_moment_and_the_tuis_code_stands() {
    let home = Home::new();
    let pid = home.dir.path().join("pid");
    let started = Instant::now();
    let (code, ended_at) = crate::block_on_local(async {
        let codex = FakeCodex::start(Behaviour::Ready).await;
        // A server that takes SIGTERM and carries on.
        let program = codex_on_loopback(
            &home,
            codex.address.port(),
            &format!("trap '' TERM\necho $$ > '{}'", pid.display()),
        );
        let setup = Setup {
            startup: Duration::from_secs(10),
            endpoint: |_| Endpoint::loopback(),
        };
        let code = supervise_with(&home.env, &[program.into_os_string()], setup).await;
        (code, Instant::now())
    })
    .unwrap();
    assert_eq!(code.unwrap(), 7);
    assert!(
        pid.exists(),
        "the server did run, and what it was told was ignored"
    );
    // Asked at once, killed after the 1.5 seconds it had, not a minute later
    // when its own `sleep` would end, and not before.
    let took = ended_at - started;
    assert!(
        took >= Duration::from_millis(1400) && took < Duration::from_secs(12),
        "{took:?}"
    );
    let pid = fs::read_to_string(&pid).unwrap();
    let alive = Command::new("kill")
        .args(["-0", pid.trim()])
        .stderr(Stdio::null())
        .status()
        .unwrap()
        .success();
    assert!(!alive, "the server {pid} is still running");
}

#[test]
fn the_servers_standard_error_is_read_for_as_long_as_it_runs_not_only_until_it_is_up() {
    let home = Home::new();
    // Three megabytes, far more than a pipe holds: a server that is not read
    // blocks on its own stderr and never gets to its last line.
    let codex = home.codex(
        "head -c 3000000 /dev/zero | tr '\\0' 'x' >&2\necho >&2\necho 'all of it written' >&2",
    );
    let started = Instant::now();
    let tail = crate::block_on_local(async {
        let plan = Plan {
            env: &home.env,
            executable: &codex,
            bridge: Bridge::parse(BRIDGE).unwrap(),
            split: Split::default(),
            endpoint: Endpoint::open(&home.env).unwrap(),
            bypass: false,
            startup: Duration::from_secs(5),
        };
        let tail = Rc::new(RefCell::new(Tail::default()));
        let mut session = Session::default();
        let drain = session.start_backend(&plan, &tail).unwrap();
        let status = tokio::time::timeout(Duration::from_secs(20), exit_of(&mut session.backend))
            .await
            .expect("the server ended: it was not left blocked on a full pipe");
        assert!(status.unwrap().success());
        drain.await.unwrap();
        session.finish(&plan).await.unwrap();
        let text = tail.borrow().text().to_string();
        text
    })
    .unwrap();
    assert!(
        tail.ends_with("\nall of it written\n"),
        "{:?}",
        &tail[tail.len() - 40..]
    );
    assert!(cf_base::text::utf16_len(&tail) <= 4000);
    assert!(started.elapsed() < Duration::from_secs(20));
}

#[test]
#[allow(clippy::disallowed_methods)] // The test asks the system whether a process is there.
fn a_server_that_does_not_come_up_in_time_is_ended_and_said_with_what_it_wrote() {
    let home = Home::new();
    let pid = home.dir.path().join("pid");
    let codex = home.codex(&format!(
        "echo $$ > '{}'\necho 'still loading' >&2\nexec sleep 60",
        pid.display()
    ));
    // Time enough for the stand-in's shell to start and say it on a loaded
    // machine: 600 ms was not, once in six runs at a load of 13 to 24.
    let started = Instant::now();
    let failed = home
        .supervise(codex, &[], Duration::from_secs(2))
        .unwrap_err();
    let took = started.elapsed();
    assert_eq!(
        failed.to_string(),
        "Codex server could not start: still loading\n"
    );
    assert!(
        took >= Duration::from_secs(2) && took < Duration::from_secs(12),
        "{took:?}"
    );
    assert!(home.left_behind().is_empty());
    // It was ended with the window, not left running.
    let pid = fs::read_to_string(&pid).unwrap();
    let alive = Command::new("kill")
        .args(["-0", pid.trim()])
        .stderr(Stdio::null())
        .status()
        .unwrap()
        .success();
    assert!(!alive, "the server {pid} is still running");
}

#[test]
fn a_socket_folder_already_gone_is_no_failure_and_one_that_cannot_go_is() {
    let dir = tempfile::tempdir().unwrap();
    assert!(remove_folder(&dir.path().join("gone")).is_ok());
    let locked = dir.path().join("locked");
    fs::create_dir_all(locked.join("codex-1")).unwrap();
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o500)).unwrap();
    let kept = remove_folder(&locked.join("codex-1"));
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(
        kept.map_err(|cause| cause.kind()),
        Err(std::io::ErrorKind::PermissionDenied)
    );
}

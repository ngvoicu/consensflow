//! What the app starts as its daemon: the native one, the bundled `cf` running
//! `cf ui --json --no-open`, on the PATH the human's login shell sets up. For
//! the flip release alone there is a way back: a home with a `use-node` file in
//! it (`cf_base::way_back`, which the native `cf` and `bin/cf.mjs` ask as well,
//! so the daemon and every `cf` verb of that home are one implementation) gets
//! the bundled runtime running the bundled CLI, `node cf.mjs ui --json
//! --no-open`, as before. The home decides, and nothing in the environment: the
//! app takes only the login shell's PATH, so a variable left in a shell profile
//! would run the terminal's verbs on one implementation and the app's daemon on
//! the other. The portable Windows app carries the runtime and the CLI inside its
//! exe and unpacks them first.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use cf_base::env::Env;
use cf_base::way_back::{self, Choice};
use tauri::{AppHandle, Manager};

use crate::daemon::DaemonFailure;

/// How long the human's login shell has to say its PATH.
const LOGIN_PATH_TIMEOUT: Duration = Duration::from_secs(5);

/// The daemon the home chooses, `cf ui --json --no-open` as the native one or
/// as Node's, on the human's login PATH. A runtime or CLI missing from the app
/// is not worth another start. Which daemon it is, and what said so (the file
/// in the home, or the default), is one line of the app's error log.
pub(crate) fn daemon_command(app: &AppHandle) -> Result<Command, DaemonFailure> {
    let missing = |cause| DaemonFailure {
        cause,
        retry: false,
    };
    let (node, cli) = bundled_cli(app).map_err(missing)?;
    let (mut command, choice) = command_in(&Env::from_process(), &node, &cli);
    eprintln!("consensflow: {}", chosen(&choice));
    if !choice.node && !Path::new(command.get_program()).exists() {
        return Err(missing(format!(
            "the bundled native ConsensFlow is missing from this app ({:?})",
            command.get_program()
        )));
    }
    if let Some(path) = login_path() {
        command.env("PATH", path);
    }
    // node.exe is a console program: started from a windowed app it gets a
    // console window of its own, and every console program it starts shows in
    // it. The daemon runs without one; its windows are the app's panes.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    Ok(command)
}

/// The daemon of the home `env` names, and what chose it: the way back's file
/// in the home or its absence, and nothing else of the environment.
fn command_in(env: &Env, node: &Path, cli: &Path) -> (Command, Choice) {
    let choice = way_back::choose(env);
    (command_for(node, cli, choice.node), choice)
}

/// The daemon `node` and the CLI `cf.mjs` make: Node's, `node cf.mjs ui
/// --json --no-open`, when the home has taken the way back; or else the
/// native `cf` beside `cf.mjs` running `cf ui --json --no-open`, told where the
/// bundled node is, which a window's npm shim on Windows may run on (Node's
/// daemon names its own).
fn command_for(node: &Path, cli: &Path, use_node: bool) -> Command {
    if !use_node {
        let mut command = Command::new(native_cf(cli));
        command
            .args(["ui", "--json", "--no-open"])
            .env("CONSENSFLOW_NODE", node);
        return command;
    }
    let mut command = Command::new(node);
    command.arg(cli).args(["ui", "--json", "--no-open"]);
    command
}

/// The native `cf` of the bundle whose CLI is `cli`: beside its `cf.mjs`, which
/// a portable runtime's own folder holds as an installed app's resources do.
pub(crate) fn native_cf(cli: &Path) -> PathBuf {
    cli.with_file_name(if cfg!(windows) { "cf.exe" } else { "cf" })
}

/// What the app's error log says of the daemon it starts: which one, and what
/// chose it, the file in the home (the way back) or nothing (the default).
fn chosen(choice: &Choice) -> String {
    match (&choice.file, choice.node) {
        (Some(file), true) => format!(
            "starting Node's daemon: {} is there, the way back to Node",
            file.display()
        ),
        (Some(file), false) => format!(
            "starting the native daemon: the default, there is no {}",
            file.display()
        ),
        (None, _) => "starting the native daemon: the default, there is no home to look in for \
                      the way back to Node"
            .to_owned(),
    }
}

/// The bundled runtime and CLI: the Node and the `cf.mjs` of this app's own bundle.
pub(crate) fn bundled_cli(app: &AppHandle) -> Result<(PathBuf, PathBuf), String> {
    #[cfg(windows)]
    if let Some(runtime) = portable_runtime(app)? {
        // The terminals' console host is in the runtime; without it Windows' own serves.
        if let Err(error) = crate::portable::find_libraries_in(&plain_path(runtime.clone())) {
            eprintln!("consensflow: the runtime's console host is not found ({error}); Windows' own serves");
        }
        return present(
            runtime.join("node.exe"),
            runtime.join("cli").join("bin").join("cf.mjs"),
        );
    }
    let resources = app
        .path()
        .resource_dir()
        .map_err(|error| format!("the app could not find its own resources: {error}"))?;
    // Tauri strips the target triple from a sidecar's name and, on Windows,
    // keeps the `.exe`: `node` on macOS, `node.exe` beside the app there.
    let sidecar = if cfg!(windows) { "node.exe" } else { "node" };
    let resource_node = resources.join("binaries").join(sidecar);
    let node = if resource_node.exists() {
        resource_node
    } else {
        std::env::current_exe()
            .map_err(|error| format!("the app could not find itself: {error}"))?
            .parent()
            .ok_or_else(|| "the app executable has no directory".to_string())?
            .join(sidecar)
    };
    present(node, resources.join("cli").join("bin").join("cf.mjs"))
}

/// The runtime and the CLI, when both are there.
fn present(node: PathBuf, cli: PathBuf) -> Result<(PathBuf, PathBuf), String> {
    // Tauri may answer its folders in Windows' verbatim form (`\\?\C:\…`),
    // which Node cannot take as a script path: it stops at the drive with
    // `lstat 'C:'`. The plain spelling names the same file.
    let node = plain_path(node);
    let cli = plain_path(cli);
    if !node.is_absolute() || !node.exists() {
        return Err(format!(
            "the bundled runtime is missing from this app ({node:?})"
        ));
    }
    if !cli.is_absolute() || !cli.exists() {
        return Err(format!(
            "the bundled ConsensFlow is missing from this app ({cli:?})"
        ));
    }
    Ok((node, cli))
}

/// The portable app's runtime (see `portable`), unpacked from its own exe by
/// its first start under the app's local data folder: on Windows,
/// `%LOCALAPPDATA%\<identifier>\portable-runtime`. `None` for an app
/// installed with its runtime beside it.
#[cfg_attr(not(windows), allow(dead_code))]
fn portable_runtime(app: &AppHandle) -> Result<Option<PathBuf>, String> {
    let exe = std::env::current_exe()
        .map_err(|error| format!("the app could not find itself: {error}"))?;
    let local_data = app
        .path()
        .app_local_data_dir()
        .map_err(|error| format!("the app could not find its local data folder: {error}"))?;
    crate::portable::unpacked_runtime(&exe, &local_data, &app.package_info().version.to_string())
}

/// A Windows path without the `\\?\` verbatim prefix; any other path as it is.
pub(crate) fn plain_path(path: PathBuf) -> PathBuf {
    let text = path.to_string_lossy();
    if let Some(rest) = text.strip_prefix("\\\\?\\UNC\\") {
        return PathBuf::from(format!("\\\\{rest}"));
    }
    if let Some(rest) = text.strip_prefix("\\\\?\\") {
        return PathBuf::from(rest);
    }
    path
}

/// The PATH the human's login shell sets up, where their harness CLIs live,
/// for the daemon and every pane it opens.
fn login_path() -> Option<String> {
    login_path_in(Path::new(&std::env::var_os("SHELL")?), LOGIN_PATH_TIMEOUT)
}

/// A login file may print (`nvm use` does), wait for input or never finish,
/// and the PATH used to be the shell's whole output, read on the main thread
/// before the window existed, for as long as the shell took. So the PATH is
/// read between markers no login file prints, from a shell that exits
/// cleanly within `timeout`; otherwise there is none, and the daemon keeps
/// the PATH the app was started with.
fn login_path_in(shell: &Path, timeout: Duration) -> Option<String> {
    use std::hash::BuildHasher;
    use std::io::Read;

    let marker = format!(
        "<consensflow-path-{:016x}>",
        std::hash::RandomState::new().hash_one(std::process::id())
    );
    let mut child = Command::new(shell)
        .args(["-lc", &format!("printf '{marker}%s{marker}' \"$PATH\"")])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut output = child.stdout.take()?;
    let (chunks, printed) = mpsc::channel();
    // Not joined: a process a login file started may hold the output open
    // long after the shell has gone.
    thread::spawn(move || {
        let mut chunk = [0; 4096];
        while let Ok(read) = output.read(&mut chunk) {
            if read == 0 || chunks.send(chunk[..read].to_vec()).is_err() {
                return;
            }
        }
    });
    let deadline = Instant::now() + timeout;
    let succeeded = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.success(),
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break false;
            }
        }
    };
    if !succeeded {
        return None;
    }
    // The shell has gone, so what it printed is in the pipe, a moment from
    // the reader at most.
    let mut bytes = Vec::new();
    loop {
        let text = String::from_utf8_lossy(&bytes);
        let mut parts = text.split(marker.as_str());
        if let (Some(_), Some(path), Some(_)) = (parts.next(), parts.next(), parts.next()) {
            return (!path.is_empty()).then(|| path.to_string());
        }
        bytes.extend(printed.recv_timeout(Duration::from_secs(1)).ok()?);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NODE: &str = "/bundle/binaries/node";
    const CLI: &str = "/bundle/cli/bin/cf.mjs";

    /// A home of this test's own, with the way back's file in it or not, and
    /// the environment that names it, with `stray` added to it.
    fn home_with(way_back: bool, stray: &[(&str, &str)]) -> (tempfile::TempDir, Env) {
        let home = tempfile::tempdir().expect("a home");
        if way_back {
            std::fs::write(home.path().join(way_back::FILE), "").expect("the way back");
        }
        let mut vars = vec![(
            "CONSENSFLOW_HOME".to_owned(),
            home.path().to_string_lossy().into_owned(),
        )];
        vars.extend(
            stray
                .iter()
                .map(|(name, value)| ((*name).to_owned(), (*value).to_owned())),
        );
        (home, Env::from_vars(vars))
    }

    /// What a command runs, and with what: its program, words and added variables.
    fn described(command: &Command) -> (PathBuf, Vec<String>, Vec<(String, String)>) {
        (
            PathBuf::from(command.get_program()),
            command
                .get_args()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect(),
            command
                .get_envs()
                .filter_map(|(name, value)| {
                    Some((
                        name.to_string_lossy().into_owned(),
                        value?.to_string_lossy().into_owned(),
                    ))
                })
                .collect(),
        )
    }

    /// The native daemon as the app starts it: the `cf` beside `cf.mjs`, told the bundled node.
    fn native() -> (PathBuf, Vec<String>, Vec<(String, String)>) {
        let cf = if cfg!(windows) {
            "/bundle/cli/bin/cf.exe"
        } else {
            "/bundle/cli/bin/cf"
        };
        (
            PathBuf::from(cf),
            vec!["ui".into(), "--json".into(), "--no-open".into()],
            vec![("CONSENSFLOW_NODE".into(), NODE.into())],
        )
    }

    /// Node's: the bundled runtime running the bundled CLI, with nothing added.
    fn node() -> (PathBuf, Vec<String>, Vec<(String, String)>) {
        (
            PathBuf::from(NODE),
            vec![CLI.into(), "ui".into(), "--json".into(), "--no-open".into()],
            vec![],
        )
    }

    #[test]
    fn the_daemon_is_the_native_one_by_default() {
        let (_home, env) = home_with(false, &[]);
        let (command, choice) = command_in(&env, Path::new(NODE), Path::new(CLI));
        assert_eq!(described(&command), native());
        assert!(!choice.node);
    }

    #[test]
    fn the_daemon_is_node_running_cf_mjs_when_the_home_has_the_file() {
        let (_home, env) = home_with(true, &[]);
        let (command, choice) = command_in(&env, Path::new(NODE), Path::new(CLI));
        assert_eq!(described(&command), node());
        assert!(choice.node);
    }

    #[test]
    fn the_environment_says_nothing_of_which_daemon_it_is() {
        // `CONSENSFLOW_DAEMON` was the switch before the flip, and a terminal does
        // not inherit the app's environment, so the app that obeyed it would be one
        // implementation to the home and another to its `cf`.
        for stray in ["node", "native", "", "yes"] {
            let (_home, env) = home_with(false, &[("CONSENSFLOW_DAEMON", stray)]);
            let (command, _) = command_in(&env, Path::new(NODE), Path::new(CLI));
            assert_eq!(described(&command), native(), "no file, {stray:?}");
            let (_home, env) = home_with(true, &[("CONSENSFLOW_DAEMON", stray)]);
            let (command, _) = command_in(&env, Path::new(NODE), Path::new(CLI));
            assert_eq!(described(&command), node(), "the file, {stray:?}");
        }
    }

    #[test]
    fn a_home_with_no_folder_to_look_in_is_the_native_daemons() {
        let (command, choice) = command_in(&Env::default(), Path::new(NODE), Path::new(CLI));
        assert_eq!(described(&command), native());
        assert_eq!(choice.file, None);
    }

    #[test]
    fn the_log_says_which_daemon_and_what_chose_it() {
        let (home, env) = home_with(false, &[]);
        let file = home.path().join(way_back::FILE);
        let (_, by_default) = command_in(&env, Path::new(NODE), Path::new(CLI));
        assert_eq!(
            chosen(&by_default),
            format!(
                "starting the native daemon: the default, there is no {}",
                file.display()
            )
        );
        let (home, env) = home_with(true, &[]);
        let file = home.path().join(way_back::FILE);
        let (_, by_file) = command_in(&env, Path::new(NODE), Path::new(CLI));
        assert_eq!(
            chosen(&by_file),
            format!(
                "starting Node's daemon: {} is there, the way back to Node",
                file.display()
            )
        );
        let (_, homeless) = command_in(&Env::default(), Path::new(NODE), Path::new(CLI));
        assert!(chosen(&homeless).starts_with("starting the native daemon: the default, "));
    }

    #[test]
    fn the_native_cf_is_the_one_beside_the_cli() {
        let cf = if cfg!(windows) { "cf.exe" } else { "cf" };
        assert_eq!(
            native_cf(Path::new("/bundle/cli/bin/cf.mjs")),
            Path::new("/bundle/cli/bin").join(cf)
        );
    }

    /// A stand-in for the human's login shell: `body` runs with the command
    /// the app gives it (`-lc <command>`) as `$2`.
    #[cfg(unix)]
    fn login_shell(home: &Path, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;

        let shell = home.join("login-shell");
        std::fs::write(&shell, format!("#!/bin/sh\n{body}\n")).expect("write the shell");
        std::fs::set_permissions(&shell, std::fs::Permissions::from_mode(0o755))
            .expect("make the shell executable");
        shell
    }

    /// What a login file prints (`nvm use` says which Node it took) is not
    /// part of the PATH.
    #[cfg(unix)]
    #[test]
    fn the_login_path_is_read_past_what_login_files_print() {
        let home = tempfile::tempdir().expect("home");
        let shell = login_shell(
            home.path(),
            "echo 'Now using node v22.9.0 (npm v10.8.3)'; exec /bin/sh -c \"$2\"",
        );
        assert_eq!(
            login_path_in(&shell, Duration::from_secs(5)),
            std::env::var("PATH").ok()
        );
    }

    /// A login shell that fails, or takes too long, leaves the daemon on the
    /// PATH the app was started with; the app's start does not wait on it.
    #[cfg(unix)]
    #[test]
    fn a_login_shell_that_fails_or_hangs_gives_no_path() {
        let home = tempfile::tempdir().expect("home");
        let failing = login_shell(home.path(), "/bin/sh -c \"$2\"; exit 3");
        assert_eq!(login_path_in(&failing, Duration::from_secs(5)), None);

        let hanging = login_shell(home.path(), "exec /bin/sleep 3");
        let started = Instant::now();
        assert_eq!(login_path_in(&hanging, Duration::from_millis(300)), None);
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "the shell held up the start"
        );
    }

    /// Something a login file starts may keep the shell's output open after
    /// the shell has gone; what the shell printed is read all the same.
    #[cfg(unix)]
    #[test]
    fn a_login_path_is_read_while_a_started_process_holds_the_output() {
        let home = tempfile::tempdir().expect("home");
        let shell = login_shell(home.path(), "/bin/sleep 3 & exec /bin/sh -c \"$2\"");
        let started = Instant::now();
        assert_eq!(
            login_path_in(&shell, Duration::from_secs(5)),
            std::env::var("PATH").ok()
        );
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "the read waited for the process"
        );
    }

    #[test]
    fn a_verbatim_windows_path_is_spelled_plainly_for_node() {
        assert_eq!(
            plain_path(PathBuf::from(r"\\?\C:\Users\me\app\cli\bin\cf.mjs")),
            PathBuf::from(r"C:\Users\me\app\cli\bin\cf.mjs")
        );
        assert_eq!(
            plain_path(PathBuf::from(r"\\?\UNC\server\share\cf.mjs")),
            PathBuf::from(r"\\server\share\cf.mjs")
        );
        assert_eq!(
            plain_path(PathBuf::from("/Applications/ConsensFlow.app/node")),
            PathBuf::from("/Applications/ConsensFlow.app/node")
        );
    }
}

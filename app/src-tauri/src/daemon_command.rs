//! What the app starts as its daemon: the bundled `cf` running `cf ui --json
//! --no-open`, on the PATH the human's login shell sets up. The portable
//! Windows app carries the `cf` inside its exe and unpacks it first.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager};

use crate::daemon::DaemonFailure;

/// How long the human's login shell has to say its PATH.
const LOGIN_PATH_TIMEOUT: Duration = Duration::from_secs(5);

/// The bundle's `cf` as its folder names it.
const CF: &str = if cfg!(windows) { "cf.exe" } else { "cf" };

/// The daemon, `cf ui --json --no-open` of the bundled `cf`, on the human's
/// login PATH. A `cf` missing from the app is not worth another start. Which
/// `cf` it starts is one line of the app's error log.
pub(crate) fn daemon_command(app: &AppHandle) -> Result<Command, DaemonFailure> {
    let cf = bundled_cf(app).map_err(|cause| DaemonFailure {
        cause,
        retry: false,
    })?;
    eprintln!("consensflow: {}", starting(&cf));
    let mut command = command_for(&cf);
    if let Some(path) = login_path() {
        command.env("PATH", path);
    }
    // cf.exe is a console program: started from a windowed app it gets a
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

/// The daemon `cf` makes: `cf ui --json --no-open`, with nothing of the app's
/// own added to its environment.
fn command_for(cf: &Path) -> Command {
    let mut command = Command::new(cf);
    command.args(["ui", "--json", "--no-open"]);
    command
}

/// What the app's error log says of the daemon it starts: which `cf` it runs.
fn starting(cf: &Path) -> String {
    format!("starting the daemon: {} ui --json --no-open", cf.display())
}

/// The `cf` of the bundle whose resources, or portable runtime, are in `root`.
fn cf_in(root: &Path) -> PathBuf {
    root.join("cli").join("bin").join(CF)
}

/// The `cf` of this app's own bundle, when it is there.
pub(crate) fn bundled_cf(app: &AppHandle) -> Result<PathBuf, String> {
    #[cfg(windows)]
    if let Some(runtime) = portable_runtime(app)? {
        // The terminals' console host is in the runtime; without it Windows' own serves.
        if let Err(error) = crate::portable::find_libraries_in(&plain_path(runtime.clone())) {
            eprintln!("consensflow: the runtime's console host is not found ({error}); Windows' own serves");
        }
        return present(cf_in(&runtime));
    }
    let resources = app
        .path()
        .resource_dir()
        .map_err(|error| format!("the app could not find its own resources: {error}"))?;
    present(cf_in(&resources))
}

/// `cf`, when it is there.
fn present(cf: PathBuf) -> Result<PathBuf, String> {
    // Tauri may answer its folders in Windows' verbatim form (`\\?\C:\…`),
    // which cmd.exe starts nothing through, and which the terminal's command
    // would name. The plain spelling names the same file.
    let cf = plain_path(cf);
    if !cf.is_absolute() || !cf.exists() {
        return Err(format!(
            "the bundled ConsensFlow is missing from this app ({cf:?})"
        ));
    }
    Ok(cf)
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

    #[test]
    fn the_daemon_is_the_bundled_cf_running_ui_and_nothing_of_the_apps_is_added_to_it() {
        let cf = Path::new("/bundle/cli/bin/cf");
        assert_eq!(
            described(&command_for(cf)),
            (
                PathBuf::from(cf),
                vec!["ui".into(), "--json".into(), "--no-open".into()],
                vec![]
            )
        );
    }

    #[test]
    fn the_cf_is_the_one_in_the_clis_folder_of_the_bundle_or_the_portable_runtime() {
        let cf = if cfg!(windows) { "cf.exe" } else { "cf" };
        assert_eq!(
            cf_in(Path::new("/bundle")),
            Path::new("/bundle").join("cli").join("bin").join(cf)
        );
    }

    #[test]
    fn the_log_says_which_cf_the_daemon_is() {
        assert_eq!(
            starting(Path::new("/bundle/cli/bin/cf")),
            "starting the daemon: /bundle/cli/bin/cf ui --json --no-open"
        );
    }

    #[test]
    fn a_cf_that_is_there_is_found_and_one_that_is_not_says_the_app_is_missing_it() {
        let bundle = tempfile::tempdir().expect("a bundle");
        let cf = cf_in(bundle.path());
        let error = present(cf.clone()).expect_err("no cf is there yet");
        assert!(
            error.starts_with("the bundled ConsensFlow is missing from this app ("),
            "{error}"
        );
        std::fs::create_dir_all(cf.parent().expect("a folder")).expect("its folder");
        std::fs::write(&cf, "the program").expect("a cf");
        assert_eq!(present(cf.clone()), Ok(cf));
        // A relative name is no place the app was installed in.
        assert!(present(PathBuf::from("cf")).is_err());
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
    fn a_verbatim_windows_path_is_spelled_plainly() {
        assert_eq!(
            plain_path(PathBuf::from(r"\\?\C:\Users\me\app\cli\bin\cf.exe")),
            PathBuf::from(r"C:\Users\me\app\cli\bin\cf.exe")
        );
        assert_eq!(
            plain_path(PathBuf::from(r"\\?\UNC\server\share\cf.exe")),
            PathBuf::from(r"\\server\share\cf.exe")
        );
        assert_eq!(
            plain_path(PathBuf::from("/Applications/ConsensFlow.app/cf")),
            PathBuf::from("/Applications/ConsensFlow.app/cf")
        );
    }
}

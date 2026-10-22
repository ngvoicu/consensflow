//! What the app starts as its daemon: the bundled `cf` running `cf ui --json
//! --no-open`, on the PATH the human's login shell sets up, else the app's own.
//! The daemon puts the system's folders of programs after it for everything it
//! runs, and looks for a harness on it as given (`cf_base::env`). The portable
//! Windows app carries the `cf` inside its exe and unpacks it first.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, ExitStatus, Stdio};
use std::sync::{mpsc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use cf_base::env::{path_with_folders, Env};
use tauri::{AppHandle, Manager};

use crate::daemon::DaemonFailure;

/// How long the human's login shell has to say its PATH. One answers in well
/// under a second on a machine at rest; on the owner's, at load 17–18 from
/// builds, it had not in the five seconds it was given (2026-10-15), and the
/// daemon ran on the Finder's PATH. It is asked on the daemon's own thread
/// while the window opens, and only what the board shows waits for it: a
/// shell that never answers costs each start this long, and the daemon then
/// runs on the app's PATH and the system's folders.
const LOGIN_PATH_TIMEOUT: Duration = Duration::from_secs(30);

/// The bundle's `cf` as its folder names it.
const CF: &str = if cfg!(windows) { "cf.exe" } else { "cf" };

/// The daemon, `cf ui --json --no-open` of the bundled `cf`, on the human's
/// login PATH, which `lookup` asks for and the app's quit ends. A `cf` missing
/// from the app is not worth another start. Which `cf` it starts is one line of
/// the app's error log, and a login shell that gave no PATH another.
pub(crate) fn daemon_command(
    app: &AppHandle,
    lookup: &LoginLookup,
) -> Result<Command, DaemonFailure> {
    let cf = bundled_cf(app).map_err(|cause| DaemonFailure {
        cause,
        retry: false,
    })?;
    eprintln!("consensflow: {}", starting(&cf));
    let mut command = command_for(&cf);
    let env = Env::from_process();
    if let Some(said) = on_login_path(&mut command, &env, LOGIN_PATH_TIMEOUT, lookup) {
        eprintln!("consensflow: {said}");
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
    // The daemon runs as long as the app does, and the app opens sockets and pipes on its
    // other threads as it starts it: what it was left of those it would hold to the end.
    cf_process::hold_only_the_streams(&mut command);
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

/// Gives the daemon's `command` the PATH of the daemon and every pane it opens:
/// the one the human's login shell (`SHELL` of `env`) sets up, where their
/// harness CLIs live, else the app's own, as it is. The daemon puts the
/// system's folders after it itself, and looks for a harness on it as given:
/// one the human keeps in the home comes before an older one of Homebrew's.
/// What the app's log is to say when a login shell was asked and gave none
/// within `timeout`: the PATH the daemon then runs on, with those folders.
///
/// On Windows no shell is asked, though `SHELL` names one (Git Bash's): an app
/// started from the Start menu has the PATH of the user and the machine, which
/// a terminal there has too, and a POSIX shell's (`/c/…`, joined by `:`) is
/// none a Windows program can use.
fn on_login_path(
    command: &mut Command,
    env: &Env,
    timeout: Duration,
    lookup: &LoginLookup,
) -> Option<String> {
    let shell = env.path("SHELL").filter(|_| !env.on_windows());
    let login = shell.and_then(|shell| login_path_in(shell, timeout, lookup));
    let given = login
        .as_deref()
        .map(OsStr::new)
        .or_else(|| env.os("PATH"))
        .unwrap_or_default();
    if !given.is_empty() {
        command.env("PATH", given);
    }
    let runs_on = path_with_folders(given, env.tool_folders());
    shell
        .filter(|_| login.is_none())
        .map(|shell| unanswered(shell, timeout, &runs_on))
}

/// What the app's error log says of a login shell that gave no PATH: how long
/// it was given, and the PATH the daemon runs on instead, which the next
/// report then shows.
fn unanswered(shell: &Path, timeout: Duration, path: &OsStr) -> String {
    format!(
        "the login shell {} gave no PATH within {} s, and the daemon runs on {}",
        shell.display(),
        timeout.as_secs(),
        path.to_string_lossy()
    )
}

/// The login shell a start of the daemon is asking for its PATH, held while it
/// asks, so that the app's quit ends it: a shell that never answered outlived
/// the app (the reviews of 2026-10-16). Only the shell, the lookup's own child,
/// is ever ended: what its login files started (an `ssh-agent -D`, a
/// `gpg-agent --no-detach` other programs use) is the user's, as when a
/// terminal's shell ends, and so is anything a login file put in a session of
/// its own to go on with.
#[derive(Default)]
pub(crate) struct LoginLookup {
    asking: Mutex<Asking>,
}

/// The shell asked, and whether the app is quitting.
#[derive(Default)]
struct Asking {
    shell: Option<Child>,
    ended: bool,
}

impl LoginLookup {
    fn lock(&self) -> MutexGuard<'_, Asking> {
        self.asking
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }

    /// Starts `shell` and holds it, in one step against the app's quit: its
    /// output, to read; none once the app is quitting, when no shell is
    /// started, or where it cannot be.
    fn start(&self, shell: &mut Command) -> Option<ChildStdout> {
        let mut asking = self.lock();
        if asking.ended {
            return None;
        }
        let mut started = shell.spawn().ok()?;
        let Some(output) = started.stdout.take() else {
            let _ = started.kill();
            let _ = started.wait();
            return None;
        };
        asking.shell = Some(started);
        Some(output)
    }

    /// How the shell held ended: none while it runs.
    fn status(&self) -> std::io::Result<Option<ExitStatus>> {
        match self.lock().shell.as_mut() {
            Some(shell) => shell.try_wait(),
            None => Err(std::io::ErrorKind::NotFound.into()),
        }
    }

    /// The lookup is over: the shell is ended if it still runs, under the lock,
    /// and only then let go and waited for. A shell let go first was one a quit
    /// could find nowhere, and the app could end before anything ended it (the
    /// third review of 2026-10-16).
    fn finish(&self) {
        self.finish_with(|| {});
    }

    /// [`LoginLookup::finish`], with `between` run once the lock is let go and
    /// before the shell is waited for: where the app's quit may come, and the
    /// app end.
    fn finish_with(&self, between: impl FnOnce()) {
        let shell = {
            let mut asking = self.lock();
            if let Some(shell) = asking.shell.as_mut() {
                let _ = shell.kill();
            }
            asking.shell.take()
        };
        between();
        if let Some(mut shell) = shell {
            let _ = shell.wait();
        }
    }

    /// The app is quitting: the shell asked ends now, and no other is started.
    pub(crate) fn end(&self) {
        let mut asking = self.lock();
        asking.ended = true;
        if let Some(shell) = asking.shell.as_mut() {
            let _ = shell.kill();
        }
    }
}

/// A login file may print (`nvm use` does), wait for input or never finish,
/// and the PATH used to be the shell's whole output, read on the main thread
/// before the window existed, for as long as the shell took. So the PATH is
/// read between markers no login file prints, from a shell that exits cleanly,
/// all within `timeout` and before the app quits (`lookup`); otherwise there
/// is none, the daemon keeps the PATH the app was started with, and the shell
/// is ended if it still runs. Only the shell: what its login files started is
/// the user's ([`LoginLookup`]).
pub(crate) fn login_path_in(
    shell: &Path,
    timeout: Duration,
    lookup: &LoginLookup,
) -> Option<String> {
    use std::hash::BuildHasher;
    use std::io::Read;

    let marker = format!(
        "<consensflow-path-{:016x}>",
        std::hash::RandomState::new().hash_one(std::process::id())
    );
    let mut asked = Command::new(shell);
    // A login file may start a program that runs on after the shell (an agent of ssh's, of
    // gpg's), and the app opens sockets and pipes on its other threads as it asks.
    cf_process::hold_only_the_streams(&mut asked);
    asked
        .args(["-lc", &format!("printf '{marker}%s{marker}' \"$PATH\"")])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut output = lookup.start(&mut asked)?;
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
    let status = loop {
        match lookup.status() {
            Ok(Some(status)) => break Some(status),
            // The app's quit ends the shell, which then has ended.
            Ok(None) if Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(10));
            }
            _ => break None,
        }
    };
    let path = status
        .filter(ExitStatus::success)
        .and_then(|_| between(&marker, &printed, deadline));
    lookup.finish();
    path
}

/// What the shell printed between the markers, once they are there: none
/// after a second with nothing more, or by the deadline. The shell has gone,
/// so what it printed is in the pipe a moment from the reader at most; a
/// process it started may hold the output open and write on for ever, and is
/// read no longer than the deadline lets.
fn between(marker: &str, printed: &mpsc::Receiver<Vec<u8>>, deadline: Instant) -> Option<String> {
    let mut bytes = Vec::new();
    loop {
        let text = String::from_utf8_lossy(&bytes);
        let mut parts = text.split(marker);
        if let (Some(_), Some(path), Some(_)) = (parts.next(), parts.next(), parts.next()) {
            return (!path.is_empty()).then(|| path.to_string());
        }
        let left = deadline.checked_duration_since(Instant::now())?;
        bytes.extend(
            printed
                .recv_timeout(left.min(Duration::from_secs(1)))
                .ok()?,
        );
    }
}

#[cfg(test)]
mod tests;

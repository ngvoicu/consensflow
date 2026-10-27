//! What the app starts as its daemon: the bundled `cf` running `cf ui --json
//! --no-open`, in the environment the human's interactive login shell sets up
//! (a terminal's: its PATH, LANG and what the human's rc files export), else on
//! that shell's login PATH, else on the app's own.
//! The daemon puts the system's folders of programs after it for everything it
//! runs, and looks for a harness on it as given (`cf_base::env`). The portable
//! Windows app carries the `cf` inside its exe and unpacks it first.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, ExitStatus, Stdio};
use std::sync::{mpsc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use cf_base::env::{path_with_folders, Env, TERMINAL_ENV};
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

/// What starts a console program with no console window of its own (Windows'
/// `CREATE_NO_WINDOW`): the daemon, and the PowerShell asked for its environment.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

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
    if let Some(said) = on_login_environment(&mut command, &env, LOGIN_PATH_TIMEOUT, lookup) {
        eprintln!("consensflow: {said}");
    }
    // cf.exe is a console program: started from a windowed app it gets a
    // console window of its own, and every console program it starts shows in
    // it. The daemon runs without one; its windows are the app's panes.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
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

/// Gives the daemon's `command` the environment of the daemon and every pane it
/// opens: the one the human's interactive login shell (`SHELL` of `env`) sets up,
/// as a terminal's shell does, so a member window has what the same harness has
/// when the human opens it in a terminal (T-540: a Finder-started app had the
/// login PATH alone, and its windows lacked LANG, the `.zshrc` PATH and what the
/// human exports there). Where that shell gives none, the PATH its login files set
/// up ([`login_path_in`]), where their harness CLIs live; else the app's own, as
/// it is. The daemon puts the system's folders after the PATH itself, and looks
/// for a harness on it as given: one the human keeps in the home comes before an
/// older one of Homebrew's. What the app's log is to say when a login shell was
/// asked and gave no PATH within `timeout`: the PATH the daemon then runs on,
/// with those folders. Nothing of the environment's values is said: a variable
/// the human exports may be a key.
///
/// On Windows no `SHELL` is asked, though it names one (Git Bash's): a POSIX shell's
/// PATH (`/c/…`, joined by `:`) is none a Windows program can use. An app started from
/// the Start menu has the environment of the user and the machine, which a terminal
/// there has too, and a terminal's PowerShell adds what the user's profile sets: that
/// PowerShell is asked ([`profile_environment_in`]), and where it gives nothing the
/// daemon keeps the app's environment.
fn on_login_environment(
    command: &mut Command,
    env: &Env,
    timeout: Duration,
    lookup: &LoginLookup,
) -> Option<String> {
    let shell = env.path("SHELL").filter(|_| !env.on_windows());
    // One limit for both questions, as `LOGIN_PATH_TIMEOUT` says, asked at once: the login
    // files' PATH, which is the fallback, and the interactive shell's whole environment,
    // each given the whole limit, so that slow login files, which both run, do not leave
    // the second question less of it.
    let (login_path, taken) = thread::scope(|both| {
        let path = both.spawn(|| shell.and_then(|shell| login_path_in(shell, timeout, lookup)));
        let taken = match shell {
            Some(shell) => login_environment_in(shell, timeout, lookup),
            None if cfg!(windows) => profile_environment_in(&powershell_of(env), timeout, lookup),
            None => None,
        };
        (path.join().unwrap_or_default(), taken)
    });
    let login = taken
        .iter()
        .flatten()
        .find(|(name, _)| name.eq_ignore_ascii_case("PATH"))
        .and_then(|(_, value)| value.to_str())
        .map(str::to_owned)
        .or(login_path);
    let mut names = Vec::new();
    for (name, value) in taken.iter().flatten() {
        if kept_from_the_app(env, name) {
            continue;
        }
        command.env(name, value);
        if !name.eq_ignore_ascii_case("PATH") {
            names.push(name.to_string_lossy().into_owned());
        }
    }
    // Names only: the daemon has the values, and gives them to every window's pane,
    // which the app's own terminal starts from the app's environment.
    if !names.is_empty() {
        command.env(
            TERMINAL_ENV,
            serde_json::to_string(&names).unwrap_or_default(),
        );
    }
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

/// The harnesses' homes, which a run that starts the app with its own (a test's, a live
/// run's throwaway config) keeps over what the rc says; where the app has none, the
/// rc's is taken, as a terminal has it.
const HOMES: [&str; 2] = ["CLAUDE_CONFIG_DIR", "CODEX_HOME"];

/// Whether `name` is a harness's home the app was started with, which the rc does not
/// replace.
fn kept_from_the_app(env: &Env, name: &OsStr) -> bool {
    let name = name.to_string_lossy();
    HOMES.contains(&&*name) && env.os(&name).is_some_and(|value| !value.is_empty())
}

/// The variables of a terminal's environment the daemon is not given: where the shell
/// was and how deep, the last command it ran, and the terminal it was asked in, which is
/// no pane's (a pane says its own, `cf-panes`' `advertise_terminal`); ConsensFlow's own
/// (`CONSENSFLOW_*`, `CF_*`), which the app and the daemon set for themselves, so that an
/// rc's `CONSENSFLOW_HOME` never replaces the Candidate's own home; the identity of a
/// Claude Code session the app removes from itself ([`crate::CLAUDE_SESSION_IDENTITY`]);
/// and the dynamic loader's (`DYLD_*`, `LD_*`), on purpose: a library an rc injects into
/// its terminal's programs is not given to every agent ConsensFlow starts.
fn not_taken(name: &OsStr) -> bool {
    let name = name.to_string_lossy();
    matches!(&*name, "PWD" | "OLDPWD" | "SHLVL" | "_")
        || ["TERM", "CONSENSFLOW_", "CF_", "DYLD_", "LD_"]
            .iter()
            .any(|prefix| name.starts_with(prefix))
        || crate::CLAUDE_SESSION_IDENTITY.contains(&&*name)
}

/// The environment of the human's interactive login shell, as a terminal's shell
/// has it once its login and rc files have run (`-ilc`), each variable as the
/// shell holds it but those [`not_taken`]. It is read as `env -0` prints it between
/// markers, as [`login_path_in`] reads the PATH, with the same bounds: none from a
/// shell that fails, hangs, or prints no PATH.
pub(crate) fn login_environment_in(
    shell: &Path,
    timeout: Duration,
    lookup: &LoginLookup,
) -> Option<Vec<(OsString, OsString)>> {
    let marker = marker("env");
    let printed = asked(
        shell,
        &[
            "-ilc",
            &format!("printf '{marker}'; /usr/bin/env -0; printf '{marker}'"),
        ],
        &marker,
        timeout,
        lookup,
    )?;
    taken(&printed)
}

/// On Windows, the environment a terminal's PowerShell gives what it starts once the
/// user's profile has run: asked of `powershell` ([`powershell_of`]) with the profile
/// loaded (`-Command` loads it, as a terminal's PowerShell does), each `NAME=value` with
/// a NUL after it, in UTF-8, between markers, with the same bounds as a login shell's.
pub(crate) fn profile_environment_in(
    powershell: &Path,
    timeout: Duration,
    lookup: &LoginLookup,
) -> Option<Vec<(OsString, OsString)>> {
    let marker = marker("env");
    let script = format!(
        "[Console]::OutputEncoding = [Text.Encoding]::UTF8; [Console]::Out.Write('{marker}'); \
         Get-ChildItem env: | ForEach-Object {{ [Console]::Out.Write($_.Name + '=' + $_.Value + [char]0) }}; \
         [Console]::Out.Write('{marker}')"
    );
    let printed = asked(
        powershell,
        &["-NoLogo", "-NonInteractive", "-Command", &script],
        &marker,
        timeout,
        lookup,
    )?;
    taken(&printed)
}

/// The PowerShell a terminal opens on Windows: PowerShell 7 (`pwsh.exe`) where it is
/// in a folder of the PATH, as Windows Terminal opens it once it is installed, else
/// Windows PowerShell, which every Windows has.
fn powershell_of(env: &Env) -> PathBuf {
    env.os("PATH")
        .into_iter()
        .flat_map(|path| std::env::split_paths(path).collect::<Vec<_>>())
        .map(|folder| folder.join("pwsh.exe"))
        .find(|pwsh| pwsh.is_file())
        .unwrap_or_else(|| {
            let root = env.os("SystemRoot").unwrap_or(OsStr::new(r"C:\Windows"));
            Path::new(root).join(r"System32\WindowsPowerShell\v1.0\powershell.exe")
        })
}

/// The variables of what a shell printed (`NAME=value`, a NUL after each), but those
/// [`not_taken`]; none where it names no PATH (Windows names it `Path`).
fn taken(printed: &[u8]) -> Option<Vec<(OsString, OsString)>> {
    let vars: Vec<(OsString, OsString)> = printed
        .split(|&byte| byte == 0)
        .filter_map(variable)
        .filter(|(name, _)| !not_taken(name))
        .collect();
    vars.iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("PATH"))
        .then_some(vars)
}

/// A `NAME=value` of `env -0`'s, as the variable it is: none for an entry with no
/// name.
fn variable(entry: &[u8]) -> Option<(OsString, OsString)> {
    let equals = entry.iter().position(|&byte| byte == b'=')?;
    (equals > 0).then(|| (os(&entry[..equals]), os(&entry[equals + 1..])))
}

/// `bytes` as the platform's text of a variable.
#[cfg(unix)]
fn os(bytes: &[u8]) -> OsString {
    use std::os::unix::ffi::OsStringExt;
    OsString::from_vec(bytes.to_vec())
}

/// `bytes` as the platform's text of a variable: no shell is asked on Windows.
#[cfg(not(unix))]
fn os(bytes: &[u8]) -> OsString {
    OsString::from(String::from_utf8_lossy(bytes).into_owned())
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

/// The shells asked, each by the number its start was given, and whether the app is
/// quitting.
#[derive(Default)]
struct Asking {
    shells: Vec<(u64, Child)>,
    next: u64,
    ended: bool,
}

/// A shell a lookup started: the number it is held by, and its output, to read.
pub(crate) struct Started {
    pub(crate) asked: u64,
    pub(crate) output: ChildStdout,
}

impl LoginLookup {
    fn lock(&self) -> MutexGuard<'_, Asking> {
        self.asking
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }

    /// Starts `shell` and holds it beside any other asked, in one step against the
    /// app's quit; none once the app is quitting, when no shell is started, or where it
    /// cannot be.
    fn start(&self, shell: &mut Command) -> Option<Started> {
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
        let asked = asking.next;
        asking.next += 1;
        asking.shells.push((asked, started));
        Some(Started { asked, output })
    }

    /// How the shell held as `asked` ended: none while it runs.
    fn status(&self, asked: u64) -> std::io::Result<Option<ExitStatus>> {
        match self
            .lock()
            .shells
            .iter_mut()
            .find(|(held, _)| *held == asked)
        {
            Some((_, shell)) => shell.try_wait(),
            None => Err(std::io::ErrorKind::NotFound.into()),
        }
    }

    /// The lookup is over: the shell is ended if it still runs, under the lock,
    /// and only then let go and waited for. A shell let go first was one a quit
    /// could find nowhere, and the app could end before anything ended it (the
    /// third review of 2026-10-16).
    fn finish(&self, asked: u64) {
        self.finish_with(asked, || {});
    }

    /// [`LoginLookup::finish`], with `between` run once the lock is let go and
    /// before the shell is waited for: where the app's quit may come, and the
    /// app end.
    fn finish_with(&self, asked: u64, between: impl FnOnce()) {
        let shell = {
            let mut asking = self.lock();
            let at = asking.shells.iter().position(|(held, _)| *held == asked);
            at.map(|at| asking.shells.remove(at).1).map(|mut shell| {
                let _ = shell.kill();
                shell
            })
        };
        between();
        if let Some(mut shell) = shell {
            let _ = shell.wait();
        }
    }

    /// The app is quitting: every shell asked ends now, and no other is started.
    pub(crate) fn end(&self) {
        let mut asking = self.lock();
        asking.ended = true;
        for (_, shell) in &mut asking.shells {
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
    let marker = marker("path");
    let printed = asked(
        shell,
        &["-lc", &format!("printf '{marker}%s{marker}' \"$PATH\"")],
        &marker,
        timeout,
        lookup,
    )?;
    (!printed.is_empty()).then(|| String::from_utf8_lossy(&printed).into_owned())
}

/// A marker no login file prints, for what is read of `what`.
fn marker(what: &str) -> String {
    use std::hash::BuildHasher;

    format!(
        "<consensflow-{what}-{:016x}>",
        std::hash::RandomState::new().hash_one(std::process::id())
    )
}

/// What `shell` started with `args` printed between two `marker`s,
/// from a shell that exits cleanly, all within `timeout` and before the app quits
/// (`lookup`); none otherwise, and the shell is ended if it still runs. Only the
/// shell: what its login files started is the user's ([`LoginLookup`]).
fn asked(
    shell: &Path,
    args: &[&str],
    marker: &str,
    timeout: Duration,
    lookup: &LoginLookup,
) -> Option<Vec<u8>> {
    use std::io::Read;

    let mut asked = Command::new(shell);
    // A login file may start a program that runs on after the shell (an agent of ssh's, of
    // gpg's), and the app opens sockets and pipes on its other threads as it asks.
    cf_process::hold_only_the_streams(&mut asked);
    asked
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    // PowerShell is a console program: started from a windowed app, it would show a
    // console of its own for as long as the profile takes, as the daemon would.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        asked.creation_flags(CREATE_NO_WINDOW);
    }
    let Started { asked, mut output } = lookup.start(&mut asked)?;
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
        match lookup.status(asked) {
            Ok(Some(status)) => break Some(status),
            // The app's quit ends the shell, which then has ended.
            Ok(None) if Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(10));
            }
            _ => break None,
        }
    };
    let said = status
        .filter(ExitStatus::success)
        .and_then(|_| between(marker.as_bytes(), &printed, deadline));
    lookup.finish(asked);
    said
}

/// What the shell printed between the markers, once they are there: none
/// after a second with nothing more, or by the deadline. The shell has gone,
/// so what it printed is in the pipe a moment from the reader at most; a
/// process it started may hold the output open and write on for ever, and is
/// read no longer than the deadline lets.
fn between(marker: &[u8], printed: &mpsc::Receiver<Vec<u8>>, deadline: Instant) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    loop {
        if let Some(first) = find(&bytes, marker) {
            let after = first + marker.len();
            if let Some(second) = find(&bytes[after..], marker) {
                return Some(bytes[after..after + second].to_vec());
            }
        }
        let left = deadline.checked_duration_since(Instant::now())?;
        bytes.extend(
            printed
                .recv_timeout(left.min(Duration::from_secs(1)))
                .ok()?,
        );
    }
}

/// Where `needle` first is in `bytes`.
fn find(bytes: &[u8], needle: &[u8]) -> Option<usize> {
    bytes
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(test)]
mod tests;

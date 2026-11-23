use std::path::{Path, PathBuf};
use tauri::{AppHandle, Runtime};

/// Starts the app an update installed, by `restart` (Tauri's, which never returns), once every
/// descriptor of this app's is close-on-exec, which a descriptor left inheritable would otherwise
/// be the new app's for as long as it runs. Called off the main thread, as the install is,
/// Tauri's restart only asks the event loop to end, and the loop's end starts the new app
/// ([`relaunch_installed`]) after the app's last code, which marks them all again (`at_exit`,
/// lib.rs). This one is for the ways Tauri starts the new app itself, without that: a restart
/// called on the main thread, one whose request to end the loop failed, and one the relaunch
/// could not start. The app is ending, and its own programs are started holding only their
/// streams, so nothing here needs a descriptor passed on.
#[cfg(target_os = "macos")]
pub(super) fn restart_swept<T>(restart: impl FnOnce() -> T) -> T {
    if let Err(failed) = cf_process::close_all_on_exec() {
        eprintln!(
            "ConsensFlow restarts holding descriptors it could not mark close-on-exec: {failed}"
        );
    }
    restart()
}

/// The app an update installed at this one's path, once the install has swapped it in: the
/// event loop's end starts it ([`relaunch_installed`]).
#[cfg(target_os = "macos")]
pub(super) static INSTALLED: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// Starts the app an update installed and ends this process. Called at the event loop's end
/// (lib.rs), after the app's last code and before Tauri's restart.
///
/// In the app it is started through LaunchServices (`open -n` on the installed bundle's path,
/// from a helper that waits for this process to end): a fresh app process of launchd's, as a
/// Finder open starts one, and not this app's child, which is what a direct start makes it (T-2105:
/// a Mac's Local Network decision may not reach an app so started after an update).
///
/// In a packaged self-test it is started directly, as Tauri 2.11 started it: the app a smoke
/// runs reads the smoke's FIFO and writes its reports on its output, and the smoke ends the app's
/// process group; one started by LaunchServices (its input and output `/dev/null`, a session of
/// its own) would hear nothing, say nothing, and outlive the smoke (T-930). The direct start has
/// this app's environment, streams, process group and arguments, holding nothing of this app's
/// but its streams. Returns where no update was installed, or where the new app could not be
/// started: Tauri's restart then starts it as it does.
#[cfg(target_os = "macos")]
pub(crate) fn relaunch_installed<R: Runtime>(app: &AppHandle<R>) {
    relaunch_once(
        INSTALLED.get().map(PathBuf::as_path),
        || app.cleanup_before_exit(),
        |installed| {
            start_installed(
                installed,
                crate::selftest::enabled(),
                |installed| relaunch(installed, std::env::args_os().skip(1)).map(drop),
                |installed| launch_services(installed).map(drop),
            )
        },
        || std::process::exit(0),
    );
}

/// How the installed app is started: through LaunchServices (`through_launch_services`) in the
/// app, and directly (`direct`) only in a packaged self-test, whose smoke hands the app its
/// streams and ends its process group. A direct start makes the new app this one's child, and
/// the Local Network grant of a Mac may then not reach it after an update (T-2105); a start of
/// LaunchServices' is the one a Finder open is.
#[cfg(target_os = "macos")]
pub(super) fn start_installed(
    installed: &Path,
    selftest: bool,
    direct: impl FnOnce(&Path) -> Result<(), String>,
    through_launch_services: impl FnOnce(&Path) -> Result<(), String>,
) -> Result<(), String> {
    if selftest {
        direct(installed)
    } else {
        through_launch_services(installed)
    }
}

/// The most the helper waits for the old app to end before it opens the new one anyway.
#[cfg(target_os = "macos")]
const WAIT_FOR_THE_OLD_APP: &str = "300";

/// Starts the installed app through LaunchServices once this app has ended: `open -n` on the
/// bundle's path, by a helper that outlives this app, with this app's arguments and its
/// `CONSENSFLOW_HOME` where it has one.
#[cfg(target_os = "macos")]
#[allow(clippy::disallowed_methods)] // Finds ConsensFlow's home (CONSENSFLOW_HOME) itself, as the app's setup does.
pub(super) fn launch_services(installed: &Path) -> Result<std::process::Child, String> {
    let home = std::env::var_os("CONSENSFLOW_HOME").map(PathBuf::from);
    launch_services_with(
        Path::new("/usr/bin/open"),
        std::process::id(),
        installed,
        std::env::args_os().skip(1),
        home.as_deref(),
    )
}

/// The helper: it waits (up to 30 seconds) for process `old` to end, then runs `opener -n` with
/// `app` (and the home and `args`), a fresh process of the bundle at that path. It holds only
/// the standard streams, nulled, and is in a process group of its own, so that the end of this
/// app's group does not end it.
#[cfg(target_os = "macos")]
#[allow(clippy::disallowed_methods)] // The helper is a shell of the system's, started as this app starts the installed one: holding only its streams, older than cf-process's rule.
pub(super) fn launch_services_with(
    opener: &Path,
    old: u32,
    app: &Path,
    args: impl IntoIterator<Item = std::ffi::OsString>,
    home: Option<&Path>,
) -> Result<std::process::Child, String> {
    use std::os::unix::process::CommandExt;
    let mut command = std::process::Command::new("/bin/sh");
    command
        .arg("-c")
        .arg(
            "waited=0; while kill -0 \"$1\" 2>/dev/null && [ \"$waited\" -lt \"$2\" ]; do \
             sleep 0.1; waited=$((waited + 1)); done; shift 2; exec \"$@\"",
        )
        .arg("sh")
        .arg(old.to_string())
        .arg(WAIT_FOR_THE_OLD_APP)
        .arg(opener)
        .arg("-n");
    if let Some(home) = home {
        let mut variable = std::ffi::OsString::from("CONSENSFLOW_HOME=");
        variable.push(home);
        command.arg("--env").arg(variable);
    }
    command.arg(app);
    let mut args = args.into_iter().peekable();
    if args.peek().is_some() {
        command.arg("--args").args(args);
    }
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .process_group(0);
    cf_process::hold_only_the_streams(&mut command);
    command.spawn().map_err(|e| e.to_string())
}
/// The installed app is started once, by `start`, or not at all here (T-941): Tauri's cleanup
/// runs first, a panic of it stopped and said, and a start that succeeds is followed by `exit`
/// and nothing else, so Tauri's restart, which runs where this returns, never starts it a second
/// time. Where nothing was installed nothing runs; where the start fails it is said, and this
/// returns.
#[cfg(target_os = "macos")]
pub(super) fn relaunch_once(
    installed: Option<&Path>,
    cleanup: impl FnOnce(),
    start: impl FnOnce(&Path) -> Result<(), String>,
    exit: impl FnOnce(),
) {
    let Some(installed) = installed else {
        return;
    };
    if std::panic::catch_unwind(std::panic::AssertUnwindSafe(cleanup)).is_err() {
        eprintln!("ConsensFlow's cleanup before starting the installed update panicked");
    }
    match start(installed) {
        Ok(()) => exit(),
        Err(error) => eprintln!(
            "ConsensFlow could not start the installed update itself ({error}); Tauri's restart starts it"
        ),
    }
}

/// Starts the executable of the bundle at `app` (its Info.plist names it) with `args`, this
/// process's environment and input, and nothing of its but the standard streams.
#[cfg(target_os = "macos")]
#[allow(clippy::disallowed_methods)] // The app starts the installed update itself, holding only its streams: older than cf-process's rule.
pub(super) fn relaunch(
    app: &Path,
    args: impl IntoIterator<Item = std::ffi::OsString>,
) -> Result<std::process::Child, String> {
    let tools = crate::update_install::Tools::system();
    let executable = crate::update_install::plist_value(&tools, app, "CFBundleExecutable")?;
    let mut parts = Path::new(&executable).components();
    if !matches!(
        (parts.next(), parts.next()),
        (Some(std::path::Component::Normal(_)), None)
    ) {
        return Err(format!(
            "the bundle names no executable of its own: {executable:?}"
        ));
    }
    let mut command = std::process::Command::new(app.join("Contents/MacOS").join(&executable));
    command.args(args);
    cf_process::hold_only_the_streams(&mut command);
    command.spawn().map_err(|e| e.to_string())
}

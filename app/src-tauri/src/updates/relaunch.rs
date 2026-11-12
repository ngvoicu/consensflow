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

/// Starts the app an update installed, as this one was started, and ends this process. Called
/// at the event loop's end (lib.rs), after the app's last code and before Tauri's restart, which
/// since Tauri 2.12.2 starts the new app through LaunchServices (`open -n`): launchd's child, in
/// a session of its own, its input and output `/dev/null`. A self-test's app reads the smoke's
/// FIFO and writes its reports on its output, and the smoke ends the app's process group: the
/// app an update restarted so would hear nothing, say nothing, and outlive the smoke (T-930).
/// This starts the bundle's executable itself, as Tauri 2.11 did: this app's environment,
/// streams, process group and arguments, holding nothing of this app's but its streams. Returns
/// where no update was installed, or where the new app could not be started: Tauri's restart
/// then starts it as it does.
#[cfg(target_os = "macos")]
pub(crate) fn relaunch_installed<R: Runtime>(app: &AppHandle<R>) {
    relaunch_once(
        INSTALLED.get().map(PathBuf::as_path),
        || app.cleanup_before_exit(),
        |installed| relaunch(installed, std::env::args_os().skip(1)).map(drop),
        || std::process::exit(0),
    );
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

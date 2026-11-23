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

/// What the helper that starts the installed app is given.
#[cfg(target_os = "macos")]
pub(super) struct Handoff<'a> {
    /// `/usr/bin/open`.
    pub opener: &'a Path,
    /// The old app's process: the helper starts nothing while it runs.
    pub old: u32,
    /// The installed bundle.
    pub app: &'a Path,
    pub args: Vec<std::ffi::OsString>,
    pub home: Option<&'a Path>,
    /// The bundle's executable: started directly where the opener fails.
    pub executable: &'a Path,
    /// The system's `logger`, where the log cannot be written.
    pub logger: &'a Path,
    /// Where the helper says what it did and why: the app's error log.
    pub log: Option<&'a Path>,
}

/// Starts the installed app through LaunchServices once this app has ended: `open -n` on the
/// bundle's path, by a helper that outlives this app, with this app's arguments and its
/// `CONSENSFLOW_HOME` where it has one.
#[cfg(target_os = "macos")]
#[allow(clippy::disallowed_methods)] // Finds ConsensFlow's home (CONSENSFLOW_HOME) itself, as the app's setup does.
pub(super) fn launch_services(installed: &Path) -> Result<std::process::Child, String> {
    let home = std::env::var_os("CONSENSFLOW_HOME").map(PathBuf::from);
    let executable = bundle_executable(installed)?;
    let log = crate::error_log();
    launch_services_with(Handoff {
        opener: Path::new("/usr/bin/open"),
        old: std::process::id(),
        app: installed,
        args: std::env::args_os().skip(1).collect(),
        home: home.as_deref(),
        executable: &executable,
        logger: Path::new("/usr/bin/logger"),
        log: log.as_deref(),
    })
}

/// The helper.
#[cfg(target_os = "macos")]
#[allow(clippy::disallowed_methods)] // The helper is a shell of the system's, started as this app starts the installed one: holding only its streams, older than cf-process's rule.
pub(super) fn launch_services_with(handoff: Handoff) -> Result<std::process::Child, String> {
    use std::os::unix::process::CommandExt;
    // No `-n`: where a person reopened the app while the helper waited, `open` brings that one
    // up and starts no second (T-2118).
    let mut prefix: Vec<std::ffi::OsString> = Vec::new();
    if let Some(home) = handoff.home {
        let mut variable = std::ffi::OsString::from("CONSENSFLOW_HOME=");
        variable.push(home);
        prefix.extend(["--env".into(), variable]);
    }
    prefix.push(handoff.app.into());
    if !handoff.args.is_empty() {
        prefix.push("--args".into());
    }
    let mut command = std::process::Command::new("/bin/sh");
    command
        .arg("-c")
        .arg(HELPER)
        .arg("sh")
        .arg(handoff.old.to_string())
        .arg(handoff.executable)
        .arg(handoff.opener)
        .arg(handoff.logger)
        .arg(handoff.log.unwrap_or_else(|| Path::new("")))
        .arg(prefix.len().to_string())
        .args(&prefix)
        .args(&handoff.args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .process_group(0);
    cf_process::hold_only_the_streams(&mut command);
    command.spawn().map_err(|e| e.to_string())
}

/// The helper's script, given the old app's pid, the bundle's executable, the opener, `logger`,
/// the app's log, the number of the opener's arguments that are not the old app's own, and then
/// the opener's arguments (the home, the bundle, `--args` where there are any) and the old app's
/// arguments.
///
/// It waits for the old app to end, however long that takes, and nothing else decides: an app
/// that hangs is one the person force-quits, and the helper goes on. Then it opens the bundle
/// with no `-n`, so that where the person reopened the app meanwhile that one is brought up and
/// no second starts. A failed `open` is said and retried after 0.5, 1 and 2 seconds; the helper
/// reports the failed tries on a successful retry, or starts the app directly after all fail,
/// the way it was before LaunchServices. Where that fails too the old app is gone and no one
/// else will start one, so the helper says what failed and to open ConsensFlow from the Finder:
/// in the app's log, and where that cannot be written, in the system log (`logger`). `/bin/sleep`
/// is the system's by its path: the helper keeps the old app's environment, whose PATH may hold
/// no `sleep` (T-2114, T-2118).
#[cfg(target_os = "macos")]
const HELPER: &str = r#"old=$1; executable=$2; opener=$3; logger=$4; log=$5; keep=$6; shift 6
say() { printf '%s\n' "$1" >> "$log" 2>/dev/null || "$logger" -t ConsensFlow "$1"; }
while kill -0 "$old" 2>/dev/null; do /bin/sleep 0.1; done
"$opener" "$@" && exit 0
opened=$?
say "ConsensFlow relaunch: $opener failed with status $opened"
for delay in 0.5 1 2; do
    /bin/sleep "$delay"
    if "$opener" "$@"; then
        say "ConsensFlow relaunch: $opener succeeded after failed tries with statuses $opened"
        exit 0
    else
        status=$?
        say "ConsensFlow relaunch: $opener failed with status $status"
        opened="$opened, $status"
    fi
done
say "ConsensFlow relaunch: $opener failed with status $opened; starting the installed app directly"
shift "$keep"
"$executable" "$@" && exit 0
started=$?
say "ConsensFlow could not start after the update: $opener failed with status $opened and $executable failed with status $started. Open ConsensFlow from the Finder."
exit 1
"#;

/// The executable of the bundle at `app`: its Info.plist names it, and it is one file of
/// `Contents/MacOS`.
#[cfg(target_os = "macos")]
pub(super) fn bundle_executable(app: &Path) -> Result<PathBuf, String> {
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
    Ok(app.join("Contents/MacOS").join(&executable))
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
    let mut command = std::process::Command::new(bundle_executable(app)?);
    command.args(args);
    cf_process::hold_only_the_streams(&mut command);
    command.spawn().map_err(|e| e.to_string())
}

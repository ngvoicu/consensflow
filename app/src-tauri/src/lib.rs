//! ConsensFlow's desktop shell: the Tauri app that holds the page, its daemon and its panes.
//!
//! The crate answers to the workspace's lints (Cargo.toml: no `unwrap`, `expect` or print to
//! stdout or stderr outside tests). A diagnostic is said through [`diag::say`], the app's door to
//! its stderr, which is its error log; a module that still prints or unwraps allows that lint for
//! itself on its `mod` line below, with its reason. Where the app reads its environment or starts
//! a process itself, the item says so with its own `#[allow(clippy::disallowed_methods)]` and its
//! reason (clippy.toml); the lints that refuse an HTTPS client are allowed only by the one
//! constructor (update_page.rs) and never for the crate (tools/xtask/tests/tls_clients.rs, T-1253).

use serde_json::{json, Value};
use tauri::{AppHandle, Manager, RunEvent, Runtime, WebviewUrl, WebviewWindowBuilder};

pub mod commands;
mod daemon;
mod daemon_command;
mod diag;
mod file_drop;
#[allow(
    clippy::print_stderr,
    reason = "its repair failures are lines of the error log written straight to stderr, not through diag::say"
)]
mod launcher;
mod link;
#[cfg(windows)]
#[allow(
    clippy::print_stderr,
    reason = "its reload and give-up lines are lines of the error log written straight to stderr, not through diag::say"
)]
mod page_crash;
mod panics;
// Only the Windows app loads a failed page again; tested everywhere.
#[cfg_attr(not(windows), allow(dead_code))]
mod page_reloads;
// Only the Windows app reads a runtime from its own exe; tested everywhere.
#[cfg_attr(not(windows), allow(dead_code))]
mod portable;
#[allow(
    clippy::print_stderr,
    reason = "its page-event, bridge and quit failures are lines of the error log written straight to stderr, not through diag::say"
)]
pub mod runtime;
mod selftest;
// The build script's reading of the identity a build is made with, tested with the app's tests.
#[cfg(test)]
#[path = "../build/identity.rs"]
mod built_identity;
#[cfg(test)]
mod test_manifest;
#[cfg(test)]
mod testing;
mod tls;
#[cfg(target_os = "macos")]
#[allow(
    clippy::print_stderr,
    reason = "its install failure is a line of the error log written straight to stderr, not through diag::say"
)]
mod update_install;
mod update_page;
#[allow(
    clippy::print_stderr,
    clippy::unwrap_used,
    reason = "its check, download and install failures are lines of the error log written straight to stderr, not through diag::say; it parses the feed's URL built from a literal and serializes its own plain snapshot with unwrap"
)]
pub mod updates;

use runtime::AppRuntime;

/// The page's end of the self-test channel. Refuses unless the app itself was
/// started in self-test mode, so a published page cannot open it.
#[tauri::command]
fn selftest_report(event: String, data: Value) -> Value {
    if !selftest::enabled() {
        return json!({"ok":false,"error":"self-test reporting is not enabled"});
    }
    selftest::report(&event, &data);
    if event == "update-blocked"
        && selftest::config().is_some_and(|c| c.updater_expected_version.is_some())
        && !selftest::wait_for_update_probe()
    {
        return json!({"ok":false,"error":"updater probe did not continue"});
    }
    json!({"ok":true})
}

/// A command that panics, which the packaged smoke's page asks of an app built for it: a panic
/// in a command the page asks ends in an answer to the page, and the app runs on (T-1050). Only
/// in the tests and in an app built with the feature `test-support`, never in a release.
#[cfg(any(test, feature = "test-support"))]
#[tauri::command]
fn selftest_panic() -> Value {
    deliberate_panic()
}

/// The same of an asynchronous command, whose task panics.
#[cfg(any(test, feature = "test-support"))]
#[tauri::command]
async fn selftest_panic_async() -> Value {
    panics::settled("selftest_panic_async", async { deliberate_panic() }).await
}

#[cfg(any(test, feature = "test-support"))]
fn deliberate_panic() -> Value {
    panic!("{SELFTEST_PANIC}")
}

/// What `selftest_panic` panics with.
#[cfg(any(test, feature = "test-support"))]
const SELFTEST_PANIC: &str = "the deliberate panic of a command the page asked (T-1050)";

/// The identity this build was made with, kept in the app's own file in these
/// words, where a test reads it without starting the app
/// (app/scripts/built-identity.mjs): a Windows exe says it nowhere else, and one
/// of the release's identity started by a test keeps its runtime in the
/// release's folder of the machine's user, whatever the test sets `LOCALAPPDATA`
/// to (T-951). `run` reads it, so that no linker leaves it out.
pub static BUILT_IDENTITY: &str = concat!(
    "consensflow-built-identity:",
    env!("CONSENSFLOW_BUILT_IDENTITY"),
    ";"
);

/// The installed release's bundle identity: the only build that may keep the
/// live home, `~/.consensflow`.
pub const PRODUCTION_IDENTIFIER: &str = "dev.ngvoicu.consensflow";

/// The home a build that is NOT the installed release must use instead.
///
/// A candidate opened from Finder, or `tauri dev` of the release identity,
/// starts with no `CONSENSFLOW_HOME`, and every reader then falls back to
/// `~/.consensflow` — the live instance's state. The kernel lock refuses that
/// only while the live app runs; after a reboot the newer build would open
/// and migrate the live state. So the build decides: the release keeps the
/// default, everything else gets `~/.consensflow-candidate`, but a smoke's build, which
/// starts on no home but the one its smoke names. An explicit, non-empty
/// `CONSENSFLOW_HOME` always wins — tests and smokes set one.
fn isolated_home(
    identifier: &str,
    debug: bool,
    configured: Option<&std::ffi::OsStr>,
    home: Option<&std::path::Path>,
) -> Result<Option<std::path::PathBuf>, String> {
    if configured.is_some_and(|value| !value.is_empty()) {
        return Ok(None);
    }
    if identifier == PRODUCTION_IDENTIFIER && !debug {
        return Ok(None);
    }
    if identifier == cf_base::bundle::SMOKE {
        return Err(format!(
            "{} is a smoke's build, which starts only on the home its smoke \
             names (CONSENSFLOW_HOME): without one it would run on the Candidate's",
            cf_base::bundle::SMOKE
        ));
    }
    home.map(|home| Some(home.join(".consensflow-candidate")))
        .ok_or_else(|| "this build needs HOME to find ~/.consensflow-candidate".to_string())
}

/// The title of the app's window: the product's name, and where the build is of another
/// identity than the release's and its name does not say so already, what that identity is
/// (`dev.ngvoicu.consensflow.smoke` reads "ConsensFlow Smoke"), so that no window of a test's
/// build reads as the live app's.
fn window_title(name: &str, identifier: &str) -> String {
    if identifier == PRODUCTION_IDENTIFIER {
        return name.to_owned();
    }
    let said = identifier
        .strip_prefix(PRODUCTION_IDENTIFIER)
        .and_then(|rest| rest.strip_prefix('.'))
        .map_or_else(
            || identifier.to_owned(),
            |suffix| {
                let words: Vec<String> = suffix
                    .split(['-', '.', '_'])
                    .filter(|word| !word.is_empty())
                    .map(|word| {
                        let mut letters = word.chars();
                        letters.next().map_or_else(String::new, |first| {
                            first.to_uppercase().chain(letters).collect()
                        })
                    })
                    .collect();
                words.join(" ")
            },
        );
    if name.to_lowercase().contains(&said.to_lowercase()) {
        name.to_owned()
    } else {
        format!("{name} {said}")
    }
}

/// Which of the variables `names` the app was started with it does not keep: those of a
/// session that started it ([`cf_proto::agents::is_parent_session_variable`]). A window of a
/// board gives its member the board's (T-868): with its token set the bundled `cf` is that
/// board's command, and `cf ui`, the daemon, is none, so the daemon never started. A harness
/// marks its own session (`tauri dev`, a test or bench run, the app opened from an agent's
/// shell, T-971): Claude's `CLAUDE_CODE_CHILD_SESSION` switches off transcript saving, its
/// `CLAUDE_CODE_SESSION_ID` or Codex's `CODEX_THREAD_ID` impersonates the parent, and the
/// messaging socket and token reach into it. Every program the app starts would have them:
/// the terminal of the app's own, and the app an update restarts, which Tauri starts with a
/// bare `Command` that holds whatever the process does. Configuration such as
/// `CLAUDE_CONFIG_DIR`, `CLAUDE_CODE_USE_BEDROCK` or `CONSENSFLOW_HOME` stays.
fn inherited_variables<'a>(names: impl IntoIterator<Item = &'a str>) -> Vec<&'a str> {
    names
        .into_iter()
        .filter(|name| cf_proto::agents::is_parent_session_variable(name))
        .collect()
}

/// Takes the variables the app does not keep ([`inherited_variables`]) out of its own
/// environment, which every program it starts is given: the daemon, the login shell it asks
/// for the human's environment, the terminals' panes, the opener, and the app an update
/// restarts. Before any thread exists, as nothing may read the environment meanwhile.
#[allow(clippy::disallowed_methods)] // Reads the app's own environment to clean it, before cf_base::env::Env has one.
fn remove_inherited_variables() {
    let names: Vec<String> = std::env::vars_os()
        .filter_map(|(name, _)| name.into_string().ok())
        .collect();
    for name in inherited_variables(names.iter().map(String::as_str)) {
        std::env::remove_var(name);
    }
}

/// How big the error log grows before it is moved aside.
const ERROR_LOG_LIMIT: u64 = 10 * 1024 * 1024;

/// Where the app and its daemon write their error output: `<home>/app/app.log`
/// in ConsensFlow's home (`CONSENSFLOW_HOME`, or `.consensflow` in the user's
/// home, as the daemon finds it), with one previous file kept once it passes
/// its limit. A Finder-launched app's stderr is /dev/null and a windowed app
/// on Windows has none, so panics and daemon errors used to leave no trace.
#[allow(clippy::disallowed_methods)] // Finds ConsensFlow's home (CONSENSFLOW_HOME) itself, as the daemon does.
pub(crate) fn error_log() -> Option<std::path::PathBuf> {
    let home = std::env::var_os("CONSENSFLOW_HOME")
        .filter(|home| !home.is_empty())
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::home_dir().map(|home| home.join(".consensflow")))?;
    prepare_error_log(&home, ERROR_LOG_LIMIT).ok()
}

fn prepare_error_log(home: &std::path::Path, limit: u64) -> std::io::Result<std::path::PathBuf> {
    let directory = home.join("app");
    std::fs::create_dir_all(&directory)?;
    let log = directory.join("app.log");
    if std::fs::metadata(&log).is_ok_and(|meta| meta.len() > limit) {
        std::fs::rename(&log, directory.join("app.log.1"))?;
    }
    Ok(log)
}

/// Points this process's stderr at the error log: every message of the host's
/// own and its panics, and on Unix the daemon's, which inherits it. Best
/// effort: the app runs without it.
#[cfg(unix)]
fn redirect_stderr(log: &std::path::Path) {
    use std::os::fd::AsRawFd;
    if let Ok(file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log)
    {
        // SAFETY: both descriptors are valid; dup2 replaces fd 2 atomically.
        unsafe { libc::dup2(file.as_raw_fd(), 2) };
    }
}

#[cfg(windows)]
fn redirect_stderr(log: &std::path::Path) {
    use std::os::windows::io::IntoRawHandle;
    use windows_sys::Win32::System::Console::{SetStdHandle, STD_ERROR_HANDLE};
    if let Ok(file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log)
    {
        // SAFETY: the handle is an open file's, given up to stderr for the
        // life of the process; Rust's stderr looks it up at every write.
        unsafe { SetStdHandle(STD_ERROR_HANDLE, file.into_raw_handle()) };
    }
}

/// What the page may ask the app, each by its command, refused for every window but
/// the main one: what the app is built with, and what the tests of a command ask through.
pub(crate) fn page_commands<R: Runtime>(
) -> impl Fn(tauri::ipc::Invoke<R>) -> bool + Send + Sync + 'static {
    let handler: fn(tauri::ipc::Invoke<R>) -> bool = tauri::generate_handler![
        commands::daemon_request,
        commands::pane_page_prepare,
        commands::pane_page_activate,
        commands::pane_input_enqueue,
        commands::pane_reply_enqueue,
        commands::pane_input_skip,
        commands::pane_input_wait,
        commands::pane_input_forget,
        commands::pane_resize,
        commands::pane_ack,
        commands::agents_screen,
        commands::subscribe_output,
        file_drop::short_paths,
        file_drop::focus_after_drop,
        updates::update_status,
        updates::update_channel,
        updates::update_check,
        updates::update_download,
        updates::update_install,
        updates::update_open_page,
        updates::local_network::local_network_notice,
        updates::local_network::local_network_notice_open,
        link::open_link,
        selftest_report,
        #[cfg(any(test, feature = "test-support"))]
        selftest_panic,
        #[cfg(any(test, feature = "test-support"))]
        selftest_panic_async,
    ];
    // Every command run with its panic stopped at the app's edge (T-1050): a synchronous one
    // runs in the webview's own frame, which cannot unwind.
    move |invoke| {
        panics::answered(invoke, |invoke| {
            if !commands::window_command_allowed(
                invoke.message.webview_ref().label(),
                invoke.message.command(),
            ) {
                invoke
                    .resolver
                    .reject("This command is unavailable in this window");
                return true;
            }
            handler(invoke)
        })
    }
}

/// What the app's builder is given for its page, by `with_page`: Tauri's builder, or
/// in the tests one that keeps the handlers of its windows' events, which Tauri's
/// stand-in runtime never tells a window.
pub(crate) trait PageBuilder<R: Runtime>: Sized {
    fn manage<T: Send + Sync + 'static>(self, state: T) -> Self;

    fn on_window_event<F>(self, handler: F) -> Self
    where
        F: Fn(&tauri::Window<R>, &tauri::WindowEvent) + Send + Sync + 'static;

    fn invoke_handler<F>(self, handler: F) -> Self
    where
        F: Fn(tauri::ipc::Invoke<R>) -> bool + Send + Sync + 'static;
}

impl<R: Runtime> PageBuilder<R> for tauri::Builder<R> {
    fn manage<T: Send + Sync + 'static>(self, state: T) -> Self {
        tauri::Builder::manage(self, state)
    }

    fn on_window_event<F>(self, handler: F) -> Self
    where
        F: Fn(&tauri::Window<R>, &tauri::WindowEvent) + Send + Sync + 'static,
    {
        tauri::Builder::on_window_event(self, handler)
    }

    fn invoke_handler<F>(self, handler: F) -> Self
    where
        F: Fn(tauri::ipc::Invoke<R>) -> bool + Send + Sync + 'static,
    {
        tauri::Builder::invoke_handler(self, handler)
    }
}

/// The app's builder with what its page stands on: a file dropped on its window,
/// which the app hears itself, the page asking of it only what the app heard
/// (`file_drop`), and the commands the page may ask. The app is built with it, and
/// the tests of the drop on the stand-in runtime.
pub(crate) fn with_page<R: Runtime, B: PageBuilder<R>>(builder: B) -> B {
    builder
        .manage(file_drop::Dropped::default())
        .on_window_event(|window, event| {
            panics::contained("the app's handler of a window's event", || {
                file_drop::hear(window, event);
            });
        })
        .invoke_handler(page_commands())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    std::hint::black_box(BUILT_IDENTITY);
    // The process's one TLS provider, before the plugins, any client or any thread exists: reqwest
    // panics at the build of a client where the process has none (tls.rs, T-1247).
    tls::install_ring_as_default();
    let context = tauri::generate_context!();
    settle_home(&context.config().identifier);
    remove_inherited_variables();
    // Every panic from here on is written down, and on macOS AppKit's dispatch of an event
    // ends what unwinds out of it there, before tao's frame aborts the app (panics.rs). Once
    // the home is settled, so a candidate's panic is never written in the live home.
    panics::record_in(error_log);
    #[cfg(target_os = "macos")]
    panics::appkit::guard_event_dispatch();
    if let Some(log) = error_log() {
        redirect_stderr(&log);
    }
    let app = built(
        with_page(
            tauri::Builder::default()
                .plugin(tauri_plugin_dialog::init())
                .plugin(tauri_plugin_updater::Builder::new().build()),
        )
        .setup(setup)
        .build(context),
    );
    app.run(on_event);
}

/// Settles the home this build runs on, before any thread exists, so that the updater and the
/// daemon — which inherits this environment, and passes it to every pane — all see the one home.
/// A build that cannot have one is said, and ends the app with status 1.
#[allow(clippy::disallowed_methods)] // The app's start settles its home from the environment, before any thread.
fn settle_home(identifier: &str) {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"));
    match isolated_home(
        identifier,
        cfg!(debug_assertions),
        std::env::var_os("CONSENSFLOW_HOME").as_deref(),
        home.as_deref().map(std::path::Path::new),
    ) {
        Ok(Some(isolated)) => std::env::set_var("CONSENSFLOW_HOME", isolated),
        Ok(None) => {}
        Err(message) => {
            diag::say(format!("consensflow: {message}"));
            std::process::exit(1);
        }
    }
}

/// What the app does once Tauri has built it: its runtime and the daemon, the person told of a
/// panic that was contained, the update check, and the main window.
fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    tell_of_contained_panics(app.handle().clone());
    // Read before the runtime starts the daemon, which makes the ledger it looks for.
    let earlier_run = updates::earlier_run(app);
    app.manage(AppRuntime::start(app.handle()));
    // The terminal's command names this bundle's `cf`, whichever build wrote it.
    launcher::repair_in_background(app.handle());
    updates::setup(app, earlier_run)?;
    open_main_window(app)
}

/// A panic the app contained is told to the person in a dialog.
fn tell_of_contained_panics(handle: AppHandle) {
    panics::tell_with(move || {
        let shown = handle.clone();
        // Told on a thread of its own (panics::stopped), so this posts the dialog to the event
        // loop, which shows it after the dispatch that stopped the error has ended.
        let _ = handle.run_on_main_thread(move || {
            use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
            let told = panics::told(error_log().as_deref());
            shown
                .dialog()
                .message(told.clone())
                .title("ConsensFlow kept running")
                .kind(MessageDialogKind::Warning)
                .show(|_| {});
            // The packaged smoke reads that the person was told (T-1050).
            if selftest::enabled() {
                selftest::report("told", &json!({ "message": told }));
            }
        });
    });
}

/// The app's one window. The product name is its title, and the identity where it is not the
/// release's, so that no candidate's or test's window reads as the live app's.
fn open_main_window(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let title = window_title(&app.package_info().name, &app.config().identifier);
    // The page runs on while its window is hidden, minimised or behind a locked
    // screen: it acknowledges the windows' output, and the pane host stops reading a
    // window that is owed too much of it, its agent waiting on its terminal. WebKit's
    // own policy throttles a page not shown to a turn every second or so (T-454).
    // Only macOS has the setting; elsewhere it does nothing.
    let mut window = WebviewWindowBuilder::new(app, "main", WebviewUrl::default())
        .title(title)
        .on_page_load(|window, payload| {
            if payload.event() == tauri::webview::PageLoadEvent::Started {
                if let Some(runtime) = window.try_state::<AppRuntime>() {
                    if let Err(error) = runtime.inputs.end_page() {
                        diag::say(format!("could not fence the departing page: {error}"));
                    }
                }
            }
        })
        .inner_size(1280.0, 820.0)
        .min_inner_size(720.0, 520.0)
        .maximized(true)
        .background_throttling(tauri::utils::config::BackgroundThrottlingPolicy::Disabled);
    if let Some(config) = selftest::config() {
        window = window.initialization_script(selftest::initialization_script(&config));
        selftest::watch_for_quit(app.handle().clone());
    }
    let window = window.build()?;
    #[cfg(windows)]
    page_crash::load_again_when_it_fails(&window);
    #[cfg(not(windows))]
    let _ = window;
    Ok(())
}

/// The app's end: the runtime shut down, and an update's restart.
fn on_event(handle: &AppHandle, event: RunEvent) {
    panics::contained("the app's handler of its event loop", || {
        if matches!(event, RunEvent::Exit) {
            at_exit(|| {
                if let Some(runtime) = handle.try_state::<AppRuntime>() {
                    runtime.shutdown();
                }
            });
            // An update's restart: its app is started here, not by Tauri (updates.rs).
            #[cfg(target_os = "macos")]
            updates::relaunch_installed(handle);
        }
    });
}

/// The app a build made: an error a build ends in (a webview the system cannot give, a
/// configuration it cannot read) is said in the error log and ends the app with status 1.
fn built<R: Runtime>(built: tauri::Result<tauri::App<R>>) -> tauri::App<R> {
    built.unwrap_or_else(|error| {
        diag::say(format!("consensflow: the app could not be built: {error}"));
        std::process::exit(1)
    })
}

/// The app's last code, on the main thread, as its event loop ends: `shutdown`, then every
/// descriptor it holds made close-on-exec. After this Tauri ends the process, or the app an update
/// installed is started: `app.restart()` called off the main thread (the update's install is an
/// async command) only asks the loop to end, and the new app is started here, after this
/// (`updates::relaunch_installed`). A descriptor of this app's that is not close-on-exec then (a socket or a pipe another
/// thread made in std's two steps, a file a framework opened so) would be the new app's for as
/// long as it runs. A quit that is no restart loses nothing by it: the process ends.
/// A shutdown that panics is written down by the hook and the marking still comes: the event
/// loop's handler is contained, so a restart may follow it (T-929).
fn at_exit(shutdown: impl FnOnce()) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(shutdown));
    #[cfg(target_os = "macos")]
    if let Err(failed) = cf_process::close_all_on_exec() {
        diag::say(format!(
            "ConsensFlow ends holding descriptors it could not mark close-on-exec: {failed}"
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;
    use std::path::{Path, PathBuf};

    const HOME: &str = "/Users/test";

    /// The Windows app links the C runtime statically, Tauri 2.12's default since tauri-build
    /// 2.7, and its configuration says so (T-930, T-941): it needs no Visual C++ redistributable,
    /// and a Tauri whose default moves does not move it.
    #[test]
    fn the_windows_app_links_its_c_runtime_statically() {
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).expect("the configuration");
        assert_eq!(
            config["build"]["windows"]["staticVCRuntime"],
            serde_json::Value::Bool(true),
            "{}",
            config["build"]
        );
    }

    /// A build that fails is said in the error log and ends the app with status 1: the person who
    /// reads the log finds the cause in a line of its own, not a panic's report. The test runs in
    /// a child, its own binary again, since the end is the process's.
    #[test]
    #[allow(clippy::disallowed_methods)] // The test tells itself it is its own child by a CONSENSFLOW_TEST_* variable.
    fn a_build_that_fails_is_said_and_ends_the_app_without_a_panic() {
        const NAME: &str = "tests::a_build_that_fails_is_said_and_ends_the_app_without_a_panic";
        const MARKER: &str = "CONSENSFLOW_TEST_BUILD_FAILS";
        if std::env::var_os(MARKER).is_some() {
            let failed = tauri::Error::Io(std::io::Error::other("no window system"));
            built::<tauri::test::MockRuntime>(Err(failed));
            return;
        }
        let ran = crate::testing::in_a_child(NAME, MARKER);
        let said = crate::testing::said(&ran);
        assert_eq!(ran.status.code(), Some(1), "{said}");
        assert!(
            said.contains("consensflow: the app could not be built: ")
                && said.contains("no window system"),
            "{said}"
        );
        assert!(!said.contains("panicked"), "{said}");
    }

    #[test]
    fn the_installed_release_keeps_the_live_home() {
        assert_eq!(
            isolated_home(PRODUCTION_IDENTIFIER, false, None, Some(Path::new(HOME))),
            Ok(None)
        );
    }

    #[test]
    fn every_other_build_gets_the_candidate_home() {
        let candidate = Some(PathBuf::from("/Users/test/.consensflow-candidate"));
        // A candidate bundle, an old probe bundle, and `tauri dev` of the
        // release identity all land in the one development home.
        for (identifier, debug) in [
            ("dev.ngvoicu.consensflow.candidate", false),
            ("dev.ngvoicu.consensflow.paste-probe", false),
            (PRODUCTION_IDENTIFIER, true),
        ] {
            assert_eq!(
                isolated_home(identifier, debug, None, Some(Path::new(HOME))),
                Ok(candidate.clone()),
                "{identifier} debug={debug}"
            );
        }
    }

    #[test]
    fn an_explicit_home_always_wins() {
        for (identifier, debug) in [
            (PRODUCTION_IDENTIFIER, false),
            ("dev.ngvoicu.consensflow.candidate", false),
            (PRODUCTION_IDENTIFIER, true),
        ] {
            assert_eq!(
                isolated_home(
                    identifier,
                    debug,
                    Some(OsStr::new("/isolated/state")),
                    Some(Path::new(HOME))
                ),
                Ok(None)
            );
        }
    }

    #[test]
    fn an_empty_home_setting_counts_as_unset() {
        assert_eq!(
            isolated_home(
                "dev.ngvoicu.consensflow.candidate",
                false,
                Some(OsStr::new("")),
                Some(Path::new(HOME))
            ),
            Ok(Some(PathBuf::from("/Users/test/.consensflow-candidate")))
        );
    }

    /// What a window gives its member (T-868) is none of an app's started from it; the rest of
    /// ConsensFlow's variables are: the home a run chose, the smoke's switch, the daemon's own.
    #[test]
    fn the_variables_a_window_of_a_board_gives_its_member_are_never_inherited() {
        let names = [
            "CONSENSFLOW_URL",
            "CONSENSFLOW_TOKEN",
            "CONSENSFLOW_PARTICIPANT",
            "CONSENSFLOW_PROJECT",
            "CONSENSFLOW_HOME",
            "CONSENSFLOW_SELFTEST",
            "CONSENSFLOW_TOOL_FOLDERS",
            "CONSENSFLOW_TERMINAL_ENV",
            "CF_PANE_PROCESS_OWNER",
            "PATH",
        ];
        assert_eq!(
            inherited_variables(names),
            [
                "CONSENSFLOW_URL",
                "CONSENSFLOW_TOKEN",
                "CONSENSFLOW_PARTICIPANT",
                "CONSENSFLOW_PROJECT",
            ]
        );
    }

    #[test]
    fn a_parent_claude_sessions_identity_is_never_inherited() {
        // Seen in a live Claude Code 2.1 session's environment on 2026-09-19.
        let names = [
            "CLAUDECODE",
            "CLAUDE_CODE_CHILD_SESSION",
            "CLAUDE_CODE_SESSION_ID",
            "CLAUDE_CODE_MESSAGING_SOCKET",
            "CLAUDE_CODE_MESSAGING_TOKEN",
            "CLAUDE_PID",
            "CLAUDE_EFFORT",
            "CLAUDE_CONFIG_DIR",
            "CLAUDE_CODE_USE_BEDROCK",
            "PATH",
        ];
        assert_eq!(
            inherited_variables(names),
            [
                "CLAUDECODE",
                "CLAUDE_CODE_CHILD_SESSION",
                "CLAUDE_CODE_SESSION_ID",
                "CLAUDE_CODE_MESSAGING_SOCKET",
                "CLAUDE_CODE_MESSAGING_TOKEN",
                "CLAUDE_PID",
                "CLAUDE_EFFORT",
            ],
            "configuration (CLAUDE_CONFIG_DIR, CLAUDE_CODE_USE_BEDROCK) stays"
        );
    }

    /// Every harness's session is taken out of what the app was started with (T-971), as
    /// Claude Code's was: enumerated from the harnesses the build runs.
    #[test]
    fn no_harness_sessions_variable_is_ever_inherited() {
        for harness in cf_proto::agents::Harness::ALL {
            let names = harness.session_variables();
            assert_eq!(
                inherited_variables(names.iter().copied()),
                names,
                "{}",
                harness.as_str()
            );
        }
        assert_eq!(
            inherited_variables(["CODEX_HOME", "PI_CODING_AGENT_DIR", "PATH"]),
            Vec::<&str>::new()
        );
    }

    #[test]
    fn the_error_log_lives_in_the_home_and_keeps_one_previous_file() {
        let home = tempfile::tempdir().expect("home");
        let log = prepare_error_log(home.path(), 16).expect("prepare");
        assert_eq!(log, home.path().join("app").join("app.log"));
        std::fs::write(&log, "small").expect("write");
        prepare_error_log(home.path(), 16).expect("small stays");
        assert_eq!(std::fs::read_to_string(&log).expect("read"), "small");
        std::fs::write(&log, "far more than sixteen bytes").expect("grow");
        prepare_error_log(home.path(), 16).expect("rotate");
        assert!(!log.exists(), "a large log is moved aside");
        assert_eq!(
            std::fs::read_to_string(home.path().join("app").join("app.log.1")).expect("previous"),
            "far more than sixteen bytes"
        );
    }

    /// The host's own errors reach the error log on every platform. On
    /// Windows they went nowhere: a windowed app has no console, and only the
    /// macOS build pointed its stderr at the log. The test runs in a child,
    /// its own binary again, since the redirect is for good.
    #[test]
    #[allow(clippy::disallowed_methods)] // The test tells itself it is its own child by a CONSENSFLOW_TEST_* variable.
    fn the_hosts_own_errors_reach_the_error_log() {
        const LOG: &str = "CONSENSFLOW_TEST_ERROR_LOG";
        if let Some(log) = std::env::var_os(LOG) {
            redirect_stderr(Path::new(&log));
            eprintln!("consensflow: the host's own error");
            return;
        }
        let home = tempfile::tempdir().expect("home");
        let log = home.path().join("app.log");
        let status = crate::testing::command(std::env::current_exe().expect("the test binary"))
            .args([
                "--exact",
                "tests::the_hosts_own_errors_reach_the_error_log",
                "--nocapture",
            ])
            .env(LOG, &log)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .expect("run the test in a child");
        assert!(status.success(), "{status}");
        assert!(std::fs::read_to_string(&log)
            .expect("the log")
            .contains("consensflow: the host's own error"));
    }

    /// The app's last code before Tauri starts the app an update installed (with a bare
    /// `Command`, on the main thread, after the loop has ended) leaves it nothing: here a
    /// descriptor is left open and not close-on-exec, the app's end is run, and the descriptor is
    /// looked at after, where Tauri's start of the new app would be. The marking is for the whole
    /// process, so the test runs in one of its own, its binary run again.
    #[cfg(target_os = "macos")]
    #[test]
    #[allow(clippy::disallowed_methods)] // The test tells itself it is its own child by a CONSENSFLOW_TEST_* variable.
    fn the_apps_end_leaves_every_descriptor_close_on_exec_for_the_restart_after_it() {
        const ENDED: &str = "CONSENSFLOW_TEST_AT_EXIT";
        if std::env::var_os(ENDED).is_some() {
            use std::os::fd::AsRawFd;

            let stray = std::fs::File::open("/dev/null").expect("a file");
            let fd = stray.as_raw_fd();
            // SAFETY: `fcntl` is asked to clear the flags of a descriptor this test holds.
            assert_eq!(unsafe { libc::fcntl(fd, libc::F_SETFD, 0) }, 0);
            let mut shut_down = false;
            at_exit(|| shut_down = true);
            assert!(shut_down, "the runtime was not shut down");
            // SAFETY: `fcntl` is asked the flags of a descriptor this test holds.
            let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
            assert_ne!(flags & libc::FD_CLOEXEC, 0, "left inheritable");
            return;
        }
        let ran = crate::testing::command(std::env::current_exe().expect("the test binary"))
            .args([
                "--exact",
                "tests::the_apps_end_leaves_every_descriptor_close_on_exec_for_the_restart_after_it",
                "--nocapture",
            ])
            .env(ENDED, "1")
            .stdin(std::process::Stdio::null())
            .output()
            .expect("run the test in a child");
        let said = format!(
            "{}{}",
            String::from_utf8_lossy(&ran.stdout),
            String::from_utf8_lossy(&ran.stderr)
        );
        assert!(ran.status.success(), "{said}");
        assert!(said.contains("1 passed"), "the test did not run: {said}");
    }

    /// A smoke's build is started by its smoke, which names its home: one opened by
    /// hand, or by anything else, would run on the Candidate's home, its lock and its
    /// roster, and it starts on none.
    #[test]
    fn a_smoke_build_starts_only_on_the_home_its_smoke_names() {
        const SMOKE: &str = "dev.ngvoicu.consensflow.smoke";
        for configured in [None, Some(OsStr::new(""))] {
            let refused = isolated_home(SMOKE, false, configured, Some(Path::new(HOME)));
            assert!(
                refused
                    .as_ref()
                    .is_err_and(|said| said.contains("CONSENSFLOW_HOME")),
                "{refused:?}"
            );
        }
        assert_eq!(
            isolated_home(
                SMOKE,
                false,
                Some(OsStr::new("/smoke/home")),
                Some(Path::new(HOME))
            ),
            Ok(None)
        );
    }

    /// No window of a test's build reads as the live app's: its identity is in its title.
    #[test]
    fn a_window_says_which_build_it_is_when_it_is_not_the_releases() {
        for (name, identifier, title) in [
            ("ConsensFlow", PRODUCTION_IDENTIFIER, "ConsensFlow"),
            (
                "ConsensFlow",
                "dev.ngvoicu.consensflow.smoke",
                "ConsensFlow Smoke",
            ),
            (
                "ConsensFlow Candidate",
                "dev.ngvoicu.consensflow.candidate",
                "ConsensFlow Candidate",
            ),
            (
                "ConsensFlow",
                "dev.ngvoicu.consensflow.paste-probe",
                "ConsensFlow Paste Probe",
            ),
            (
                "ConsensFlow",
                "dev.example.other",
                "ConsensFlow dev.example.other",
            ),
        ] {
            assert_eq!(window_title(name, identifier), title, "{identifier}");
        }
    }

    /// A shutdown that panics still leaves every descriptor close-on-exec: the event loop's
    /// handler is contained, so a panic there no longer ends the app before a restart starts the
    /// app an update installed, and the marking must come all the same (T-929).
    #[cfg(target_os = "macos")]
    #[test]
    #[allow(clippy::disallowed_methods)] // The test tells itself it is its own child by a CONSENSFLOW_TEST_* variable.
    fn a_shutdown_that_panics_still_leaves_every_descriptor_close_on_exec() {
        const ENDED: &str = "CONSENSFLOW_TEST_AT_EXIT_PANICS";
        if std::env::var_os(ENDED).is_some() {
            use std::os::fd::AsRawFd;

            let stray = std::fs::File::open("/dev/null").expect("a file");
            let fd = stray.as_raw_fd();
            // SAFETY: `fcntl` is asked to clear the flags of a descriptor this test holds.
            assert_eq!(unsafe { libc::fcntl(fd, libc::F_SETFD, 0) }, 0);
            at_exit(|| panic!("the shutdown of T-929 panics"));
            // SAFETY: `fcntl` is asked the flags of a descriptor this test holds.
            let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
            assert_ne!(flags & libc::FD_CLOEXEC, 0, "left inheritable");
            return;
        }
        let ran = crate::testing::command(std::env::current_exe().expect("the test binary"))
            .args([
                "--exact",
                "tests::a_shutdown_that_panics_still_leaves_every_descriptor_close_on_exec",
                "--nocapture",
            ])
            .env(ENDED, "1")
            .stdin(std::process::Stdio::null())
            .output()
            .expect("run the test in a child");
        let said = format!(
            "{}{}",
            String::from_utf8_lossy(&ran.stdout),
            String::from_utf8_lossy(&ran.stderr)
        );
        assert!(ran.status.success(), "{said}");
        assert!(said.contains("1 passed"), "the test did not run: {said}");
    }

    /// The app's file says the identity its build was made with, as Tauri's configuration says
    /// it: its file, the target's own file and what the build was given in `TAURI_CONFIG`, read
    /// as the build script reads them (T-978: not the file alone, which a `--config` or a
    /// target's file naming an identifier would make wrong).
    #[test]
    fn the_app_says_the_identity_its_build_was_made_with() {
        let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let read = |name: &str| -> Result<Option<String>, String> {
            match std::fs::read_to_string(folder.join(name)) {
                Ok(text) => Ok(Some(text)),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(error) => Err(format!("{name}: {error}")),
            }
        };
        let (identity, _) = built_identity::built_identity(
            &read,
            std::env::consts::OS,
            option_env!("TAURI_CONFIG"),
        )
        .expect("the identity of the build");
        assert_eq!(
            BUILT_IDENTITY,
            format!("consensflow-built-identity:{identity};")
        );
    }

    #[test]
    fn a_candidate_without_a_home_directory_refuses_to_start() {
        assert!(isolated_home("dev.ngvoicu.consensflow.candidate", false, None, None).is_err());
    }

    fn selftest_config(step_ms: Option<u64>) -> selftest::Config {
        selftest::Config {
            dir: "/workspace".into(),
            tag: "tag".into(),
            updater_expected_version: None,
            step_ms,
            panic_probe: false,
        }
    }

    /// The page asks the command that panics only of an app started for the smoke's case of it
    /// (T-1050).
    #[test]
    fn the_selftest_tells_the_page_whether_it_asks_the_command_that_panics() {
        let asked = selftest::initialization_script(&selftest::Config {
            panic_probe: true,
            ..selftest_config(None)
        });
        assert!(asked.contains(r#""panicProbe":true"#), "{asked}");
        let not = selftest::initialization_script(&selftest_config(None));
        assert!(not.contains(r#""panicProbe":false"#), "{not}");
    }

    #[test]
    fn the_selftest_tells_the_page_the_step_it_was_given_and_null_where_it_was_given_none() {
        let told = selftest::initialization_script(&selftest_config(Some(135_000)));
        assert!(told.contains(r#""stepMs":135000"#), "{told}");
        let untold = selftest::initialization_script(&selftest_config(None));
        assert!(untold.contains(r#""stepMs":null"#), "{untold}");
        // What the page was always told is told as it was.
        assert!(
            told.contains(r#""dir":"/workspace""#) && told.contains(r#""tag":"tag""#),
            "{told}"
        );
    }
}

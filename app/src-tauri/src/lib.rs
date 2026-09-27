use std::io::{BufRead, Write};

use serde_json::{json, Value};
use tauri::{AppHandle, Manager, RunEvent, WebviewUrl, WebviewWindowBuilder};

pub mod arbiter;
pub mod bridge;
pub mod commands;
#[cfg(windows)]
mod job_object;
#[cfg(target_os = "macos")]
mod process_tree;
pub mod pty;
#[cfg(target_os = "macos")]
mod update_install;
pub mod updates;

use commands::AppRuntime;

/// The packaged smoke's only door into the shipping app.
///
/// Everything here is inert unless the app was STARTED with
/// `CONSENSFLOW_SELFTEST=1`: the page is told nothing, the reporting command
/// refuses, and neither extra thread exists. That is the whole guard, and it
/// is deliberately an environment variable rather than a flag the page could
/// set — a self-test channel that anything loaded later could open would be a
/// way to read a real user's panes off their screen.
///
/// The quit is stdin EOF rather than a report, because the smoke has work to
/// do WHILE the app is still up: the state root's kernel lock can only be
/// proved against a live owner. So the page says when it is finished looking,
/// the smoke does its own probes, and closing the pipe ends the app through
/// its ordinary `RunEvent::Exit`.
mod selftest {
    use super::{json, AppHandle, BufRead, Value, Write};

    static UPDATE_CONTINUE: std::sync::atomic::AtomicBool =
        std::sync::atomic::AtomicBool::new(false);

    pub fn wait_for_update_probe() -> bool {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        while !UPDATE_CONTINUE.load(std::sync::atomic::Ordering::Acquire) {
            if std::time::Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        true
    }

    pub struct Config {
        pub dir: String,
        pub tag: String,
        pub updater_expected_version: Option<String>,
    }

    /// Read at every call, not cached: the guard is the environment the app
    /// was started with, and nothing in the process may widen it later.
    pub fn enabled() -> bool {
        matches!(std::env::var("CONSENSFLOW_SELFTEST").as_deref(), Ok("1"))
    }

    pub fn config() -> Option<Config> {
        if !enabled() {
            return None;
        }
        Some(Config {
            dir: std::env::var("CONSENSFLOW_SELFTEST_DIR").unwrap_or_default(),
            tag: std::env::var("CONSENSFLOW_SELFTEST_TAG").unwrap_or_default(),
            updater_expected_version: std::env::var("CONSENSFLOW_SELFTEST_UPDATER_EXPECTED").ok(),
        })
    }

    /// How long the whole self-test may take before the app quits itself.
    ///
    /// A wedged run must never leave a maximized window sitting on someone's
    /// screen waiting to be noticed, so the app is its own dead man's switch
    /// and exits non-zero, which the smoke reads as the failure it is.
    fn deadline_ms() -> u64 {
        std::env::var("CONSENSFLOW_SELFTEST_DEADLINE_MS")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(240_000)
    }

    /// Opens the channel BEFORE any page script runs.
    ///
    /// A page that dies on its first import reports nothing, and silence is
    /// the one answer a smoke must never accept — it looks exactly like a
    /// hang. So the flag arrives with two listeners that forward whatever
    /// killed the page, queued until Tauri's own injection has landed.
    pub fn initialization_script(config: &Config) -> String {
        format!(
            "window.__CONSENSFLOW_SELFTEST__ = {};\n{}",
            json!({"dir": config.dir, "tag": config.tag, "updaterExpectedVersion":config.updater_expected_version}),
            PAGE_CHANNEL,
        )
    }

    const PAGE_CHANNEL: &str = r#"
(function () {
  var queued = [];
  function core() {
    var tauri = window.__TAURI__;
    return tauri && tauri.core && typeof tauri.core.invoke === 'function' ? tauri.core : null;
  }
  function send(event, data) {
    var ready = core();
    if (ready === null) {
      queued.push([event, data]);
      return;
    }
    ready.invoke('selftest_report', { event: event, data: data });
  }
  function flush() {
    if (core() === null) return;
    var pending = queued;
    queued = [];
    for (var index = 0; index < pending.length; index += 1) {
      send(pending[index][0], pending[index][1]);
    }
  }
  window.addEventListener('error', function (event) {
    send('page-error', {
      message: String(event.message),
      source: String(event.filename),
      line: event.lineno,
    });
    flush();
  });
  window.addEventListener('unhandledrejection', function (event) {
    send('page-rejection', { reason: String(event.reason && event.reason.stack ? event.reason.stack : event.reason) });
    flush();
  });
  window.addEventListener('DOMContentLoaded', function () {
    send('page-init', { tauri: core() !== null });
    flush();
  });
  setInterval(flush, 250);
})();
"#;

    /// One line of the self-test channel: the app's own stdout, which only
    /// the process that launched it can read.
    pub fn report(event: &str, data: &Value) {
        let line = json!({"event": event, "data": data, "pid": std::process::id()});
        let mut out = std::io::stdout().lock();
        let _ = writeln!(out, "consensflow-selftest {line}");
        let _ = out.flush();
    }

    pub fn watch_for_quit(handle: AppHandle) {
        let deadline = deadline_ms();
        let expiry = handle.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(deadline));
            report("deadline", &json!({ "ms": deadline }));
            // Deliberately abrupt. This path only runs when the self-test is
            // already wedged, and a graceful exit here has been observed to
            // end the process 0 — which would read as a pass. The smoke kills
            // the whole process group after a failure, so nothing is orphaned.
            let _ = expiry;
            std::process::exit(2);
        });
        std::thread::spawn(move || {
            // Whatever the smoke wrote first is drained and dropped: a stray
            // byte must not be able to keep the app alive, and only the pipe
            // closing means "done".
            let stdin = std::io::stdin();
            let mut lines = stdin.lock().lines();
            while let Some(Ok(line)) = lines.next() {
                if line == "continue-updater"
                    && config().is_some_and(|c| c.updater_expected_version.is_some())
                {
                    UPDATE_CONTINUE.store(true, std::sync::atomic::Ordering::Release);
                }
            }
            report("quit", &json!({"reason":"stdin-eof"}));
            handle.exit(0);
        });
    }
}

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
/// default, everything else gets `~/.consensflow-candidate`. An explicit,
/// non-empty `CONSENSFLOW_HOME` always wins — tests and smokes set one.
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
    home.map(|home| Some(home.join(".consensflow-candidate")))
        .ok_or_else(|| "this build needs HOME to find ~/.consensflow-candidate".to_string())
}

/// The variables a Claude Code session exports to its own children. A
/// ConsensFlow started from inside one (`tauri dev`, a test or bench run) would
/// hand them to every pane: `CLAUDE_CODE_CHILD_SESSION` switches off transcript
/// saving, `CLAUDE_CODE_SESSION_ID` impersonates the parent, and the messaging
/// socket and token reach into the parent session. Only this identity is
/// removed; configuration such as `CLAUDE_CONFIG_DIR` or
/// `CLAUDE_CODE_USE_BEDROCK` stays.
const CLAUDE_SESSION_IDENTITY: [&str; 11] = [
    "CLAUDECODE",
    "CLAUDE_CODE_ENTRYPOINT",
    "CLAUDE_CODE_SESSION_ID",
    "CLAUDE_CODE_CHILD_SESSION",
    "CLAUDE_CODE_SESSION_ATTENDED",
    "CLAUDE_CODE_MESSAGING_SOCKET",
    "CLAUDE_CODE_MESSAGING_TOKEN",
    "CLAUDE_CODE_BRIDGE_SESSION_ID",
    "CLAUDE_CODE_EXECPATH",
    "CLAUDE_PID",
    "CLAUDE_EFFORT",
];

fn inherited_session_variables<'a>(names: impl IntoIterator<Item = &'a str>) -> Vec<&'a str> {
    names
        .into_iter()
        .filter(|name| CLAUDE_SESSION_IDENTITY.contains(name))
        .collect()
}

/// Where the app and its daemon write their error output: `<home>/app/app.log`,
/// with one previous file kept once it passes `limit` bytes. A Finder-launched
/// app's stderr is /dev/null, so panics and daemon errors used to leave no
/// trace at all.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn prepare_error_log(home: &std::path::Path, limit: u64) -> std::io::Result<std::path::PathBuf> {
    let directory = home.join("app");
    std::fs::create_dir_all(&directory)?;
    let log = directory.join("app.log");
    if std::fs::metadata(&log).is_ok_and(|meta| meta.len() > limit) {
        std::fs::rename(&log, directory.join("app.log.1"))?;
    }
    Ok(log)
}

/// Points this process's stderr, and so the daemon's and every pane host
/// message, at the error log. Best effort: the app runs without it.
#[cfg(target_os = "macos")]
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let context = tauri::generate_context!();
    // Settled before any thread exists, so the updater and the Node process —
    // which inherits this environment, and passes it to every pane — all see
    // the one home.
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"));
    match isolated_home(
        &context.config().identifier,
        cfg!(debug_assertions),
        std::env::var_os("CONSENSFLOW_HOME").as_deref(),
        home.as_deref().map(std::path::Path::new),
    ) {
        Ok(Some(isolated)) => std::env::set_var("CONSENSFLOW_HOME", isolated),
        Ok(None) => {}
        Err(message) => {
            eprintln!("consensflow: {message}");
            std::process::exit(1);
        }
    }
    let names: Vec<String> = std::env::vars_os()
        .filter_map(|(name, _)| name.into_string().ok())
        .collect();
    for name in inherited_session_variables(names.iter().map(String::as_str)) {
        std::env::remove_var(name);
    }
    #[cfg(target_os = "macos")]
    if let Some(home) = std::env::var_os("CONSENSFLOW_HOME")
        .filter(|value| !value.is_empty())
        .map(std::path::PathBuf::from)
        .or_else(|| {
            home.as_ref()
                .map(|home| std::path::Path::new(home).join(".consensflow"))
        })
    {
        if let Ok(log) = prepare_error_log(&home, 10 * 1024 * 1024) {
            redirect_stderr(&log);
        }
    }
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(|invoke| {
            if !commands::window_command_allowed(
                invoke.message.webview_ref().label(),
                invoke.message.command(),
            ) {
                invoke
                    .resolver
                    .reject("This command is unavailable in this window");
                return true;
            }
            let handler: Box<dyn Fn(tauri::ipc::Invoke<tauri::Wry>) -> bool> =
                Box::new(tauri::generate_handler![
                    commands::core_request,
                    commands::pane_input_enqueue,
                    commands::pane_reply_enqueue,
                    commands::pane_input_wait,
                    commands::pane_resize,
                    commands::pane_ack,
                    commands::roster_handle,
                    commands::open_agents_window,
                    commands::subscribe_output,
                    updates::update_status,
                    updates::update_channel,
                    updates::update_check,
                    updates::update_download,
                    updates::update_install,
                    selftest_report,
                ]);
            handler(invoke)
        })
        .setup(|app| {
            app.manage(AppRuntime::start(app.handle()));
            updates::setup(app)?;
            // The product name, so a candidate's window never reads as the live app's.
            let title = app.package_info().name.clone();
            let mut window = WebviewWindowBuilder::new(app, "main", WebviewUrl::default())
                .title(title)
                .inner_size(1280.0, 820.0)
                .min_inner_size(720.0, 520.0)
                .maximized(true);
            if let Some(config) = selftest::config() {
                window = window.initialization_script(selftest::initialization_script(&config));
                selftest::watch_for_quit(app.handle().clone());
            }
            window.build()?;
            Ok(())
        })
        .build(context)
        .expect("error while building the ConsensFlow app");

    app.run(|handle, event| {
        if matches!(event, RunEvent::Exit) {
            handle.state::<AppRuntime>().shutdown();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;
    use std::path::{Path, PathBuf};

    const HOME: &str = "/Users/test";

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
            inherited_session_variables(names),
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

    #[test]
    fn a_candidate_without_a_home_directory_refuses_to_start() {
        assert!(isolated_home("dev.ngvoicu.consensflow.candidate", false, None, None).is_err());
    }
}

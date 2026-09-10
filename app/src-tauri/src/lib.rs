use std::io::{BufRead, Write};

use serde_json::{json, Value};
use tauri::{AppHandle, Manager, RunEvent, WebviewUrl, WebviewWindowBuilder};

pub mod arbiter;
pub mod bridge;
pub mod commands;
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
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
                    commands::open_pm,
                    commands::open_lead,
                    commands::open_shell,
                    commands::open_consult,
                    commands::close_pane,
                    commands::delete_pane,
                    commands::pane_input_enqueue,
                    commands::pane_reply_enqueue,
                    commands::pane_input_wait,
                    commands::pane_resize,
                    commands::pane_ack,
                    commands::set_policy,
                    commands::answers_list,
                    commands::deliver_now,
                    commands::deliver_cancel,
                    commands::held_send,
                    commands::tab_resume,
                    commands::tab_delete,
                    commands::rename_session,
                    commands::list_state,
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
            let mut window = WebviewWindowBuilder::new(app, "main", WebviewUrl::default())
                .title("ConsensFlow")
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
        .build(tauri::generate_context!())
        .expect("error while building the ConsensFlow app");

    app.run(|handle, event| {
        if matches!(event, RunEvent::Exit) {
            handle.state::<AppRuntime>().shutdown();
        }
    });
}

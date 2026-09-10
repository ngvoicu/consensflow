use crate::commands::AppRuntime;
use semver::Version;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tauri_plugin_updater::{RemoteRelease, Update, UpdaterExt};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Channel {
    Alpha,
    Stable,
}

impl Channel {
    fn endpoint(self) -> url::Url {
        let tag = if self == Self::Alpha {
            "update-alpha"
        } else {
            "update-stable"
        };
        format!("https://github.com/ngvoicu/consensflow/releases/download/{tag}/latest.json")
            .parse()
            .unwrap()
    }

    fn default_for(version: &str) -> Self {
        if Version::parse(version).is_ok_and(|v| !v.pre.is_empty()) {
            Self::Alpha
        } else {
            Self::Stable
        }
    }

    fn accepts(self, version: &Version) -> bool {
        version.build.is_empty()
            && (version.pre.is_empty()
                || (self == Self::Alpha && version.pre.as_str().split('.').next() == Some("alpha")))
    }
}

fn current_snapshot<R: Runtime>(app: &AppHandle<R>) -> UpdateSnapshot {
    let mut snapshot = app.state::<UpdateManager>().snapshot();
    snapshot.blockers = match app.try_state::<AppRuntime>() {
        Some(runtime) => match runtime.pane_table().update_blockers() {
            Ok(panes) => panes
                .iter()
                .map(|p| serde_json::json!({"id":p.id,"generation":p.generation}))
                .collect(),
            Err(error) => {
                snapshot.error = Some(error.to_string());
                vec![serde_json::json!({"id":"pane-state-unavailable","generation":0})]
            }
        },
        None => vec![serde_json::json!({"id":"app-runtime-unavailable","generation":0})],
    };
    snapshot
}

fn publish<R: Runtime>(app: &AppHandle<R>) -> Value {
    let snapshot = current_snapshot(app);
    let _ = app.emit("update-state-changed", &snapshot);
    serde_json::to_value(snapshot).unwrap()
}

fn command_error<R: Runtime>(app: &AppHandle<R>, error: String) -> Value {
    let mut result = publish(app);
    result["ok"] = false.into();
    result["error"] = error.into();
    result
}

#[tauri::command]
pub fn update_status<R: Runtime>(app: AppHandle<R>) -> Value {
    serde_json::to_value(current_snapshot(&app)).unwrap()
}

#[tauri::command]
pub fn update_channel<R: Runtime>(app: AppHandle<R>, channel: Channel) -> Value {
    match app.state::<UpdateManager>().set_channel(channel) {
        Ok(()) => publish(&app),
        Err(error) => command_error(&app, error),
    }
}

#[tauri::command]
pub async fn update_check<R: Runtime>(app: AppHandle<R>) -> Value {
    let manager = app.state::<UpdateManager>();
    if !manager.begin_check() {
        return update_status(app.clone());
    }
    publish(&app);
    let snapshot = manager.snapshot();
    let checked = async {
        let fixture = test_update_url(
            crate::selftest::enabled(),
            std::env::var("CONSENSFLOW_SELFTEST_UPDATER_URL")
                .ok()
                .as_deref(),
        )?;
        let mut builder = app
            .updater_builder()
            .target(tauri_plugin_updater::target().ok_or("This platform has no update target")?)
            .endpoints(vec![fixture
                .clone()
                .unwrap_or_else(|| snapshot.channel.endpoint())])
            .map_err(|e| e.to_string())?
            .timeout(std::time::Duration::from_secs(20))
            .configure_client(|client| client.https_only(true));
        if fixture.is_some() {
            // Only the existing opt-in packaged self-test may replace the
            // distribution boundary. TLS and archive signatures still verify.
            let pem = std::fs::read(
                std::env::var("CONSENSFLOW_SELFTEST_UPDATER_CERT").map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
            let certificate = reqwest::Certificate::from_pem(&pem).map_err(|e| e.to_string())?;
            let public = std::fs::read_to_string(
                std::env::var("CONSENSFLOW_SELFTEST_UPDATER_KEY").map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
            builder = builder
                .pubkey(public.trim())
                .no_proxy()
                .configure_client(move |client| {
                    client
                        .https_only(true)
                        .add_root_certificate(certificate.clone())
                });
        }
        let updater = builder.build().map_err(|e| e.to_string())?;
        let mut candidate = updater
            .check()
            .await
            .map_err(|e| describe_check_error(snapshot.channel, e))?;
        let available = candidate
            .as_ref()
            .map(|update| {
                validate_release(
                    &snapshot.current_version,
                    snapshot.channel,
                    &update.raw_json,
                    &update.target,
                )
            })
            .transpose()?;
        if let (Some(endpoint), Some(update)) = (fixture, candidate.as_mut()) {
            update.download_url = endpoint.join("archive").map_err(|e| e.to_string())?;
        }
        Ok::<_, String>((candidate, available))
    }
    .await;
    match checked {
        Ok((pending, available)) => {
            let mut state = manager.state.lock().unwrap();
            state.snapshot.phase = if pending.is_some() {
                "available"
            } else {
                "idle"
            };
            state.pending = pending;
            state.snapshot.available = available;
            state.snapshot.last_checked = time::OffsetDateTime::now_utc()
                .format(&time::format_description::well_known::Rfc3339)
                .ok();
        }
        Err(error) => manager.fail(format!("Could not check for updates: {error}")),
    }
    publish(&app)
}

fn describe_check_error(channel: Channel, error: tauri_plugin_updater::Error) -> String {
    if matches!(error, tauri_plugin_updater::Error::ReleaseNotFound) {
        let label = if channel == Channel::Alpha {
            "Alpha"
        } else {
            "Stable"
        };
        format!("The {label} update feed is unavailable. Please try again later.")
    } else {
        error.to_string()
    }
}

fn test_update_url(enabled: bool, address: Option<&str>) -> Result<Option<url::Url>, String> {
    if !enabled {
        return Ok(None);
    }
    let Some(address) = address else {
        return Ok(None);
    };
    let url: url::Url = address
        .parse()
        .map_err(|e: url::ParseError| e.to_string())?;
    if url.scheme() != "https"
        || !matches!(url.host_str(), Some("localhost" | "127.0.0.1"))
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("The packaged updater fixture must use loopback HTTPS".into());
    }
    Ok(Some(url))
}

#[tauri::command]
pub async fn update_download<R: Runtime>(app: AppHandle<R>) -> Value {
    let result = app
        .state::<UpdateManager>()
        .download(|_| {
            publish(&app);
        })
        .await;
    match result {
        Ok(()) => publish(&app),
        Err(error) => command_error(&app, error),
    }
}

#[tauri::command]
pub async fn update_install<R: Runtime>(app: AppHandle<R>) -> Value {
    #[cfg(not(target_os = "macos"))]
    return command_error(
        &app,
        "In-app installation is currently available on macOS".into(),
    );
    #[cfg(target_os = "macos")]
    {
        let swapped = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let swapped_in_worker = std::sync::Arc::clone(&swapped);
        let worker = app.clone();
        let installed = tauri::async_runtime::spawn_blocking(move || {
            let executable = std::env::current_exe().map_err(|e| e.to_string())?;
            let target = tauri_plugin_updater::extract_path_from_executable(&executable)
                .map_err(|e| e.to_string())?;
            let manager = worker.state::<UpdateManager>();
            let panes = worker.state::<AppRuntime>().pane_table();
            let permit = manager.install(&panes, |bytes, version| {
                publish(&worker);
                crate::update_install::install_archive(&target, bytes, version)?;
                swapped_in_worker.store(true, std::sync::atomic::Ordering::Release);
                Ok(())
            })?;
            if worker.state::<AppRuntime>().begin_shutdown() {
                let cleanup = worker.clone();
                if !finish_before_deadline(std::time::Duration::from_secs(5), move || {
                    cleanup.state::<AppRuntime>().finish_shutdown();
                }) {
                    eprintln!("ConsensFlow update installed; internal drain failed or exceeded its five-second deadline. Restarting with no open panes.");
                }
            }
            Ok::<_, String>(permit)
        })
        .await;
        match installed {
            Ok(Ok(_permit)) => {
                // Admission stays closed through restart. Node has exited;
                // a stalled bridge drain cannot keep the old process alive.
                app.restart();
            }
            Ok(Err(error)) => command_error(&app, error),
            Err(error) => {
                let message = if swapped.load(std::sync::atomic::Ordering::Acquire) {
                    format!("The update is installed, but restart could not finish: {error}. Quit and reopen ConsensFlow.")
                } else {
                    format!("Installation failed: {error}")
                };
                app.state::<UpdateManager>().fail(message);
                publish(&app)
            }
        }
    }
}

// The internal Node process has already exited and installation admission
// excludes every pane. A broken bridge callback must not prevent restart.
fn finish_before_deadline(
    timeout: std::time::Duration,
    finish: impl FnOnce() + Send + 'static,
) -> bool {
    let (sent, done) = std::sync::mpsc::sync_channel(1);
    if std::thread::Builder::new()
        .name("consensflow-update-drain".into())
        .spawn(move || {
            finish();
            let _ = sent.send(());
        })
        .is_err()
    {
        return false;
    }
    done.recv_timeout(timeout).is_ok()
}

pub fn setup(app: &tauri::App) -> tauri::Result<()> {
    let directory = std::env::var_os("CONSENSFLOW_HOME")
        .map(PathBuf::from)
        .map(|p| p.join("app"))
        .unwrap_or(app.path().app_config_dir()?);
    app.manage(UpdateManager::new(
        &app.package_info().version.to_string(),
        directory.join("updates.json"),
    ));
    let menu = tauri::menu::Menu::default(app.handle())?;
    let check = tauri::menu::MenuItem::with_id(
        app,
        "check-updates",
        "Check for Updates…",
        true,
        None::<&str>,
    )?;
    if let Some(submenu) = menu.items()?.first().and_then(|item| item.as_submenu()) {
        submenu.insert(&check, 1)?;
    }
    app.set_menu(menu)?;
    app.on_menu_event(|app, event| {
        if event.id().as_ref() == "check-updates" {
            let _ = app.emit("check-updates", ());
        }
    });
    Ok(())
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSnapshot {
    ok: bool,
    current_version: String,
    channel: Channel,
    phase: &'static str,
    available: Option<ReleaseInfo>,
    downloaded_bytes: u64,
    total_bytes: Option<u64>,
    last_checked: Option<String>,
    error: Option<String>,
    blockers: Vec<Value>,
}

struct UpdateState {
    snapshot: UpdateSnapshot,
    pending: Option<Update>,
    bytes: Option<Vec<u8>>,
}

pub struct UpdateManager {
    state: Mutex<UpdateState>,
    preferences: PathBuf,
}

impl UpdateManager {
    pub fn new(current: &str, preferences: PathBuf) -> Self {
        let channel = std::fs::read(&preferences)
            .ok()
            .and_then(|b| serde_json::from_slice::<Channel>(&b).ok())
            .unwrap_or_else(|| Channel::default_for(current));
        Self {
            state: Mutex::new(UpdateState {
                snapshot: UpdateSnapshot {
                    ok: true,
                    current_version: current.into(),
                    channel,
                    phase: "idle",
                    available: None,
                    downloaded_bytes: 0,
                    total_bytes: None,
                    last_checked: None,
                    error: None,
                    blockers: Vec::new(),
                },
                pending: None,
                bytes: None,
            }),
            preferences,
        }
    }

    fn snapshot(&self) -> UpdateSnapshot {
        self.state.lock().unwrap().snapshot.clone()
    }

    fn set_channel(&self, channel: Channel) -> Result<(), String> {
        let mut state = self.state.lock().unwrap();
        if matches!(
            state.snapshot.phase,
            "checking" | "downloading" | "installing"
        ) {
            return Err("Wait for the current update operation to finish".into());
        }
        let parent = self
            .preferences
            .parent()
            .ok_or("Invalid update preferences path")?;
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        let temporary = self.preferences.with_extension("tmp");
        std::fs::write(
            &temporary,
            serde_json::to_vec(&channel).map_err(|e| e.to_string())?,
        )
        .and_then(|()| std::fs::rename(&temporary, &self.preferences))
        .map_err(|e| e.to_string())?;
        state.snapshot.channel = channel;
        state.snapshot.phase = "idle";
        state.snapshot.available = None;
        state.snapshot.error = None;
        state.snapshot.last_checked = None;
        state.snapshot.downloaded_bytes = 0;
        state.snapshot.total_bytes = None;
        state.pending = None;
        state.bytes = None;
        Ok(())
    }

    fn begin_check(&self) -> bool {
        let mut state = self.state.lock().unwrap();
        if matches!(
            state.snapshot.phase,
            "checking" | "downloading" | "ready" | "installing"
        ) {
            return false;
        }
        state.snapshot.phase = "checking";
        state.snapshot.error = None;
        state.snapshot.available = None;
        state.pending = None;
        state.bytes = None;
        true
    }

    fn fail(&self, error: String) {
        let mut state = self.state.lock().unwrap();
        state.snapshot.phase = "error";
        state.snapshot.error = Some(error);
    }

    fn install(
        &self,
        panes: &std::sync::Arc<crate::pty::PaneTable>,
        action: impl FnOnce(&[u8], &str) -> Result<(), String>,
    ) -> Result<crate::pty::UpdatePermit, String> {
        let (permit, bytes, version) = {
            let mut state = self.state.lock().unwrap();
            if state.snapshot.phase != "ready" || state.bytes.is_none() {
                return Err("Download and verify the update before installing".into());
            }
            let version = state
                .snapshot
                .available
                .as_ref()
                .ok_or("No update is available")?
                .version
                .clone();
            let permit = panes.begin_update().map_err(|e| e.to_string())?;
            state.snapshot.phase = "installing";
            state.snapshot.error = None;
            (permit, state.bytes.take().unwrap(), version)
        };
        if let Err(error) = action(&bytes, &version) {
            let mut state = self.state.lock().unwrap();
            state.bytes = Some(bytes);
            state.snapshot.phase = "ready";
            state.snapshot.error = Some(error.clone());
            return Err(error);
        }
        Ok(permit)
    }

    async fn download(&self, notify: impl Fn(&UpdateSnapshot)) -> Result<(), String> {
        let mut update = {
            let mut state = self.state.lock().unwrap();
            if !matches!(state.snapshot.phase, "available" | "error") {
                return Err("Choose an available update before downloading".into());
            }
            let update = state
                .pending
                .clone()
                .ok_or("Check for updates before downloading")?;
            state.snapshot.phase = "downloading";
            state.snapshot.error = None;
            state.snapshot.downloaded_bytes = 0;
            state.snapshot.total_bytes = None;
            update
        };
        notify(&self.snapshot());
        update.timeout = Some(std::time::Duration::from_secs(600));
        let mut last_notice = std::time::Instant::now();
        let downloaded = update
            .download(
                |length, total| {
                    let snapshot = {
                        let mut state = self.state.lock().unwrap();
                        state.snapshot.downloaded_bytes += length as u64;
                        state.snapshot.total_bytes = total;
                        state.snapshot.clone()
                    };
                    if last_notice.elapsed() >= std::time::Duration::from_millis(100) {
                        notify(&snapshot);
                        last_notice = std::time::Instant::now();
                    }
                },
                || {},
            )
            .await;
        match downloaded {
            Ok(bytes) => {
                let mut state = self.state.lock().unwrap();
                state.bytes = Some(bytes);
                state.snapshot.phase = "ready";
            }
            Err(error) => {
                let error = format!("Download or signature verification failed: {error}");
                self.fail(error.clone());
                notify(&self.snapshot());
                return Err(error);
            }
        }
        notify(&self.snapshot());
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct ReleaseInfo {
    version: String,
    notes: String,
    date: Option<String>,
}

pub fn validate_release(
    current: &str,
    channel: Channel,
    document: &Value,
    target: &str,
) -> Result<ReleaseInfo, String> {
    let release: RemoteRelease =
        serde_json::from_value(document.clone()).map_err(|e| e.to_string())?;
    let current = Version::parse(current).map_err(|e| e.to_string())?;
    if release.version <= current || !channel.accepts(&release.version) {
        return Err("This release is not newer or does not belong to the selected channel".into());
    }
    let version = release.version.to_string();
    if document["version"].as_str() != Some(&version) {
        return Err("The release version must be a canonical semantic version".into());
    }
    let notes = release.notes.clone().unwrap_or_default();
    if notes.len() > 64 * 1024 || release.pub_date.is_none() {
        return Err("The release needs a publication date and bounded release notes".into());
    }
    let url = release.download_url(target).map_err(|e| e.to_string())?;
    let prefix = format!("/ngvoicu/consensflow/releases/download/v{version}/");
    let filename = url.path().strip_prefix(&prefix).unwrap_or_default();
    if url.scheme() != "https"
        || url.host_str() != Some("github.com")
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !filename.ends_with(".app.tar.gz")
        || !filename.contains(&version)
        || !filename
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        || release
            .signature(target)
            .map_err(|e| e.to_string())?
            .trim()
            .is_empty()
    {
        return Err(
            "The update archive must be a signed, versioned ConsensFlow GitHub asset".into(),
        );
    }
    Ok(ReleaseInfo {
        version,
        notes,
        date: document["pub_date"].as_str().map(str::to_owned),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn release(version: &str) -> serde_json::Value {
        json!({"version":version,"notes":"A verified update","pub_date":"2026-09-09T12:00:00Z",
            "platforms":{"darwin-aarch64":{"url":format!("https://github.com/ngvoicu/consensflow/releases/download/v{version}/ConsensFlow_{version}_aarch64.app.tar.gz"),"signature":"test-signature"}},
            "consensflow":{"verified_harnesses":{"claude-code":["2.1.265"],"pi":["0.85.1"]}}})
    }

    #[test]
    fn releases_follow_channel_and_version_policy() {
        let current = "3.0.0-alpha.35";
        assert!(validate_release(
            current,
            Channel::Alpha,
            &release("3.0.0-alpha.36"),
            "darwin-aarch64"
        )
        .is_ok());
        assert!(
            validate_release(current, Channel::Alpha, &release("3.0.0"), "darwin-aarch64").is_ok()
        );
        for version in [
            "3.0.0-alpha.35",
            "3.0.0-alpha.34",
            "2.9.0",
            "3.0.0-beta.1",
            "3.0.0-alpha.36+local",
        ] {
            assert!(
                validate_release(current, Channel::Alpha, &release(version), "darwin-aarch64")
                    .is_err(),
                "{version}"
            );
        }
        assert!(validate_release(
            "3.0.0",
            Channel::Stable,
            &release("3.0.1"),
            "darwin-aarch64"
        )
        .is_ok());
        assert!(validate_release(
            "3.0.0",
            Channel::Stable,
            &release("3.0.1-alpha.1"),
            "darwin-aarch64"
        )
        .is_err());
    }

    #[test]
    fn native_metadata_does_not_gate_signed_application_releases() {
        let mut next = release("3.0.0-alpha.36");
        next["consensflow"] = json!({"verified_harnesses": "obsolete metadata"});
        assert!(
            validate_release("3.0.0-alpha.35", Channel::Alpha, &next, "darwin-aarch64").is_ok()
        );
        next.as_object_mut().unwrap().remove("consensflow");
        assert!(
            validate_release("3.0.0-alpha.35", Channel::Alpha, &next, "darwin-aarch64").is_ok()
        );
    }

    #[test]
    fn packaged_test_transport_is_inert_normally_and_restricted_to_local_https() {
        assert!(test_update_url(false, Some("http://attacker.test/feed"))
            .unwrap()
            .is_none());
        assert!(test_update_url(true, None).unwrap().is_none());
        for address in [
            "http://127.0.0.1:10000/feed",
            "https://attacker.test/feed",
            "https://user:password@localhost:10000/feed",
            "file:///tmp/feed",
        ] {
            assert!(test_update_url(true, Some(address)).is_err(), "{address}");
        }
        assert!(test_update_url(true, Some("https://127.0.0.1:10000/feed"))
            .unwrap()
            .is_some());
    }

    #[test]
    fn metadata_rejects_other_origins_targets_and_unbounded_notes() {
        for url in [
            "http://github.com/ngvoicu/consensflow/releases/download/v3.0.0/update.tar.gz",
            "https://attacker.test/update.tar.gz",
            "https://github.com/other/repo/releases/download/v3.0.0/update.tar.gz",
            "https://github.com/ngvoicu/consensflow/releases/download/latest/app.tar.gz",
        ] {
            let mut doc = release("3.0.0");
            doc["platforms"]["darwin-aarch64"]["url"] = url.into();
            assert!(
                validate_release("3.0.0-alpha.35", Channel::Alpha, &doc, "darwin-aarch64").is_err(),
                "{url}"
            );
        }
        let mut doc = release("3.0.0");
        doc["notes"] = "x".repeat(64 * 1024 + 1).into();
        assert!(
            validate_release("3.0.0-alpha.35", Channel::Alpha, &doc, "darwin-aarch64").is_err()
        );
        assert!(validate_release(
            "3.0.0-alpha.35",
            Channel::Alpha,
            &release("3.0.0"),
            "darwin-x86_64"
        )
        .is_err());
    }

    #[test]
    fn real_updater_reports_unavailable_feeds_without_hiding_other_failures() {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let dir = tempfile::tempdir().unwrap();
        let mut context = tauri::test::mock_context(tauri::test::noop_assets());
        context.config_mut().plugins.0.insert(
            "updater".into(),
            json!({"pubkey":"unused-for-metadata-check"}),
        );
        let app = tauri::test::mock_builder()
            .plugin(tauri_plugin_updater::Builder::new().build())
            .build(context)
            .unwrap();
        for (channel, response, unavailable) in [
            (Channel::Alpha, "404 Not Found", true),
            (Channel::Stable, "503 Service Unavailable", true),
            (Channel::Stable, "200 OK", false),
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let endpoint = format!("http://{}", listener.local_addr().unwrap());
            let server = std::thread::spawn(move || {
                let (mut socket, _) = listener.accept().unwrap();
                socket
                    .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                    .unwrap();
                let mut header = Vec::new();
                while !header.ends_with(b"\r\n\r\n") {
                    let mut byte = [0];
                    socket.read_exact(&mut byte).unwrap();
                    header.push(byte[0]);
                }
                write!(socket, "HTTP/1.1 {response}\r\nContent-Length: 2\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{{}}").unwrap();
            });
            let error = tauri::async_runtime::block_on(async {
                app.updater_builder()
                    .target("darwin-aarch64")
                    .endpoints(vec![endpoint.parse().unwrap()])
                    .unwrap()
                    .timeout(std::time::Duration::from_secs(5))
                    .no_proxy()
                    .executable_path(dir.path().join("Test.app/Contents/MacOS/app"))
                    .build()
                    .unwrap()
                    .check()
                    .await
                    .err()
                    .expect("a failed feed must not report up to date")
            });
            server.join().unwrap();
            let original = error.to_string();
            let message = describe_check_error(channel, error);
            if unavailable {
                let label = if channel == Channel::Alpha {
                    "Alpha"
                } else {
                    "Stable"
                };
                assert_eq!(
                    message,
                    format!("The {label} update feed is unavailable. Please try again later.")
                );
            } else {
                assert_eq!(
                    message, original,
                    "malformed metadata retains its diagnostic"
                );
            }
            let manager = UpdateManager::new("3.0.0-alpha.42", dir.path().join("prefs.json"));
            assert!(manager.begin_check());
            manager.fail(message);
            assert_eq!(manager.snapshot().phase, "error");
            assert!(manager.snapshot().available.is_none());
            assert!(manager.begin_check(), "a failed check must be retryable");
        }
    }

    #[test]
    fn channel_preferences_survive_restart_and_busy_operations_are_serialized() {
        let path =
            std::env::temp_dir().join(format!("cf-update-prefs-{}.json", std::process::id()));
        let manager = UpdateManager::new("3.0.0-alpha.35", path.clone());
        assert_eq!(manager.snapshot().channel, Channel::Alpha);
        manager.set_channel(Channel::Stable).unwrap();
        assert_eq!(
            UpdateManager::new("3.0.0-alpha.35", path.clone())
                .snapshot()
                .channel,
            Channel::Stable
        );
        assert!(manager.begin_check());
        assert!(!manager.begin_check());
        assert!(manager.set_channel(Channel::Alpha).is_err());
        manager.fail("Offline".into());
        assert_eq!(manager.snapshot().phase, "error");
        assert!(manager.begin_check(), "a failed check must be retryable");
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn install_failure_preserves_verified_download_and_pane_admission() {
        let _serial = crate::pty::serial_pty_test();
        let manager =
            UpdateManager::new("3.0.0-alpha.35", PathBuf::from("unused-test-preferences"));
        let panes = std::sync::Arc::new(crate::pty::PaneTable::new());
        assert!(manager
            .install(&panes, |_, _| panic!(
                "must not install without a verified download"
            ))
            .is_err());
        {
            let mut state = manager.state.lock().unwrap();
            state.snapshot.phase = "ready";
            state.snapshot.available = Some(ReleaseInfo {
                version: "3.0.0-alpha.36".into(),
                notes: String::new(),
                date: None,
            });
            state.bytes = Some(b"verified archive".to_vec());
        }
        let result = manager.install(&panes, |bytes, version| {
            assert_eq!(bytes, b"verified archive");
            assert_eq!(version, "3.0.0-alpha.36");
            assert!(panes.begin_update().is_err());
            Err("Disk full".into())
        });
        assert!(result.is_err());
        assert_eq!(manager.snapshot().phase, "ready");
        assert_eq!(manager.snapshot().error.as_deref(), Some("Disk full"));
        assert!(manager.state.lock().unwrap().bytes.is_some());
        assert!(panes.begin_update().is_ok());
        let permit = manager.install(&panes, |_, _| Ok(())).unwrap();
        assert!(
            panes.begin_update().is_err(),
            "successful install retains admission until restart"
        );
        drop(permit);
    }

    #[test]
    fn real_updater_verifies_download_and_rejects_tampering_before_ready() {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        use std::process::{Command, Stdio};
        use tauri_plugin_updater::UpdaterExt;
        let dir = tempfile::tempdir().unwrap();
        let key = dir.path().join("test.key");
        let archive = dir.path().join("archive.tar.gz");
        let bytes = b"signed test archive bytes";
        std::fs::write(&archive, bytes).unwrap();
        let signer = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../node_modules/.bin/tauri");
        assert!(Command::new(&signer)
            .args([
                "signer",
                "generate",
                "--ci",
                "--password",
                "",
                "--write-keys"
            ])
            .arg(&key)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap()
            .success());
        assert!(Command::new(&signer)
            .args(["signer", "sign", "--password", "", "--private-key-path"])
            .arg(&key)
            .arg(&archive)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap()
            .success());
        let public = std::fs::read_to_string(key.with_extension("key.pub")).unwrap();
        let signature = std::fs::read_to_string(archive.with_extension("gz.sig")).unwrap();
        let mut context = tauri::test::mock_context(tauri::test::noop_assets());
        context
            .config_mut()
            .plugins
            .0
            .insert("updater".into(), json!({"pubkey":public}));
        let app = tauri::test::mock_builder()
            .plugin(tauri_plugin_updater::Builder::new().build())
            .build(context)
            .unwrap();
        for tampered in [false, true] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let endpoint = format!("http://{}", listener.local_addr().unwrap());
            let mut doc = release("9.0.0");
            doc["platforms"]["darwin-aarch64"]["url"] = format!("{endpoint}/archive").into();
            doc["platforms"]["darwin-aarch64"]["signature"] = signature.clone().into();
            let body = serde_json::to_vec(&doc).unwrap();
            let server = std::thread::spawn(move || {
                for response in [
                    body,
                    if tampered {
                        b"tampered archive".to_vec()
                    } else {
                        bytes.to_vec()
                    },
                ] {
                    let (mut socket, _) = listener.accept().unwrap();
                    socket
                        .set_read_timeout(Some(std::time::Duration::from_secs(10)))
                        .unwrap();
                    let mut header = Vec::new();
                    while !header.ends_with(b"\r\n\r\n") {
                        let mut byte = [0];
                        socket.read_exact(&mut byte).unwrap();
                        header.push(byte[0]);
                    }
                    write!(socket, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n", response.len()).unwrap();
                    socket.write_all(&response).unwrap();
                }
            });
            tauri::async_runtime::block_on(async {
                let update = app
                    .updater_builder()
                    .target("darwin-aarch64")
                    .endpoints(vec![endpoint.parse().unwrap()])
                    .unwrap()
                    .no_proxy()
                    .executable_path(dir.path().join("Test.app/Contents/MacOS/app"))
                    .build()
                    .unwrap()
                    .check()
                    .await
                    .unwrap()
                    .unwrap();
                let manager = UpdateManager::new("0.1.0", dir.path().join("prefs.json"));
                manager.state.lock().unwrap().pending = Some(update);
                manager.state.lock().unwrap().snapshot.phase = "available";
                let result = manager.download(|_| {}).await;
                if tampered {
                    assert!(result.is_err());
                    assert_eq!(manager.snapshot().phase, "error");
                    assert!(manager.state.lock().unwrap().bytes.is_none());
                } else {
                    result.unwrap();
                    assert_eq!(manager.snapshot().phase, "ready");
                    assert_eq!(
                        manager.state.lock().unwrap().bytes.as_deref(),
                        Some(bytes.as_slice())
                    );
                }
            });
            server.join().unwrap();
        }
    }
}
#[cfg(unix)]
#[test]
fn restart_does_not_wait_forever_for_a_stalled_drain() {
    use std::io::Read;
    use std::os::unix::net::UnixStream;
    let (mut reader, writer) = UnixStream::pair().unwrap();
    let started = std::time::Instant::now();
    assert!(!finish_before_deadline(
        std::time::Duration::from_millis(30),
        move || {
            let _ = reader.read(&mut [0_u8; 1]);
        }
    ));
    assert!(started.elapsed() < std::time::Duration::from_secs(1));
    drop(writer);
    assert!(finish_before_deadline(
        std::time::Duration::from_secs(1),
        || {}
    ));
    assert!(!finish_before_deadline(
        std::time::Duration::from_secs(1),
        || panic!("drain failed")
    ));
}

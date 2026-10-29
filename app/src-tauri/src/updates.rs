use crate::runtime::AppRuntime;
use semver::Version;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tauri_plugin_updater::{RemoteRelease, Update, UpdaterExt};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Channel {
    Alpha,
    Stable,
}

impl Channel {
    /// The feed this channel's apps read: the `latest.json` of a rolling
    /// GitHub release, which the macOS updater and the Windows page both read
    /// here. `app/feeds.json` names the feeds and `cf-publish`
    /// (`tools/cf-publish`) moves them. The apps before the flip release read
    /// `update-alpha` and `update-stable`; those stay pinned to the flip
    /// release, with its assets, for the apps that read them still.
    fn endpoint(self) -> url::Url {
        let tag = if self == Self::Alpha {
            "feed-alpha"
        } else {
            "feed-stable"
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

    pub(crate) fn accepts(self, version: &Version) -> bool {
        version.build.is_empty()
            && (version.pre.is_empty()
                || (self == Self::Alpha && version.pre.as_str().split('.').next() == Some("alpha")))
    }
}

fn current_snapshot<R: Runtime>(app: &AppHandle<R>) -> UpdateSnapshot {
    let mut snapshot = app.state::<UpdateManager>().snapshot();
    snapshot.blockers = match app.try_state::<AppRuntime>() {
        Some(runtime) => match runtime.panes.update_blockers() {
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

/// Whether this platform downloads and installs an update itself. Where it
/// does not (Windows, for now), a check finds the release the feed names and
/// the page it is downloaded from.
const INSTALLS_IN_APP: bool = cfg!(target_os = "macos");

/// What a check found: the release, the signed update to download where the
/// app installs it, and the page to download it from where it does not.
struct Found {
    pending: Option<Update>,
    available: Option<ReleaseInfo>,
    page: Option<String>,
}

#[tauri::command]
pub async fn update_check<R: Runtime>(app: AppHandle<R>) -> Value {
    let manager = app.state::<UpdateManager>();
    if !manager.begin_check() {
        return update_status(app.clone());
    }
    publish(&app);
    let snapshot = manager.snapshot();
    let checked = if INSTALLS_IN_APP {
        check_in_app(&app, &snapshot).await
    } else {
        check_by_page(&snapshot).await
    };
    match checked {
        Ok(found) => manager.found(found),
        Err(error) => manager.fail(format!("Could not check for updates: {error}")),
    }
    publish(&app)
}

/// The signed update the feed offers this platform, checked, for the app to
/// download and install.
async fn check_in_app<R: Runtime>(
    app: &AppHandle<R>,
    snapshot: &UpdateSnapshot,
) -> Result<Found, String> {
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
    let mut pending = updater
        .check()
        .await
        .map_err(|e| describe_check_error(snapshot.channel, e))?;
    let available = pending
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
    if let (Some(endpoint), Some(update)) = (fixture, pending.as_mut()) {
        update.download_url = endpoint.join("archive").map_err(|e| e.to_string())?;
    }
    Ok(Found {
        pending,
        available,
        page: None,
    })
}

/// The release the feed names, checked as any release is, and the page it
/// is downloaded from: the feed holds no archive this platform installs.
async fn check_by_page(snapshot: &UpdateSnapshot) -> Result<Found, String> {
    let document =
        crate::update_page::read_feed(snapshot.channel.endpoint(), snapshot.channel).await?;
    let available =
        crate::update_page::newer_release(&snapshot.current_version, snapshot.channel, &document)?;
    let page = available
        .as_ref()
        .map(|release| crate::update_page::page_of(&release.version));
    Ok(Found {
        pending: None,
        available,
        page,
    })
}

/// Opens the page of the release a check found, where the app does not
/// install it itself.
#[tauri::command]
pub fn update_open_page<R: Runtime>(app: AppHandle<R>) -> Value {
    let Some(page) = app.state::<UpdateManager>().snapshot().page else {
        return command_error(
            &app,
            "Check for updates before opening a download page".into(),
        );
    };
    match crate::update_page::open(&page) {
        Ok(()) => publish(&app),
        Err(error) => command_error(&app, format!("The download page could not open: {error}")),
    }
}

fn describe_check_error(channel: Channel, error: tauri_plugin_updater::Error) -> String {
    if matches!(error, tauri_plugin_updater::Error::ReleaseNotFound) {
        feed_unavailable(channel)
    } else {
        error.to_string()
    }
}

/// What a check says of a channel's feed that is not there.
pub(crate) fn feed_unavailable(channel: Channel) -> String {
    let label = if channel == Channel::Alpha {
        "Alpha"
    } else {
        "Stable"
    };
    format!("The {label} update feed is unavailable. Please try again later.")
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
            let panes = std::sync::Arc::clone(&worker.state::<AppRuntime>().panes);
            let permit = manager.install(&panes, |bytes, version| {
                publish(&worker);
                let directory = manager.preferences.parent().ok_or("Invalid preferences path")?.join("updates");
                crate::update_install::install_archive(&target, bytes, version, &directory)?;
                swapped_in_worker.store(true, std::sync::atomic::Ordering::Release);
                Ok(())
            })?;
            let runtime = worker.state::<AppRuntime>();
            if runtime.begin_shutdown() && !runtime.finish_shutdown() {
                eprintln!("ConsensFlow update installed; internal drain failed or exceeded its five-second deadline. Restarting with no open panes.");
            }
            Ok::<_, String>((permit, target))
        })
        .await;
        match installed {
            Ok(Ok((_permit, target))) => {
                // Admission stays closed through restart. The daemon has exited;
                // a stalled bridge drain cannot keep the old process alive.
                let _ = INSTALLED.set(target);
                restart_swept(|| -> Value { app.restart() })
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
fn restart_swept<T>(restart: impl FnOnce() -> T) -> T {
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
static INSTALLED: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

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
fn relaunch_once(
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
fn relaunch(
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

fn preferences_directory(home: &Path, configured: Option<PathBuf>) -> PathBuf {
    configured
        .unwrap_or_else(|| home.join(".consensflow"))
        .join("app")
}

pub fn setup(app: &tauri::App) -> tauri::Result<()> {
    let directory = preferences_directory(
        &app.path().home_dir()?,
        std::env::var_os("CONSENSFLOW_HOME").map(PathBuf::from),
    );
    let preferences = directory.join("updates.json");
    let legacy = app.path().app_config_dir()?.join("updates.json");
    if std::env::var_os("CONSENSFLOW_HOME").is_none() && !preferences.exists() && legacy.is_file() {
        std::fs::create_dir_all(&directory)?;
        std::fs::copy(legacy, &preferences)?;
    }
    app.manage(UpdateManager::new(
        &app.package_info().version.to_string(),
        preferences,
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
        crate::panics::contained("the app's handler of its menu", || {
            if event.id().as_ref() == "check-updates" {
                let _ = app.emit("check-updates", ());
            }
        });
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
    /// Where an available release is downloaded from by hand, on a platform
    /// whose app does not install it itself; none where it does.
    page: Option<String>,
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
                    page: None,
                },
                pending: None,
                bytes: None,
            }),
            preferences,
        }
    }

    /// The state, also after a panic while it was held: what it holds is whole at every
    /// step, and a poisoned lock must not panic every command of the page after it.
    fn state(&self) -> MutexGuard<'_, UpdateState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn snapshot(&self) -> UpdateSnapshot {
        self.state().snapshot.clone()
    }

    fn set_channel(&self, channel: Channel) -> Result<(), String> {
        let mut state = self.state();
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
        state.snapshot.page = None;
        state.snapshot.error = None;
        state.snapshot.last_checked = None;
        state.snapshot.downloaded_bytes = 0;
        state.snapshot.total_bytes = None;
        state.pending = None;
        state.bytes = None;
        Ok(())
    }

    fn begin_check(&self) -> bool {
        let mut state = self.state();
        if matches!(
            state.snapshot.phase,
            "checking" | "downloading" | "ready" | "installing"
        ) {
            return false;
        }
        state.snapshot.phase = "checking";
        state.snapshot.error = None;
        state.snapshot.available = None;
        state.snapshot.page = None;
        state.pending = None;
        state.bytes = None;
        true
    }

    /// What a check found becomes the state: available, or nothing newer.
    fn found(&self, found: Found) {
        let mut state = self.state();
        state.snapshot.phase = if found.available.is_some() {
            "available"
        } else {
            "idle"
        };
        state.pending = found.pending;
        state.snapshot.available = found.available;
        state.snapshot.page = found.page;
        state.snapshot.last_checked = time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .ok();
    }

    fn fail(&self, error: String) {
        let mut state = self.state();
        state.snapshot.phase = "error";
        state.snapshot.error = Some(error);
    }

    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    fn install(
        &self,
        panes: &std::sync::Arc<cf_panes::pty::PaneTable>,
        action: impl FnOnce(&[u8], &str) -> Result<(), String>,
    ) -> Result<cf_panes::pty::UpdatePermit, String> {
        let (permit, bytes, version) = {
            let mut state = self.state();
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
            let mut state = self.state();
            state.bytes = Some(bytes);
            state.snapshot.phase = "ready";
            state.snapshot.error = Some(error.clone());
            return Err(error);
        }
        Ok(permit)
    }

    async fn download(&self, notify: impl Fn(&UpdateSnapshot)) -> Result<(), String> {
        let mut update = {
            let mut state = self.state();
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
                        let mut state = self.state();
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
                let mut state = self.state();
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
    pub(crate) version: String,
    pub(crate) notes: String,
    pub(crate) date: Option<String>,
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

    /// A panic while the update state was held leaves it poisoned; the page's commands read it on
    /// (on the main thread, where a panic of theirs is the event loop's), and do not panic in turn.
    #[test]
    fn the_update_state_is_read_after_a_panic_that_held_it() {
        let dir = tempfile::tempdir().unwrap();
        let manager = UpdateManager::new("3.0.6", dir.path().join("updates.json"));
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _held = manager.state();
            panic!("while the update state is held");
        }));
        assert!(manager.state.is_poisoned());
        assert_eq!(manager.snapshot().current_version, "3.0.6");
        assert!(manager.set_channel(Channel::Alpha).is_ok());
    }

    #[test]
    fn preferences_use_consensflow_home() {
        let home = PathBuf::from("/users/test");
        assert_eq!(
            preferences_directory(&home, None),
            home.join(".consensflow/app")
        );
        assert_eq!(
            preferences_directory(&home, Some(PathBuf::from("/isolated/state"))),
            PathBuf::from("/isolated/state/app")
        );
    }

    /// What an installed app reads is a promise to it for as long as it is
    /// installed: the two feeds, pinned here, and the same two the release
    /// workflow moves (`app/feeds.json`, which `cf-publish` holds as it was built).
    /// The macOS updater and the Windows page both take their address from
    /// `Channel::endpoint`. The feeds of the apps before the flip release are
    /// not among them.
    #[test]
    fn each_channel_reads_its_own_feed_and_the_manifest_the_workflow_moves_names_the_same() {
        let download = "https://github.com/ngvoicu/consensflow/releases/download";
        assert_eq!(
            Channel::Alpha.endpoint().as_str(),
            format!("{download}/feed-alpha/latest.json")
        );
        assert_eq!(
            Channel::Stable.endpoint().as_str(),
            format!("{download}/feed-stable/latest.json")
        );
        let manifest: Value = serde_json::from_str(include_str!("../../feeds.json")).unwrap();
        for (channel, name) in [(Channel::Alpha, "alpha"), (Channel::Stable, "stable")] {
            let feed = manifest["feeds"][name].as_str().unwrap();
            let legacy = manifest["legacy"][name].as_str().unwrap();
            assert_eq!(
                channel.endpoint().as_str(),
                format!("{download}/{feed}/latest.json"),
                "{name}"
            );
            assert_ne!(feed, legacy, "{name}: the new feed is not the old one");
        }
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
            json!({"pubkey":"unused-for-metadata-check", "dangerousInsecureTransportProtocol":true}),
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
        let _serial = cf_panes::pty::serial_pty_test();
        let manager =
            UpdateManager::new("3.0.0-alpha.35", PathBuf::from("unused-test-preferences"));
        let panes = std::sync::Arc::new(cf_panes::pty::PaneTable::new());
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

    #[cfg(unix)]
    #[test]
    fn real_updater_verifies_download_and_rejects_tampering_before_ready() {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        use std::process::Stdio;
        use tauri_plugin_updater::UpdaterExt;
        let dir = tempfile::tempdir().unwrap();
        let key = dir.path().join("test.key");
        let archive = dir.path().join("archive.tar.gz");
        let bytes = b"signed test archive bytes";
        std::fs::write(&archive, bytes).unwrap();
        let signer = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../node_modules/.bin/tauri");
        assert!(crate::testing::command(&signer)
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
        assert!(crate::testing::command(&signer)
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
        context.config_mut().plugins.0.insert(
            "updater".into(),
            json!({"pubkey":public, "dangerousInsecureTransportProtocol":true}),
        );
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

    /// Tauri's restart starts the new app with a bare `Command`: everything of this app's that
    /// is not close-on-exec as it does would be the new app's for as long as it runs. Here a
    /// descriptor is left open and not close-on-exec, and the restart (a stand-in, which
    /// answers what it found where Tauri's never returns) looks at it. The sweep is for the whole
    /// process, so the test runs in one of its own, its binary run again.
    #[cfg(target_os = "macos")]
    #[test]
    fn the_app_restarts_with_every_descriptor_close_on_exec() {
        const SWEPT: &str = "CONSENSFLOW_TEST_RESTART_SWEPT";
        if std::env::var_os(SWEPT).is_some() {
            use std::os::fd::AsRawFd;

            let stray = std::fs::File::open("/dev/null").expect("a file");
            let fd = stray.as_raw_fd();
            // SAFETY: `fcntl` is asked to clear the flags of a descriptor this test holds.
            assert_eq!(unsafe { libc::fcntl(fd, libc::F_SETFD, 0) }, 0);
            let flags = restart_swept(|| {
                // SAFETY: `fcntl` is asked the flags of a descriptor this test holds.
                unsafe { libc::fcntl(fd, libc::F_GETFD) }
            });
            assert_ne!(flags & libc::FD_CLOEXEC, 0, "left inheritable");
            return;
        }
        let ran = crate::testing::command(std::env::current_exe().expect("the test binary"))
            .args([
                "--exact",
                "updates::tests::the_app_restarts_with_every_descriptor_close_on_exec",
                "--nocapture",
            ])
            .env(SWEPT, "1")
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

    /// The app an update installed starts as this one was started: its environment (an app on a
    /// `CONSENSFLOW_HOME` comes back on it), its input (a self-test's FIFO), its output (where a
    /// self-test reports) and its arguments, and nothing else of this app's: a descriptor left
    /// open and not close-on-exec is not the new app's. Tauri 2.12's restart, through
    /// LaunchServices, gives it `/dev/null` for both streams (T-930). The bundle is a stand-in
    /// whose executable writes what it was given; the relaunch runs in a process of its own, the
    /// test's binary run again, given a home and an input of the test's.
    #[cfg(target_os = "macos")]
    #[test]
    fn an_installed_update_starts_with_this_apps_environment_input_and_arguments() {
        const BUNDLE: &str = "CONSENSFLOW_TEST_RELAUNCH_BUNDLE";
        if let Some(bundle) = std::env::var_os(BUNDLE) {
            use std::os::fd::AsRawFd;

            let stray = std::fs::File::open("/dev/null").expect("a file");
            // SAFETY: `fcntl` is asked to clear the flags of a descriptor this test holds.
            assert_eq!(
                unsafe { libc::fcntl(stray.as_raw_fd(), libc::F_SETFD, 0) },
                0
            );
            let mut started = relaunch(
                Path::new(&bundle),
                [
                    "--first".into(),
                    "second word".into(),
                    stray.as_raw_fd().to_string().into(),
                ],
            )
            .expect("start the installed app");
            assert!(started.wait().expect("the stand-in ends").success());
            return;
        }
        let folder = tempfile::tempdir().expect("a folder");
        let bundle = folder.path().join("Installed.app");
        let programs = bundle.join("Contents/MacOS");
        std::fs::create_dir_all(&programs).expect("the bundle's folders");
        std::fs::write(
            bundle.join("Contents/Info.plist"),
            concat!(
                r#"<?xml version="1.0" encoding="UTF-8"?><plist version="1.0"><dict>"#,
                "<key>CFBundleExecutable</key><string>stand-in</string></dict></plist>"
            ),
        )
        .expect("the Info.plist");
        let said = folder.path().join("said");
        let stand_in = programs.join("stand-in");
        std::fs::write(
            &stand_in,
            format!(
                "#!/bin/sh\nheld='holds none'\nif [ -e \"/dev/fd/$3\" ]; then held='holds the stray'; fi\necho 'the stand-in on its output'\n{{ printf '%s\\n' \"$CONSENSFLOW_HOME\" \"$1\" \"$2\" \"$held\"; cat; }} > '{}.part'\nmv '{0}.part' '{0}'\n",
                said.display()
            ),
        )
        .expect("the stand-in");
        std::fs::set_permissions(
            &stand_in,
            std::os::unix::fs::PermissionsExt::from_mode(0o755),
        )
        .expect("make it runnable");
        let input = folder.path().join("input");
        std::fs::write(&input, "the smoke's input\n").expect("the input");
        let home = folder.path().join("home of its own");
        let ran = crate::testing::command(std::env::current_exe().expect("the test binary"))
            .args([
                "--exact",
                "updates::tests::an_installed_update_starts_with_this_apps_environment_input_and_arguments",
                "--nocapture",
            ])
            .env(BUNDLE, &bundle)
            .env("CONSENSFLOW_HOME", &home)
            .stdin(std::fs::File::open(&input).expect("open the input"))
            .output()
            .expect("run the test in a child");
        let output = format!(
            "{}{}",
            String::from_utf8_lossy(&ran.stdout),
            String::from_utf8_lossy(&ran.stderr)
        );
        assert!(ran.status.success(), "{output}");
        assert!(
            output.contains("1 passed"),
            "the test did not run: {output}"
        );
        assert!(
            output.contains("the stand-in on its output"),
            "the installed app wrote elsewhere: {output}"
        );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while !said.exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let said = std::fs::read_to_string(&said).expect("the stand-in said what it was given");
        assert_eq!(
            said,
            format!(
                "{}\n--first\nsecond word\nholds none\nthe smoke's input\n",
                home.display()
            )
        );
    }

    /// The steps of the event loop's end for an update: `relaunch_once` run contained, as
    /// lib.rs runs it, then Tauri's restart, which starts the app where `exit` was not reached.
    /// A start of ours and Tauri's restart each start the installed app.
    #[cfg(target_os = "macos")]
    fn ended(
        installed: Option<&Path>,
        cleanup_panics: bool,
        started: Result<(), String>,
    ) -> Vec<&'static str> {
        let steps = std::cell::RefCell::new(Vec::new());
        let exited = std::cell::Cell::new(false);
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            relaunch_once(
                installed,
                || {
                    steps.borrow_mut().push("cleanup");
                    assert!(!cleanup_panics, "Tauri's cleanup panics");
                },
                |_| {
                    steps.borrow_mut().push("start");
                    started
                },
                || {
                    steps.borrow_mut().push("exit");
                    exited.set(true);
                },
            );
        }));
        if !exited.get() {
            steps.borrow_mut().push("Tauri's restart");
        }
        steps.into_inner()
    }

    /// The installed app is started once (T-941): where nothing was installed nothing of the
    /// app's runs and Tauri's restart is left as it is; a start is followed by the exit alone,
    /// even where Tauri's cleanup panicked; a start that fails leaves the start to Tauri.
    #[cfg(target_os = "macos")]
    #[test]
    fn the_installed_app_is_started_once_whatever_the_cleanup_and_the_start_do() {
        let app = Path::new("/Applications/ConsensFlow.app");
        assert_eq!(ended(None, false, Ok(())), ["Tauri's restart"]);
        assert_eq!(
            ended(Some(app), false, Ok(())),
            ["cleanup", "start", "exit"]
        );
        assert_eq!(ended(Some(app), true, Ok(())), ["cleanup", "start", "exit"]);
        assert_eq!(
            ended(Some(app), false, Err("no executable".into())),
            ["cleanup", "start", "Tauri's restart"]
        );
    }
}

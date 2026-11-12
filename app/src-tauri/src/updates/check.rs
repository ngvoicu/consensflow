use super::{Channel, UpdateSnapshot};
use semver::Version;
use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Runtime};
use tauri_plugin_updater::{RemoteRelease, Update, UpdaterExt};

/// What a check found: the release, the signed update to download where the
/// app installs it, and the page to download it from where it does not.
pub(super) struct Found {
    pub(super) pending: Option<Update>,
    pub(super) available: Option<ReleaseInfo>,
    pub(super) page: Option<String>,
}

/// The signed update the feed offers this platform, checked, for the app to
/// download and install.
#[allow(clippy::disallowed_methods)] // Reads the packaged smoke's CONSENSFLOW_SELFTEST_UPDATER_* variables: older than cf_base::env::Env.
pub(super) async fn check_in_app<R: Runtime>(
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
pub(super) async fn check_by_page(snapshot: &UpdateSnapshot) -> Result<Found, String> {
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

pub(super) fn describe_check_error(channel: Channel, error: tauri_plugin_updater::Error) -> String {
    if matches!(error, tauri_plugin_updater::Error::ReleaseNotFound) {
        feed_unavailable(channel)
    } else {
        error.to_string()
    }
}

/// What a check says of a channel's feed that is not there.
pub(crate) fn feed_unavailable(channel: Channel) -> String {
    format!(
        "The {} update feed is unavailable. Please try again later.",
        channel.label()
    )
}

pub(super) fn test_update_url(
    enabled: bool,
    address: Option<&str>,
) -> Result<Option<url::Url>, String> {
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

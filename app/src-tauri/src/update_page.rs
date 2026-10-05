//! Where the app does not install an update itself (Windows, for now): the
//! release the update feed names, checked as the updater checks one, and the
//! page it is downloaded from, opened in the browser. The page is made here
//! from the version checked; nothing the feed says names it.

use std::process::Command;
use std::time::Duration;

use semver::Version;
use serde_json::Value;

use crate::updates::{feed_unavailable, Channel, ReleaseInfo};

/// The most a feed may hold: one release's metadata, its notes bounded.
const FEED_MAX_BYTES: usize = 1024 * 1024;

/// The feed at `endpoint`, read over HTTPS within 20 seconds. One that is
/// not there is said as the updater says it.
pub async fn read_feed(endpoint: url::Url, channel: Channel) -> Result<Value, String> {
    let client = reqwest::Client::builder()
        .https_only(true)
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|e| e.to_string())?;
    let response = client
        .get(endpoint)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Err(feed_unavailable(channel));
    }
    let body = response.bytes().await.map_err(|e| e.to_string())?;
    if body.len() > FEED_MAX_BYTES {
        return Err("The update feed is larger than a release's metadata".into());
    }
    serde_json::from_slice(&body).map_err(|e| e.to_string())
}

/// The release `document` names, if it is newer than `current`, checked as
/// the updater checks one (a canonical version the channel takes, a
/// publication date, bounded notes), less the archive this platform has
/// none of.
pub fn newer_release(
    current: &str,
    channel: Channel,
    document: &Value,
) -> Result<Option<ReleaseInfo>, String> {
    let named = document["version"]
        .as_str()
        .ok_or("The release names no version")?;
    let version = Version::parse(named).map_err(|e| e.to_string())?;
    let current = Version::parse(current).map_err(|e| e.to_string())?;
    if version <= current {
        return Ok(None);
    }
    if !channel.accepts(&version) {
        return Err("This release is not newer or does not belong to the selected channel".into());
    }
    if version.to_string() != named {
        return Err("The release version must be a canonical semantic version".into());
    }
    let notes = document["notes"].as_str().unwrap_or_default();
    let date = document["pub_date"].as_str();
    if notes.len() > 64 * 1024 || date.is_none() {
        return Err("The release needs a publication date and bounded release notes".into());
    }
    Ok(Some(ReleaseInfo {
        version: named.to_owned(),
        notes: notes.to_owned(),
        date: date.map(str::to_owned),
    }))
}

/// The page a release is downloaded from by hand: its installer and its
/// portable exe.
pub fn page_of(version: &str) -> String {
    format!("https://github.com/ngvoicu/consensflow/releases/tag/v{version}")
}

/// Opens `page` in the browser.
pub fn open(page: &str) -> Result<(), String> {
    let opener = if cfg!(windows) {
        "explorer.exe"
    } else if cfg!(target_os = "macos") {
        "/usr/bin/open"
    } else {
        "xdg-open"
    };
    // Explorer answers 1 even when the page opened: only a start that
    // failed is a failure.
    Command::new(opener)
        .arg(page)
        .status()
        .map(drop)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A feed as the releases write it: the Mac's archive alone among its platforms.
    fn feed(version: &str) -> Value {
        json!({"version": version, "notes": "The terminals take half the window.",
            "pub_date": "2026-10-05T18:00:00Z",
            "platforms": {"darwin-aarch64": {
                "url": format!("https://github.com/ngvoicu/consensflow/releases/download/v{version}/ConsensFlow_{version}_aarch64.app.tar.gz"),
                "signature": "test-signature"}}})
    }

    #[test]
    fn a_newer_release_of_the_channel_is_offered_with_its_page_though_the_feed_has_no_windows_archive(
    ) {
        let found = newer_release("3.0.0-alpha.76", Channel::Alpha, &feed("3.0.0-alpha.77"))
            .unwrap()
            .expect("a newer release");
        assert_eq!(found.version, "3.0.0-alpha.77");
        assert_eq!(found.notes, "The terminals take half the window.");
        assert_eq!(found.date.as_deref(), Some("2026-10-05T18:00:00Z"));
        assert_eq!(
            page_of(&found.version),
            "https://github.com/ngvoicu/consensflow/releases/tag/v3.0.0-alpha.77"
        );
    }

    #[test]
    fn the_same_release_or_an_older_one_is_no_update() {
        for version in ["3.0.0-alpha.76", "3.0.0-alpha.75", "2.9.0"] {
            assert!(
                newer_release("3.0.0-alpha.76", Channel::Alpha, &feed(version))
                    .unwrap()
                    .is_none(),
                "{version}"
            );
        }
    }

    #[test]
    fn a_release_the_channel_does_not_take_or_without_its_date_or_with_unbounded_notes_is_refused()
    {
        assert!(newer_release("3.0.0", Channel::Stable, &feed("3.0.1-alpha.1")).is_err());
        assert!(newer_release("3.0.0-alpha.76", Channel::Alpha, &feed("3.0.0-beta.1")).is_err());
        let mut undated = feed("3.0.0-alpha.77");
        undated.as_object_mut().unwrap().remove("pub_date");
        assert!(newer_release("3.0.0-alpha.76", Channel::Alpha, &undated).is_err());
        let mut long = feed("3.0.0-alpha.77");
        long["notes"] = "x".repeat(64 * 1024 + 1).into();
        assert!(newer_release("3.0.0-alpha.76", Channel::Alpha, &long).is_err());
        for version in [json!("v3.0.0-alpha.77"), json!(77), Value::Null] {
            let mut named = feed("3.0.0-alpha.77");
            named["version"] = version.clone();
            assert!(
                newer_release("3.0.0-alpha.76", Channel::Alpha, &named).is_err(),
                "{version}"
            );
        }
    }
}

//! The notice of the first launch after an update on a Mac (T-2105): macOS keeps a Local Network
//! decision for an app, and a Mac may need it allowed again after an update although the switch
//! still shows on. The notice says so once for each version, with a button to the right page of
//! the system's settings. A first install has nothing to restore and shows none.

use serde_json::{json, Value};
use std::path::Path;
use std::sync::Mutex;
use tauri::{AppHandle, Manager, Runtime};

/// The page of Local Network in the system's settings, where the system's link lands on it.
const LOCAL_NETWORK_PAGE: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_LocalNetwork";
/// Privacy & Security, where it does not: macOS 27 lands the link above on the parent page.
const PRIVACY_PAGE: &str = "x-apple.systempreferences:com.apple.preference.security";
/// The first macOS whose settings do not open Local Network by the link.
const FIRST_MAJOR_WITHOUT_THE_LINK: u32 = 27;

/// Where the button goes, by the macOS version.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Target {
    pub url: &'static str,
    pub button: &'static str,
    /// The way to the switch by hand, said where the button does not land on it.
    pub path: Option<&'static str>,
}

pub(crate) fn target_for(major: Option<u32>) -> Target {
    match major {
        Some(major) if major < FIRST_MAJOR_WITHOUT_THE_LINK => Target {
            url: LOCAL_NETWORK_PAGE,
            button: "Open Local Network settings",
            path: None,
        },
        _ => Target {
            url: PRIVACY_PAGE,
            button: "Open Privacy & Security",
            path: Some("Privacy & Security, then Local Network"),
        },
    }
}

/// The major version of a `sw_vers -productVersion` answer.
pub(crate) fn major_of(version: &str) -> Option<u32> {
    version.trim().split('.').next()?.parse().ok()
}

/// What the file says of the version last run, and whether this launch is the first after an
/// update: a file naming another version. No file is a first install; the app an update replaces
/// marks the file (`mark_updated`), so an update from a version that wrote none still shows it.
pub(crate) fn after_update(seen: Option<&str>, current: &str) -> bool {
    seen.is_some_and(|seen| seen != current)
}

/// The notice for this launch, and `current` recorded as the version last run.
pub(crate) fn record(file: &Path, current: &str) -> bool {
    let seen = std::fs::read(file)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .and_then(|value| value["seen"].as_str().map(str::to_owned));
    let show = after_update(seen.as_deref(), current);
    write(file, current);
    show
}

/// The app an update installed has not run yet: its first launch shows the notice.
pub(crate) fn mark_updated(file: &Path) {
    write(file, "");
}

fn write(file: &Path, seen: &str) {
    let written = file
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| std::fs::write(file, json!({ "seen": seen }).to_string()));
    if let Err(error) = written {
        eprintln!("ConsensFlow could not write {}: {error}", file.display());
    }
}

/// The notice of this launch, as the page asks for it.
pub(crate) struct Notice(Mutex<bool>);

impl Notice {
    pub(crate) fn new(show: bool) -> Self {
        Self(Mutex::new(show))
    }
}

#[cfg(target_os = "macos")]
#[allow(clippy::disallowed_methods)] // The system's own tool for its version, run once by the notice.
fn system_major() -> Option<u32> {
    let answer = std::process::Command::new("/usr/bin/sw_vers")
        .arg("-productVersion")
        .output()
        .ok()?;
    major_of(&String::from_utf8_lossy(&answer.stdout))
}

#[cfg(not(target_os = "macos"))]
fn system_major() -> Option<u32> {
    None
}

/// What the page shows: nothing, or the notice with its button and the way to the switch.
pub(crate) fn answer(show: bool, major: Option<u32>) -> Value {
    if !show {
        return json!({"ok": true, "show": false});
    }
    let target = target_for(major);
    json!({"ok": true, "show": true, "button": target.button, "path": target.path})
}

#[tauri::command]
pub fn local_network_notice<R: Runtime>(app: AppHandle<R>) -> Value {
    let show = cfg!(target_os = "macos")
        && app.try_state::<Notice>().is_some_and(|notice| {
            std::mem::take(&mut *notice.0.lock().unwrap_or_else(|e| e.into_inner()))
        });
    answer(show, system_major())
}

#[tauri::command]
pub fn local_network_notice_open() -> Value {
    match crate::update_page::open(target_for(system_major()).url) {
        Ok(()) => json!({"ok": true}),
        Err(error) => {
            json!({"ok": false, "error": format!("The settings could not open: {error}")})
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The button's target on each macOS version: the link to Local Network where it lands there
    /// (15 and 26), Privacy & Security with the way named on 27 and later, and where the version
    /// is not known (the safe page, whose way is named).
    #[test]
    fn the_button_goes_to_the_page_the_macos_version_opens() {
        for major in [15, 26] {
            let target = target_for(Some(major));
            assert_eq!(
                target.url,
                "x-apple.systempreferences:com.apple.preference.security?Privacy_LocalNetwork"
            );
            assert_eq!(target.button, "Open Local Network settings");
            assert_eq!(target.path, None);
        }
        for major in [Some(27), Some(28), None] {
            let target = target_for(major);
            assert_eq!(
                target.url,
                "x-apple.systempreferences:com.apple.preference.security"
            );
            assert_eq!(target.button, "Open Privacy & Security");
            assert_eq!(target.path, Some("Privacy & Security, then Local Network"));
        }
        assert_eq!(major_of("27.0.1\n"), Some(27));
        assert_eq!(major_of("15.4"), Some(15));
        assert_eq!(major_of("beta"), None);
    }

    /// Shown once after an update, never at a first install, never twice for a version.
    #[test]
    fn the_notice_is_for_the_first_launch_after_an_update_only() {
        let directory = tempfile::tempdir().expect("a folder");
        let file = directory.path().join("app").join("local-network.json");
        assert!(!record(&file, "3.0.7-alpha.15"), "shown at a first install");
        assert!(!record(&file, "3.0.7-alpha.15"), "shown at a second launch");
        assert!(record(&file, "3.0.7-alpha.16"), "not shown after an update");
        assert!(
            !record(&file, "3.0.7-alpha.16"),
            "shown twice for a version"
        );
        // An update from a version that wrote no file: the installing app marks it.
        let unmarked = directory.path().join("other.json");
        mark_updated(&unmarked);
        assert!(
            record(&unmarked, "3.0.7-alpha.16"),
            "an in-app update is not shown"
        );
        assert!(!record(&unmarked, "3.0.7-alpha.16"));
    }

    /// What the page is told: nothing without the notice, the button and the way with it.
    #[test]
    fn the_page_is_told_the_button_and_the_way_only_with_the_notice() {
        assert_eq!(answer(false, Some(26)), json!({"ok": true, "show": false}));
        assert_eq!(
            answer(true, Some(27)),
            json!({"ok": true, "show": true, "button": "Open Privacy & Security", "path": "Privacy & Security, then Local Network"})
        );
        assert_eq!(
            answer(true, Some(26)),
            json!({"ok": true, "show": true, "button": "Open Local Network settings", "path": null})
        );
    }
}

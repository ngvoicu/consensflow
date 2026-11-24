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

/// Local Network privacy came with macOS 15 (Apple's TN3179, "Platform support": macOS 15).
const FIRST_MAJOR_WITH_THE_SETTING: u32 = 15;
/// The list's add (+) and remove (-) buttons: "These buttons were introduced in macOS 27.2.
/// On earlier versions of macOS there's no way to reset your program's Local Network
/// privilege" (TN3179, "Reset local network state").
const FIRST_VERSION_WITH_REMOVE_AND_ADD: (u32, u32) = (27, 2);

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

/// The major and minor version of a `sw_vers -productVersion` answer (`27.2.1` is 27 and 2).
#[cfg(any(target_os = "macos", test))]
pub(crate) fn version_of(version: &str) -> Option<(u32, u32)> {
    let mut parts = version.trim().split('.');
    let major = parts.next()?.parse().ok()?;
    Some((
        major,
        parts.next().map_or(Some(0), |minor| minor.parse().ok())?,
    ))
}

/// Whether this launch is the first after an update: the file names another version. No file is
/// an update where the home shows an earlier run (the builds before this one wrote none: an
/// in-app update by them, or an app replaced by hand), and a first install where it does not.
pub(crate) fn after_update(seen: Option<&str>, current: &str, earlier_run: bool) -> bool {
    seen.map_or(earlier_run, |seen| seen != current)
}

/// The notice for this launch, and `current` recorded as the version last run.
pub(crate) fn record(file: &Path, current: &str, earlier_run: bool) -> bool {
    let seen = std::fs::read(file)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .and_then(|value| value["seen"].as_str().map(str::to_owned));
    let show = after_update(seen.as_deref(), current, earlier_run);
    write(file, current);
    show
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
fn system_version() -> Option<(u32, u32)> {
    let answer = std::process::Command::new("/usr/bin/sw_vers")
        .arg("-productVersion")
        .output()
        .ok()?;
    version_of(&String::from_utf8_lossy(&answer.stdout))
}

#[cfg(not(target_os = "macos"))]
fn system_version() -> Option<(u32, u32)> {
    None
}

/// What the page shows: nothing, or the notice with its button and the way to the switch.
pub(crate) fn answer(show: bool, version: Option<(u32, u32)>) -> Value {
    if !show {
        return json!({"ok": true, "show": false});
    }
    if version.is_some_and(|(major, _)| major < FIRST_MAJOR_WITH_THE_SETTING) {
        return json!({"ok": true, "show": false});
    }
    let target = target_for(version.map(|(major, _)| major));
    json!({
        "ok": true,
        "show": true,
        "button": target.button,
        "path": target.path,
        "removeAndAdd": version.is_some_and(|version| version >= FIRST_VERSION_WITH_REMOVE_AND_ADD),
    })
}

/// Whether ConsensFlow's home shows an earlier run.
///
/// The ledger the daemon keeps and the updates file are what an earlier run leaves; the app's
/// folder and its error log are made by every launch, this one's included, and are not.
pub(crate) fn earlier_run(home: &Path) -> bool {
    home.join("consensflow.db").exists() || home.join("app").join("updates.json").exists()
}

#[tauri::command]
pub fn local_network_notice<R: Runtime>(app: AppHandle<R>) -> Value {
    let show = cfg!(target_os = "macos")
        && app.try_state::<Notice>().is_some_and(|notice| {
            std::mem::take(&mut *notice.0.lock().unwrap_or_else(|e| e.into_inner()))
        });
    answer(show, system_version())
}

#[tauri::command]
pub fn local_network_notice_open() -> Value {
    if !cfg!(target_os = "macos") {
        return json!({"ok": false, "error": "The Local Network settings are a Mac's"});
    }
    match crate::update_page::open(target_for(system_version().map(|(major, _)| major)).url) {
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
    }

    /// A first install says nothing; the first launch after an update says it once. The
    /// builds before this one wrote no marker (alpha.15's installer, in-app or by hand, leaves
    /// none), so a missing marker is an update where the home shows an earlier run (T-2114).
    #[test]
    fn the_notice_is_for_the_first_launch_after_an_update_only() {
        let directory = tempfile::tempdir().expect("a folder");
        let file = directory.path().join("app").join("local-network.json");
        assert!(
            !record(&file, "3.0.7-alpha.15", false),
            "shown at a first install"
        );
        assert!(
            !record(&file, "3.0.7-alpha.15", false),
            "shown at a second launch"
        );
        assert!(
            record(&file, "3.0.7-alpha.16", false),
            "not shown after an update"
        );
        assert!(
            !record(&file, "3.0.7-alpha.16", false),
            "shown twice for a version"
        );
    }

    /// The upgrade from alpha.15's shape: no marker, a home that was used. In-app and by hand
    /// are the same file state, and the second launch says nothing.
    #[test]
    fn an_upgrade_from_a_build_that_wrote_no_marker_shows_the_notice_once() {
        let directory = tempfile::tempdir().expect("a folder");
        let home = directory.path();
        // What the app itself makes at a first launch: its folder and error log. No earlier run.
        std::fs::create_dir_all(home.join("app")).expect("the app folder");
        std::fs::write(home.join("app").join("app.log"), "").expect("the log");
        assert!(!earlier_run(home), "a first launch's own folder is a run");
        // A used home: the ledger the daemon keeps, or the updates file.
        std::fs::write(home.join("consensflow.db"), "").expect("the ledger");
        assert!(earlier_run(home));
        let marker = home.join("app").join("local-network.json");
        assert!(
            record(&marker, "3.0.7-alpha.16", earlier_run(home)),
            "in-app upgrade"
        );
        assert!(
            !record(&marker, "3.0.7-alpha.16", earlier_run(home)),
            "shown twice"
        );
        let by_hand = tempfile::tempdir().expect("a folder");
        std::fs::create_dir_all(by_hand.path().join("app")).expect("the app folder");
        std::fs::write(by_hand.path().join("app").join("updates.json"), "{}").expect("updates");
        assert!(earlier_run(by_hand.path()));
        let marker = by_hand.path().join("app").join("local-network.json");
        assert!(
            record(&marker, "3.0.7-alpha.16", earlier_run(by_hand.path())),
            "manual upgrade"
        );
    }

    /// Where the setting exists: Local Network privacy came with macOS 15, and its add and
    /// remove buttons with 27.2 (Apple's TN3179: "These buttons were introduced in macOS 27.2.
    /// On earlier versions of macOS there's no way to reset your program's Local Network
    /// privilege"). Below 15 no notice; the remove-and-add line from 27.2 only.
    #[test]
    fn the_page_is_told_the_notice_and_its_lines_only_where_macos_has_them() {
        assert_eq!(version_of("27.2.1\n"), Some((27, 2)));
        assert_eq!(version_of("15"), Some((15, 0)));
        assert_eq!(version_of("beta"), None);
        assert_eq!(
            answer(false, Some((26, 0))),
            json!({"ok": true, "show": false})
        );
        for version in [(13, 5), (14, 8)] {
            assert_eq!(
                answer(true, Some(version)),
                json!({"ok": true, "show": false}),
                "{version:?}"
            );
        }
        let shown = |version| answer(true, version);
        assert_eq!(
            shown(Some((26, 4))),
            json!({"ok": true, "show": true, "button": "Open Local Network settings", "path": null, "removeAndAdd": false})
        );
        assert_eq!(
            shown(Some((27, 1))),
            json!({"ok": true, "show": true, "button": "Open Privacy & Security", "path": "Privacy & Security, then Local Network", "removeAndAdd": false})
        );
        assert_eq!(shown(Some((27, 2)))["removeAndAdd"], json!(true));
        assert_eq!(shown(Some((28, 0)))["removeAndAdd"], json!(true));
        // A version that cannot be read is told as the cautious one: the page that is surely
        // there, and no instruction that may name a missing button.
        assert_eq!(shown(None)["removeAndAdd"], json!(false));
        assert_eq!(shown(None)["button"], json!("Open Privacy & Security"));
    }
}

//! Where the app does not install an update itself (Windows, for now): the
//! release the update feed names, checked as the updater checks one, and the
//! page it is downloaded from, opened in the browser. The page is made here
//! from the version checked; nothing the feed says names it.

use std::path::Path;
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
    let response = client()?
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

/// The client the feed is read with: HTTPS only, and a call that takes no more than 20 seconds.
/// It is built on the process's TLS provider, which the app installs at its start (`run`); it is
/// installed here too, where nothing has installed it, so that this client is built whatever
/// reaches it first: reqwest panics at the build of a client where the process has none, and the
/// updater's plugin, which installs ring before its own client, is not on this path (T-1247).
fn client() -> Result<reqwest::Client, String> {
    crate::tls::install_ring_as_default();
    reqwest::Client::builder()
        .https_only(true)
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|e| e.to_string())
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

/// Opens `page` in the browser, and says if it could not.
///
/// On Windows it is `ShellExecuteW`, as a link a program printed in a pane is (`link::shell`):
/// `explorer.exe` read its command line for its own fields (commas and equals signs, a `/root`),
/// answered 1 whether or not a page opened, and its answer was dropped (the review of T-1307,
/// T-1314). A test build opens nothing: a page opened is a page in the browser of the machine
/// the test runs on, so on Windows the shell is a stand-in the test supplies (`link::shell`
/// panics where there is none), and elsewhere the opener is `open_with`'s, which a test of the
/// opener starts itself.
pub fn open(page: &str) -> Result<(), String> {
    #[cfg(windows)]
    {
        crate::link::shell::open(page)
    }
    #[cfg(all(not(windows), test))]
    {
        Err(format!("a test build opens no page: {page}"))
    }
    #[cfg(all(not(windows), not(test)))]
    open_system(page)
}

/// Opens `page` with the system's opener.
#[cfg(all(not(windows), not(test)))]
fn open_system(page: &str) -> Result<(), String> {
    let opener = if cfg!(target_os = "macos") {
        "/usr/bin/open"
    } else {
        "xdg-open"
    };
    open_with(Path::new(opener), page)
}

/// Opens `page` with `opener`, given the page as its one argument.
#[cfg_attr(any(test, windows), allow(dead_code))]
pub(crate) fn open_with(opener: &Path, page: &str) -> Result<(), String> {
    let mut command = Command::new(opener);
    // Started while the app's other threads open sockets and pipes.
    cf_process::hold_only_the_streams(&mut command);
    command
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

    /// The feed's client is made in a process that has done nothing else with TLS, as the app's is
    /// on Windows, where no check goes through the updater's plugin (the Mac's does, and the plugin
    /// installs ring as the process's provider before it builds its own client). The provider is
    /// the process's, and other tests of this binary run the plugin's check, so this test runs in
    /// a process of its own, its binary run again.
    #[test]
    fn the_feeds_client_is_built_in_a_process_that_has_installed_no_tls_provider() {
        const CHILD: &str = "CONSENSFLOW_TEST_FEED_CLIENT";
        if std::env::var_os(CHILD).is_some() {
            assert!(
                rustls::crypto::CryptoProvider::get_default().is_none(),
                "the child starts with no TLS provider installed"
            );
            client().expect("the feed's client");
            return;
        }
        let ran = crate::testing::in_a_child(
            "update_page::tests::the_feeds_client_is_built_in_a_process_that_has_installed_no_tls_provider",
            CHILD,
        );
        assert!(ran.status.success(), "{}", crate::testing::said(&ran));
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

    /// A test build reaches no opener of the machine (T-1315, the review of T-1314): with nothing
    /// standing in for it the download page opens nothing and says so.
    #[cfg(not(windows))]
    #[test]
    fn a_test_build_opens_no_download_page() {
        assert_eq!(
            open("https://github.com/ngvoicu/consensflow/releases/tag/v3.0.7"),
            Err(
                "a test build opens no page: https://github.com/ngvoicu/consensflow/releases/tag/v3.0.7"
                    .to_owned()
            )
        );
    }

    /// What the stand-in shell was asked by the download page, the file of each call.
    #[cfg(windows)]
    static ASKED_OF_THE_SHELL: std::sync::Mutex<Vec<Vec<u16>>> = std::sync::Mutex::new(Vec::new());

    #[cfg(windows)]
    fn shell_starts_it(call: &crate::link::shell::Call) -> isize {
        ASKED_OF_THE_SHELL
            .lock()
            .expect("the record")
            .push(call.file.clone());
        42
    }

    #[cfg(windows)]
    fn shell_finds_no_program(_: &crate::link::shell::Call) -> isize {
        31
    }

    /// On Windows the download page opens as a link in a pane does, through `ShellExecuteW` with
    /// the page as the file, and the shell's answer is the opener's (Explorer answered 1 either
    /// way and its answer was dropped). The call is the test build's stand-in.
    #[cfg(windows)]
    #[test]
    fn on_windows_the_download_page_opens_through_the_shell_and_its_answer_is_read() {
        use crate::link::shell;

        let _turn = shell::TURN
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let page = page_of("3.0.7");
        ASKED_OF_THE_SHELL.lock().expect("the record").clear();
        *shell::TEST_SHELL.lock().expect("the stand-in") = Some(shell_starts_it);
        let started = open(&page);
        *shell::TEST_SHELL.lock().expect("the stand-in") = Some(shell_finds_no_program);
        let refused = open(&page);
        *shell::TEST_SHELL.lock().expect("the stand-in") = None;
        assert_eq!(started, Ok(()));
        assert_eq!(
            *ASKED_OF_THE_SHELL.lock().expect("the record"),
            [shell::wide(&page)],
            "the shell was asked once, for the page"
        );
        assert_eq!(
            refused,
            Err(
                "Windows could not open the address: no program is set to open web addresses \
                 (ShellExecute answered 31)"
                    .to_owned()
            )
        );
    }

    /// The page is opened while the app's other threads open sockets and pipes: here a
    /// descriptor is left open and not close-on-exec as the opener starts, and the opener says
    /// whether it has it, and which page it was given.
    #[cfg(target_os = "macos")]
    #[test]
    fn the_opener_holds_nothing_of_the_apps_but_its_streams() {
        use std::os::fd::AsRawFd;
        use std::os::unix::fs::PermissionsExt;

        let folder = tempfile::tempdir().expect("a folder");
        let said = folder.path().join("said");
        let stray = std::fs::File::open("/dev/null").expect("a file");
        let fd = stray.as_raw_fd();
        // SAFETY: `fcntl` is asked to clear the flags of a descriptor this test holds.
        assert_eq!(unsafe { libc::fcntl(fd, libc::F_SETFD, 0) }, 0);
        let opener = folder.path().join("open");
        std::fs::write(
            &opener,
            format!(
                "#!/bin/sh\nif [ -e /dev/fd/{fd} ]; then held=held; else held=clear; fi; echo \"$held $1\" > '{}'\n",
                said.display()
            ),
        )
        .expect("the opener");
        std::fs::set_permissions(&opener, std::fs::Permissions::from_mode(0o755))
            .expect("an executable");
        let page = page_of("3.0.6");
        // The opener was written a moment ago, and a program started on another thread
        // meanwhile can hold it open for writing: such a start is refused, and tried again.
        let opened = (0..50)
            .map(|_| open_with(&opener, &page))
            .find(|opened| {
                let busy = matches!(opened, Err(refused) if refused.contains("Text file busy"));
                if busy {
                    std::thread::sleep(Duration::from_millis(100));
                }
                !busy
            })
            .expect("the opener started");
        assert_eq!(opened, Ok(()));
        assert_eq!(
            std::fs::read_to_string(&said).expect("what it said"),
            format!("clear {page}\n")
        );
    }
}

//! Before a later release moves a feed: the old feeds serve the bridge, with
//! its files, read from GitHub as the simulator has it.

// A failure in a test is the test's answer, in its helpers too.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use cf_publish::checks::check_prerequisites;
use cf_publish::testing::worlds::{crossed, feed, serving, Crossed};
use cf_publish::testing::{
    archive_name as archive, download, installer_name as installer, latest_json,
    portable_name as portable, published_assets, quick, rule, v, with_archive_url, Files, Github,
    Served, Spec, BRIDGE, LATER, OLDER,
};
#[cfg(unix)]
use cf_publish::testing::{run_binary_on, TempDir};

/// What `check_prerequisites` finds for `version`, once `change` has altered GitHub.
fn found_for(change: impl FnOnce(&Github, &Files), version: &str) -> Vec<String> {
    let Crossed { github, files } = crossed();
    change(&github, &files);
    check_prerequisites(&v(version), github.base(), &rule(), quick())
}

fn found(change: impl FnOnce(&Github, &Files)) -> Vec<String> {
    found_for(change, LATER)
}

/// The old feed serving `version`'s latest.json.
fn old_feed_serves(github: &Github, version: &str) {
    serving(github, "update-alpha", version);
}

#[test]
fn passes_once_the_old_feed_serves_the_bridge_and_its_files_download_as_published() {
    assert_eq!(found(|_, _| {}), Vec::<String>::new());
}

#[test]
fn refuses_where_the_old_feed_still_serves_the_release_before_the_bridge() {
    // The bridge's tag is in the repository, and its release failed before it moved update-alpha.
    assert_eq!(
        found(|github, _| old_feed_serves(github, OLDER)),
        ["update-alpha serves 3.0.0-alpha.80, not the bridge 3.0.0-alpha.81: the apps that read it do not reach the bridge"]
    );
}

#[test]
fn refuses_where_the_old_feed_names_some_other_release_than_the_bridge_an_earlier_node_free_one_among_them(
) {
    let problems = found(|github, _| old_feed_serves(github, "3.0.0-alpha.78"));
    assert!(
        problems[0].starts_with("update-alpha serves 3.0.0-alpha.78, not the bridge"),
        "{problems:?}"
    );
    let newer = found(|github, _| old_feed_serves(github, "3.0.0-alpha.85"));
    assert!(
        newer[0].starts_with("update-alpha serves 3.0.0-alpha.85, not the bridge"),
        "{newer:?}"
    );
}

#[test]
fn refuses_a_feed_that_names_the_bridge_but_is_not_its_latest_json_byte_for_byte() {
    let problems = found(|github, files| {
        feed(
            github,
            "update-alpha",
            files
                .text("latest.json")
                .replacen("notes", "other notes", 1),
        );
    });
    assert_eq!(
        problems,
        ["update-alpha names the bridge 3.0.0-alpha.81, but is not the bridge's own latest.json byte for byte"]
    );
}

#[test]
fn refuses_a_feed_that_is_not_there_a_required_feed_is_not_absent() {
    let problems =
        found(|github, _| github.serve("/update-alpha/latest.json", Served::Status(404)));
    assert_eq!(
        problems,
        ["update-alpha cannot be read (HTTP 404): the apps before the bridge read it"]
    );
}

#[test]
fn refuses_a_feed_that_fails_to_load_whichever_way_it_fails() {
    for (instead, says) in [
        (
            Served::Status(503),
            "update-alpha cannot be read (HTTP 503)",
        ),
        (
            Served::Status(500),
            "update-alpha cannot be read (HTTP 500)",
        ),
        (Served::Reset, "update-alpha cannot be read (unreachable: "),
    ] {
        let problems =
            found(|github, _| github.serve("/update-alpha/latest.json", instead.clone()));
        assert_eq!(problems.len(), 1, "{instead:?}: {problems:?}");
        assert!(problems[0].starts_with(says), "{problems:?}");
    }
}

#[test]
fn refuses_a_feed_that_serves_something_that_is_not_a_latest_json() {
    let problems = found(|github, _| feed(github, "update-alpha", "<html>an error page</html>"));
    assert_eq!(
        problems,
        ["update-alpha serves something that is not a latest.json"]
    );
}

#[test]
fn refuses_a_bridge_archive_that_serves_junk_though_its_address_answers() {
    let problems = found(|github, _| {
        github.serve(&download(BRIDGE, &archive(BRIDGE)), "junk, not the archive");
    });
    assert_eq!(
        problems,
        [format!(
            "the bridge's {} does not download as the one its SHA256SUMS lists",
            archive(BRIDGE)
        )]
    );
}

#[test]
fn refuses_the_bridges_windows_installer_or_portable_that_is_gone_or_serves_junk() {
    for name in [installer(BRIDGE), portable(BRIDGE)] {
        let gone = found(|github, _| github.serve(&download(BRIDGE, &name), Served::Status(404)));
        assert_eq!(
            gone,
            [format!(
                "the bridge's {name} cannot be downloaded (HTTP 404)"
            )]
        );
        let junk = found(|github, _| github.serve(&download(BRIDGE, &name), "junk"));
        assert_eq!(
            junk,
            [format!(
                "the bridge's {name} does not download as the one its SHA256SUMS lists"
            )]
        );
    }
}

#[test]
fn refuses_where_the_bridges_own_latest_json_is_not_there_to_hold_the_feed_to() {
    let problems = found(|github, _| {
        github.serve(&download(BRIDGE, "latest.json"), Served::Status(404));
    });
    assert_eq!(
        problems,
        [format!(
            "the bridge's v{BRIDGE}/latest.json cannot be read (HTTP 404), so no feed can be held to it"
        )]
    );
}

#[test]
fn refuses_a_bridge_whose_latest_json_names_another_release_or_an_archive_elsewhere() {
    /// The bridge published with a latest.json that `wrong` makes of its own, and update-alpha serving it.
    fn published(wrong: impl Fn(&str) -> String) -> impl FnOnce(&Github, &Files) {
        move |github, files| {
            let text = wrong(&files.text("latest.json"));
            let assets = published_assets(files).with("latest.json", &text);
            github.release(&format!("v{BRIDGE}"), Spec::new().assets(assets.iter()));
            feed(github, "update-alpha", &text);
        }
    }
    let elsewhere = found(published(|text| {
        with_archive_url(text, "https://example.invalid/a.tar.gz")
    }));
    assert!(
        elsewhere[0].contains("names https://example.invalid/a.tar.gz for the Mac, not "),
        "{elsewhere:?}"
    );
    let another = found(published(|text| {
        text.replace(
            &format!("\"version\":\"{BRIDGE}\""),
            "\"version\":\"3.0.0-alpha.99\"",
        )
    }));
    assert_eq!(
        another,
        [format!(
            "the bridge's v{BRIDGE}/latest.json names 3.0.0-alpha.99, not {BRIDGE}"
        )]
    );
}

#[test]
fn refuses_where_sha256sums_is_gone_or_lists_no_file_the_old_apps_need() {
    let gone =
        found(|github, _| github.serve(&download(BRIDGE, "SHA256SUMS"), Served::Status(404)));
    assert_eq!(
        gone,
        [format!(
            "the bridge's v{BRIDGE}/SHA256SUMS cannot be read (HTTP 404)"
        )]
    );
    let without = found(|github, files| {
        let sums = published_assets(files).text("SHA256SUMS");
        let trimmed = sums
            .split('\n')
            .filter(|line| !line.ends_with(&installer(BRIDGE)))
            .collect::<Vec<_>>()
            .join("\n");
        let assets = published_assets(files).with("SHA256SUMS", trimmed);
        github.release(&format!("v{BRIDGE}"), Spec::new().assets(assets.iter()));
    });
    assert_eq!(
        without,
        [format!(
            "the bridge's v{BRIDGE}/SHA256SUMS lists no {}",
            installer(BRIDGE)
        )]
    );
}

#[test]
fn waits_out_an_old_feed_served_stale_for_a_moment() {
    let asked = Arc::new(AtomicUsize::new(0));
    let passed = found(|github, _| {
        let asked = Arc::clone(&asked);
        let stale = latest_json(github.base(), OLDER, "notes");
        github.serve_with("/update-alpha/latest.json", move || {
            let times = asked.fetch_add(1, Ordering::SeqCst) + 1;
            (times < 2).then(|| Served::Body(stale.clone().into_bytes()))
        });
    });
    assert_eq!(passed, Vec::<String>::new());
    assert_eq!(
        asked.load(Ordering::SeqCst),
        2,
        "read again until it served the bridge"
    );
}

#[test]
fn has_nothing_to_ask_of_the_feeds_for_the_bridge_itself_and_reads_none() {
    let Crossed { github, .. } = crossed();
    old_feed_serves(&github, OLDER);
    let problems = check_prerequisites(&v(BRIDGE), github.base(), &rule(), quick());
    assert_eq!(problems, Vec::<String>::new());
    assert_eq!(github.requests(), Vec::<String>::new());
}

#[test]
fn refuses_a_release_before_the_bridge() {
    let problems = found_for(|_, _| {}, OLDER);
    assert_eq!(problems.len(), 1);
    assert!(
        problems[0].contains("comes before the bridge"),
        "{problems:?}"
    );
}

#[test]
fn reads_through_a_redirect_as_a_release_download_is_served() {
    // GitHub answers a release download with a redirect to where it keeps the file: the
    // reads follow it, or every one of them would be a refusal on the day.
    let problems = found(|github, files| {
        let moved = format!("{}/moved/latest.json", github.base());
        github.serve("/update-alpha/latest.json", Served::Redirect(moved));
        github.serve("/moved/latest.json", files.text("latest.json"));
    });
    assert_eq!(problems, Vec::<String>::new());
}

#[test]
#[cfg(unix)]
fn refuses_a_release_where_curl_is_not_there_to_read_the_feeds_and_says_so() {
    // The reads are a program the runner has: where it is missing, nothing can be held to
    // the bridge, and the release is refused in words, not by a crash.
    let Crossed { github, .. } = crossed();
    let nothing = TempDir::new("no-programs");
    let ran = run_binary_on(
        std::path::Path::new(env!("CARGO_BIN_EXE_cf-publish")),
        &[
            "feeds",
            "prerequisites",
            "--version",
            LATER,
            "--base",
            github.base(),
            "--attempts",
            "2",
            "--wait",
            "1",
        ],
        &[],
        nothing.path(),
    );
    assert_eq!(ran.status, 1, "{}", ran.said());
    assert!(
        ran.stderr
            .contains("cannot be read (unreachable: curl could not be started"),
        "{}",
        ran.stderr
    );
    assert_eq!(github.requests(), Vec::<String>::new());
}

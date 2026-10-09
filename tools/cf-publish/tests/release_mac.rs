//! The Mac job of the release workflow (.github/workflows/release.yml), the
//! steps that ask the rule of the feeds run as written, as a hand run and as a
//! tag run: the early question to the old feeds, and the plan of the feeds from
//! the archive. The release jobs run on macOS and Linux: so do these.
#![cfg(unix)]
// A failure in a test is the test's answer, in its helpers too.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use cf_publish::testing::workflow::release::{DOWNLOADS, NOTES, PREREQUISITES};
use cf_publish::testing::workflow::{job, read, step};

mod the_mac_job_as_a_hand_run_and_as_a_tag_run {
    use std::fs;
    use std::path::Path;

    use cf_publish::manifest::Manifest;
    use cf_publish::testing::workflow::bash;
    use cf_publish::testing::worlds::feed;
    use cf_publish::testing::{
        archive_of, built_files, folder_of, latest_json, published_assets, Github, Ran, Spec,
        TempDir,
    };

    use super::*;

    /// A version that comes after the bridge the repository records, and one before it.
    const LATER: &str = "99.0.0-alpha.1";
    const BEFORE: &str = "1.0.0";

    /// A checkout of nothing but the version `package.json` says, and the built cf-publish
    /// where the Mac job's build leaves it.
    fn project(version: &str) -> TempDir {
        let root = TempDir::new("mac");
        fs::write(
            root.path().join("package.json"),
            format!("{{\"version\":\"{version}\"}}"),
        )
        .unwrap();
        let release = root
            .path()
            .join("app")
            .join("src-tauri")
            .join("target")
            .join("release");
        fs::create_dir_all(&release).unwrap();
        std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_cf-publish"), release.join("cf-publish"))
            .unwrap();
        root
    }

    fn run(script: &str, root: &Path, event: &str, github: &Github) -> Ran {
        bash(
            script,
            root,
            &[
                ("GITHUB_EVENT_NAME", event),
                ("GITHUB_REPOSITORY", github.repo()),
            ],
            &[],
        )
    }

    #[test]
    fn asks_the_old_feeds_before_the_build_a_tag_is_refused_a_hand_run_is_told_what_a_tag_would_be()
    {
        let github = Github::start();
        let bridge = Manifest::embedded().unwrap().bridge.version.to_string();
        // The bridge's tag is made and its release failed: update-alpha serves the release before it.
        feed(
            &github,
            "update-alpha",
            latest_json(github.base(), "3.0.0-alpha.1", "notes"),
        );
        let files = built_files(github.base(), &bridge);
        github.release(
            &format!("v{bridge}"),
            Spec::new().assets(published_assets(&files).iter()),
        );
        // The workflow gives the command no patience of its own: a read that does not serve the bridge is read again for a minute.
        let mac = job(&read("release.yml"), "mac");
        let script = step(&mac, PREREQUISITES)
            .script()
            .replace(DOWNLOADS, github.base());
        let quick = script.replace(
            "cf-publish feeds prerequisites",
            "cf-publish feeds prerequisites --attempts 2 --wait 1",
        );
        assert_ne!(
            quick, script,
            "the step does not run cf-publish feeds prerequisites"
        );
        let root = project(LATER);

        let tag = run(&quick, root.path(), "push", &github);
        assert_eq!(tag.status, 1, "{}", tag.said());
        assert!(
            tag.stderr
                .lines()
                .any(|line| line
                    .starts_with("feeds: update-alpha serves 3.0.0-alpha.1, not the bridge")),
            "{}",
            tag.stderr
        );

        let hand = run(&quick, root.path(), "workflow_dispatch", &github);
        assert_eq!(hand.status, 0, "{}", hand.said());
        assert!(
            hand.stderr.lines().any(|line| line
                .starts_with("feeds (a tag would be refused): update-alpha serves 3.0.0-alpha.1")),
            "{}",
            hand.stderr
        );

        // The bridge itself has nothing to ask, whatever the feeds serve.
        fs::write(
            root.path().join("package.json"),
            format!("{{\"version\":\"{bridge}\"}}"),
        )
        .unwrap();
        let itself = run(&quick, root.path(), "push", &github);
        assert_eq!(itself.status, 0, "{}", itself.said());
        assert!(
            itself.stdout.contains("it is the bridge"),
            "{}",
            itself.stdout
        );
    }

    #[test]
    fn plans_the_feeds_from_the_archive_a_tag_is_refused_for_a_release_before_the_bridge_a_hand_run_is_told(
    ) {
        let github = Github::start();
        let dir = folder_of(&cf_publish::testing::Files::default().with(
            "archive.tar.gz",
            archive_of(&["ConsensFlow.app/Contents/MacOS/app"]),
        ));
        let mac = job(&read("release.yml"), "mac");
        let notes = step(&mac, NOTES).script();
        let plan = &notes[notes
            .find("dry=()")
            .expect("the step plans after it sets dry")..];
        assert!(
            plan.contains("cf-publish feeds plan"),
            "the step plans the feeds"
        );
        let root = project(BEFORE);
        let planned = |event: &str, version: &str| {
            let script = format!(
                "set -euo pipefail\nversion={version}\nout={}\narchive=archive.tar.gz\n{plan}",
                dir.path().display()
            );
            run(&script, root.path(), event, &github)
        };

        let tag = planned("push", BEFORE);
        assert_eq!(tag.status, 1, "{}", tag.said());
        assert!(
            tag.stderr
                .lines()
                .any(|line| line.starts_with("feeds: 1.0.0 comes before the bridge")),
            "{}",
            tag.stderr
        );
        assert_eq!(tag.stdout, "");

        let hand = planned("workflow_dispatch", BEFORE);
        assert_eq!(hand.status, 0, "{}", hand.said());
        assert!(
            hand.stderr.lines().any(|line| line
                .starts_with("feeds (a tag would be refused): 1.0.0 comes before the bridge")),
            "{}",
            hand.stderr
        );

        let later = planned("push", LATER);
        assert_eq!(later.status, 0, "{}", later.said());
        assert_eq!(later.stdout, "feed-alpha\n");
    }
}

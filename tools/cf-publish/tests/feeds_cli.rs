//! The checks from the command line, against GitHub as the simulator has it:
//! `cf-publish feeds prerequisites` and `cf-publish feeds check`, run as the
//! workflow runs them, with the manifest the binary was built with.

// A failure in a test is the test's answer, in its helpers too.
#![allow(clippy::expect_used, clippy::unwrap_used)]

mod the_checks_from_the_command_line {
    use std::path::Path;

    use cf_publish::manifest::Manifest;
    use cf_publish::testing::worlds::{crossed_as, feed, serving, Crossed};
    use cf_publish::testing::{
        built_files, download, folder_of, gh_dir, portable_name as portable, published_assets,
        run_binary, Ran, Served, Spec, TempDir,
    };
    use cf_publish::version::Channel;

    const AFTER: &str = "99.0.0-alpha.1";

    fn run(args: &[&str]) -> Ran {
        run_binary(Path::new(env!("CARGO_BIN_EXE_cf-publish")), args, &[], None)
    }

    /// Whether `text` has a line that starts with `prefix`.
    fn has_line(text: &str, prefix: &str) -> bool {
        text.lines().any(|line| line.starts_with(prefix))
    }

    /// The repository's own bridge, crossed, and a later release published after it.
    fn world() -> (Crossed, TempDir, String) {
        let bridge = Manifest::embedded().unwrap().bridge.version.to_string();
        let crossed = crossed_as(&bridge);
        let built = built_files(crossed.github.base(), AFTER);
        crossed.github.release(
            &format!("v{AFTER}"),
            Spec::new().assets(published_assets(&built).iter()),
        );
        feed(&crossed.github, "feed-alpha", built.text("latest.json"));
        let on_disk = built
            .clone()
            .with("SHA256SUMS", published_assets(&built).text("SHA256SUMS"));
        (crossed, folder_of(&on_disk), bridge)
    }

    #[test]
    fn says_the_prerequisites_hold_or_what_they_lack_a_refusal_fails_a_hand_run_only_says_it() {
        let (crossed, _dir, bridge) = world();
        let github = &crossed.github;
        let base = [
            "feeds",
            "prerequisites",
            "--version",
            AFTER,
            "--base",
            github.base(),
            "--attempts",
            "2",
            "--wait",
            "1",
        ];
        let holds = run(&base);
        assert_eq!(holds.status, 0, "{}", holds.stderr);
        assert!(
            holds
                .stdout
                .contains("may move its feeds (the old feeds serve the bridge, with its files)"),
            "{}",
            holds.stdout
        );

        let legacy = Manifest::embedded()
            .unwrap()
            .legacy
            .of(Channel::Alpha)
            .to_string();
        github.serve(&format!("/{legacy}/latest.json"), Served::Status(404));
        let refused = run(&base);
        assert_eq!(refused.status, 1);
        assert_eq!(refused.stdout, "");
        assert!(
            has_line(
                &refused.stderr,
                "feeds: update-alpha cannot be read (HTTP 404)"
            ),
            "{}",
            refused.stderr
        );

        let mut hand = base.to_vec();
        hand.push("--dry-run");
        let dry = run(&hand);
        assert_eq!(dry.status, 0, "a hand run is not failed by it");
        assert!(
            has_line(
                &dry.stderr,
                "feeds (a tag would be refused): update-alpha cannot be read"
            ),
            "{}",
            dry.stderr
        );

        let bridged = run(&[
            "feeds",
            "prerequisites",
            "--version",
            &bridge,
            "--base",
            github.base(),
        ]);
        assert_eq!(bridged.status, 0, "{}", bridged.stderr);
        assert!(
            bridged.stdout.contains("it is the bridge"),
            "{}",
            bridged.stdout
        );
    }

    #[test]
    fn says_the_feeds_serve_what_the_rule_says_once_published_or_what_they_do_not() {
        let (crossed, dir, _) = world();
        let github = &crossed.github;
        let dir = dir.path().to_string_lossy().into_owned();
        let base = [
            "feeds",
            "check",
            "--dir",
            &dir,
            "--version",
            AFTER,
            "--base",
            github.base(),
            "--attempts",
            "1",
            "--wait",
            "1",
        ];
        let passed = run(&base);
        assert_eq!(passed.status, 0, "{}", passed.stderr);
        assert!(
            passed
                .stdout
                .contains("serve this release as the rule says"),
            "{}",
            passed.stdout
        );

        github.serve(&download(AFTER, &portable(AFTER)), "junk");
        let failed = run(&base);
        assert_eq!(failed.status, 1);
        assert!(
            failed
                .stderr
                .contains("does not download from v99.0.0-alpha.1 as the one built"),
            "{}",
            failed.stderr
        );
    }

    #[test]
    fn says_the_feeds_are_right_for_a_release_that_a_later_one_has_gone_past_and_not_for_one_that_names_an_earlier(
    ) {
        let (crossed, dir, _) = world();
        let github = &crossed.github;
        let dir = dir.path().to_string_lossy().into_owned();
        let base = [
            "feeds",
            "check",
            "--dir",
            &dir,
            "--version",
            AFTER,
            "--base",
            github.base(),
            "--attempts",
            "1",
            "--wait",
            "1",
        ];
        serving(github, "feed-alpha", "99.0.0-alpha.2");
        let passed = run(&base);
        assert_eq!(passed.status, 0, "{}", passed.stderr);
        assert!(
            passed
                .stdout
                .contains("serve this release as the rule says"),
            "{}",
            passed.stdout
        );

        serving(github, "feed-alpha", "3.0.0-alpha.1");
        let failed = run(&base);
        assert_eq!(failed.status, 1);
        assert!(
            failed
                .stderr
                .lines()
                .any(|line| line == "feeds: feed-alpha does not serve this release's latest.json"),
            "{}",
            failed.stderr
        );
    }

    #[test]
    fn never_asks_gh_it_reads_what_installed_apps_read() {
        // The check reads public download addresses: a `gh` it never runs, and a token it never
        // needs. A `gh` that asks the simulator is first on its PATH, to tell if it ever did.
        let (crossed, dir, _) = world();
        let github = &crossed.github;
        let dir = dir.path().to_string_lossy().into_owned();
        let bin = gh_dir(Path::new(env!("CARGO_BIN_EXE_fake-gh")));
        let ran = run_binary(
            Path::new(env!("CARGO_BIN_EXE_cf-publish")),
            &[
                "feeds",
                "check",
                "--dir",
                &dir,
                "--version",
                AFTER,
                "--base",
                github.base(),
                "--attempts",
                "1",
                "--wait",
                "1",
            ],
            &[
                ("GH_SIM", github.base()),
                ("GH_TOKEN", "a-token"),
                ("GH_REPO", "ngvoicu/consensflow"),
            ],
            Some(bin.path()),
        );
        assert_eq!(ran.status, 0, "{}", ran.said());
        assert_eq!(github.calls(), Vec::<Vec<String>>::new());
    }
}

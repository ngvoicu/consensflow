//! The plan of the feeds from the command line: `cf-publish feeds plan`, run as
//! the Mac job runs it, over archives made as the release makes its own, and
//! what every command says when it is asked for too little or too much.

// A failure in a test is the test's answer, in its helpers too.
#![allow(clippy::expect_used, clippy::unwrap_used)]

mod the_plan_from_the_command_line {
    use std::fs;
    use std::path::Path;

    use cf_publish::manifest::Manifest;
    use cf_publish::testing::{
        archive_of, node_free_layout, old_apps_layout, run_binary, Ran, TempDir,
    };
    use cf_publish::version::Channel;

    fn run(args: &[&str]) -> Ran {
        run_binary(Path::new(env!("CARGO_BIN_EXE_cf-publish")), args, &[], None)
    }

    /// A version that comes after, and one before, the bridge the repository records.
    const AFTER: &str = "99.0.0-alpha.1";
    const BEFORE: &str = "1.0.0";

    fn plan(version: &str, archive: &Path, more: &[&str]) -> Ran {
        let archive = archive.to_string_lossy();
        let mut args = vec!["feeds", "plan", "--version", version, "--archive", &archive];
        args.extend(more);
        run(&args)
    }

    fn archive(dir: &TempDir, name: &str, files: &[String]) -> std::path::PathBuf {
        let names: Vec<&str> = files.iter().map(String::as_str).collect();
        let target = dir.path().join(name);
        fs::write(&target, archive_of(&names)).unwrap();
        target
    }

    #[test]
    #[cfg_attr(windows, ignore = "the release pipeline is macOS and Linux")]
    fn names_the_feeds_one_per_line_the_bridge_to_both_generations_a_later_release_to_the_new() {
        let dir = TempDir::new("feeds");
        let old = archive(&dir, "old.tar.gz", &old_apps_layout());
        let free = archive(&dir, "free.tar.gz", &node_free_layout());
        let manifest = Manifest::embedded().unwrap();
        let bridge = manifest.bridge.version.to_string();

        let moving = plan(&bridge, &old, &[]);
        assert_eq!(moving.status, 0, "{}", moving.stderr);
        assert_eq!(
            moving.stdout,
            format!("feed-alpha\n{}\n", manifest.legacy.of(Channel::Alpha))
        );
        assert!(moving.stderr.contains("is the bridge"), "{}", moving.stderr);

        let later = plan(AFTER, &free, &[]);
        assert_eq!(later.status, 0, "{}", later.stderr);
        assert_eq!(later.stdout, "feed-alpha\n");
        assert!(later.stderr.contains("a later release"), "{}", later.stderr);

        let refused = plan(&bridge, &free, &[]);
        assert_eq!(refused.status, 1);
        assert_eq!(refused.stdout, "", "nothing is named to move");
        assert!(
            refused.stderr.contains("must keep the layout"),
            "{}",
            refused.stderr
        );
    }

    #[test]
    #[cfg_attr(windows, ignore = "the release pipeline is macOS and Linux")]
    fn refuses_a_release_before_the_bridge_which_a_hand_run_only_says_it_would() {
        let dir = TempDir::new("feeds");
        let old = archive(&dir, "old.tar.gz", &old_apps_layout());
        let refused = plan(BEFORE, &old, &[]);
        assert_eq!(refused.status, 1);
        assert_eq!(refused.stdout, "");
        assert!(
            refused.stderr.contains("comes before the bridge"),
            "{}",
            refused.stderr
        );

        let dry = plan(BEFORE, &old, &["--dry-run"]);
        assert_eq!(dry.status, 0, "a hand run is not failed by it");
        assert_eq!(dry.stdout, "");
        assert!(
            dry.stderr.starts_with("feeds (a tag would be refused): ")
                && dry.stderr.contains("comes before the bridge"),
            "{}",
            dry.stderr
        );
    }

    #[test]
    fn says_what_a_command_lacks_and_does_nothing() {
        for (command, lacks) in [
            ("plan", "feeds: plan needs --version and --archive"),
            (
                "prerequisites",
                "feeds: prerequisites needs --version and --base",
            ),
            ("check", "feeds: check needs --dir, --version and --base"),
            (
                "publish",
                "feeds: usage: cf-publish feeds plan|prerequisites|check",
            ),
        ] {
            let ran = run(&["feeds", command]);
            assert_eq!(ran.status, 1, "{command}");
            assert_eq!(ran.stderr.trim_end(), lacks, "{command}");
            assert_eq!(ran.stdout, "", "{command}");
        }
        let nothing = run(&[]);
        assert_eq!(nothing.status, 1);
        assert!(
            nothing
                .stderr
                .starts_with("cf-publish: usage: cf-publish feeds"),
            "{}",
            nothing.stderr
        );
    }

    #[test]
    fn refuses_a_word_it_does_not_take_an_option_it_does_not_know_and_a_number_that_is_none() {
        let waits = ["feeds", "prerequisites", "--version", "1.0.0", "--base"];
        for (args, says) in [
            (
                vec!["feeds", "plan", "extra"],
                "feeds: Unexpected argument 'extra'. This command does not take positional arguments",
            ),
            (
                vec!["feeds", "plan", "--bogus"],
                "feeds: Unknown option '--bogus'",
            ),
            (vec!["feeds", "plan", "-x"], "feeds: Unknown option '-x'"),
            (
                vec!["feeds", "plan", "--version"],
                "feeds: Option '--version <value>' argument missing",
            ),
            (
                vec!["publish", "--bogus"],
                "publish: Unknown option '--bogus'",
            ),
            (
                [&waits[..], &["http://127.0.0.1:1", "--attempts", "many"]].concat(),
                "feeds: --attempts needs a whole number, not many",
            ),
            (
                [&waits[..], &["http://127.0.0.1:1", "--attempts", "0"]].concat(),
                "feeds: --attempts needs at least 1",
            ),
            (
                [&waits[..], &["http://127.0.0.1:1", "--wait=soon"]].concat(),
                "feeds: --wait needs a whole number, not soon",
            ),
        ] {
            let ran = run(&args);
            assert_eq!(ran.status, 1, "{args:?}");
            assert_eq!(ran.stderr.trim_end(), says, "{args:?}");
            assert_eq!(ran.stdout, "", "{args:?}");
        }
    }
}

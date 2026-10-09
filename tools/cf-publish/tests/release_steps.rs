//! The publish job of the release workflow (.github/workflows/release.yml),
//! its steps run as written: the check of the publisher against the hash the
//! job that built it reported, the publishing, and the check of the feeds, by
//! bash, with a `gh` that is GitHub as the simulator has it. A hand run of the
//! workflow shows none of them (it publishes nothing) and a slip in one is found
//! on release day. The release jobs run on macOS and Linux: so do these.
#![cfg(unix)]
// A failure in a test is the test's answer, in its helpers too.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use cf_publish::testing::workflow::release::{CHECK, DOWNLOADS, HASH, PUBLISH};
use cf_publish::testing::workflow::{job, read, step};

mod the_publish_job_run_as_written {
    use std::collections::BTreeMap;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};

    use cf_publish::digest::sha256_hex;
    use cf_publish::manifest::Manifest;
    use cf_publish::testing::workflow::{bash, expand};
    use cf_publish::testing::worlds::feed;
    use cf_publish::testing::{
        archive_of, built_files, folder_of, gh_dir, latest_json, Github, Ran, TempDir, OLD_LAYOUT,
    };

    use super::*;

    /// What the jobs built `release` into: a folder, its archive laid out as the old apps check for.
    struct Built {
        release: String,
        dir: TempDir,
        latest: String,
    }

    /// The publish job's machine: GitHub, a `gh` that asks it, and `$RUNNER_TEMP` with the
    /// publisher downloaded into it.
    struct World {
        github: Github,
        gh: TempDir,
        runner_temp: TempDir,
        /// What the publisher hashes to, as the job that built it reported.
        sha256: String,
        /// The bridge the repository records, built.
        bridge: Built,
    }

    fn built(github: &Github, release: &str) -> Built {
        let files = built_files(github.base(), release);
        let archive = format!("ConsensFlow_{release}_aarch64.app.tar.gz");
        let files = files.with(&archive, archive_of(&OLD_LAYOUT));
        Built {
            release: release.to_string(),
            latest: files.text("latest.json"),
            dir: folder_of(&files),
        }
    }

    impl World {
        fn new() -> Self {
            let github = Github::start();
            feed(
                &github,
                "update-alpha",
                latest_json(github.base(), "3.0.0-alpha.1", "notes"),
            );
            let runner_temp = TempDir::new("runner");
            let folder = runner_temp.path().join("publisher");
            fs::create_dir(&folder).unwrap();
            fs::copy(env!("CARGO_BIN_EXE_cf-publish"), folder.join("cf-publish")).unwrap();
            let sha256 = sha256_hex(&fs::read(folder.join("cf-publish")).unwrap());
            let version = Manifest::embedded().unwrap().bridge.version.to_string();
            let bridge = built(&github, &version);
            Self {
                gh: gh_dir(Path::new(env!("CARGO_BIN_EXE_fake-gh"))),
                github,
                runner_temp,
                sha256,
                bridge,
            }
        }

        fn version(&self) -> &str {
            &self.bridge.release
        }

        fn built(&self, release: &str) -> Built {
            built(&self.github, release)
        }

        fn publisher(&self) -> PathBuf {
            self.runner_temp.path().join("publisher").join("cf-publish")
        }

        /// Runs a step of the publish job for a release, in the folder the jobs built it into, with the
        /// environment the step gives itself and what an Actions runner gives every step. `env` is added
        /// to it, replacing; `without` names what the step is run without.
        fn run(&self, name: &str, env: &[(&str, &str)], without: &[&str], built: &Built) -> Ran {
            let publish = job(&read("release.yml"), "publish");
            let step = step(&publish, name);
            let runner_temp = self.runner_temp.path().to_string_lossy().into_owned();
            let known = [
                ("github.token", "a-token"),
                ("github.repository", self.github.repo()),
                ("runner.temp", runner_temp.as_str()),
                ("needs.publisher.outputs.sha256", self.sha256.as_str()),
            ];
            let ref_name = format!("v{}", built.release);
            let mut vars: BTreeMap<String, String> = [
                ("RUNNER_TEMP", runner_temp.as_str()),
                ("GITHUB_REF_NAME", ref_name.as_str()),
                ("GITHUB_REPOSITORY", self.github.repo()),
                ("GITHUB_EVENT_NAME", "push"),
                ("GITHUB_REF_TYPE", "tag"),
                ("GH_SIM", self.github.base()),
                ("HOME", runner_temp.as_str()),
            ]
            .iter()
            .map(|(name, value)| ((*name).to_string(), (*value).to_string()))
            .collect();
            for (variable, value) in step.env() {
                vars.insert(variable, expand(&value, &known));
            }
            for (variable, value) in env {
                vars.insert((*variable).to_string(), (*value).to_string());
            }
            for variable in without {
                vars.remove(*variable);
            }
            let vars: Vec<(&str, &str)> =
                vars.iter().map(|(n, v)| (n.as_str(), v.as_str())).collect();
            let script = step.script().replace(DOWNLOADS, self.github.base());
            bash(&script, built.dir.path(), &vars, &[self.gh.path()])
        }

        fn step(&self, name: &str, built: &Built) -> Ran {
            self.run(name, &[], &[], built)
        }
    }

    fn has_line(text: &str, line: &str) -> bool {
        text.lines().any(|found| found == line)
    }

    #[test]
    fn publishes_the_bridge_and_moves_its_feeds_and_the_step_after_it_finds_them_right() {
        let world = World::new();
        let version = world.version().to_string();
        let published = world.step(PUBLISH, &world.bridge);
        assert_eq!(published.status, 0, "{}", published.said());
        assert!(
            has_line(
                &published.stdout,
                &format!("publish: {version}: the release was created; feed-alpha created, update-alpha replaced")
            ),
            "{}",
            published.stdout
        );
        assert!(!world.github.is_draft(&format!("v{version}")));
        for feed in ["feed-alpha", "update-alpha"] {
            assert_eq!(world.github.names(feed).unwrap(), ["latest.json"], "{feed}");
        }
        let checked = world.step(CHECK, &world.bridge);
        assert_eq!(checked.status, 0, "{}", checked.said());
        assert!(
            checked
                .stdout
                .contains("feeds: serve this release as the rule says"),
            "{}",
            checked.stdout
        );
    }

    #[test]
    fn is_run_again_after_a_failure_and_finishes_the_work() {
        let world = World::new();
        world.github.fail(|args, _| {
            args.iter()
                .any(|arg| arg == "update-alpha")
                .then(|| "the network went away".to_string())
        });
        let cut = world.step(PUBLISH, &world.bridge);
        assert_eq!(cut.status, 1);
        assert!(
            cut.stderr.contains("the network went away"),
            "{}",
            cut.stderr
        );
        world.github.clear_fail();
        let again = world.step(PUBLISH, &world.bridge);
        assert_eq!(again.status, 0, "{}", again.said());
        assert!(
            again
                .stdout
                .contains("the release was kept; feed-alpha kept, update-alpha replaced"),
            "{}",
            again.stdout
        );
        let tag = format!("v{}", world.version());
        let creates = world
            .github
            .calls()
            .iter()
            .filter(|args| args[1] == "create" && args[2] == tag)
            .count();
        assert_eq!(creates, 1);
    }

    #[test]
    fn is_run_again_after_a_later_release_went_out_leaves_the_feed_at_that_release_and_says_so() {
        let world = World::new();
        let github = &world.github;
        let version = world.version().to_string();
        let later = world.built("99.0.0-alpha.1");
        // The bridge reaches both feeds, and the edit that marks update-alpha as pinned fails.
        github.fail(|args, _| {
            (args[1] == "edit" && args[2] == "update-alpha").then(|| "the edit failed".to_string())
        });
        let cut = world.step(PUBLISH, &world.bridge);
        assert_eq!(cut.status, 1);
        assert!(
            cut.stderr
                .contains("marking update-alpha as pinned: the edit failed"),
            "{}",
            cut.stderr
        );
        github.clear_fail();
        // The later release finds the bridge delivered, and moves feed-alpha.
        let next = world.step(PUBLISH, &later);
        assert_eq!(next.status, 0, "{}", next.said());
        assert_eq!(github.asset_text("feed-alpha", "latest.json"), later.latest);
        // The bridge's failed job is run again from the Actions page.
        let again = world.step(PUBLISH, &world.bridge);
        assert_eq!(again.status, 0, "{}", again.said());
        assert!(
            has_line(
                &again.stdout,
                &format!("publish: feed-alpha names 99.0.0-alpha.1, which comes after {version}: it is left alone, and nothing is moved backward")
            ),
            "{}",
            again.stdout
        );
        assert!(
            has_line(
                &again.stdout,
                &format!("publish: {version}: the release was kept; feed-alpha superseded by 99.0.0-alpha.1, update-alpha kept")
            ),
            "{}",
            again.stdout
        );
        assert_eq!(github.asset_text("feed-alpha", "latest.json"), later.latest);
        assert_eq!(github.title("update-alpha"), "Alpha update feed, pinned");
        // The step after each finds the feeds right.
        for release in [&world.bridge, &later] {
            let checked = world.step(CHECK, release);
            assert_eq!(checked.status, 0, "{}", checked.said());
            assert!(
                checked
                    .stdout
                    .contains("feeds: serve this release as the rule says"),
                "{}",
                checked.stdout
            );
        }
        // A feed that went back to the bridge would serve what the bridge published: the record the publish step left
        // in the folder, of what feed-alpha named, is what tells the check step it went backward.
        feed(
            github,
            "feed-alpha",
            latest_json(github.base(), &version, "notes"),
        );
        let back = world.step(CHECK, &world.bridge);
        assert_eq!(back.status, 1, "{}", back.said());
        assert!(
            has_line(
                &back.stderr,
                &format!("feeds: feed-alpha went backward: it named 99.0.0-alpha.1 when this release was published, and names {version} now")
            ),
            "{}",
            back.stderr
        );
    }

    #[test]
    fn publishes_nothing_for_a_hand_run_on_a_tag_the_step_refuses_of_itself() {
        let world = World::new();
        let ran = world.run(
            PUBLISH,
            &[("GITHUB_EVENT_NAME", "workflow_dispatch")],
            &[],
            &world.bridge,
        );
        assert_eq!(ran.status, 1);
        assert!(
            ran.stderr.contains(
                "only the push of a version tag publishes; this is workflow_dispatch on tag"
            ),
            "{}",
            ran.stderr
        );
        assert_eq!(world.github.calls(), Vec::<Vec<String>>::new());
    }

    #[test]
    fn gets_nowhere_without_the_token_and_the_repository_the_step_gives_gh() {
        for missing in ["GH_TOKEN", "GH_REPO"] {
            let world = World::new();
            let ran = world.run(PUBLISH, &[], &[missing], &world.bridge);
            assert_eq!(ran.status, 1, "{missing}: {}", ran.said());
            assert_eq!(
                world.github.calls(),
                Vec::<Vec<String>>::new(),
                "{missing}: gh reached GitHub"
            );
            assert!(!world.github.has("feed-alpha"));
        }
    }

    #[test]
    fn runs_the_publisher_only_once_it_is_the_binary_the_job_that_built_it_reported() {
        let world = World::new();
        let publisher = world.publisher();
        // An artifact comes without the bit that makes a file run.
        fs::set_permissions(&publisher, fs::Permissions::from_mode(0o644)).unwrap();
        let wrong = "0".repeat(64);
        let refused = world.run(
            HASH,
            &[("PUBLISHER_SHA256", wrong.as_str())],
            &[],
            &world.bridge,
        );
        assert_eq!(refused.status, 1, "{}", refused.said());
        assert!(
            refused.stderr.contains(&format!(
                "the publisher downloaded hashes to {}, and the job that built it says {wrong}",
                world.sha256
            )),
            "{}",
            refused.stderr
        );
        let silent = world.run(HASH, &[("PUBLISHER_SHA256", "")], &[], &world.bridge);
        assert_eq!(silent.status, 1, "{}", silent.said());
        assert!(silent.stderr.contains("says nothing"), "{}", silent.stderr);
        assert_eq!(
            fs::metadata(&publisher).unwrap().permissions().mode() & 0o111,
            0,
            "not made to run"
        );

        let right = world.step(HASH, &world.bridge);
        assert_eq!(right.status, 0, "{}", right.said());
        assert_ne!(
            fs::metadata(&publisher).unwrap().permissions().mode() & 0o111,
            0,
            "made to run"
        );
    }

    #[test]
    fn refuses_a_publisher_that_was_replaced_since_it_was_built() {
        let world = World::new();
        let publisher = world.publisher();
        fs::write(&publisher, "#!/bin/sh\ntouch replaced-publisher-ran\n").unwrap();
        let ran = world.step(HASH, &world.bridge);
        assert_eq!(ran.status, 1, "{}", ran.said());
        assert!(
            ran.stderr.contains("the publisher downloaded hashes to"),
            "{}",
            ran.stderr
        );
        assert!(!world
            .bridge
            .dir
            .path()
            .join("replaced-publisher-ran")
            .exists());
    }
}

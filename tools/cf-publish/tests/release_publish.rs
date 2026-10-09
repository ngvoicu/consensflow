//! The release workflow (.github/workflows/release.yml), its text: who may
//! publish, what holds the repository's write token, how the publisher is built
//! and checked before it runs, and the order of the steps that ask the rule of
//! the feeds. The logic is cf-publish's, held to its cases in tests/feeds_*.rs
//! and tests/publish_*.rs; the steps run as written are tests/release_steps.rs
//! (the publish job) and tests/release_mac.rs (the Mac job).

// A failure in a test is the test's answer, in its helpers too.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use cf_publish::testing::workflow::release::{
    BUILD_PUBLISHER, BUILD_RULE, CHECK, HASH, NOTES, PREREQUISITES, PUBLISH, PUBLISHER,
};
use cf_publish::testing::workflow::{job, live, read, step};

mod who_may_publish {
    use super::*;

    #[test]
    fn is_the_push_of_a_tag_not_a_hand_run_though_it_is_on_a_tag() {
        let publish = job(&read("release.yml"), "publish");
        let condition = publish
            .lines()
            .find_map(|line| line.strip_prefix("    if: "))
            .expect("the publish job has a condition");
        let needed: Vec<&str> = condition.split("&&").map(str::trim).collect();
        assert!(
            needed.contains(&"github.event_name == 'push'"),
            "{condition}"
        );
        assert!(needed.contains(&"github.ref_type == 'tag'"), "{condition}");
        assert!(!condition.contains("||"), "no way round it");
    }

    #[test]
    fn is_the_push_of_a_tag_for_the_group_of_releases_that_go_out_one_at_a_time_too() {
        let text = read("release.yml");
        let group = text
            .lines()
            .find_map(|line| line.strip_prefix("  group: "))
            .expect("the workflow has a concurrency group");
        let group = group.trim_start_matches("${{ ").trim_end_matches(" }}");
        let (tagged, trial) = group.split_once("||").expect("two groups");
        assert!(tagged.contains("github.event_name == 'push'"), "{group}");
        assert!(tagged.contains("github.ref_type == 'tag'"), "{group}");
        assert_eq!(trial.trim(), "format('release-trial-{0}', github.run_id)");
    }

    #[test]
    fn keeps_every_call_of_gh_in_the_publisher_which_is_tested_the_workflow_only_runs_it() {
        let text = read("release.yml");
        assert!(
            !live(&text).contains("gh release"),
            "a gh release call in a step"
        );
        assert!(
            !text.contains("--clobber"),
            "a file replaced by deleting it first"
        );
    }

    #[test]
    fn has_the_publish_job_run_the_publisher_that_was_built_and_nothing_of_the_tree() {
        let publish = job(&read("release.yml"), "publish");
        assert!(
            !publish.contains("actions/checkout"),
            "a checkout of the tree"
        );
        assert!(
            !publish.contains("actions/setup-node"),
            "a Node on the write path"
        );
        assert!(
            !live(&publish).contains("npm "),
            "an installer on the write path"
        );
        let needs = publish
            .lines()
            .find_map(|line| line.strip_prefix("    needs: "))
            .unwrap();
        for job in ["mac", "windows", "gate", "publisher"] {
            assert!(
                needs.contains(job),
                "the publish job does not wait for {job}: {needs}"
            );
        }
        // The release's files and the publisher, downloaded into folders of their own.
        assert!(
            publish.contains("pattern: ConsensFlow-*"),
            "the release's files by name"
        );
        assert!(
            publish.contains("          name: cf-publish\n"),
            "the publisher by name"
        );
        assert!(publish.contains("          path: ${{ runner.temp }}/dist\n"));
        assert!(publish.contains("          path: ${{ runner.temp }}/publisher\n"));
        // The hash of the job that built it is what the publisher is checked against, before it runs.
        let at = |name: &str| {
            publish
                .find(&format!("- name: {name}"))
                .unwrap_or_else(|| panic!("no step {name}"))
        };
        assert!(
            at(HASH) < at(PUBLISH) && at(PUBLISH) < at(CHECK),
            "check, publish, then check the feeds"
        );
        let hashing = step(&publish, HASH);
        assert!(
            hashing.env().contains(&(
                "PUBLISHER_SHA256".to_string(),
                "${{ needs.publisher.outputs.sha256 }}".to_string()
            )),
            "{:?}",
            hashing.env()
        );
        for name in [PUBLISH, CHECK] {
            let running = step(&publish, name);
            assert!(
                running.script().contains(&format!("\"{PUBLISHER}\" ")),
                "{name} runs another binary"
            );
            assert_eq!(
                running.field("working-directory").as_deref(),
                Some("${{ runner.temp }}/dist"),
                "{name} works where the release's files are"
            );
        }
        assert!(
            live(&publish)
                .lines()
                .all(|line| !line.contains("cf-publish ")
                    || line.contains(PUBLISHER)
                    || line.contains("name: cf-publish")),
            "a run of cf-publish that is not the downloaded one"
        );
    }

    #[test]
    fn builds_the_publisher_where_no_secret_is() {
        let publisher = job(&read("release.yml"), "publisher");
        assert!(
            publisher.contains("    permissions:\n      contents: read\n"),
            "the publisher job reads the tree and nothing more"
        );
        for word in [
            "secrets.",
            "GH_TOKEN",
            "github.token",
            "environment:",
            "contents: write",
            "id-token",
        ] {
            assert!(!publisher.contains(word), "the publisher job holds {word}");
        }
        for line in publisher.lines().filter(|line| line.contains("uses:")) {
            assert!(
                line.contains("uses: actions/"),
                "a third-party action builds what runs under the token: {line}"
            );
        }
        let checkout = publisher
            .split("- uses: actions/checkout")
            .nth(1)
            .expect("a checkout");
        assert!(
            checkout
                .lines()
                .take(4)
                .any(|line| line.trim() == "persist-credentials: false"),
            "the checkout leaves no credential behind"
        );
        let building = step(&publisher, BUILD_PUBLISHER);
        assert!(building
            .script()
            .contains("cargo build --release --locked -p cf-publish"));
        assert!(building
            .script()
            .contains("shasum -a 256 app/src-tauri/target/release/cf-publish"));
        assert!(building.script().contains("\"$GITHUB_OUTPUT\""));
        assert_eq!(building.field("id").as_deref(), Some("build"));
        assert!(publisher.contains("      sha256: ${{ steps.build.outputs.sha256 }}\n"));
        assert!(publisher.contains("          name: cf-publish\n"));
        assert!(publisher.contains("          path: app/src-tauri/target/release/cf-publish\n"));
        assert!(
            !publisher.contains("\n    if:"),
            "it runs on a hand run too: that is the trial of its build, its hash and its upload"
        );
    }

    #[test]
    fn gives_the_write_token_to_the_two_steps_that_run_the_publisher_and_to_no_other() {
        let text = read("release.yml");
        let publish = job(&text, "publish");
        assert!(
            !publish.contains("\n    env:"),
            "the token is not the job's"
        );
        for name in [PUBLISH, CHECK] {
            let env = step(&publish, name).env();
            for (variable, value) in [
                ("GH_TOKEN", "${{ github.token }}"),
                ("GH_REPO", "${{ github.repository }}"),
            ] {
                assert!(
                    env.contains(&(variable.to_string(), value.to_string())),
                    "{name} lacks {variable}: {env:?}"
                );
            }
        }
        let live = live(&text);
        for word in [
            "GH_TOKEN:",
            "GH_REPO:",
            "github.token",
            "github.repository }}",
        ] {
            assert_eq!(
                live.matches(word).count(),
                2,
                "{word} is in some other place, or one fewer"
            );
        }
        assert_eq!(
            live.matches("contents: write").count(),
            1,
            "the write permission is one job's"
        );
        assert!(publish.contains("    permissions:\n      contents: write\n"));
    }

    #[test]
    fn checks_the_feeds_after_it_publishes_them_and_asks_the_old_feeds_before_it_builds() {
        let text = read("release.yml");
        let lines: Vec<&str> = text.lines().collect();
        let at = |name: &str| {
            lines
                .iter()
                .position(|line| line.trim() == format!("- name: {name}"))
        };
        assert!(
            at(PUBLISH).is_some() && at(PUBLISH) < at(CHECK),
            "the check follows the publishing"
        );
        let mac = job(&text, "mac");
        let steps: Vec<&str> = mac.lines().collect();
        let first = |text: &str| steps.iter().position(|line| line.contains(text));
        let asking = first(&format!("name: {PREREQUISITES}"));
        assert!(asking.is_some(), "the Mac job has no early check");
        assert!(asking < first("- run: npm ci"), "before the build");
        assert!(asking < first(&format!("name: {NOTES}")));
    }

    #[test]
    fn builds_the_rule_in_the_mac_job_before_it_asks_it_in_a_step_that_holds_no_secret_and_calls_it_by_its_path(
    ) {
        let mac = job(&read("release.yml"), "mac");
        let building = step(&mac, BUILD_RULE);
        assert!(building
            .script()
            .contains("cargo build --release --locked -p cf-publish"));
        assert!(!building.text().contains("env:") && !building.text().contains("secrets."));
        let steps: Vec<&str> = mac.lines().collect();
        let at = |text: &str| steps.iter().position(|line| line.contains(text));
        assert!(at(&format!("name: {BUILD_RULE}")) < at(&format!("name: {PREREQUISITES}")));
        for (name, command) in [
            (PREREQUISITES, "feeds prerequisites"),
            (NOTES, "feeds plan"),
        ] {
            let script = step(&mac, name).script();
            assert!(
                script.contains(&format!(
                    "app/src-tauri/target/release/cf-publish {command} "
                )),
                "{name} does not call the built cf-publish by its path"
            );
        }
        assert!(
            !live(&mac).contains("app/scripts/feeds")
                && !live(&mac).contains("app/scripts/publish"),
            "the Mac job still asks a script of the feeds"
        );
    }
}

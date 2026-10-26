//! How the release workflow (.github/workflows/release.yml) uses the release
//! tool, tools/cf-release: it is built in a step of its own that holds no key,
//! and the steps that need it run the binary by its path. A step that holds a key
//! never builds anything, and never goes through `cargo run`: that would build,
//! and run, whatever the lockfile names with the key in its environment. The
//! tool's own behaviour is its crate's tests'; this holds the workflow's text.
//! The same is held of cf-publish itself in tests/release_publish.rs. The gate
//! job builds the tool the same way and asks it whether CI passed the commit
//! (`ci-passed`), in place of running CI's gate again.

// A failure in a test is the test's answer, in its helpers too.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use cf_publish::testing::workflow::release::NOTES;
use cf_publish::testing::workflow::{job, read, steps, StepText};

/// What builds the tool: with the lockfile, or not at all.
const BUILD: &str = "cargo build --release --locked -p cf-release";

/// Where the steps that need the tool run it from, as the workflow writes it.
const TOOL: &str = "app/src-tauri/target/release/cf-release";

/// The Mac job's steps, comments left out.
fn mac() -> Vec<StepText> {
    steps(&job(&read("release.yml"), "mac"))
}

/// Whether a step holds a key of the repository.
fn holds_a_key(step: &StepText) -> bool {
    step.text().contains("secrets.")
}

/// Whether `text` says the word `cargo` (and not a longer one that holds it).
fn goes_through_cargo(text: &str) -> bool {
    text.split(|at: char| !at.is_alphanumeric() && at != '_')
        .any(|word| word == "cargo")
}

/// The index of the step that builds the tool.
fn builder_of(steps: &[StepText]) -> usize {
    let builders: Vec<usize> = (0..steps.len())
        .filter(|at| steps[*at].text().contains(BUILD))
        .collect();
    assert_eq!(builders.len(), 1, "{} steps build the tool", builders.len());
    builders[0]
}

mod the_release_tool_in_the_release_workflow {
    use super::*;

    #[test]
    fn is_built_with_the_lockfile_in_a_step_of_its_own_that_holds_no_key() {
        let steps = mac();
        let builder = &steps[builder_of(&steps)];
        assert!(
            !holds_a_key(builder),
            "the step that builds the tool holds a key"
        );
        assert!(!goes_through_cargo(&builder.text().replace(BUILD, "")));
        assert_eq!(
            builder.text().trim().lines().count(),
            2,
            "the step builds the tool and does nothing else"
        );
    }

    #[test]
    fn is_built_before_every_step_that_holds_a_key() {
        let steps = mac();
        let built = builder_of(&steps);
        let keyed: Vec<usize> = (0..steps.len())
            .filter(|at| holds_a_key(&steps[*at]))
            .collect();
        assert!(keyed.len() >= 2, "the steps that hold a key are found");
        for at in keyed {
            assert!(built < at, "{} comes before the build", steps[at].name());
        }
    }

    #[test]
    fn never_has_a_step_that_holds_a_key_build_or_run_anything_through_cargo() {
        for step in mac().iter().filter(|step| holds_a_key(step)) {
            assert!(
                !goes_through_cargo(&step.text()),
                "{} goes through cargo with a key",
                step.name()
            );
        }
    }

    #[test]
    fn is_what_the_notes_step_runs_by_its_path_to_read_the_version_and_to_make_the_feeds_entry() {
        let steps = mac();
        let notes_at = steps
            .iter()
            .position(|step| step.name() == NOTES)
            .expect("the notes step is found");
        let notes = steps[notes_at].text();
        assert!(
            notes.contains(&format!("release_tool=\"$GITHUB_WORKSPACE/{TOOL}\"")),
            "the tool is named by its path"
        );
        assert!(
            notes.contains("version=$(\"$release_tool\" version)"),
            "the version is its"
        );
        assert!(
            notes.contains("\"$release_tool\" prepare-update --repo \"$PWD\""),
            "the entry is its"
        );
        assert!(!goes_through_cargo(&notes), "the step goes through cargo");
        assert!(!notes.contains(".mjs"), "the step runs a script of Node's");
        assert!(
            builder_of(&steps) < notes_at,
            "the tool is built before the step that runs it"
        );
    }
}

mod the_gate_of_the_release_workflow {
    use cf_publish::testing::workflow::{files, live, step};

    use super::*;

    /// The gate job's step that asks whether CI passed the commit.
    const CI_PASSED: &str = "CI passed this commit on both platforms";

    fn gate() -> String {
        job(&read("release.yml"), "gate")
    }

    #[test]
    fn checks_that_ci_passed_the_commit_and_runs_no_gate_of_its_own() {
        let gate = gate();
        assert!(
            !live(&gate).contains("uses: ./.github/workflows/ci.yml"),
            "the release runs CI's gate again"
        );
        let asking = step(&gate, CI_PASSED);
        let script = asking.script();
        assert!(
            script.starts_with(&format!("{TOOL} ci-passed --github \"$GITHUB_REPOSITORY\"")),
            "{script}"
        );
        // The exact commit of the run, and its event, so that a hand run is told and not failed.
        assert!(script.contains("--commit \"$GITHUB_SHA\""), "{script}");
        assert!(
            script.contains("--event \"$GITHUB_EVENT_NAME\""),
            "{script}"
        );
        assert_eq!(
            asking.env(),
            [("GH_TOKEN".to_string(), "${{ github.token }}".to_string())]
        );
    }

    #[test]
    fn builds_the_tool_with_the_lockfile_where_no_secret_is_before_it_asks() {
        let gate = gate();
        assert!(!gate.contains("secrets."), "the gate holds a secret");
        assert!(
            !gate.contains("environment:"),
            "the gate is in the release environment"
        );
        let steps = steps(&gate);
        let built = builder_of(&steps);
        let asked = steps
            .iter()
            .position(|step| step.name() == CI_PASSED)
            .expect("the gate asks CI");
        assert!(
            built < asked,
            "the tool is built after the step that runs it"
        );
        assert!(!goes_through_cargo(&steps[asked].text()));
        // Its token reads the runs of CI, and writes nothing.
        let permissions: Vec<&str> = gate
            .split("\n    permissions:\n")
            .nth(1)
            .expect("the gate names its permissions")
            .lines()
            .take_while(|line| line.starts_with("      "))
            .map(str::trim)
            .collect();
        assert_eq!(permissions, ["actions: read", "contents: read"]);
        assert!(gate.contains("persist-credentials: false"), "{gate}");
    }

    #[test]
    fn is_what_the_publish_job_waits_for() {
        let publish = job(&read("release.yml"), "publish");
        let needs = publish
            .lines()
            .find_map(|line| line.strip_prefix("    needs: "))
            .unwrap();
        assert!(needs.contains("gate"), "{needs}");
    }

    #[test]
    fn leaves_ci_a_workflow_no_other_one_calls_and_runs_it_on_a_promotions_branch() {
        for file in files() {
            assert!(
                !read(&file).contains("uses: ./.github/workflows/ci.yml"),
                "{file} runs CI's gate"
            );
        }
        let ci = read("ci.yml");
        let on = ci
            .split("\non:\n")
            .nth(1)
            .expect("ci.yml says when it runs");
        let on: Vec<&str> = on
            .lines()
            .take_while(|line| line.is_empty() || line.starts_with("  "))
            .collect();
        assert!(
            !on.iter().any(|line| line.trim() == "workflow_call:"),
            "{on:?}"
        );
        assert!(
            on.contains(&"    branches: [main, 'release/*']"),
            "a promotion's branch runs no CI: {on:?}"
        );
    }
}

mod the_steps_of_a_job {
    use super::*;

    const JOB: &str = "\
  mac:
    runs-on: macos-latest
    steps:
      # A comment before the first step.
      - uses: actions/checkout@v7
        with:
          fetch-depth: 0
      # A comment between two, which belongs to neither.
      - name: Build
        run: cargo build
      - name: Sign
        env:
          KEY: ${{ secrets.KEY }}
        run: sign

  windows:
    uses: ./.github/workflows/windows-build.yml
";

    #[test]
    fn are_read_off_with_their_comments_left_out_and_each_ends_where_the_next_begins() {
        let found = steps(JOB);
        assert_eq!(
            found.iter().map(StepText::name).collect::<Vec<_>>(),
            ["uses: actions/checkout@v7", "Build", "Sign"]
        );
        assert_eq!(
            found[0].text(),
            "      - uses: actions/checkout@v7\n        with:\n          fetch-depth: 0"
        );
        assert_eq!(
            found[1].text(),
            "      - name: Build\n        run: cargo build"
        );
        assert!(holds_a_key(&found[2]));
        assert!(!holds_a_key(&found[1]));
        // The next job is no part of the last step, nor are the blank lines before it.
        assert!(!found[2].text().contains("windows"), "{}", found[2].text());
    }

    #[test]
    fn go_through_cargo_by_the_word_and_not_by_a_longer_one() {
        assert!(goes_through_cargo("run: cargo build"));
        assert!(goes_through_cargo("$(cargo)"));
        assert!(!goes_through_cargo("run: ./cargo_wrapper"));
        assert!(!goes_through_cargo("run: cargoes"));
    }
}

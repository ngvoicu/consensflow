//! The packaged job of CI (.github/workflows/ci.yml, T-904): the app built as a
//! download is (ad hoc: no key reaches a build of CI) and run as a user would,
//! on every run of the gate, so that a regression only a built app shows (alpha.3's
//! board case, T-863) is found at the push that made it, with the merge named, and
//! not by the release's run after its tag is pushed. The job's text is held here;
//! that the gate waits for it is `xtask`'s test of the gate's `needs`
//! (`the_gate_is_green_only_when_every_other_job_of_ci_passed`), and what the smoke
//! does is the smoke's own.

// A failure in a test is the test's answer, in its helpers too.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::fs;
use std::path::Path;

use cf_publish::testing::workflow::{job, job_names, live, read, steps, StepText};

/// The job's id in the workflow.
const JOB: &str = "packaged";

/// The identity of every app a smoke builds or starts (`cf_base::bundle::SMOKE`): never the
/// live app's, which to macOS any copy started with it is.
const SMOKE_IDENTITY: &str = "dev.ngvoicu.consensflow.smoke";

/// What the build step runs: the app alone (the DMG is the release's), of the smoke's identity.
const BUILD: &str = "npm --prefix app run build -- --bundles app --config '{\"identifier\":\"dev.ngvoicu.consensflow.smoke\"}'";

/// Where that build leaves the app.
const BUILT: &str = "app/src-tauri/target/release/bundle/macos/ConsensFlow.app";

/// The variable `cargo xtask smoke` reads the cases to leave out from (tools/xtask/src/smoke.rs).
const SKIP: &str = "CONSENSFLOW_SMOKE_SKIP";

/// The case the job leaves out: the app hidden while a window's agent floods it, held to read the
/// flood in time where the host does not wait on its page for each message (alpha.5's host-display
/// core, A5-04). The name of the case's function in crates/cf-e2e/tests/smoke/hidden.rs.
const HIDDEN: &str = "the_built_app_reads_its_windows_while_it_is_hidden";

/// The names of the smoke's cases, the ones its test ignores for there being no built app to give
/// them: each `#[ignore…]` and the function after it, in the smoke test's root and its parts.
fn smoke_cases() -> Vec<String> {
    let tests = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/cf-e2e/tests");
    let mut files = vec![tests.join("smoke.rs")];
    let parts = fs::read_dir(tests.join("smoke")).expect("the smoke's parts");
    files.extend(parts.map(|part| part.expect("a part").path()));
    let mut cases = Vec::new();
    for file in files {
        let text = fs::read_to_string(&file).expect("a file of the smoke");
        let mut ignored = false;
        for line in text.lines() {
            if line.starts_with("#[ignore") {
                ignored = true;
            } else if let Some(rest) = line.strip_prefix("fn ").filter(|_| ignored) {
                cases.push(rest.split('(').next().expect("a name").to_string());
                ignored = false;
            }
        }
    }
    cases
}

/// The job's text, comments left out.
fn packaged() -> String {
    live(&job(&read("ci.yml"), JOB))
}

/// The job's steps, in order.
fn the_steps() -> Vec<StepText> {
    steps(&packaged())
}

/// The step whose script holds `text`, the only one that does.
fn running(text: &str) -> (usize, StepText) {
    let found: Vec<(usize, StepText)> = the_steps()
        .into_iter()
        .enumerate()
        .filter(|(_, step)| step.text().contains(text))
        .collect();
    assert_eq!(found.len(), 1, "{} steps hold {text}", found.len());
    found.into_iter().next().unwrap()
}

/// A limit of the workflow, in minutes, as `timeout-minutes` says it.
fn minutes(text: &str) -> u64 {
    text.trim().parse().expect("a number of minutes")
}

mod the_packaged_job_of_ci {
    use super::*;

    #[test]
    fn runs_on_the_mac_alone_in_a_job_of_its_own() {
        let job = packaged();
        assert!(
            job.lines().any(|line| line == "    runs-on: macos-latest"),
            "{job}"
        );
        // A matrix is a job a platform: the packaged app is a Mac bundle, and the Windows one is
        // held by its own script.
        assert!(!job.contains("matrix"), "{job}");
        // (Not the word alone: the case the job leaves out has windows in its name.)
        assert!(!job.contains("windows-latest"), "{job}");
        assert!(!job.contains("Windows"), "{job}");
    }

    #[test]
    fn starts_from_the_setup_of_the_other_jobs_so_that_its_build_is_cached_as_theirs_are() {
        let steps = the_steps();
        assert_eq!(steps[0].text().trim(), "- uses: actions/checkout@v7");
        let setup = &steps[1];
        assert!(
            setup
                .text()
                .starts_with("      - uses: ./.github/actions/setup"),
            "{}",
            setup.text()
        );
        // A red smoke is this job's reason to be, and the build it fails after is the minutes:
        // the cache is saved whatever the smoke says.
        assert!(
            setup.text().contains("          cache-on-failure: true"),
            "{}",
            setup.text()
        );
    }

    #[test]
    fn builds_the_app_ad_hoc_and_of_the_smokes_identity_not_the_release_s() {
        let (_, build) = running("npm --prefix app run build");
        assert_eq!(build.script(), BUILD);
        // The app alone: the DMG is the release's, and costs a minute a push.
        assert!(build.script().contains("--bundles app"));
        // The identity is the one every smoke build has, in the command a person may copy.
        assert!(build.script().contains(SMOKE_IDENTITY));
        // Ad hoc: the product's configuration signs with "-", and nothing here asks for more.
        assert!(!build.text().contains("APPLE_"), "{}", build.text());
        assert!(!build.text().contains("TAURI_SIGNING"), "{}", build.text());
    }

    #[test]
    fn smokes_the_app_it_built_after_building_it() {
        let (built_at, _) = running("npm --prefix app run build");
        let (smoked_at, smoke) = running("cargo xtask smoke");
        assert!(built_at < smoked_at, "{built_at} {smoked_at}");
        // The app the build leaves, named: not whatever bundle the smoke finds.
        assert_eq!(smoke.script(), format!("cargo xtask smoke --app {BUILT}"));
        // And after it only what runs for its failure: a step that follows a smoke and runs for
        // a pass is one the smoke's result does not reach.
        for step in &the_steps()[smoked_at + 1..] {
            assert!(step.text().contains("if: failure()"), "{}", step.text());
        }
    }

    #[test]
    fn leaves_out_the_hidden_windows_case_and_no_other_until_the_host_stops_waiting_on_its_page() {
        let (_, smoke) = running("cargo xtask smoke");
        // One variable, the one the command reads, with the one case in it. The command has no
        // option for it, so the script stays as the test above holds it.
        assert_eq!(
            smoke.env(),
            [(SKIP.to_string(), HIDDEN.to_string())],
            "{}",
            smoke.text()
        );
        // It names a case there is, which the test program matches by part of a path: one that
        // is no case's name leaves nothing out, and one that is part of another's leaves that out
        // too.
        let cases = smoke_cases();
        assert_eq!(
            cases.iter().filter(|case| case.contains(HIDDEN)).count(),
            1,
            "{cases:?}"
        );
        assert!(cases.iter().any(|case| case == HIDDEN), "{cases:?}");
        // The cases that stay are the pane case and the app's answer to a command that panics.
        assert!(cases.len() >= 3, "{cases:?}");
        // And the line says what it waits for, in words the next person who reads it finds
        // (comments are left out of the job's text above, so the whole file is read).
        let ci = read("ci.yml");
        let said = ci.lines().filter(|line| line.contains("A5-04")).count();
        assert!(said >= 1, "ci.yml does not say what the case waits for");
    }

    #[test]
    fn keeps_the_machines_of_a_failed_smoke_which_go_with_the_runner() {
        let steps = the_steps();
        let (smoked_at, _) = running("cargo xtask smoke");
        let after = &steps[smoked_at + 1..];
        // The folder is named as the Rust that wrote there does (cf-e2e's `temp_dir`), by
        // Node, which reads the same variable; and the upload takes what the smoke called
        // its machines, as the rig's reports are taken.
        let naming = after
            .iter()
            .find(|step| step.text().contains("SMOKE_TEMP="))
            .expect("a step names the folder");
        assert!(
            naming
                .script()
                .contains("node -p \"require('os').tmpdir()\""),
            "{}",
            naming.text()
        );
        assert!(naming.text().contains("if: failure()"));
        let upload = after
            .iter()
            .find(|step| step.text().contains("actions/upload-artifact@"))
            .expect("a step uploads the machines");
        assert!(upload.text().contains("if: failure()"), "{}", upload.text());
        assert!(
            upload
                .text()
                .contains("path: ${{ env.SMOKE_TEMP }}/cf-smoke-*"),
            "{}",
            upload.text()
        );
        // A pass keeps none, and a failure that kept none says nothing.
        assert!(
            upload.text().contains("if-no-files-found: ignore"),
            "{}",
            upload.text()
        );
        // The action is the one the daemons' job uploads with, pinned to its commit.
        assert!(
            upload
                .text()
                .contains("actions/upload-artifact@cf430e030ddbb5b0abf93d22962f4752f3646cd9"),
            "{}",
            upload.text()
        );
    }

    #[test]
    fn does_not_ask_for_the_bundle_as_it_ships_which_the_release_run_alone_does() {
        // CONSENSFLOW_SMOKE_SHIPPED has the smoke start the bundle itself and not a copy of the
        // smoke's identity (T-860): where the signed app is, on GitHub's runner, in the release
        // workflow. A run of CI starts a bundle of the smoke's identity, as every other does.
        let ci = live(&read("ci.yml"));
        assert!(
            !ci.contains("CONSENSFLOW_SMOKE_SHIPPED"),
            "ci.yml asks for it"
        );
    }

    #[test]
    fn holds_no_key_and_no_environment_that_does() {
        let job = packaged();
        assert!(!job.contains("secrets."), "{job}");
        assert!(!job.contains("environment:"), "{job}");
    }

    #[test]
    fn gives_a_limit_to_the_job_and_to_each_step_that_takes_minutes() {
        let job = packaged();
        let own = job
            .lines()
            .find_map(|line| line.strip_prefix("    timeout-minutes: "))
            .expect("the job has a limit");
        let (_, build) = running("npm --prefix app run build");
        let (_, smoke) = running("cargo xtask smoke");
        let build_limit = minutes(&build.field("timeout-minutes").expect("the build's limit"));
        let smoke_limit = minutes(&smoke.field("timeout-minutes").expect("the smoke's limit"));
        // `cargo xtask ci-times` holds each to a share of the limit it is given, so each is a bound
        // set over what the step takes; the job's, over the two and the setup between them.
        assert!(
            minutes(own) > build_limit.max(smoke_limit),
            "{own} {build_limit} {smoke_limit}"
        );
    }
}

mod the_setup_of_the_gate {
    use std::fs;
    use std::path::Path;

    use super::*;

    /// The action every job of the gate starts from, comments left out.
    fn setup() -> String {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.github/actions/setup/action.yml");
        live(&fs::read_to_string(path).unwrap())
    }

    #[test]
    fn saves_the_build_of_a_failed_job_only_where_the_job_asks_for_it() {
        let setup = setup();
        // Off unless asked: the other jobs keep the cache of a green job alone, as they did.
        assert!(
            setup.contains("  cache-on-failure:\n    description:"),
            "{setup}"
        );
        assert!(setup.contains("    default: 'false'"), "{setup}");
        // And handed to the cache, which is what saves it.
        assert!(
            setup.contains("        cache-on-failure: ${{ inputs.cache-on-failure }}"),
            "{setup}"
        );
        // Every other job of the workflow, whichever there are.
        let ci = read("ci.yml");
        let others: Vec<String> = job_names(&ci)
            .into_iter()
            .filter(|name| name != JOB)
            .collect();
        assert!(others.len() >= 4, "{others:?}");
        let asking: Vec<&String> = others
            .iter()
            .filter(|name| live(&job(&ci, name)).contains("cache-on-failure"))
            .collect();
        assert!(asking.is_empty(), "{asking:?}");
    }
}

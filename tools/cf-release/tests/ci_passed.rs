//! The release workflow's gate (.github/workflows/release.yml, job `gate`), its
//! step run as written: this crate's own `cf-release` where the job's build
//! leaves it, and a `gh` first on the PATH that answers the Actions API from
//! files, as GitHub would for the commit. A tag on a commit CI did not pass on
//! both platforms fails the step, so `publish`, which needs it, never runs; a
//! hand run is told and not failed. The rule's own cases are the module's
//! tests (src/ci_passed/tests.rs). The release jobs run on Linux: so does this.
#![cfg(unix)]
// A failure in a test is the test's answer, in its helpers too.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::ffi::{OsStr, OsString};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use cf_base::env::Env;
use cf_release::process;

/// The step of the gate job that asks.
const STEP: &str = "      - name: CI passed this commit on both platforms";

const COMMIT: &str = "2819dc1f0a6b4c3d5e7f8091a2b3c4d5e6f70819";
const OTHER: &str = "a09025c1aaaabbbbccccddddeeeeffff00001111";
const REPOSITORY: &str = "ngvoicu/consensflow";

/// GitHub's `gh api`, answered from the files in `$FAKE`: `runs.json` for CI's
/// runs, `jobs-<id>.json` for a run's jobs; every question kept in `asked`.
const GH: &str = r#"#!/bin/bash
echo "$*" >> "$FAKE/asked"
case "$2" in
  */actions/workflows/ci.yml/runs\?*) cat "$FAKE/runs.json" ;;
  */actions/runs/*/jobs\?*)
    id="${2#*/actions/runs/}"
    cat "$FAKE/jobs-${id%%/*}.json" ;;
  *) echo "gh: no answer for $2" >&2; exit 1 ;;
esac
"#;

/// The script of the gate's step, as the workflow writes it.
fn script() -> String {
    let workflow = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join(".github")
            .join("workflows")
            .join("release.yml"),
    )
    .unwrap();
    let gate = workflow
        .split("\n  gate:\n")
        .nth(1)
        .expect("the release workflow has a job gate");
    let step = gate.split(STEP).nth(1).expect("the gate has the step");
    let run = step.split("        run: |\n").nth(1).expect("its run");
    run.lines()
        .take_while(|line| line.starts_with("          ") || line.is_empty())
        .map(|line| line.get(10..).unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

/// A runner's checkout: the tool where the build leaves it, `gh` on a PATH of its own, and its answers.
struct Runner {
    _temp: tempfile::TempDir,
    root: PathBuf,
    bin: PathBuf,
    fake: PathBuf,
}

impl Runner {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("checkout");
        let release = root.join("app/src-tauri/target/release");
        fs::create_dir_all(&release).unwrap();
        std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_cf-release"), release.join("cf-release"))
            .unwrap();
        let bin = temp.path().join("bin");
        let fake = temp.path().join("github");
        fs::create_dir_all(&bin).unwrap();
        fs::create_dir_all(&fake).unwrap();
        let gh = bin.join("gh");
        fs::write(&gh, GH).unwrap();
        fs::set_permissions(&gh, fs::Permissions::from_mode(0o755)).unwrap();
        let runner = Self {
            _temp: temp,
            root,
            bin,
            fake,
        };
        runner.runs(&[]);
        runner
    }

    /// CI's runs GitHub lists: id, commit, event, status, conclusion.
    fn runs(&self, runs: &[(u64, &str, &str, &str, &str)]) {
        let listed: Vec<serde_json::Value> = runs
            .iter()
            .map(|(id, commit, event, status, conclusion)| {
                serde_json::json!({
                    "id": id, "head_sha": commit, "event": event,
                    "status": status, "conclusion": conclusion,
                })
            })
            .collect();
        let body = serde_json::json!({ "total_count": listed.len(), "workflow_runs": listed });
        fs::write(self.fake.join("runs.json"), body.to_string()).unwrap();
    }

    /// The jobs of run `id`, by name and conclusion.
    fn jobs(&self, id: u64, jobs: &[(&str, &str)]) {
        let listed: Vec<serde_json::Value> = jobs
            .iter()
            .map(|(name, conclusion)| serde_json::json!({ "name": name, "conclusion": conclusion }))
            .collect();
        let body = serde_json::json!({ "total_count": listed.len(), "jobs": listed });
        fs::write(self.fake.join(format!("jobs-{id}.json")), body.to_string()).unwrap();
    }

    /// The step, run by `event` in the checkout: its status, what it printed, what it said.
    fn run(&self, event: &str) -> (i32, String, String) {
        let mut path = OsString::from(&self.bin);
        if let Some(rest) = Env::from_process().search_path() {
            path.push(":");
            path.push(rest);
        }
        let env = Env::from_vars([
            (OsString::from("PATH"), path),
            ("FAKE".into(), self.fake.clone().into()),
            ("CHECKOUT".into(), self.root.clone().into()),
            ("GH_TOKEN".into(), "a token".into()),
            ("GITHUB_REPOSITORY".into(), REPOSITORY.into()),
            ("GITHUB_SHA".into(), COMMIT.into()),
            ("GITHUB_EVENT_NAME".into(), event.into()),
        ]);
        // A step runs in the checkout, under bash -e as GitHub runs it.
        let line = format!("cd \"$CHECKOUT\"\n{}", script());
        let args = [OsString::from("-e"), "-c".into(), line.into()];
        let ran = process::capture(OsStr::new("bash"), &args, &env).unwrap();
        (ran.code, ran.stdout, ran.stderr)
    }

    fn asked(&self) -> String {
        fs::read_to_string(self.fake.join("asked")).unwrap_or_default()
    }
}

const GREEN: [(&str, &str); 3] = [
    ("test (macos-latest, rig)", "success"),
    ("gate (macos-latest)", "success"),
    ("gate (windows-latest)", "success"),
];

#[test]
fn a_tag_on_a_commit_ci_passed_on_both_platforms_goes_on() {
    let runner = Runner::new();
    runner.runs(&[(7, COMMIT, "push", "completed", "success")]);
    runner.jobs(7, &GREEN);
    let (status, out, err) = runner.run("push");
    assert_eq!(status, 0, "{err}");
    assert!(
        out.starts_with(&format!("CI passed {COMMIT} on both platforms: run 7")),
        "{out}"
    );
    // The commit asked for is the run's own, by the token the step was given.
    assert!(
        runner.asked().starts_with(&format!(
            "api repos/{REPOSITORY}/actions/workflows/ci.yml/runs?head_sha={COMMIT}&"
        )),
        "{}",
        runner.asked()
    );
}

#[test]
fn a_tag_on_a_commit_whose_ci_is_red_fails_the_gate() {
    let runner = Runner::new();
    runner.runs(&[(7, COMMIT, "push", "completed", "failure")]);
    let (status, out, err) = runner.run("push");
    assert_eq!((status, out.as_str()), (1, ""), "{err}");
    assert!(
        err.starts_with(&format!(
            "cf-release ci-passed: CI did not pass {} on both platforms: run 7 ended failure.",
            &COMMIT[..8]
        )),
        "{err}"
    );
}

#[test]
fn a_tag_on_a_commit_ci_never_ran_on_fails_the_gate() {
    let runner = Runner::new();
    let (status, _, err) = runner.run("push");
    assert_eq!(status, 1, "{err}");
    assert!(
        err.starts_with(&format!(
            "cf-release ci-passed: CI (ci.yml) never ran on {}: ",
            &COMMIT[..8]
        )),
        "{err}"
    );
}

#[test]
fn a_tag_on_a_commit_whose_green_ci_is_another_commits_fails_the_gate() {
    let runner = Runner::new();
    runner.runs(&[(7, OTHER, "push", "completed", "success")]);
    runner.jobs(7, &GREEN);
    let (status, _, err) = runner.run("push");
    assert_eq!(status, 1, "{err}");
    assert!(err.contains("CI (ci.yml) never ran on 2819dc1f"), "{err}");
}

#[test]
fn a_tag_on_a_commit_without_a_platforms_summary_fails_the_gate() {
    let runner = Runner::new();
    runner.runs(&[(7, COMMIT, "push", "completed", "success")]);
    runner.jobs(7, &GREEN[..2]);
    let (status, _, err) = runner.run("push");
    assert_eq!(status, 1, "{err}");
    assert!(
        err.contains("run 7 ended success, but it has no job gate (windows-latest)"),
        "{err}"
    );
}

#[test]
fn a_hand_run_is_told_what_a_tag_would_be_refused_for_and_goes_on() {
    let runner = Runner::new();
    runner.runs(&[(7, COMMIT, "push", "completed", "failure")]);
    let (status, out, err) = runner.run("workflow_dispatch");
    assert_eq!((status, out.as_str()), (0, ""), "{err}");
    assert!(
        err.starts_with(&format!(
            "a run by workflow_dispatch publishes nothing; a tag on {COMMIT} would be refused: CI did not pass"
        )),
        "{err}"
    );
}

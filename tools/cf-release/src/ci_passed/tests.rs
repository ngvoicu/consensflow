//! `ci-passed` against a GitHub that is a script: the runs of CI it lists for a
//! commit, and each run's jobs, as the Actions API answers them.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde_json::json;

use super::*;

const COMMIT: &str = "2819dc1f0a6b4c3d5e7f8091a2b3c4d5e6f70819";
const OTHER: &str = "a09025c1aaaabbbbccccddddeeeeffff00001111";
const REPOSITORY: &str = "ngvoicu/consensflow";

/// A GitHub that answers what the test gave it, and keeps what it was asked.
#[derive(Default)]
struct Script {
    /// Each run of CI it lists, whatever commit it is asked for: GitHub's filter
    /// is its own, and the step holds each run to the commit itself.
    runs: Vec<Value>,
    /// Each run's jobs, by its id.
    jobs: BTreeMap<u64, Vec<Value>>,
    /// What every question is answered with instead, as a gh that cannot reach GitHub.
    fails: Option<&'static str>,
    asked: Vec<String>,
}

impl Script {
    fn run(self, id: u64, commit: &str, event: &str, ended: &str) -> Self {
        self.run_from(REPOSITORY, id, commit, event, ended)
    }

    /// A run whose commit came from the repository `from`.
    fn run_from(mut self, from: &str, id: u64, commit: &str, event: &str, ended: &str) -> Self {
        let (status, conclusion) = match ended {
            "in_progress" | "queued" => (ended, Value::Null),
            _ => ("completed", json!(ended)),
        };
        self.runs.push(json!({
            "id": id, "head_sha": commit, "event": event,
            "head_repository": { "full_name": from },
            "status": status, "conclusion": conclusion,
        }));
        self
    }

    fn jobs(mut self, id: u64, jobs: &[(&str, &str)]) -> Self {
        let listed = jobs
            .iter()
            .map(|(name, conclusion)| json!({ "name": name, "conclusion": conclusion }))
            .collect();
        self.jobs.insert(id, listed);
        self
    }

    /// A run of CI on `commit` by `event` that ended `ended`, its two summaries `summaries`.
    fn ci(self, id: u64, commit: &str, event: &str, ended: &str, summaries: &str) -> Self {
        self.run(id, commit, event, ended).jobs(
            id,
            &[
                ("test (macos-latest, rig)", summaries),
                ("gate (macos-latest)", summaries),
                ("test (windows-latest, rig)", summaries),
                ("gate (windows-latest)", summaries),
            ],
        )
    }
}

/// One page of `items` as GitHub gives it: the `page`th of `per_page`, under `key`.
fn page(key: &str, items: &[Value], page: usize, per_page: usize) -> String {
    let start = (page - 1) * per_page;
    let slice: Vec<Value> = items.iter().skip(start).take(per_page).cloned().collect();
    json!({ "total_count": items.len(), key: slice }).to_string()
}

/// The `name=value` words of the query of `path`.
fn query(path: &str) -> BTreeMap<String, String> {
    path.split_once('?')
        .map(|(_, query)| query)
        .unwrap_or_default()
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .map(|(name, value)| (name.to_owned(), value.to_owned()))
        .collect()
}

impl GitHub for Script {
    fn get(&mut self, path: &str) -> Result<String, String> {
        self.asked.push(path.to_owned());
        if let Some(said) = self.fails {
            return Err(said.to_owned());
        }
        let words = query(path);
        let at = |name: &str| words.get(name).and_then(|value| value.parse().ok());
        let (number, per_page) = (at("page").unwrap_or(1), at("per_page").unwrap_or(30));
        let runs = format!("repos/{REPOSITORY}/actions/workflows/ci.yml/runs?");
        if path.starts_with(&runs) {
            return Ok(page("workflow_runs", &self.runs, number, per_page));
        }
        let id: u64 = path
            .strip_prefix(&format!("repos/{REPOSITORY}/actions/runs/"))
            .and_then(|rest| rest.split('/').next())
            .and_then(|id| id.parse().ok())
            .unwrap_or_else(|| panic!("GitHub's script does not answer {path}"));
        let jobs = self.jobs.get(&id).cloned().unwrap_or_default();
        Ok(page("jobs", &jobs, number, per_page))
    }
}

/// What the step says of `COMMIT` on `script`, run by `event`: its status as
/// cf-release ends it, what it printed and what it said.
fn answered(script: &mut Script, event: Option<&str>) -> (Result<(), String>, String, String) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let mut console = Console {
        out: &mut out,
        err: &mut err,
    };
    let result =
        answer(script, REPOSITORY, COMMIT, event, &mut console).map_err(|failure| match failure {
            Failure::Failed(said) => said,
            other => panic!("not a refusal: {other}"),
        });
    (
        result,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

fn refusal(script: &mut Script) -> String {
    match answered(script, Some("push")).0 {
        Ok(()) => panic!("it passed"),
        Err(said) => said,
    }
}

#[test]
fn a_commit_whose_ci_run_passed_both_platforms_passes_and_says_which_run() {
    let mut script = Script::default().ci(7, COMMIT, "push", "success", "success");
    let (result, out, err) = answered(&mut script, Some("push"));
    result.unwrap();
    assert_eq!(
        out,
        format!(
            "CI passed {COMMIT} on both platforms: run 7, https://github.com/{REPOSITORY}/actions/runs/7, \
             with gate (macos-latest) and gate (windows-latest) green\n"
        )
    );
    assert_eq!(err, "");
    // CI is asked for by its file and the commit, then the run's jobs.
    assert_eq!(
        script.asked,
        [
            format!(
                "repos/{REPOSITORY}/actions/workflows/ci.yml/runs?head_sha={COMMIT}&per_page=100&page=1"
            ),
            format!("repos/{REPOSITORY}/actions/runs/7/jobs?per_page=100&page=1"),
        ]
    );
}

#[test]
fn ci_that_ended_red_is_refused() {
    for ended in ["failure", "cancelled", "timed_out"] {
        let mut script = Script::default().ci(7, COMMIT, "push", ended, "failure");
        let said = refusal(&mut script);
        assert!(
            said.starts_with(&format!(
                "CI did not pass 2819dc1f on both platforms: run 7 ended {ended}. Nothing is published"
            )),
            "{said}"
        );
        assert!(
            said.contains("`gh run rerun 7 --failed --repo ngvoicu/consensflow`"),
            "{said}"
        );
    }
}

#[test]
fn a_commit_ci_never_ran_on_is_refused() {
    let mut script = Script::default();
    let said = refusal(&mut script);
    assert_eq!(
        said,
        "CI (ci.yml) never ran on 2819dc1f: nothing is published unless CI's full gate passed this very \
         commit on macOS and Windows. Run CI on it, by a push to main or to a release/ branch or by hand \
         (`gh workflow run ci.yml --ref <branch> --repo ngvoicu/consensflow`), and once it is green rerun \
         this run's failed jobs"
    );
}

#[test]
fn ci_green_on_another_commit_is_not_this_ones() {
    // GitHub answers a green run of another commit (its parent, main's head):
    // the step holds the run to the commit, not to GitHub's filter.
    let mut script = Script::default().ci(7, OTHER, "push", "success", "success");
    let said = refusal(&mut script);
    assert!(
        said.starts_with("CI (ci.yml) never ran on 2819dc1f: "),
        "{said}"
    );
    assert!(
        script
            .asked
            .iter()
            .all(|path| !path.contains("/runs/7/jobs")),
        "{:?}",
        script.asked
    );
}

#[test]
fn a_run_without_a_platforms_summary_is_refused() {
    let mut script = Script::default().run(7, COMMIT, "push", "success").jobs(
        7,
        &[
            ("gate (macos-latest)", "success"),
            ("test (windows-latest, rig)", "success"),
        ],
    );
    assert!(refusal(&mut script).starts_with(
        "CI did not pass 2819dc1f on both platforms: run 7 ended success, but it has no job gate (windows-latest)."
    ));
    // The jobs of no platform at all.
    let mut script = Script::default()
        .run(7, COMMIT, "push", "success")
        .jobs(7, &[]);
    assert!(refusal(&mut script).contains(
        "run 7 ended success, but it has no job gate (macos-latest), it has no job gate (windows-latest)"
    ));
}

#[test]
fn a_summary_that_did_not_pass_is_refused_though_the_run_says_success() {
    for ended in ["skipped", "failure", "cancelled"] {
        let mut script = Script::default().run(7, COMMIT, "push", "success").jobs(
            7,
            &[
                ("gate (macos-latest)", "success"),
                ("gate (windows-latest)", ended),
            ],
        );
        assert!(
            refusal(&mut script).contains(&format!(
                "run 7 ended success, but gate (windows-latest) ended {ended}"
            )),
            "{ended}"
        );
    }
}

#[test]
fn a_summary_is_its_name_whole_not_a_job_that_holds_it() {
    let mut script = Script::default().run(7, COMMIT, "push", "success").jobs(
        7,
        &[
            ("gate (macos-latest)", "success"),
            ("gate (windows-latest) shard 1", "success"),
            ("release / gate (windows-latest)", "success"),
        ],
    );
    assert!(refusal(&mut script).contains("it has no job gate (windows-latest)"));
}

#[test]
fn a_pull_requests_run_tested_a_merge_and_is_not_ci_of_the_commit() {
    let mut script = Script::default().ci(7, COMMIT, "pull_request", "success", "success");
    let said = refusal(&mut script);
    assert!(
        said.starts_with(
            "CI (ci.yml) never ran on 2819dc1f (1 run of a pull request tested a merge of it, which is not it): "
        ),
        "{said}"
    );
}

#[test]
fn a_run_by_hand_is_ci_of_the_commit_as_a_push_is() {
    let mut script = Script::default().ci(7, COMMIT, "workflow_dispatch", "success", "success");
    answered(&mut script, Some("push")).0.unwrap();
}

#[test]
fn a_run_still_going_passes_nothing_yet() {
    for status in ["in_progress", "queued"] {
        let mut script = Script::default().run(7, COMMIT, "push", status);
        let said = refusal(&mut script);
        assert_eq!(
            said,
            format!(
                "CI did not pass 2819dc1f on both platforms: run 7 is {status}. Nothing is published unless \
                 CI's full gate passed this very commit on macOS and Windows. Once CI's run ends green, rerun \
                 this run's failed jobs"
            )
        );
    }
}

#[test]
fn one_green_run_of_the_commit_passes_it_beside_red_ones() {
    let mut script = Script::default()
        .ci(9, COMMIT, "workflow_dispatch", "failure", "failure")
        .ci(8, COMMIT, "push", "success", "success")
        .run(7, COMMIT, "push", "in_progress");
    let (result, out, _) = answered(&mut script, Some("push"));
    result.unwrap();
    assert!(out.contains("run 8,"), "{out}");
    // Each red run is told when none passed.
    let mut script = Script::default()
        .ci(9, COMMIT, "push", "failure", "failure")
        .ci(8, COMMIT, "push", "success", "skipped");
    let said = refusal(&mut script);
    assert!(
        said.starts_with(
            "CI did not pass 2819dc1f on both platforms: run 9 ended failure; run 8 ended success, but \
             gate (macos-latest) ended skipped, gate (windows-latest) ended skipped. "
        ),
        "{said}"
    );
}

#[test]
fn every_page_of_a_runs_jobs_is_read() {
    // The summaries come after 150 jobs of shards: the second page holds them.
    let mut jobs: Vec<(String, String)> = (0..150)
        .map(|at| (format!("test (shard {at})"), "success".to_owned()))
        .collect();
    jobs.push(("gate (macos-latest)".into(), "success".into()));
    jobs.push(("gate (windows-latest)".into(), "success".into()));
    let borrowed: Vec<(&str, &str)> = jobs
        .iter()
        .map(|(name, ended)| (name.as_str(), ended.as_str()))
        .collect();
    let mut script = Script::default()
        .run(7, COMMIT, "push", "success")
        .jobs(7, &borrowed);
    answered(&mut script, Some("push")).0.unwrap();
    assert!(
        script.asked.contains(&format!(
            "repos/{REPOSITORY}/actions/runs/7/jobs?per_page=100&page=2"
        )),
        "{:?}",
        script.asked
    );
}

#[test]
fn a_github_that_cannot_be_asked_is_a_refusal_with_what_gh_said() {
    let mut script = Script {
        fails: Some("gh api ended 1: HTTP 403: Resource not accessible by integration"),
        ..Script::default()
    };
    let said = refusal(&mut script);
    assert_eq!(
        said,
        format!(
            "GitHub could not be asked for repos/{REPOSITORY}/actions/workflows/ci.yml/runs?head_sha={COMMIT}\
             &per_page=100&page=1: gh api ended 1: HTTP 403: Resource not accessible by integration: nothing \
             is published unless CI's full gate passed this very commit on macOS and Windows, and whether CI \
             passed it could not be told. Read the job's token and permissions (actions: read), then rerun \
             this run's failed jobs"
        )
    );
}

#[test]
fn a_github_that_cannot_be_asked_fails_a_hand_run_too() {
    // The hand run is the trial of the job's token: a 403 there is no green job.
    for fails in [
        "gh api ended 1: HTTP 403: Resource not accessible by integration",
        "gh was not found: is it installed, and on the PATH?",
    ] {
        let mut script = Script {
            fails: Some(fails),
            ..Script::default()
        };
        let (result, out, _) = answered(&mut script, Some("workflow_dispatch"));
        let said = result.expect_err("a hand run that could not ask GitHub went on");
        assert!(said.starts_with("GitHub could not be asked for "), "{said}");
        assert!(
            out.starts_with("::error::GitHub could not be asked for "),
            "{out}"
        );
    }
    // An answer that cannot be read is no answer either.
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let mut console = Console {
        out: &mut out,
        err: &mut err,
    };
    let failure = answer(
        &mut NoJson,
        REPOSITORY,
        COMMIT,
        Some("workflow_dispatch"),
        &mut console,
    )
    .expect_err("an answer that could not be read passed a hand run");
    assert!(
        failure.to_string().contains("could not be read"),
        "{failure}"
    );
}

/// A GitHub whose every answer is no JSON, as a proxy's page.
struct NoJson;

impl GitHub for NoJson {
    fn get(&mut self, _path: &str) -> Result<String, String> {
        Ok("<html>a proxy's page</html>".to_owned())
    }
}

#[test]
fn a_refusal_is_written_as_an_error_on_the_runs_page_and_a_hand_runs_as_a_warning() {
    let mut script = Script::default().ci(7, COMMIT, "push", "failure", "failure");
    let (result, out, _) = answered(&mut script, Some("push"));
    let said = result.unwrap_err();
    assert_eq!(out, format!("::error::{said}\n"));
    let mut script = Script::default().ci(7, COMMIT, "push", "failure", "failure");
    let (result, out, err) = answered(&mut script, Some("workflow_dispatch"));
    result.unwrap();
    assert_eq!(out, format!("::warning::{err}"));
}

#[test]
fn what_would_end_a_workflow_command_is_written_as_the_runner_reads_it_back() {
    assert_eq!(
        annotation("error", "100% red\r\nthen more"),
        "::error::100%25 red%0D%0Athen more"
    );
}

#[test]
fn a_run_of_a_commit_from_another_repository_is_not_ci_of_it() {
    let green = [
        ("gate (macos-latest)", "success"),
        ("gate (windows-latest)", "success"),
    ];
    // The same commit, pushed in a fork: its CI is the fork's, not this repository's.
    let mut script = Script::default()
        .run_from("someone/consensflow", 7, COMMIT, "push", "success")
        .jobs(7, &green);
    let said = refusal(&mut script);
    assert!(
        said.starts_with("CI (ci.yml) never ran on 2819dc1f: "),
        "{said}"
    );
    // A run with no head repository named is not this repository's either.
    let mut script = Script::default().ci(7, COMMIT, "push", "success", "success");
    script.runs[0]
        .as_object_mut()
        .unwrap()
        .remove("head_repository");
    assert!(refusal(&mut script).starts_with("CI (ci.yml) never ran on 2819dc1f: "));
    // The repository's name is told apart without its case, as GitHub's are.
    let mut script = Script::default()
        .run_from("NGVoicu/ConsensFlow", 7, COMMIT, "push", "success")
        .jobs(7, &green);
    answered(&mut script, Some("push")).0.unwrap();
}

#[test]
fn a_pass_tells_the_other_runs_of_the_commit_that_ended_red_as_warnings() {
    let mut script = Script::default()
        .ci(9, COMMIT, "workflow_dispatch", "failure", "failure")
        .ci(8, COMMIT, "push", "success", "success")
        .ci(6, COMMIT, "push", "success", "skipped")
        .run(5, COMMIT, "push", "in_progress");
    let (result, out, err) = answered(&mut script, Some("push"));
    result.unwrap();
    assert_eq!(err, "");
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 3, "{out}");
    assert!(lines[0].starts_with(&format!("CI passed {COMMIT} on both platforms: run 8,")));
    assert_eq!(
        lines[1],
        "::warning::run 9 ended failure, though run 8 passed the same commit: a flake of the same code, \
         or one to read"
    );
    assert_eq!(
        lines[2],
        "::warning::run 6 ended success, but gate (macos-latest) ended skipped, gate (windows-latest) ended \
         skipped, though run 8 passed the same commit: a flake of the same code, or one to read"
    );
}

#[test]
fn the_verdict_tells_ci_never_run_still_going_red_and_unasked_apart() {
    let mut never = Script::default();
    assert!(matches!(
        verdict(&mut never, REPOSITORY, COMMIT),
        Err(Refusal::NeverRan(_))
    ));
    // Going, beside a red one: not passed yet, and waited for.
    let mut going = Script::default()
        .ci(8, COMMIT, "push", "failure", "failure")
        .run(9, COMMIT, "workflow_dispatch", "in_progress");
    assert!(matches!(
        verdict(&mut going, REPOSITORY, COMMIT),
        Err(Refusal::Running(_))
    ));
    // Red: the newest red run, to rerun.
    let mut red = Script::default()
        .ci(9, COMMIT, "workflow_dispatch", "failure", "failure")
        .ci(8, COMMIT, "push", "cancelled", "cancelled");
    assert!(matches!(
        verdict(&mut red, REPOSITORY, COMMIT),
        Err(Refusal::Red { run: 9, .. })
    ));
    let mut unasked = Script {
        fails: Some("gh api ended 1: HTTP 403"),
        ..Script::default()
    };
    assert!(matches!(
        verdict(&mut unasked, REPOSITORY, COMMIT),
        Err(Refusal::Unasked(_))
    ));
    let mut passed = Script::default().ci(8, COMMIT, "push", "success", "success");
    assert_eq!(
        verdict(&mut passed, REPOSITORY, COMMIT),
        Ok(Passed {
            run: 8,
            red: vec![]
        })
    );
}

#[test]
fn a_hand_run_of_the_release_is_told_what_a_tag_would_be_refused_for_and_not_failed() {
    let mut script = Script::default().ci(7, COMMIT, "push", "failure", "failure");
    let (result, out, err) = answered(&mut script, Some("workflow_dispatch"));
    result.unwrap();
    assert_eq!(out, format!("::warning::{err}"));
    assert!(
        err.starts_with(&format!(
            "a run by workflow_dispatch publishes nothing; a tag on {COMMIT} would be refused: CI did not pass"
        )),
        "{err}"
    );
    // With no event named, a refusal fails it, as a push's does.
    let mut script = Script::default();
    assert!(answered(&mut script, None).0.is_err());
}

#[test]
fn a_commit_that_is_not_whole_is_a_usage_error_before_github_is_asked() {
    let not_hex = COMMIT.replace('2', "g");
    for commit in ["2819dc1f", "", not_hex.as_str()] {
        let mut script = Script::default();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let mut console = Console {
            out: &mut out,
            err: &mut err,
        };
        let failure = answer(&mut script, REPOSITORY, commit, None, &mut console).unwrap_err();
        assert!(
            matches!(&failure, Failure::Usage(said) if said.starts_with("--commit takes a whole commit")),
            "{failure}"
        );
        assert!(script.asked.is_empty());
    }
}

#[test]
fn the_required_jobs_are_the_platform_summaries_ci_yml_names() {
    // ci.yml's job `gate` over `os`, one a platform the app ships for: GitHub
    // names each `gate (<os>)`.
    let ci = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join(".github")
            .join("workflows")
            .join(CI_WORKFLOW),
    )
    .unwrap();
    let gate = ci
        .split("\n  gate:\n")
        .nth(1)
        .expect("ci.yml has a job gate");
    // The job's own lines, up to the next job's name.
    let systems = gate
        .lines()
        .take_while(|line| !(line.starts_with("  ") && !line.starts_with("   ")))
        .find_map(|line| line.trim().strip_prefix("os: ["))
        .and_then(|rest| rest.strip_suffix(']'))
        .expect("the gate's matrix names its systems");
    let names: Vec<String> = systems
        .split(',')
        .map(|os| format!("gate ({})", os.trim()))
        .collect();
    assert_eq!(names, REQUIRED_JOBS);
}

//! `cf-release ci-passed`: whether CI's full gate (`ci.yml`) passed the exact
//! commit on both platforms. The release workflow's `gate` job runs it on the
//! commit it releases, in place of running the gate a second time, and nothing
//! publishes unless it passes; `cargo xtask release` reads the same jobs.
//!
//! What counts is a run of `ci.yml` whose commit is this one, started by a push
//! or by hand (a pull request's run tests a merge of it, not it), that ended
//! green with each of [`REQUIRED_JOBS`] green. Those are the platforms' summary
//! jobs: each needs every job of its platform and fails unless all of them
//! passed, so a job of CI's that failed, was cancelled or was skipped fails its
//! platform's. One such run is enough: a later red run of the same commit was a
//! flake of the same code, as a green rerun is. A run that has not ended passes
//! nothing yet.
//!
//! GitHub is asked through `gh api` (its Actions API, by the workflow's file and
//! the commit), behind [`GitHub`], so that the tests give it a GitHub of their own.

use std::ffi::{OsStr, OsString};
use std::fmt::Write as _;

use cf_base::env::Env;
use serde_json::Value;

use crate::args::Flags;
use crate::cli::{Command, Console, Failure};
use crate::process;

/// `cf-release ci-passed`.
pub const COMMAND: Command = Command {
    name: "ci-passed",
    about: "Refuse a commit that CI's full gate (ci.yml) did not pass on both platforms",
    usage: "--github OWNER/NAME --commit SHA [--event EVENT]",
    run,
};

/// The workflow that is CI's full gate.
pub const CI_WORKFLOW: &str = "ci.yml";

/// The jobs of a run of [`CI_WORKFLOW`] that must each be green: each
/// platform's summary, which fails unless every job of its platform passed.
pub const REQUIRED_JOBS: [&str; 2] = ["gate (macos-latest)", "gate (windows-latest)"];

/// The events whose run of CI tests the commit itself.
const EVENTS: [&str; 2] = ["push", "workflow_dispatch"];

/// The most items GitHub gives in one page, and the pages read of one list.
const PER_PAGE: usize = 100;
const PAGES: usize = 20;

/// GitHub's API, as `gh api` answers it.
pub trait GitHub {
    /// What GET `path` answers, or what was said when it could not be asked.
    fn get(&mut self, path: &str) -> Result<String, String>;
}

/// GitHub through the machine's `gh`, logged in by `GH_TOKEN` on a runner.
struct Gh<'a> {
    env: &'a Env,
}

impl GitHub for Gh<'_> {
    fn get(&mut self, path: &str) -> Result<String, String> {
        let args = [OsString::from("api"), OsString::from(path)];
        let ran = process::capture(OsStr::new("gh"), &args, self.env)
            .map_err(|cause| cause.to_string())?;
        if ran.code != 0 {
            return Err(format!("gh api ended {}: {}", ran.code, ran.stderr.trim()));
        }
        Ok(ran.stdout)
    }
}

fn run(env: &Env, args: &[OsString], console: &mut Console) -> Result<(), Failure> {
    let flags = Flags::read(args, &["github", "commit", "event"])?;
    let repository = text(flags.require("github")?);
    let commit = text(flags.require("commit")?);
    let event = flags.get("event").map(text);
    answer(
        &mut Gh { env },
        &repository,
        &commit,
        event.as_deref(),
        console,
    )
}

fn text(word: &OsStr) -> String {
    word.to_string_lossy().into_owned()
}

/// The step's answer for `commit` of `repository`, asked of `github`: what
/// passed it on `out`, or the refusal. A run of the release workflow by
/// `event` other than a push publishes nothing, and is told what a tag on the
/// commit would be refused for instead of failed by it.
pub fn answer(
    github: &mut dyn GitHub,
    repository: &str,
    commit: &str,
    event: Option<&str>,
    console: &mut Console,
) -> Result<(), Failure> {
    let full = commit.len() == 40 && commit.bytes().all(|byte| byte.is_ascii_hexdigit());
    if !full {
        return Err(Failure::Usage(format!(
            "--commit takes a whole commit, 40 hex digits: {commit}"
        )));
    }
    match passed(github, repository, commit) {
        Ok(run) => {
            writeln!(
                console.out,
                "CI passed {commit} on both platforms: run {run}, https://github.com/{repository}/actions/runs/{run}, \
                 with {} green",
                REQUIRED_JOBS.join(" and ")
            )?;
            Ok(())
        }
        Err(refusal) => match event {
            Some(event) if event != "push" => {
                writeln!(
                    console.err,
                    "a run by {event} publishes nothing; a tag on {commit} would be refused: {refusal}"
                )?;
                Ok(())
            }
            _ => Err(Failure::Failed(refusal)),
        },
    }
}

/// A run of CI, as GitHub lists it.
struct CiRun {
    id: u64,
    commit: String,
    event: String,
    status: String,
    conclusion: String,
}

impl CiRun {
    fn of(value: &Value) -> Option<Self> {
        let text = |key: &str| value.get(key).and_then(Value::as_str).map(str::to_owned);
        Some(Self {
            id: value.get("id")?.as_u64()?,
            commit: text("head_sha")?,
            event: text("event")?,
            status: text("status").unwrap_or_default(),
            conclusion: text("conclusion").unwrap_or_default(),
        })
    }
}

/// The run of CI that passed `commit` on both platforms, or why there is none.
pub fn passed(github: &mut dyn GitHub, repository: &str, commit: &str) -> Result<u64, String> {
    let runs = list(
        github,
        &format!("repos/{repository}/actions/workflows/{CI_WORKFLOW}/runs?head_sha={commit}"),
        "workflow_runs",
    )?;
    let runs: Vec<CiRun> = runs.iter().filter_map(CiRun::of).collect();
    // GitHub filters by the commit; a run it gives of another is not this one's.
    let ours: Vec<&CiRun> = runs.iter().filter(|run| run.commit == commit).collect();
    let tested: Vec<&CiRun> = ours
        .iter()
        .copied()
        .filter(|run| EVENTS.contains(&run.event.as_str()))
        .collect();
    let short = commit.get(..8).unwrap_or(commit);
    if tested.is_empty() {
        let merges = ours.len();
        let besides = if merges > 0 {
            format!(" ({merges} run of a pull request tested a merge of it, which is not it)")
        } else {
            String::new()
        };
        return Err(format!(
            "CI ({CI_WORKFLOW}) never ran on {short}{besides}: nothing is published unless CI's full gate \
             passed this very commit on both platforms. Run CI on it, by a push to main or to a release/ \
             branch or by hand (`gh workflow run {CI_WORKFLOW} --ref <branch> --repo {repository}`), and once \
             it is green rerun this run's failed jobs"
        ));
    }
    let mut why = Vec::new();
    let mut running = false;
    for run in &tested {
        if run.status != "completed" {
            running = true;
            why.push(format!("run {} is {}", run.id, run.status));
            continue;
        }
        if run.conclusion != "success" {
            why.push(format!("run {} ended {}", run.id, run.conclusion));
            continue;
        }
        let jobs = list(
            github,
            &format!("repos/{repository}/actions/runs/{}/jobs", run.id),
            "jobs",
        )?;
        let unmet: Vec<String> = REQUIRED_JOBS
            .iter()
            .filter_map(|required| {
                let ended = jobs
                    .iter()
                    .find(|job| job.get("name").and_then(Value::as_str) == Some(required))
                    .map(|job| {
                        job.get("conclusion")
                            .and_then(Value::as_str)
                            .unwrap_or("nothing")
                    });
                match ended {
                    Some("success") => None,
                    Some(ended) => Some(format!("{required} ended {ended}")),
                    None => Some(format!("it has no job {required}")),
                }
            })
            .collect();
        if unmet.is_empty() {
            return Ok(run.id);
        }
        why.push(format!(
            "run {} ended success, but {}",
            run.id,
            unmet.join(", ")
        ));
    }
    let mut refusal = format!(
        "CI did not pass {short} on both platforms: {}. Nothing is published unless CI's full gate passed \
         this very commit on macOS and Windows",
        why.join("; ")
    );
    if running {
        let _ = write!(
            refusal,
            ". Once CI's run ends green, rerun this run's failed jobs"
        );
    } else {
        let _ = write!(
            refusal,
            ". A flake: rerun CI's failed jobs (`gh run rerun <id> --failed --repo {repository}`), and once \
             it is green rerun this run's failed jobs; a real failure spends the version"
        );
    }
    Err(refusal)
}

/// Every item of the list `key` that GET `path` answers, page by page.
fn list(github: &mut dyn GitHub, path: &str, key: &str) -> Result<Vec<Value>, String> {
    let mut items = Vec::new();
    for page in 1..=PAGES {
        let address = format!(
            "{path}{}per_page={PER_PAGE}&page={page}",
            if path.contains('?') { '&' } else { '?' }
        );
        let said = github
            .get(&address)
            .map_err(|cause| format!("GitHub could not be asked for {address}: {cause}"))?;
        let answer: Value = serde_json::from_str(&said)
            .map_err(|cause| format!("GitHub's answer to {address} could not be read: {cause}"))?;
        let Some(found) = answer.get(key).and_then(Value::as_array) else {
            return Err(format!("GitHub's answer to {address} holds no {key}"));
        };
        let total = answer
            .get("total_count")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let last = found.len() < PER_PAGE;
        items.extend(found.iter().cloned());
        if last || items.len() as u64 >= total {
            return Ok(items);
        }
    }
    Err(format!(
        "GitHub's list at {path} goes on past {PAGES} pages of {PER_PAGE}"
    ))
}

#[cfg(test)]
mod tests;

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
//! nothing yet. A run counts only where its commit is this repository's (a
//! fork's never runs CI by a push here, and is held out all the same). The
//! other runs of the commit that ended red are told beside a pass, as a warning.
//!
//! GitHub is asked through `gh api` (its Actions API, by the workflow's file and
//! the commit), behind [`GitHub`], so that the tests give it a GitHub of their own.

use std::ffi::{OsStr, OsString};

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
/// commit would be refused for instead of failed by it; but GitHub that could
/// not be asked fails it whatever the event, as the hand run is the trial of
/// the job's token. What a refusal or a warning says is also written as
/// GitHub's workflow command (`::error::`, `::warning::`), so the run's page
/// shows it.
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
    let refusal = match verdict(github, repository, commit) {
        Ok(passed) => {
            writeln!(
                console.out,
                "CI passed {commit} on both platforms: run {}, https://github.com/{repository}/actions/runs/{}, \
                 with {} green",
                passed.run,
                passed.run,
                REQUIRED_JOBS.join(" and ")
            )?;
            for red in &passed.red {
                let line = format!(
                    "{red}, though run {} passed the same commit: a flake of the same code, or one to read",
                    passed.run
                );
                writeln!(console.out, "{}", annotation("warning", &line))?;
            }
            return Ok(());
        }
        Err(refusal) => refusal,
    };
    let said = refusal.advised(repository);
    match event {
        Some(event) if event != "push" && !refusal.is_unasked() => {
            let line = format!(
                "a run by {event} publishes nothing; a tag on {commit} would be refused: {said}"
            );
            writeln!(console.out, "{}", annotation("warning", &line))?;
            writeln!(console.err, "{line}")?;
            Ok(())
        }
        _ => {
            writeln!(console.out, "{}", annotation("error", &said))?;
            Err(Failure::Failed(said))
        }
    }
}

/// `text` as GitHub's workflow command `kind` (`error`, `warning`): one line,
/// with what would end it written as the runner reads it back.
fn annotation(kind: &str, text: &str) -> String {
    let escaped = text
        .replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A");
    format!("::{kind}::{escaped}")
}

/// A run of CI, as GitHub lists it.
struct CiRun {
    id: u64,
    commit: String,
    event: String,
    /// The repository its commit came from: a fork's, for a fork's pull request.
    repository: String,
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
            repository: value
                .get("head_repository")
                .and_then(|repository| repository.get("full_name"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            status: text("status").unwrap_or_default(),
            conclusion: text("conclusion").unwrap_or_default(),
        })
    }
}

/// The run of CI that passed a commit on both platforms.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Passed {
    pub run: u64,
    /// Each other run of the commit that ended and did not pass it, as said.
    pub red: Vec<String>,
}

/// Why no run of CI passed a commit, in words that say what was found; the
/// advice of what to do is the caller's ([`Refusal::advised`] is the release
/// run's).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// No run of CI by a push or by hand tested the commit.
    NeverRan(String),
    /// A run tested it and has not ended, and none that ended passed it.
    Running(String),
    /// Every run that tested it ended without both summaries green: the newest
    /// of them, to rerun.
    Red { said: String, run: u64 },
    /// GitHub could not be asked, or what it answered could not be read: nothing
    /// is known of CI.
    Unasked(String),
}

impl Refusal {
    /// What was found.
    pub fn said(&self) -> &str {
        match self {
            Refusal::NeverRan(said)
            | Refusal::Running(said)
            | Refusal::Red { said, .. }
            | Refusal::Unasked(said) => said,
        }
    }

    pub fn is_unasked(&self) -> bool {
        matches!(self, Refusal::Unasked(_))
    }

    /// What was found, and what to do about it in the release run of a tag.
    pub fn advised(&self, repository: &str) -> String {
        let said = self.said();
        // The rule, after a colon and as a sentence of its own.
        let rule = "nothing is published unless CI's full gate passed this very commit on macOS and Windows";
        let sentence = "Nothing is published unless CI's full gate passed this very commit on macOS and Windows";
        match self {
            Refusal::NeverRan(_) => format!(
                "{said}: {rule}. Run CI on it, by a push to main or to a release/ branch or by hand \
                 (`gh workflow run {CI_WORKFLOW} --ref <branch> --repo {repository}`), and once it is green \
                 rerun this run's failed jobs"
            ),
            Refusal::Running(_) => format!(
                "{said}. {sentence}. Once CI's run ends green, rerun this run's failed jobs"
            ),
            Refusal::Red { run, .. } => format!(
                "{said}. {sentence}. A flake: rerun CI's failed jobs (`gh run rerun {run} --failed --repo {repository}`), \
                 and once it is green rerun this run's failed jobs; a real failure spends the version"
            ),
            Refusal::Unasked(_) => format!(
                "{said}: {rule}, and whether CI passed it could not be told. Read the job's token and \
                 permissions (actions: read), then rerun this run's failed jobs"
            ),
        }
    }
}

/// Whether CI's full gate passed `commit` of `repository`, asked of `github`:
/// the rule the release's gate holds a tag to, and `cargo xtask release` its
/// commit before it tags.
pub fn verdict(github: &mut dyn GitHub, repository: &str, commit: &str) -> Result<Passed, Refusal> {
    let runs = list(
        github,
        &format!("repos/{repository}/actions/workflows/{CI_WORKFLOW}/runs?head_sha={commit}"),
        "workflow_runs",
    )
    .map_err(Refusal::Unasked)?;
    let runs: Vec<CiRun> = runs.iter().filter_map(CiRun::of).collect();
    // GitHub filters by the commit; a run it gives of another is not this
    // one's, nor one whose commit came from another repository.
    let ours: Vec<&CiRun> = runs
        .iter()
        .filter(|run| run.commit == commit && run.repository.eq_ignore_ascii_case(repository))
        .collect();
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
        return Err(Refusal::NeverRan(format!(
            "CI ({CI_WORKFLOW}) never ran on {short}{besides}"
        )));
    }
    let mut passed = None;
    let mut red = Vec::new();
    let mut newest_red = None;
    let mut going = Vec::new();
    for run in &tested {
        if run.status != "completed" {
            going.push(format!("run {} is {}", run.id, run.status));
            continue;
        }
        let why = if run.conclusion != "success" {
            format!("run {} ended {}", run.id, run.conclusion)
        } else {
            let jobs = list(
                github,
                &format!("repos/{repository}/actions/runs/{}/jobs", run.id),
                "jobs",
            )
            .map_err(Refusal::Unasked)?;
            let unmet = unmet(jobs.iter().map(|job| {
                let text = |key: &str| job.get(key).and_then(Value::as_str).unwrap_or_default();
                (text("name"), text("conclusion"))
            }));
            if unmet.is_empty() {
                passed.get_or_insert(run.id);
                continue;
            }
            format!("run {} ended success, but {}", run.id, unmet.join(", "))
        };
        newest_red.get_or_insert(run.id);
        red.push(why);
    }
    if let Some(run) = passed {
        return Ok(Passed { run, red });
    }
    let why = going
        .iter()
        .chain(&red)
        .cloned()
        .collect::<Vec<_>>()
        .join("; ");
    let said = format!("CI did not pass {short} on both platforms: {why}");
    match newest_red {
        Some(run) if going.is_empty() => Err(Refusal::Red { said, run }),
        _ => Err(Refusal::Running(said)),
    }
}

/// What of [`REQUIRED_JOBS`] a run's `jobs` (each its name and how it ended)
/// do not hold green, by their whole names: a job that holds a summary's name
/// is not it.
fn unmet<'a>(jobs: impl Iterator<Item = (&'a str, &'a str)> + Clone) -> Vec<String> {
    REQUIRED_JOBS
        .iter()
        .filter_map(|required| {
            let ended = jobs
                .clone()
                .find(|(name, _)| name == required)
                .map(|(_, conclusion)| {
                    if conclusion.is_empty() {
                        "nothing"
                    } else {
                        conclusion
                    }
                });
            match ended {
                Some("success") => None,
                Some(ended) => Some(format!("{required} ended {ended}")),
                None => Some(format!("it has no job {required}")),
            }
        })
        .collect()
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

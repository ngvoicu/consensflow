//! `cargo xtask ci-times`: how long CI's jobs and steps took, against the
//! limits `ci.yml` gives them.
//!
//! A step that runs into its `timeout-minutes` is cancelled, and a run cut short
//! says nothing of the tests it never reached: on 2026-10-22 the Release run of
//! 3.0.6-alpha.1 died inside the ledger's replay, with Windows' workspace tests
//! at the 40 minutes they were given, where they had taken 20 until 2026-10-19
//! and had grown run after run since, unseen. This reads the runs GitHub keeps
//! and fails while there is still room: when a job or a step took more than a
//! share of its limit (80 % unless asked otherwise), a limit being a bound that
//! was set with a margin over what the step takes, not the time it may take.
//!
//! ```text
//! cargo xtask ci-times [RUN]... [--last N] [--share PERCENT] [--step TEXT]... [--binaries]
//! ```
//!
//! `RUN` is a run's id, of `ci.yml` or of a `release.yml` run that called it
//! (every release up to 3.0.6-alpha.2; since, the release checks CI's run instead);
//! with none, the last N (5) runs of `ci.yml` on `main` that ended. `--step`
//! keeps the steps whose name holds TEXT (`the job` names a job), to watch
//! one: `--step "cargo test --workspace"`. `--binaries` reads the log of each
//! `cargo test` step it says and adds how long its build took and which test
//! binaries ran longest ([`binaries`]): what grew, when a step did. The limits are
//! read from `.github/workflows/ci.yml`, where the steps and jobs that have one
//! say it, so that a step's bound and the bound this holds it to cannot drift. A
//! step is found by the name GitHub gives it (`name:`, or `Run ` and the first
//! line of its `run:`), a job by its id. A step that did not run (skipped, or
//! after one that failed) has no time and is left out; one that was cancelled
//! at its limit counts for the time it ran. Exit status 0 when every time is
//! within the share, 1 when one is not, or when GitHub could not be read.
//!
//! The programs are `gh` (logged in), behind [`System`] so that the tests give it
//! a GitHub that is a script, as the release's do.

mod binaries;
#[cfg(test)]
mod tests;

use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::path::Path;

use cf_base::time::parse;
use serde_json::Value;

use crate::context::Context;
use crate::dispatch::{Command, Console, Failure, Run};
use crate::process::Invocation;
use crate::release::system::{Real, System};
use crate::release::Error;

pub const COMMANDS: &[Command] = &[Command {
    words: &["ci-times"],
    about: "Read CI runs' job and step times from GitHub and fail when one passes a share (80 %) of the limit ci.yml gives it",
    usage: "[RUN]... [--last N] [--share PERCENT] [--step TEXT]... [--binaries]",
    run: Run::Native(ci_times),
}];

/// The repository whose runs are read, for `gh`.
const REPOSITORY: &str = "ngvoicu/consensflow";

/// The workflow that holds the limits, and whose runs are read by default.
const WORKFLOW: &str = "ci.yml";

/// The runs read when none is named.
const LAST: usize = 5;

/// The share of its limit a job or a step may take.
const SHARE: u64 = 80;

/// What a limit is set on.
#[derive(Debug, PartialEq, Eq)]
enum Held {
    /// A job, by its id in the workflow (`gate`; its runs are named `gate (windows-latest)`).
    Job(String),
    /// A step, by the name GitHub shows it under.
    Step(String),
}

/// A `timeout-minutes` of the workflow.
#[derive(Debug, PartialEq, Eq)]
struct Limit {
    held: Held,
    minutes: u64,
}

/// What the command line asked for.
#[derive(Debug, PartialEq, Eq)]
struct Options {
    /// The runs named; none means the last [`LAST`] of `ci.yml` on `main`.
    runs: Vec<u64>,
    last: usize,
    share: u64,
    /// Only the jobs and steps whose name holds one of these; none means all.
    only: Vec<String>,
    /// Whether the `cargo test` steps are read binary by binary.
    binaries: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            runs: Vec::new(),
            last: LAST,
            share: SHARE,
            only: Vec::new(),
            binaries: false,
        }
    }
}

fn options(args: &[OsString]) -> Result<Options, Failure> {
    let Options {
        mut runs,
        mut last,
        mut share,
        mut only,
        mut binaries,
    } = Options::default();
    let mut words = args.iter();
    while let Some(word) = words.next() {
        let text = word.to_str().unwrap_or_default();
        let mut number = |flag: &str| {
            words
                .next()
                .and_then(|value| value.to_str()?.parse::<u64>().ok())
                .filter(|value| *value > 0)
                .ok_or_else(|| Failure::Usage(format!("{flag} takes a number above 0")))
        };
        match text {
            "--last" => last = usize::try_from(number("--last")?).unwrap_or(LAST),
            "--share" => {
                share = number("--share")?;
                if share > 100 {
                    return Err(Failure::Usage("--share takes a percent, 1 to 100".into()));
                }
            }
            "--binaries" => binaries = true,
            "--step" => match words.next().and_then(|value| value.to_str()) {
                Some(name) => only.push(name.to_owned()),
                None => return Err(Failure::Usage("--step takes a name".into())),
            },
            _ => match text.parse::<u64>() {
                Ok(run) => runs.push(run),
                Err(_) => {
                    return Err(Failure::Usage(format!(
                        "ci-times does not take {}",
                        word.to_string_lossy()
                    )))
                }
            },
        }
    }
    Ok(Options {
        runs,
        last,
        share,
        only,
        binaries,
    })
}

/// A step being read from the workflow: its `name:`, the first line of its
/// `run:`, its `timeout-minutes:`.
#[derive(Default)]
struct Step {
    name: Option<String>,
    run: Option<String>,
    minutes: Option<u64>,
}

/// Puts the step read so far, if it has a limit, among `found`.
fn close(step: &mut Option<Step>, found: &mut Vec<Limit>) {
    let Some(step) = step.take() else { return };
    let shown = step
        .name
        .or_else(|| step.run.map(|run| format!("Run {run}")));
    if let (Some(shown), Some(minutes)) = (shown, step.minutes) {
        found.push(Limit {
            held: Held::Step(shown),
            minutes,
        });
    }
}

/// The limits `workflow` (a GitHub Actions file) puts on its jobs and its steps.
///
/// Read by indentation, as `ci.yml` is written: jobs at two spaces, a job's own
/// keys at four, its steps as a list at six and their keys at eight. A step is
/// named as GitHub names it.
fn limits(workflow: &str) -> Vec<Limit> {
    let mut found = Vec::new();
    let mut in_jobs = false;
    let mut job = String::new();
    let mut step: Option<Step> = None;
    // The indentation of a `run: |` key, while its literal block is read.
    let mut block_of: Option<usize> = None;
    for line in workflow.lines() {
        let indent = line.len() - line.trim_start().len();
        let text = line.trim();
        if text.is_empty() || text.starts_with('#') {
            continue;
        }
        if indent == 0 {
            close(&mut step, &mut found);
            in_jobs = text == "jobs:";
            continue;
        }
        if !in_jobs {
            continue;
        }
        if let Some(block) = block_of {
            if indent > block {
                // The first line of the block is what GitHub names the step by.
                if let Some(step) = step.as_mut().filter(|step| step.run.is_none()) {
                    step.run = Some(text.to_owned());
                }
                continue;
            }
            block_of = None;
        }
        if indent == 2 {
            close(&mut step, &mut found);
            job = text.trim_end_matches(':').to_owned();
            continue;
        }
        if indent == 4 {
            close(&mut step, &mut found);
            if let Some(minutes) = key(text, "timeout-minutes").and_then(|text| text.parse().ok()) {
                found.push(Limit {
                    held: Held::Job(job.clone()),
                    minutes,
                });
            }
            continue;
        }
        // A step starts at its dash, and its first key is on that line.
        let (own, at) = match text.strip_prefix("- ") {
            Some(first) => {
                close(&mut step, &mut found);
                step = Some(Step::default());
                (first, indent + 2)
            }
            None => (text, indent),
        };
        let Some(step) = step.as_mut() else { continue };
        if let Some(value) = key(own, "name") {
            step.name = Some(value);
        } else if let Some(value) = key(own, "timeout-minutes") {
            step.minutes = value.parse().ok();
        } else if let Some(value) = key(own, "run") {
            if value.starts_with(['|', '>']) {
                block_of = Some(at);
            } else {
                step.run = Some(value);
            }
        }
    }
    close(&mut step, &mut found);
    found
}

/// The value of `key: value` in `text`, without a comment after it or quotes around it.
fn key(text: &str, key: &str) -> Option<String> {
    let value = text.strip_prefix(key)?.strip_prefix(':')?;
    let value = value.split(" #").next().unwrap_or(value).trim();
    Some(value.trim_matches(['"', '\'']).to_owned())
}

/// A job or a step of a run, with the time it took and the limit it has.
#[derive(Debug, PartialEq, Eq)]
struct Reading {
    run: u64,
    /// The job's id, which its log is read by.
    job_id: u64,
    job: String,
    /// `the job`, or the step's name.
    what: String,
    seconds: i64,
    limit_minutes: u64,
}

impl Reading {
    /// Whether it took more than `share` percent of its limit.
    fn over(&self, share: u64) -> bool {
        self.seconds * 100 > i64::try_from(self.limit_minutes * 60 * share).unwrap_or(i64::MAX)
    }

    fn percent(&self) -> i64 {
        self.seconds * 100
            / i64::try_from(self.limit_minutes * 60)
                .unwrap_or(i64::MAX)
                .max(1)
    }
}

/// The seconds between a job's or a step's `started_at` and `completed_at`, none
/// where it did not run (skipped, or never started).
fn took(item: &Value) -> Option<i64> {
    if item["conclusion"] == "skipped" {
        return None;
    }
    let at = |field: &str| parse(item[field].as_str()?);
    Some((at("completed_at")? - at("started_at")?) / 1_000)
}

/// Every job and step of `jobs` (a run's `jobs` answer) that has a limit, with its time.
fn readings(run: u64, limits: &[Limit], jobs: &Value) -> Vec<Reading> {
    let mut found = Vec::new();
    for job in jobs["jobs"].as_array().into_iter().flatten() {
        let name = job["name"].as_str().unwrap_or_default();
        // A run that called ci.yml names its jobs `caller / gate (os)`.
        let own = name.rsplit(" / ").next().unwrap_or(name);
        let steps = job["steps"].as_array().into_iter().flatten();
        let held = limits.iter().filter(|limit| match &limit.held {
            Held::Job(id) => {
                own == id
                    || own
                        .strip_prefix(id.as_str())
                        .is_some_and(|rest| rest.starts_with(" ("))
            }
            Held::Step(_) => false,
        });
        for limit in held {
            if let Some(seconds) = took(job) {
                found.push(Reading {
                    run,
                    job_id: job["id"].as_u64().unwrap_or_default(),
                    job: name.to_owned(),
                    what: "the job".to_owned(),
                    seconds,
                    limit_minutes: limit.minutes,
                });
            }
        }
        for step in steps {
            let shown = step["name"].as_str().unwrap_or_default();
            for limit in limits {
                if limit.held == Held::Step(shown.to_owned()) {
                    if let Some(seconds) = took(step) {
                        found.push(Reading {
                            run,
                            job_id: job["id"].as_u64().unwrap_or_default(),
                            job: name.to_owned(),
                            what: shown.to_owned(),
                            seconds,
                            limit_minutes: limit.minutes,
                        });
                    }
                }
            }
        }
    }
    found
}

/// What `gh` writes to `args`; a status other than 0 is an error.
fn gh_text(system: &mut dyn System, root: &Path, args: &[&str]) -> Result<String, Error> {
    let invocation = Invocation::new("gh", root).args(args.iter().copied());
    let answered = system.capture(&invocation)?;
    if answered.code != 0 {
        return Err(Error::Program {
            command: invocation.display(),
            code: answered.code,
            said: answered.stderr.trim().to_owned(),
        });
    }
    Ok(answered.stdout)
}

/// What `gh` says to `args`, as JSON.
fn gh(system: &mut dyn System, root: &Path, args: &[&str]) -> Result<Value, Error> {
    let said = gh_text(system, root, args)?;
    serde_json::from_str(&said).map_err(|cause| Error::Answer {
        what: format!("what `gh {}` said", args.join(" ")),
        said: cause.to_string(),
    })
}

/// How many binaries a step says of its tests, the longest first.
const LONGEST: usize = 10;

/// Says, under a `cargo test` step's time, how long its build took and which
/// of its binaries ran longest, from the log of its job.
fn say_binaries(
    system: &mut dyn System,
    root: &Path,
    reading: &Reading,
    out: &mut dyn Write,
) -> Result<(), Error> {
    let url = format!("repos/{REPOSITORY}/actions/jobs/{}/logs", reading.job_id);
    let log = gh_text(system, root, &["api", "--allow-escape-sequences", &url])?;
    let Some(mut timing) = binaries::timing(&log, &reading.what) else {
        let _ = writeln!(out, "    (the log of the job has no such step)");
        return Ok(());
    };
    let total: f64 = timing.binaries.iter().map(|(_, seconds)| seconds).sum();
    if let Some(build) = timing.build {
        let _ = writeln!(out, "    build {build:.0} s");
    }
    let step = reading.seconds.max(1) as f64;
    timing.binaries.sort_by(|a, b| b.1.total_cmp(&a.1));
    for (name, seconds) in timing.binaries.iter().take(LONGEST) {
        let _ = writeln!(
            out,
            "    {seconds:7.1} s {:3.0} %  {name}",
            seconds * 100.0 / step
        );
    }
    let _ = writeln!(
        out,
        "    {total:7.1} s {:3.0} %  in all {} binaries",
        total * 100.0 / step,
        timing.binaries.len()
    );
    Ok(())
}

/// Reads the runs and says each time against its limit on `out`; whether all
/// are within the share.
fn check(
    system: &mut dyn System,
    root: &Path,
    workflow: &str,
    options: &Options,
    out: &mut dyn Write,
) -> Result<bool, Error> {
    let limits = limits(workflow);
    let runs: Vec<(u64, String)> = if options.runs.is_empty() {
        let last = options.last.to_string();
        let listed = gh(
            system,
            root,
            &[
                "run",
                "list",
                "--repo",
                REPOSITORY,
                "--workflow",
                WORKFLOW,
                "--branch",
                "main",
                "--status",
                "completed",
                "--limit",
                &last,
                "--json",
                "databaseId,headSha",
            ],
        )?;
        listed
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|run| {
                let commit = run["headSha"].as_str().unwrap_or_default();
                Some((run["databaseId"].as_u64()?, commit.get(..8)?.to_owned()))
            })
            .collect()
    } else {
        options
            .runs
            .iter()
            .map(|run| (*run, String::new()))
            .collect()
    };
    let (mut read, mut over) = (0, 0);
    for (run, commit) in &runs {
        let url = format!("repos/{REPOSITORY}/actions/runs/{run}/jobs?per_page=100");
        let jobs = gh(system, root, &["api", &url])?;
        let _ = writeln!(out, "run {run} {commit}");
        let found = readings(*run, &limits, &jobs);
        for reading in found.into_iter().filter(|reading| {
            options.only.is_empty() || options.only.iter().any(|name| reading.what.contains(name))
        }) {
            read += 1;
            let flag = if reading.over(options.share) {
                over += 1;
                format!("  OVER {} %", options.share)
            } else {
                String::new()
            };
            let _ = writeln!(
                out,
                "  {}: {}: {:.1} of {} minutes, {} %{flag}",
                reading.job,
                reading.what,
                reading.seconds as f64 / 60.0,
                reading.limit_minutes,
                reading.percent()
            );
            if options.binaries && reading.what.starts_with("Run cargo test") {
                say_binaries(system, root, &reading, out)?;
            }
        }
    }
    let _ = writeln!(
        out,
        "{over} of {read} times over {} % of their limit, in {} runs",
        options.share,
        runs.len()
    );
    if read == 0 {
        // A step renamed, or a workflow with no limit left, must not read as all well.
        return Err(Error::Refused(format!(
            "no time was read: no run was listed, or none has a job or step that {WORKFLOW} gives a limit"
        )));
    }
    Ok(over == 0)
}

fn ci_times(context: &Context, args: &[OsString], console: &mut Console) -> Result<i32, Failure> {
    let options = options(args)?;
    let file = context
        .root
        .join(".github")
        .join("workflows")
        .join(WORKFLOW);
    let Ok(workflow) = fs::read_to_string(&file) else {
        writeln!(console.err, "ci-times: could not read {}", file.display())?;
        return Ok(1);
    };
    let mut system = Real::new(&context.env);
    match check(&mut system, &context.root, &workflow, &options, console.out) {
        Ok(within) => Ok(i32::from(!within)),
        Err(error) => {
            writeln!(console.err, "ci-times: {error}")?;
            Ok(1)
        }
    }
}

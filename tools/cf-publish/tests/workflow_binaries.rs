//! A rule over every workflow: a job that runs a binary it downloaded from
//! another job of the run (it makes it executable with `chmod +x`) runs on the
//! system that job built it on. A binary built on one system does not run on
//! another: the sign job of round 2 downloaded the publisher built on Linux and
//! ran it on macOS, which a release would have found only at its first tag
//! (T-818). Reusable workflows a job calls (`uses: ./.github/workflows/…`) count
//! as the jobs they hold, so an artifact the Windows build uploads is known to
//! be built on Windows.

// A failure in a test is the test's answer, in its helpers too.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::collections::BTreeMap;

use cf_publish::testing::workflow::{files, job, read, steps};

/// The system a `runs-on` names: the runner's family, as GitHub names its images.
fn system(runs_on: &str) -> &'static str {
    let image = runs_on.trim().trim_matches(|c| c == '\'' || c == '"');
    if image.starts_with("ubuntu") {
        "linux"
    } else if image.starts_with("macos") {
        "macos"
    } else if image.starts_with("windows") {
        "windows"
    } else {
        panic!("a runs-on this rule does not know: {runs_on}")
    }
}

/// The jobs of a workflow's text, by name, in order.
fn job_names(text: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut in_jobs = false;
    for line in text.lines() {
        if line == "jobs:" {
            in_jobs = true;
        } else if in_jobs
            && !line.starts_with(' ')
            && !line.trim().is_empty()
            && !line.starts_with('#')
        {
            in_jobs = false;
        } else if in_jobs
            && line.starts_with("  ")
            && !line.starts_with("   ")
            && line.ends_with(':')
        {
            names.push(line.trim().trim_end_matches(':').to_string());
        }
    }
    names
}

/// The value of `key:` at the job's own level (four spaces in).
fn job_key(job: &str, key: &str) -> Option<String> {
    job.lines()
        .find_map(|line| line.strip_prefix(&format!("    {key}: ")))
        .map(|value| value.trim().to_string())
}

/// What a step's `with:` gives `key`.
fn with(step: &str, key: &str) -> Option<String> {
    step.lines()
        .find_map(|line| line.trim().strip_prefix(&format!("{key}: ")))
        .map(|value| value.trim().to_string())
}

/// A workflow's jobs, with the reusable ones each calls taken in: for each job,
/// the system it runs on, the artifacts it uploads, and the folders it downloads
/// named artifacts into, with the scripts that run after.
struct Job {
    workflow: String,
    name: String,
    system: Option<&'static str>,
    uploads: Vec<String>,
    downloads: Vec<(String, String)>,
    scripts: String,
}

fn jobs_of(file: &str) -> Vec<Job> {
    let text = read(file);
    let mut out = Vec::new();
    for name in job_names(&text) {
        let body = job(&text, &name);
        if let Some(called) = job_key(&body, "uses") {
            if let Some(called) = called.strip_prefix("./.github/workflows/") {
                out.extend(jobs_of(called));
            }
            continue;
        }
        let runs_on = job_key(&body, "runs-on");
        let mut job = Job {
            workflow: file.to_string(),
            name: name.clone(),
            system: runs_on
                .as_deref()
                .filter(|on| !on.contains("${{"))
                .map(system),
            uploads: Vec::new(),
            downloads: Vec::new(),
            scripts: String::new(),
        };
        for step in steps(&body) {
            let text = step.text();
            if text.contains("actions/upload-artifact@") {
                job.uploads.extend(with(&text, "name"));
            } else if text.contains("actions/download-artifact@") {
                if let (Some(name), Some(path)) = (with(&text, "name"), with(&text, "path")) {
                    job.downloads
                        .push((name, path.replace("${{ runner.temp }}", "$RUNNER_TEMP")));
                }
            } else if text.lines().any(|line| line.starts_with("        run: ")) {
                job.scripts.push_str(&step.script());
                job.scripts.push('\n');
            } else if let Some(line) = text
                .lines()
                .next()
                .and_then(|line| line.strip_prefix("      - run: "))
            {
                job.scripts.push_str(line);
                job.scripts.push('\n');
            }
        }
        out.push(job);
    }
    out
}

/// Every job of every workflow, a reusable workflow's as often as it is called.
fn all_jobs() -> Vec<Job> {
    files().iter().flat_map(|file| jobs_of(file)).collect()
}

/// Whether `scripts` make a file under `folder` executable: `chmod +x` of a path
/// in it, or of a variable set to one.
fn makes_executable(scripts: &str, folder: &str) -> bool {
    let named: Vec<String> = scripts
        .lines()
        .filter_map(|line| {
            let (variable, value) = line.trim().split_once('=')?;
            let value = value.trim_matches('"');
            (value.starts_with(folder) && !variable.contains(' ')).then(|| format!("${variable}"))
        })
        .collect();
    scripts.lines().any(|line| {
        let line = line.trim();
        line.starts_with("chmod +x")
            && (line.contains(folder)
                || named.iter().any(|variable| {
                    line.contains(&format!("\"{variable}\"")) || line.ends_with(variable.as_str())
                }))
    })
}

#[test]
fn a_job_runs_a_downloaded_binary_only_on_the_system_the_job_that_built_it_ran_on() {
    let jobs = all_jobs();
    let mut built_on: BTreeMap<String, (String, &'static str)> = BTreeMap::new();
    for job in &jobs {
        for upload in &job.uploads {
            if let Some(system) = job.system {
                built_on.insert(
                    upload.clone(),
                    (format!("{}:{}", job.workflow, job.name), system),
                );
            }
        }
    }
    let mut runs = 0;
    let mut wrong = Vec::new();
    for job in &jobs {
        for (artifact, folder) in &job.downloads {
            if !makes_executable(&job.scripts, folder) {
                continue;
            }
            runs += 1;
            let (maker, made_on) = built_on.get(artifact).unwrap_or_else(|| {
                panic!(
                    "{}:{} runs {artifact}, which no job uploads",
                    job.workflow, job.name
                )
            });
            let runs_on = job
                .system
                .expect("a job that runs a binary names its runner");
            if *made_on != runs_on {
                wrong.push(format!(
                    "{}:{} runs on {runs_on} the {artifact} that {maker} built on {made_on}",
                    job.workflow, job.name
                ));
            }
        }
    }
    assert!(
        runs >= 2,
        "the rule found {runs} downloaded binaries run; the release runs the publisher twice"
    );
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn the_rule_reads_the_systems_and_the_artifacts_it_is_about() {
    let jobs = all_jobs();
    let find = |workflow: &str, name: &str| {
        jobs.iter()
            .find(|job| job.workflow == workflow && job.name == name)
            .unwrap_or_else(|| panic!("no job {workflow}:{name}"))
    };
    assert_eq!(find("release.yml", "publish").system, Some("linux"));
    assert_eq!(find("release.yml", "sign").system, Some("macos"));
    assert_eq!(find("windows-build.yml", "windows").system, Some("windows"));
    assert!(find("windows-build.yml", "windows")
        .uploads
        .contains(&"ConsensFlow-windows".to_string()));
    assert!(makes_executable(
        "tool=\"$RUNNER_TEMP/x/cf\"\nchmod +x \"$tool\"",
        "$RUNNER_TEMP/x"
    ));
    assert!(!makes_executable("chmod +x other", "$RUNNER_TEMP/x"));
}

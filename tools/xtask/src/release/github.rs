//! What a release asks of GitHub: its workflow runs, through `gh`, and the
//! feeds, through `curl` at the address installed apps read them at.
//!
//! A run is watched as release-305.sh watched it: the newest run of the
//! workflow (on the commit, by the event) that is not the one that was newest
//! before it was asked for, then looked at until it has ended. `gh run watch`
//! can return before a run ends, so only the run's own status is read.

use cf_publish::version::{named_release, Version};
use serde_json::Value;

use super::{refused, short, Error, Release, DOWNLOADS, REPOSITORY};
use crate::process::Invocation;

/// The run to watch: which workflow, on which commit, started by which event.
pub(super) struct Watched<'a> {
    /// How the run is named in what is said: `ci`, `dry run`, `release`.
    pub label: &'a str,
    pub workflow: &'a str,
    pub commit: &'a str,
    /// `push`, `workflow_dispatch`, or any.
    pub event: Option<&'a str>,
    /// The newest such run before this one was asked for, which is not it.
    pub before: Option<u64>,
    /// Whether the run must hold a green gate of each platform (`ci.yml`'s).
    pub both_platforms: bool,
}

/// What a run says of itself.
struct Status {
    status: String,
    conclusion: String,
    /// Each job, by its name, and how it ended.
    jobs: Vec<(String, String)>,
}

impl Release<'_> {
    /// What `gh` says to `args`; a status other than 0 is an error.
    fn gh(&mut self, args: &[&str]) -> Result<String, Error> {
        let invocation = Invocation::new("gh", &self.root).args(args.iter().copied());
        let answered = self.system.capture(&invocation)?;
        if answered.code != 0 {
            return Err(Error::Program {
                command: invocation.display(),
                code: answered.code,
                said: answered.stderr.trim().to_owned(),
            });
        }
        Ok(answered.stdout)
    }

    /// The newest run of `workflow`, on `commit` and by `event` where they are named.
    pub(super) fn newest_run(
        &mut self,
        workflow: &str,
        commit: Option<&str>,
        event: Option<&str>,
    ) -> Result<Option<u64>, Error> {
        let mut args = vec!["run", "list", "--repo", REPOSITORY, "--workflow", workflow];
        if let Some(commit) = commit {
            args.extend(["--commit", commit]);
        }
        if let Some(event) = event {
            args.extend(["--event", event]);
        }
        args.extend(["--limit", "1", "--json", "databaseId"]);
        let said = self.gh(&args)?;
        let runs: Value = serde_json::from_str(&said).map_err(|cause| Error::Answer {
            what: format!("the runs of {workflow}"),
            said: cause.to_string(),
        })?;
        Ok(runs
            .get(0)
            .and_then(|run| run.get("databaseId"))
            .and_then(Value::as_u64))
    }

    /// Starts `workflow` by hand on `reference`, as a dry run or a dispatch.
    pub(super) fn dispatch(&mut self, workflow: &str, reference: &str) -> Result<(), Error> {
        self.gh(&[
            "workflow", "run", workflow, "--repo", REPOSITORY, "--ref", reference,
        ])?;
        Ok(())
    }

    fn status(&mut self, id: u64) -> Result<Status, Error> {
        let id_text = id.to_string();
        let said = self.gh(&[
            "run",
            "view",
            &id_text,
            "--repo",
            REPOSITORY,
            "--json",
            "status,conclusion,jobs",
        ])?;
        let answer = |cause: String| Error::Answer {
            what: format!("run {id}"),
            said: cause,
        };
        let run: Value = serde_json::from_str(&said).map_err(|cause| answer(cause.to_string()))?;
        let text = |value: &Value, key: &str| {
            value
                .get(key)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        };
        let jobs = run
            .get("jobs")
            .and_then(Value::as_array)
            .ok_or_else(|| answer("it lists no jobs".into()))?
            .iter()
            .map(|job| (text(job, "name"), text(job, "conclusion")))
            .collect();
        Ok(Status {
            status: text(&run, "status"),
            conclusion: text(&run, "conclusion"),
            jobs,
        })
    }

    /// Waits for the run `watched` names to appear and to end, and refuses one
    /// that did not end green. Its id.
    pub(super) fn watch(&mut self, watched: &Watched) -> Result<u64, Error> {
        let Watched {
            label, workflow, ..
        } = *watched;
        let mut found = None;
        for _ in 0..self.pace.appear_attempts {
            let newest = self.newest_run(workflow, Some(watched.commit), watched.event)?;
            if newest.is_some() && newest != watched.before {
                found = newest;
                break;
            }
            self.system.pause(self.pace.appear);
        }
        let id = found.ok_or_else(|| {
            refused(format!(
                "{label}: no new run of {workflow} appeared on {}",
                short(watched.commit)
            ))
        })?;
        self.say(&format!("{label}: run {id} of {workflow}, until it ends"));
        let ended = loop {
            let status = self.status(id)?;
            if status.status == "completed" {
                break status;
            }
            self.system.pause(self.pace.poll);
        };
        self.say(&format!("{label} run {id}: {}", ended.conclusion));
        for (job, conclusion) in &ended.jobs {
            self.say(&format!("  {job} {conclusion}"));
        }
        if ended.conclusion != "success" {
            return Err(refused(format!(
                "{label} run {id} ended {}: nothing after it was done",
                ended.conclusion
            )));
        }
        if watched.both_platforms {
            for platform in ["macos", "windows"] {
                let green = ended
                    .jobs
                    .iter()
                    .any(|(job, conclusion)| job.contains(platform) && conclusion == "success");
                if !green {
                    return Err(refused(format!(
                        "{label} run {id} holds no green {platform} gate: both platforms must pass"
                    )));
                }
            }
        }
        Ok(id)
    }

    /// What the feed `name` serves, as installed apps read it; none where it
    /// answers no document.
    pub(super) fn feed(&mut self, name: &str) -> Result<Option<String>, Error> {
        let address = format!("{DOWNLOADS}/{name}/latest.json");
        let invocation = Invocation::new("curl", &self.root)
            .args(["-sfL", "--max-time", "60"])
            .arg(&address);
        let answered = self.system.capture(&invocation)?;
        match answered.code {
            0 => Ok(Some(answered.stdout)),
            // curl's status for an HTTP error, 404 among them, with -f.
            22 => Ok(None),
            code => Err(Error::Program {
                command: invocation.display(),
                code,
                said: answered.stderr.trim().to_owned(),
            }),
        }
    }

    /// The release the feed `name` names, if it serves one.
    pub(super) fn feed_version(&mut self, name: &str) -> Result<Option<Version>, Error> {
        Ok(self
            .feed(name)?
            .and_then(|body| named_release(body.as_bytes())))
    }

    /// Reads the feed `name` until it serves `version`, and refuses one that
    /// still does not after the reads the pace allows.
    pub(super) fn wait_for_feed(&mut self, name: &str, version: &str) -> Result<(), Error> {
        let mut served = None;
        for attempt in 0..self.pace.feed_attempts {
            if attempt > 0 {
                self.system.pause(self.pace.feed);
            }
            served = self.feed_version(name)?;
            if served.as_ref().map(Version::as_str) == Some(version) {
                self.say(&format!("{name} serves {version}"));
                return Ok(());
            }
        }
        Err(refused(format!(
            "{name} does not serve {version}: it serves {}",
            served_text(served.as_ref())
        )))
    }
}

/// What a feed serves, as it is said.
pub(super) fn served_text(version: Option<&Version>) -> String {
    version.map_or_else(|| "no release".to_owned(), ToString::to_string)
}

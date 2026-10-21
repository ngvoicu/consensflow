//! What a release asks of GitHub: its workflow runs, through `gh`, and the
//! feeds, through `curl` at the address installed apps read them at.
//!
//! A run is watched as release-305.sh watched it: the newest run of the
//! workflow (on the commit, by the event) that is not the one that was newest
//! before it was asked for, then looked at until it has ended. `gh run watch`
//! can return before a run ends, so only the run's own status is read.
//!
//! A feed is read as installed apps read it, and read again on a 404 or a
//! network error within the reads the pace allows: a feed just swapped can
//! answer 404 for minutes while GitHub's download links catch up.

use cf_publish::version::{named_release, Version};
use serde_json::Value;

use super::{refused, short, Error, Release, ALPHA_FEED, DOWNLOADS, REPOSITORY, STABLE_FEED};
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
    /// The tag whose push started the run, which stays on upstream whatever
    /// the run does: what is said when it does not end green.
    pub tag: Option<&'a str>,
}

/// What a person does about a tag on upstream whose Release run `run` did not
/// end green, or might not have: rerun it, which the publisher finishes from
/// where it stopped, or take the version as spent.
pub(super) fn rerun_advice(tag: &str, run: Option<u64>) -> String {
    let rerun = match run {
        Some(id) => format!(
            "if run {id} failed, rerun it, `gh run rerun {id} --failed --repo {REPOSITORY}`"
        ),
        None => format!(
            "find its run, `gh run list --repo {REPOSITORY} --workflow release.yml --event push`, and if it failed rerun it, `gh run rerun <id> --failed --repo {REPOSITORY}`"
        ),
    };
    let version = tag.strip_prefix('v').unwrap_or(tag);
    format!(
        "{rerun}: the publisher finishes from where it stopped; then read the feeds, \
         {DOWNLOADS}/{ALPHA_FEED}/latest.json and {DOWNLOADS}/{STABLE_FEED}/latest.json. \
         A failure that is real spends {version}: release the next version"
    )
}

/// One read of a feed.
enum Read {
    /// It serves this release.
    Serves(Version),
    /// It answered, with no release in it, or with an HTTP error (404 among them).
    Nothing,
    /// It could not be reached: what curl said.
    Unreachable(String),
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
            let then = watched.tag.map_or_else(
                || "nothing after it was done".to_owned(),
                |tag| format!("{tag} is on upstream; {}", rerun_advice(tag, None)),
            );
            refused(format!(
                "{label}: no new run of {workflow} appeared on {}: {then}",
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
            let then = watched.tag.map_or_else(
                || "nothing after it was done".to_owned(),
                |tag| format!("{tag} stays on upstream; {}", rerun_advice(tag, Some(id))),
            );
            return Err(refused(format!(
                "{label} run {id} ended {}: {then}",
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

    /// The address installed apps read the feed `name` at.
    pub(super) fn feed_address(name: &str) -> String {
        format!("{DOWNLOADS}/{name}/latest.json")
    }

    /// One read of the feed `name`.
    fn read_feed(&mut self, name: &str) -> Result<Read, Error> {
        let invocation = Invocation::new("curl", &self.root)
            .args(["-sfL", "--max-time", "60"])
            .arg(Self::feed_address(name));
        let answered = self.system.capture(&invocation)?;
        Ok(match answered.code {
            0 => named_release(answered.stdout.as_bytes()).map_or(Read::Nothing, Read::Serves),
            // curl's status for an HTTP error, 404 among them, with -f.
            22 => Read::Nothing,
            code => Read::Unreachable(format!("curl ended {code}: {}", answered.stderr.trim())),
        })
    }

    /// Reads the feed `name` until `wanted` says what it serves is the answer,
    /// within the reads the pace allows: the last thing it served or answered
    /// when none was.
    fn read_until(
        &mut self,
        name: &str,
        wanted: impl Fn(&Version) -> bool,
    ) -> Result<Result<Version, String>, Error> {
        let mut last = String::from("no answer");
        for attempt in 0..self.pace.feed_attempts {
            if attempt > 0 {
                self.system.pause(self.pace.feed);
            }
            match self.read_feed(name)? {
                Read::Serves(version) if wanted(&version) => return Ok(Ok(version)),
                Read::Serves(version) => last = format!("it serves {version}"),
                Read::Nothing => last = "it serves no release".to_owned(),
                Read::Unreachable(said) => last = said,
            }
        }
        Ok(Err(last))
    }

    /// The release the feed `name` serves, read again on a 404 or a network
    /// error; a feed that never answers one is an outage, and refused.
    pub(super) fn served_by(&mut self, name: &str) -> Result<Version, Error> {
        let reads = self.pace.feed_attempts;
        self.read_until(name, |_| true)?.map_err(|last| {
            refused(format!(
                "{name} answered no release in {reads} reads ({last}): an outage of the feed or of GitHub, \
                 not a version to change; nothing was done, run again later"
            ))
        })
    }

    /// Reads the feed `name` until it serves `version`, after the Release run
    /// `run` ended green, and says what to do when it still does not.
    pub(super) fn wait_for_feed(
        &mut self,
        name: &str,
        version: &str,
        run: u64,
    ) -> Result<(), Error> {
        let reads = self.pace.feed_attempts;
        match self.read_until(name, |served| served.as_str() == version)? {
            Ok(_) => {
                self.say(&format!("{name} serves {version}"));
                Ok(())
            }
            Err(last) => Err(refused(format!(
                "release run {run} ended green, its own check of the feeds with it, yet {name} \
                 does not serve {version} after {reads} reads ({last}): the release is published, and \
                 GitHub's download links can lag; read it again in a few minutes, `curl -sL {}`, \
                 and run nothing again",
                Self::feed_address(name)
            ))),
        }
    }
}

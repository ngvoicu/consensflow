//! What a release asks of GitHub: its workflow runs, through `gh`, and the
//! feeds, through `curl` at the address installed apps read them at.
//!
//! A run is watched as release-305.sh watched it: the newest run of the
//! workflow (on the commit, by the event) that is not the one that was newest
//! before it was asked for, then looked at until it has ended. `gh run watch`
//! can return before a run ends, so only the run's own status is read.
//!
//! A hand run of the Release workflow (the trial, [`Release::trial`]) is started
//! with `gh workflow run` and watched the same way, by the event that started it.
//! A watched run that does not end green is refused with what failed in it, the
//! job and the step, and the address to read it at. A run is bound to the commit
//! it is watched for twice: it is looked for by that commit, and once found, its
//! own `headSha` must be it. A `gh` that cannot be asked about a run, once it is
//! asked for, is a refusal too, naming the run and what is left of it.
//!
//! CI is not watched by its newest run: whether it passed a commit is asked as
//! the release's own gate asks it (cf-release's `ci_passed::verdict`, through
//! `gh api`), so that a commit the command tags is one the tag's run takes, and
//! one it refuses is one the tag's run would refuse. It is asked again while a
//! run of CI on the commit goes on, or none has appeared yet.
//!
//! A feed is read as installed apps read it, and read again on a 404 or a
//! network error within the reads the pace allows: a feed just swapped can
//! answer 404 for minutes while GitHub's download links catch up.

use cf_publish::version::{named_release, Version};
use cf_release::ci_passed::{self, GitHub, Refusal};
use serde_json::Value;

use super::{
    first_word, refused, short, Error, Release, ALPHA_FEED, DOWNLOADS, REPOSITORY, STABLE_FEED,
};
use crate::process::Invocation;

/// The run to watch: which workflow, on which commit, started by which event.
pub(super) struct Watched<'a> {
    /// How the run is named in what is said: `ci`, `release`.
    pub label: &'a str,
    pub workflow: &'a str,
    pub commit: &'a str,
    /// `push`, `workflow_dispatch`, or any.
    pub event: Option<&'a str>,
    /// The newest such run before this one was asked for, which is not it.
    pub before: Option<u64>,
    /// What the run that started it left on the remotes, which stays whatever
    /// the run does: what is said when it does not end green.
    pub left: Left<'a>,
    /// For a run asked for on a branch's head, which may move on before GitHub takes
    /// it: where it was asked, so that a run of the new head is told from a late one.
    pub head: Option<HeadOf<'a>>,
}

/// A run asked for on the head of a branch.
pub(super) struct HeadOf<'a> {
    pub branch: &'a str,
    /// The newest run of the workflow by the event, of any commit, before it was asked for.
    pub newest_before: Option<u64>,
}

/// What a run that did not end green leaves behind it.
#[derive(Clone, Copy)]
pub(super) enum Left<'a> {
    /// Nothing was pushed for it.
    Nothing,
    /// Nothing was tagged: it is the hand run tried before a tag, and what to do next.
    Trial,
    /// The tag whose push started it, on upstream.
    Tag(&'a str),
    /// A promotion's branch, on both remotes, at its version commit, which CI
    /// runs on before the tag is pushed.
    Branch {
        branch: &'a str,
        tag: &'a str,
        commit: &'a str,
    },
}

impl Left<'_> {
    /// What to do about what was left, when run `run` (if there was one) did not end green.
    fn then(&self, run: Option<u64>) -> String {
        match *self {
            Left::Nothing => "nothing after it was done".to_owned(),
            Left::Trial => match run {
                Some(_) => "nothing was tagged. Read the failed step at the address; for a flake run the command \
                            again, which starts a new trial; for a real failure fix it on main so that CI runs \
                            again, then run the command again"
                    .to_owned(),
                None => format!(
                    "nothing was tagged. A hand run may still be going at GitHub: find it, `gh run list --repo \
                     {REPOSITORY} --workflow release.yml --event workflow_dispatch`, and run the command again \
                     once it ended"
                ),
            },
            Left::Tag(tag) => match run {
                Some(_) => format!("{tag} stays on upstream; {}", rerun_advice(tag, run)),
                None => format!("{tag} is on upstream; {}", rerun_advice(tag, None)),
            },
            Left::Branch {
                branch,
                tag,
                commit,
            } => {
                let rerun = match run {
                    Some(id) => format!(
                        "a flake: rerun it, `gh run rerun {id} --failed --repo {REPOSITORY}`"
                    ),
                    None => format!(
                        "start its CI by hand, `gh workflow run ci.yml --ref {branch} --repo {REPOSITORY}`"
                    ),
                };
                format!(
                    "{branch} is on origin and upstream at {}, and no tag was pushed. Its commit is the alpha's \
                     with the version alone changed: {rerun}, and once CI is green on it finish by hand, \
                     `{}`, then watch its Release run and read both feeds; or start over, \
                     `git push origin --delete {branch} && git push upstream --delete {branch} && git branch -D {branch}`, \
                     and run again",
                    short(commit),
                    finish_by_hand(tag, commit)
                )
            }
        }
    }
}

/// What tags a promotion's version commit by hand and pushes it, as the run would.
pub(super) fn finish_by_hand(tag: &str, commit: &str) -> String {
    format!(
        "git tag -a {tag} -F <notes> {commit} && git push origin {tag} && git push upstream {tag}"
    )
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

/// A job of a run: how it ended, and each of its steps by its name and how it ended.
struct Job {
    name: String,
    conclusion: String,
    steps: Vec<(String, String)>,
}

/// What a run says of itself.
struct Status {
    status: String,
    conclusion: String,
    /// The commit the run is of.
    head_sha: String,
    jobs: Vec<Job>,
}

impl Status {
    /// What did not pass in the run, as a person looks for it: each job that did
    /// not, and the steps of it that did not, or the job alone when it names none
    /// (a job cancelled whole has no step that ended).
    fn failures(&self) -> Vec<String> {
        let passed = |conclusion: &str| matches!(conclusion, "success" | "skipped" | "neutral");
        let mut failed = Vec::new();
        for job in self.jobs.iter().filter(|job| !passed(&job.conclusion)) {
            let steps: Vec<_> = job
                .steps
                .iter()
                .filter(|(_, conclusion)| !passed(conclusion))
                .collect();
            if steps.is_empty() {
                failed.push(format!(
                    "job {} ended {}",
                    job.name,
                    how_it_ended(&job.conclusion)
                ));
            }
            for (step, conclusion) in steps {
                failed.push(format!(
                    "job {}, step \"{step}\" ended {}",
                    job.name,
                    how_it_ended(conclusion)
                ));
            }
        }
        failed
    }
}

/// How a job or a step ended, as it is said: one that GitHub gives no conclusion ended without one.
fn how_it_ended(conclusion: &str) -> &str {
    if conclusion.is_empty() {
        "without a conclusion"
    } else {
        conclusion
    }
}

/// Where a run is read on GitHub.
fn run_address(id: u64) -> String {
    format!("https://github.com/{REPOSITORY}/actions/runs/{id}")
}

impl Watched<'_> {
    /// What is said when `gh` cannot be asked about the run `run`, or about the one
    /// that was asked for and is looked for: the run and its address, why, and
    /// what is left of it. The run goes on at GitHub, whatever the command does.
    fn unasked(&self, error: &Error, run: Option<u64>) -> Error {
        let about = match run {
            Some(id) => format!(
                "{} run {id} ({}): gh could not say how it went",
                self.label,
                run_address(id)
            ),
            None => format!(
                "{}: the run of {} on {} could not be looked for",
                self.label,
                self.workflow,
                short(self.commit)
            ),
        };
        refused(format!("{about} ({error}); {}", self.left.then(run)))
    }
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
        Ok(self.newest(workflow, commit, event)?.map(|(id, _)| id))
    }

    /// The same, with the commit the run is of.
    fn newest(
        &mut self,
        workflow: &str,
        commit: Option<&str>,
        event: Option<&str>,
    ) -> Result<Option<(u64, String)>, Error> {
        let mut args = vec!["run", "list", "--repo", REPOSITORY, "--workflow", workflow];
        if let Some(commit) = commit {
            args.extend(["--commit", commit]);
        }
        if let Some(event) = event {
            args.extend(["--event", event]);
        }
        args.extend(["--limit", "1", "--json", "databaseId,headSha"]);
        let said = self.gh(&args)?;
        let runs: Value = serde_json::from_str(&said).map_err(|cause| Error::Answer {
            what: format!("the runs of {workflow}"),
            said: cause.to_string(),
        })?;
        let Some(run) = runs.get(0) else {
            return Ok(None);
        };
        let id = run.get("databaseId").and_then(Value::as_u64);
        let sha = run
            .get("headSha")
            .and_then(Value::as_str)
            .unwrap_or_default();
        Ok(id.map(|id| (id, sha.to_owned())))
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
            "status,conclusion,headSha,jobs",
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
            .map(|job| Job {
                name: text(job, "name"),
                conclusion: text(job, "conclusion"),
                steps: job
                    .get("steps")
                    .and_then(Value::as_array)
                    .map(|steps| {
                        steps
                            .iter()
                            .map(|step| (text(step, "name"), text(step, "conclusion")))
                            .collect()
                    })
                    .unwrap_or_default(),
            })
            .collect();
        Ok(Status {
            status: text(&run, "status"),
            conclusion: text(&run, "conclusion"),
            head_sha: text(&run, "headSha"),
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
            let newest = self
                .newest_run(workflow, Some(watched.commit), watched.event)
                .map_err(|error| watched.unasked(&error, None))?;
            if newest.is_some() && newest != watched.before {
                found = newest;
                break;
            }
            if let Some(head) = &watched.head {
                self.main_moved(watched, head)?;
            }
            self.system.pause(self.pace.appear);
        }
        let id = found.ok_or_else(|| {
            refused(format!(
                "{label}: no new run of {workflow} appeared on {}: {}",
                short(watched.commit),
                watched.left.then(None)
            ))
        })?;
        self.say(&format!("{label}: run {id} of {workflow}, until it ends"));
        let ended = loop {
            let status = self
                .status(id)
                .map_err(|error| watched.unasked(&error, Some(id)))?;
            if status.head_sha != watched.commit {
                return Err(refused(format!(
                    "{label} run {id} ({}) is of {}, not {}: {}",
                    run_address(id),
                    short(&status.head_sha),
                    short(watched.commit),
                    watched.left.then(Some(id))
                )));
            }
            if status.status == "completed" {
                break status;
            }
            self.system.pause(self.pace.poll);
        };
        self.say(&format!("{label} run {id}: {}", ended.conclusion));
        for job in &ended.jobs {
            self.say(&format!("  {} {}", job.name, job.conclusion));
        }
        if ended.conclusion != "success" {
            let address = run_address(id);
            let failed = ended.failures();
            let at = if failed.is_empty() {
                address
            } else {
                format!("{}; {address}", failed.join(", "))
            };
            return Err(refused(format!(
                "{label} run {id} ended {} ({at}): {}",
                ended.conclusion,
                watched.left.then(Some(id))
            )));
        }
        Ok(id)
    }

    /// Refuses a run asked for on a branch's head when the branch moved on before
    /// GitHub took the head: that run is of the new head, not the commit watched
    /// for, and would be waited for in vain. Called when the run watched for is
    /// not listed yet. A newer run of another commit is not that by itself: it may
    /// be someone else's, with ours only late in GitHub's list. It is only when the
    /// branch is no longer at the commit asked for.
    fn main_moved(&mut self, watched: &Watched, head: &HeadOf) -> Result<(), Error> {
        let asked = |error: Error| watched.unasked(&error, None);
        let newest = self
            .newest(watched.workflow, None, watched.event)
            .map_err(asked)?;
        let Some((id, sha)) = newest else {
            return Ok(());
        };
        if Some(id) == head.newest_before || sha == watched.commit {
            return Ok(());
        }
        let reference = format!("refs/heads/{}", head.branch);
        let said = self
            .git(&["ls-remote", "upstream", &reference])
            .map_err(asked)?;
        let now = first_word(&said);
        if now == watched.commit {
            return Ok(());
        }
        Err(refused(format!(
            "{} moved on to {}: run {id} is of {}, not {}; nothing was tagged. Run the command again, once \
             that run ({}) has ended",
            head.branch,
            short(&now),
            short(&sha),
            short(watched.commit),
            run_address(id)
        )))
    }

    /// A hand run of the Release workflow on `commit`, started and watched to its
    /// end: the Mac job builds, signs and smoke-tests the app as the tag's run
    /// will, the Windows job builds, and nothing is published (only the push of a
    /// tag publishes). What would fail the tag's run fails this one first, with no
    /// tag made: a tag whose run failed is left behind, and a tag is never moved or
    /// pushed again, so its version would be spent.
    ///
    /// A hand run is of a branch's head, not of a commit, and `main` is the only
    /// branch the `release` environment (which holds the signing keys) admits
    /// besides tags: `commit` must be main's head, which the caller has just seen.
    /// A run of another commit would not be the one watched, which is looked for by
    /// the commit. The run, once it ends green.
    pub(super) fn trial(&mut self, commit: &str) -> Result<u64, Error> {
        let before = self.newest_run("release.yml", Some(commit), Some("workflow_dispatch"))?;
        let newest_before = self.newest_run("release.yml", None, Some("workflow_dispatch"))?;
        self.gh(&[
            "workflow",
            "run",
            "release.yml",
            "--repo",
            REPOSITORY,
            "--ref",
            "main",
        ])?;
        self.say(&format!(
            "trial: a hand run of release.yml asked for, on main at {}",
            short(commit)
        ));
        self.watch(&Watched {
            label: "trial",
            workflow: "release.yml",
            commit,
            event: Some("workflow_dispatch"),
            before,
            left: Left::Trial,
            head: Some(HeadOf {
                branch: "main",
                newest_before,
            }),
        })
    }

    /// Waits until CI's full gate passed `commit` on both platforms, by the
    /// rule the tag's gate holds it to (a run of `ci.yml` by a push or by hand,
    /// green with both summaries), and refuses it once CI ended without: what
    /// was found, then what `left` says to do. A run still going, or none yet,
    /// is waited for. The run that passed it.
    pub(super) fn ci_passed(&mut self, commit: &str, left: Left) -> Result<u64, Error> {
        let mut looks = 0;
        let mut said_going = false;
        loop {
            match ci_passed::verdict(&mut Api { release: self }, REPOSITORY, commit) {
                Ok(passed) => {
                    self.say(&format!(
                        "ci: run {} passed {} on both platforms",
                        passed.run,
                        short(commit)
                    ));
                    for red in &passed.red {
                        self.say(&format!("ci: {red}, beside it"));
                    }
                    return Ok(passed.run);
                }
                Err(Refusal::Running(said)) => {
                    if !said_going {
                        self.say(&format!(
                            "ci: waiting until CI's run of {} ends ({said})",
                            short(commit)
                        ));
                        said_going = true;
                    }
                    self.system.pause(self.pace.poll);
                }
                Err(Refusal::NeverRan(_)) if looks < self.pace.appear_attempts => {
                    looks += 1;
                    self.system.pause(self.pace.appear);
                }
                Err(Refusal::NeverRan(said)) => {
                    return Err(refused(format!("{said}; {}", left.then(None))));
                }
                Err(Refusal::Red { said, run }) => {
                    return Err(refused(format!("{said}; {}", left.then(Some(run)))));
                }
                Err(Refusal::Unasked(said)) => {
                    return Err(Error::Answer {
                        what: format!("whether CI passed {}", short(commit)),
                        said,
                    });
                }
            }
        }
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
        self.served(name, || "nothing was done, run again later".to_owned())
    }

    /// The same, after the Release run `run` ended green, when `name` served
    /// `was` before it: an outage then says the release is out, and to read
    /// the feed by hand.
    pub(super) fn served_after(
        &mut self,
        name: &str,
        run: u64,
        was: &Version,
    ) -> Result<Version, Error> {
        self.served(name, || {
            format!(
                "release run {run} ended green, so the release is published; read {name} by hand, \
                 `curl -sL {}`, which served {was} before it, and run nothing again",
                Self::feed_address(name)
            )
        })
    }

    fn served(&mut self, name: &str, then: impl Fn() -> String) -> Result<Version, Error> {
        let reads = self.pace.feed_attempts;
        self.read_until(name, |_| true)?.map_err(|last| {
            refused(format!(
                "{name} answered no release in {reads} reads ({last}): an outage of the feed or of GitHub, \
                 not a version to change; {}",
                then()
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

/// GitHub's API as the release's gate asks it, through the machine's `gh`.
struct Api<'r, 'a> {
    release: &'r mut Release<'a>,
}

impl GitHub for Api<'_, '_> {
    fn get(&mut self, path: &str) -> Result<String, String> {
        self.release
            .gh(&["api", path])
            .map_err(|error| error.to_string())
    }
}

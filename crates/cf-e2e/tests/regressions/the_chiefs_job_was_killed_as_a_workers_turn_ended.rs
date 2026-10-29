//! The chief's background job was killed as a worker's turn ended.
//!
//! Seen in Claude Code's record of poker-lab's chief on 2026-10-15, ConsensFlow 3.0.4,
//! Claude Code 2.1.301, and read as a suspicion by T-814: the chief's watcher, a
//! background shell, was `killed` at 20:43:43.03 UTC, 0.97 s after worker T-440's turn
//! ended and 0.1 s after the ledger marked T-440 done. If a worker's window, at its
//! turn's end or its close, could end a process of another window, it could end any
//! window's work, the chief's among them.
//!
//! T-827 read what ended it: the app itself. The lead's session had asked the owner to
//! quit and reopen ConsensFlow (the login PATH of `a-slow-login-shell-costs-the-path.md`
//! in consensflow-sme), and the daemon logged its input's end at 20:43:42.996, the
//! moment a 3.0.4 quit stops it, before it asks every window's programs to end; the
//! daemon's close of T-440's window, asked at its result, failed then with `eof`
//! (`window.kill_failed`) and never ran; the chief's Claude Code, asked to end, killed
//! its own background shell 38 ms later; no ConsensFlow process ran at 20:43:56, the
//! app was opened again at 20:44:35, and the chief's window resumed its session. T-440's
//! turn had left nothing running. The report is
//! `issues/a-chiefs-job-was-killed-as-a-workers-turn-ended.md` in consensflow-sme.
//!
//! What the owner should see, and these cases hold on the real pane host: a worker's
//! window that closes, after its result or at a cancel in the middle of its turn, ends
//! the jobs its turn left, and nothing of the chief's window: neither a job under the
//! chief's tool shell (Claude Code's `run_in_background`) nor one the chief detached
//! from its shell, which has no parent but the system's init (`nohup … &`, and a
//! Claude Code shell whose harness has gone). The chief's window stays as it was. The
//! rig's stranger (`rig::stranger`) holds the same of every process that is no window's.
//!
//! The plants that make these fail are `tests/plants/daemon/other-windows.toml`.

use std::path::PathBuf;
use std::thread::sleep;

use cf_e2e::process::{is_alive, signal, started, Signal};
use cf_e2e::rig::{Job, Project, Rig};
use cf_e2e::Error;
use serde_json::{json, Value};

use crate::{config, secs, Outcome};

/// The chief's own jobs, as it started them: one under its tool shell, and one it
/// detached from a shell that then ended.
struct ChiefsJobs {
    jobs: Vec<(Job, Option<String>)>,
}

impl ChiefsJobs {
    /// The chief starts both, in a turn of its own, and they run.
    fn started(rig: &Rig, project: &Project) -> cf_e2e::Result<Self> {
        rig.tell(
            project.id(),
            "JOB 600 BACKGROUND --detach 600 the chief left two jobs",
        )?;
        rig.wait_for("the chief's jobs to run", secs(90), || {
            Ok(rig.jobs("job").len() == 2)
        })?;
        // The detaching shell ends a moment after its job is up (`BACKGROUND --detach`
        // answers once it has): the job is the system's init's by then.
        sleep(secs(3));
        let jobs = rig
            .jobs("job")
            .into_iter()
            .map(|job| {
                let began = started(job.pid());
                (job, began)
            })
            .collect::<Vec<_>>();
        if jobs.iter().any(|(_, began)| began.is_none()) {
            return Err(Error::Daemon("a job of the chief's is gone".to_owned()));
        }
        Ok(Self { jobs })
    }

    /// Their files, which tell them from the worker's.
    fn files(&self) -> Vec<PathBuf> {
        self.jobs
            .iter()
            .map(|(job, _)| job.file().to_path_buf())
            .collect()
    }

    /// Holds that each still runs, the same process it was, and was never asked to end.
    fn run_on(&self, when: &str) {
        for (job, began) in &self.jobs {
            assert!(
                is_alive(job.pid()) && started(job.pid()) == *began,
                "{when}: the chief's job {} runs on",
                job.file().display()
            );
            assert!(
                !job.released() && !job.finished(),
                "{when}: the chief's job {} was never asked to end",
                job.file().display()
            );
        }
    }

    /// Ends them, so that the detached one does not outlive the case: no close ends it on
    /// a Mac (a job in a session of its own, with no marker), and the other goes with the
    /// chief's window at the rig's close all the same.
    fn end(&self, rig: &Rig) -> Outcome {
        self.kill_what_runs()?;
        rig.wait_for("the chief's jobs to be ended", secs(30), || {
            Ok(self.jobs.iter().all(|(job, _)| !is_alive(job.pid())))
        })?;
        Ok(())
    }

    /// Kills each that is still the process it was.
    fn kill_what_runs(&self) -> Outcome {
        for (job, began) in &self.jobs {
            if is_alive(job.pid()) && started(job.pid()) == *began {
                signal(job.pid(), Signal::Kill)?;
            }
        }
        Ok(())
    }
}

/// A case that fails before it ends them leaves none running: the detached one would
/// run its ten minutes out.
impl Drop for ChiefsJobs {
    fn drop(&mut self) {
        let _ = self.kill_what_runs();
    }
}

/// A project with a worker, whose chief has its two jobs running.
fn a_chief_with_jobs(rig: &Rig) -> cf_e2e::Result<(Project<'_>, String, ChiefsJobs)> {
    let project = Project::open(rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    let tier = added["member"]["tier"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    let chiefs = ChiefsJobs::started(rig, &project)?;
    Ok((project, tier, chiefs))
}

/// The jobs the worker's turn started, once there are `count` of them: every job that
/// is not the chief's.
fn the_workers_jobs(rig: &Rig, chiefs: &ChiefsJobs, count: usize) -> cf_e2e::Result<Vec<Job>> {
    let theirs = |rig: &Rig| {
        let chiefs = chiefs.files();
        rig.jobs("job")
            .into_iter()
            .filter(|job| !chiefs.iter().any(|file| file == job.file()))
            .collect::<Vec<_>>()
    };
    rig.wait_for("the worker's jobs to run", secs(90), || {
        Ok(theirs(rig).len() == count)
    })?;
    Ok(theirs(rig))
}

/// The pane of the chief's lane, as the board shows it.
fn the_chiefs_pane(project: &Project) -> cf_e2e::Result<Value> {
    Ok(project.lane("chief")?["pane"].clone())
}

/// Waits for the worker's window to close and for each of its jobs to be ended by it.
fn the_workers_close_ends(rig: &Rig, project: &Project, jobs: &[Job]) -> Outcome {
    rig.wait_for("the worker's window to close", secs(90), || {
        Ok(!project.lane("worker")?["pane"].is_object())
    })?;
    rig.wait_for("the worker's jobs to be ended", secs(60), || {
        Ok(jobs.iter().all(|job| !is_alive(job.pid())))
    })?;
    for job in jobs {
        assert!(!job.finished(), "the worker's job did not run to its end");
        #[cfg(unix)]
        assert!(
            job.released(),
            "the worker's job was asked to end, not killed"
        );
    }
    Ok(())
}

#[test]
fn a_workers_window_that_closes_after_its_result_ends_its_own_jobs_and_none_of_the_chiefs(
) -> Outcome {
    let rig = Rig::start(config())?;
    let (project, tier, chiefs) = a_chief_with_jobs(&rig)?;
    let pane = the_chiefs_pane(&project)?;
    assert!(pane.is_object(), "the chief's window is open: {pane}");

    // A job while its turn works, and one it ends its turn with.
    rig.tell(
        project.id(),
        &format!("DISPATCH --tier {tier} JOB 600 BACKGROUND 600 the worker left two jobs"),
    )?;
    let workers = the_workers_jobs(&rig, &chiefs, 2)?;
    rig.wait_for("T-1 to be done", secs(90), || {
        Ok(project.task(1)?["state"] == "done")
    })?;
    the_workers_close_ends(&rig, &project, &workers)?;

    // What a close does is done within moments of it; a moment more, and the chief's
    // window and its jobs are as they were.
    sleep(secs(3));
    chiefs.run_on("after the worker's close");
    assert_eq!(
        the_chiefs_pane(&project)?,
        pane,
        "the chief's window is the same"
    );
    chiefs.end(&rig)?;
    rig.close()?;
    Ok(())
}

#[test]
fn a_cancel_that_closes_a_workers_window_in_its_turn_ends_its_job_and_none_of_the_chiefs() -> Outcome
{
    let rig = Rig::start(config())?;
    let (project, tier, chiefs) = a_chief_with_jobs(&rig)?;
    let pane = the_chiefs_pane(&project)?;

    rig.tell(
        project.id(),
        &format!("DISPATCH --tier {tier} JOB 600 SLEEP 600 Reply with exactly: NEVER_OK"),
    )?;
    let workers = the_workers_jobs(&rig, &chiefs, 1)?;
    assert_eq!(project.task(1)?["state"], "working");
    let cancelled = rig.page("task.cancel", json!({ "project": project.id(), "task": 1 }))?;
    assert_eq!(cancelled["ok"], true, "{cancelled}");
    the_workers_close_ends(&rig, &project, &workers)?;

    sleep(secs(3));
    chiefs.run_on("after the cancel's close");
    assert_eq!(
        the_chiefs_pane(&project)?,
        pane,
        "the chief's window is the same"
    );
    chiefs.end(&rig)?;
    rig.close()?;
    Ok(())
}

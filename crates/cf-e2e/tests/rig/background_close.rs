//! A window that closes with a job its turn started still running, end to end
//! through the real pane host (`npm run test:integration`): a cancel closes it
//! mid-turn, the job is asked to end first, and whoever gave the task is told it was
//! running; a pause closes nothing. What the turn left once it is over is ended with
//! its result (`background_work`).
//!
//! The asking and the grace are Unix's. On Windows a window is ended at once and
//! nothing is asked of its programs (`pty::close` of the pane host says why): its job
//! is gone with it, having written nothing.

use std::thread;
#[cfg(windows)]
use std::time::Instant;

use cf_e2e::process::is_alive;
use cf_e2e::rig::{Job, Project, Rig};
use regex::Regex;
use serde_json::json;

use crate::background_work::{a_worker_given, notes, until_the_job_runs};
use crate::{config, secs, Outcome};

/// A job the agent's turn leaves while it is still at work, as a tool runs a command:
/// the turn works on, and the task with it.
fn a_worker_whose_turn_works_with_a_job_running(rig: &Rig) -> cf_e2e::Result<(Project<'_>, Job)> {
    let project = a_worker_given(rig, "JOB 120 SLEEP 120 Reply with exactly: WORKER_OK")?;
    let job = until_the_job_runs(rig)?;
    assert_eq!(project.task(1)?["state"], "working");
    Ok((project, job))
}

/// Waits until the chief has been told, once, that T-1's window closed with the
/// job still running, and the note names the job.
fn until_the_chief_is_told(rig: &Rig, project: &Project) -> Outcome {
    let closed = Regex::new(r"^T-1's window closed with background work still running: ")?;
    rig.wait_for("the chief to be told", secs(30), || {
        Ok(notes(project, &closed)?.len() == 1)
    })?;
    let told = notes(project, &closed)?;
    let body = told[0]["body"].as_str().unwrap_or_default();
    assert!(
        body.contains("--background-job"),
        "the note names the work: {body}"
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn cancelling_a_task_whose_job_still_runs_asks_the_job_to_end_first_and_tells_whoever_gave_the_task(
) -> Outcome {
    let rig = Rig::start(config())?;
    let (project, job) = a_worker_whose_turn_works_with_a_job_running(&rig)?;

    let stopped = rig.page("task.cancel", json!({ "project": project.id(), "task": 1 }))?;
    assert_eq!(stopped["ok"], true, "{stopped}");

    // The window closed with the job asked to end, not killed: it wrote RELEASED.
    rig.wait_for("the job to say it was asked to end", secs(30), || {
        Ok(job.released())
    })?;
    assert!(!job.finished());
    rig.wait_for("the job to be gone", secs(30), || Ok(!is_alive(job.pid())))?;

    until_the_chief_is_told(&rig, &project)?;
    rig.close()?;
    Ok(())
}

/// What the same cancel comes to on Windows: nothing is asked of the window's
/// programs, and the pane's job object ends the tree at once. The job is gone
/// with its window and had no chance to say anything, and whoever gave the task
/// is told all the same.
#[cfg(windows)]
#[test]
fn cancelling_a_task_whose_job_still_runs_ends_the_job_with_its_window_at_once_and_tells_whoever_gave_the_task(
) -> Outcome {
    let rig = Rig::start(config())?;
    let (project, job) = a_worker_whose_turn_works_with_a_job_running(&rig)?;

    let closing = Instant::now();
    let stopped = rig.page("task.cancel", json!({ "project": project.id(), "task": 1 }))?;
    assert_eq!(stopped["ok"], true, "{stopped}");

    rig.wait_for("the job to be gone", secs(30), || Ok(!is_alive(job.pid())))?;
    assert!(
        closing.elapsed() < secs(9),
        "no grace was given: the tree was ended at once ({:?})",
        closing.elapsed()
    );
    assert!(
        !job.released(),
        "it was asked nothing, and could not release"
    );
    assert!(!job.finished());

    until_the_chief_is_told(&rig, &project)?;
    rig.close()?;
    Ok(())
}

#[test]
fn pausing_a_task_whose_job_still_runs_leaves_its_window_and_the_job_as_they_are() -> Outcome {
    let rig = Rig::start(config())?;
    let (project, job) = a_worker_whose_turn_works_with_a_job_running(&rig)?;

    let paused = rig.page("task.pause", json!({ "project": project.id(), "task": 1 }))?;
    assert_eq!(paused["ok"], true, "{paused}");
    rig.wait_for("T-1 to be paused", secs(30), || {
        Ok(project.task(1)?["state"] == "paused")
    })?;

    // A paused task goes on in its window: nothing was closed, nothing asked to end.
    thread::sleep(secs(3));
    assert!(
        project.lane("worker")?["pane"].is_object(),
        "the window is open"
    );
    assert!(is_alive(job.pid()), "the job runs on");
    assert!(!job.released(), "and was not asked to end");
    rig.close()?;
    Ok(())
}

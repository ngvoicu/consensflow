//! A worker's background work outlives its turn (the report of 2026-10-09):
//! T-183's worker started a recording run under a lock tool in the background,
//! armed its harness's monitor and ended its turn. ConsensFlow took the turn's
//! end for the result, marked the task done and closed the window, and the job
//! died with it, killed outright (no RELEASED), and the monitor never woke the
//! worker. End to end through the real pane host (`npm run test:integration`):
//! fake agents in real PTYs, and a job of the fake agent's (its `background`
//! part) that writes RELEASED when it is asked to end and FINISHED when it ends
//! by itself.
//!
//! - A turn that ends while the job it started runs is not the task's result:
//!   the board says the window works, and the job runs on until it ends; the
//!   turn the job's end wakes is the one whose end is.
//! - Work that was there before the turn (a server the harness started with its
//!   window) is not the turn's.
//! - A window that closes with the job running (a cancel) asks it to end first,
//!   and whoever gave the task is told the job was running. A pause closes
//!   nothing.
//! - A job that ignores the request is killed once the grace is over.

use std::thread;
use std::time::Instant;

use cf_e2e::process::is_alive;
use cf_e2e::rig::{Config, Job, Project, Rig};
use cf_e2e::Error;
use regex::Regex;
use serde_json::{json, Value};

use crate::{config, secs, Outcome};

/// How long the daemon waits, once the job has ended, for the agent to go on
/// (`CONSENSFLOW_BACKGROUND_SETTLE_MS`): its default is ten seconds.
const SETTLE_MS: &str = "1500";

/// The rig's configuration with the daemon's settle time of the cases.
fn settling() -> Config {
    config().var("CONSENSFLOW_BACKGROUND_SETTLE_MS", SETTLE_MS)
}

/// The tier the worker was given, as a word the chief's brief names.
fn tier(added: &Value) -> &str {
    added["member"]["tier"].as_str().unwrap_or_default()
}

/// A project with one worker, given `words` through the chief, once its window
/// has started the job: the project and the job.
fn a_worker_with_a_job<'a>(rig: &'a Rig, words: &str) -> cf_e2e::Result<(Project<'a>, Job)> {
    let project = Project::open(rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    rig.tell(
        project.id(),
        &format!("DISPATCH --tier {} {words}", tier(&added)),
    )?;
    rig.wait_for("the worker's job to run", secs(60), || {
        Ok(rig.jobs("job").len() == 1)
    })?;
    let job = rig
        .jobs("job")
        .into_iter()
        .next()
        .ok_or_else(|| Error::Daemon("the worker's job is gone".to_owned()))?;
    Ok((project, job))
}

/// Waits until the worker's turn is over with its job started: the task has its
/// result, or the window says why it has not.
fn until_the_turn_ended(rig: &Rig, project: &Project) -> cf_e2e::Result<()> {
    rig.wait_for("the worker's turn to end", secs(60), || {
        let state = project.task(1)?["state"].clone();
        let over = ["done", "failed", "cancelled"]
            .iter()
            .any(|ended| state == *ended);
        Ok(over || project.lane("worker")?["activity"]["reason"].is_string())
    })
}

/// The results the chief has been given, oldest first.
fn results(project: &Project) -> cf_e2e::Result<Vec<Value>> {
    Ok(project
        .inbox("chief")?
        .into_iter()
        .filter(|message| message["kind"] == "result")
        .collect())
}

/// The notes of the chief's inbox that say `told`.
fn notes(project: &Project, told: &Regex) -> cf_e2e::Result<Vec<Value>> {
    Ok(project
        .inbox("chief")?
        .into_iter()
        .filter(|message| {
            message["kind"] == "note"
                && message["body"]
                    .as_str()
                    .is_some_and(|body| told.is_match(body))
        })
        .collect())
}

#[test]
fn a_turn_that_ends_with_the_job_it_started_running_is_not_the_result_the_job_ends_and_the_turn_it_wakes_is(
) -> Outcome {
    let rig = Rig::start(settling())?;
    let (project, job) = a_worker_with_a_job(&rig, "BACKGROUND --wake 12 started the job")?;
    until_the_turn_ended(&rig, &project)?;

    assert_eq!(
        project.task(1)?["state"],
        "working",
        "the turn ended with the job running: the task has no result yet"
    );
    assert_eq!(
        project.lane("worker")?["activity"],
        json!({ "state": "working", "reason": "background work running (1 process)" })
    );
    assert!(is_alive(job.pid()), "the job runs on");
    assert_eq!(
        results(&project)?,
        Vec::<Value>::new(),
        "the chief is given no result"
    );

    // The job ends by itself, the agent is woken, and its turn's end is the result.
    rig.wait_for("T-1 to be done", secs(60), || {
        Ok(project.task(1)?["state"] == "done")
    })?;
    assert!(job.finished(), "the job ran to its end");
    assert!(!job.released(), "and was never asked to end");
    rig.wait_for("the result to reach the chief", secs(30), || {
        Ok(!results(&project)?.is_empty())
    })?;
    assert_eq!(
        results(&project)?[0]["body"],
        "started the job\n\nwoken: started the job"
    );
    rig.close()?;
    Ok(())
}

#[test]
fn a_job_that_ends_with_no_turn_to_wake_leaves_the_last_turns_end_as_the_result_once_the_agent_has_had_a_moment(
) -> Outcome {
    let rig = Rig::start(settling())?;
    let (project, job) = a_worker_with_a_job(&rig, "BACKGROUND 6 started the job")?;
    until_the_turn_ended(&rig, &project)?;
    assert_eq!(
        project.task(1)?["state"],
        "working",
        "the job runs, and the task has no result yet"
    );

    rig.wait_for("T-1 to be done", secs(60), || {
        Ok(project.task(1)?["state"] == "done")
    })?;
    assert!(job.finished(), "the job ran to its end");
    assert!(!job.released());
    rig.wait_for("the result to reach the chief", secs(30), || {
        Ok(!results(&project)?.is_empty())
    })?;
    assert_eq!(results(&project)?[0]["body"], "started the job");
    rig.close()?;
    Ok(())
}

#[test]
fn work_the_window_had_before_its_turn_is_not_the_turns_a_server_the_harness_started_with_it_holds_nothing(
) -> Outcome {
    let rig = Rig::start(settling().var("CF_TEST_MCP_SERVER", "1"))?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    // A turn of a few seconds: the daemon has marked where it began when it ends.
    rig.tell(
        project.id(),
        &format!(
            "DISPATCH --tier {} SLEEP 3 Reply with exactly: WORKER_OK",
            tier(&added)
        ),
    )?;
    rig.wait_for("T-1 to be done", secs(60), || {
        Ok(project.task(1)?["state"] == "done")
    })?;
    assert!(
        rig.jobs("server").len() >= 2,
        "the chief and the worker each had a server"
    );
    rig.wait_for("the result to reach the chief", secs(30), || {
        Ok(!results(&project)?.is_empty())
    })?;
    assert_eq!(results(&project)?[0]["body"], "WORKER_OK");
    rig.close()?;
    Ok(())
}

#[test]
fn cancelling_a_task_whose_job_still_runs_asks_the_job_to_end_first_and_tells_whoever_gave_the_task(
) -> Outcome {
    let rig = Rig::start(settling())?;
    let (project, job) = a_worker_with_a_job(&rig, "BACKGROUND 120 started the job")?;
    until_the_turn_ended(&rig, &project)?;
    assert_eq!(project.task(1)?["state"], "working");

    let stopped = rig.page("task.cancel", json!({ "project": project.id(), "task": 1 }))?;
    assert_eq!(stopped["ok"], true, "{stopped}");

    // The window closed with the job asked to end, not killed: it wrote RELEASED.
    rig.wait_for("the job to say it was asked to end", secs(30), || {
        Ok(job.released())
    })?;
    assert!(!job.finished());
    rig.wait_for("the job to be gone", secs(30), || Ok(!is_alive(job.pid())))?;

    let closed = Regex::new(r"^T-1's window closed with background work still running: ")?;
    rig.wait_for("the chief to be told", secs(30), || {
        Ok(notes(&project, &closed)?.len() == 1)
    })?;
    let told = notes(&project, &closed)?;
    let body = told[0]["body"].as_str().unwrap_or_default();
    assert!(
        body.contains("--background-job"),
        "the note names the work: {body}"
    );
    rig.close()?;
    Ok(())
}

#[test]
fn pausing_a_task_whose_job_still_runs_leaves_its_window_and_the_job_as_they_are() -> Outcome {
    let rig = Rig::start(settling())?;
    let (project, job) = a_worker_with_a_job(&rig, "BACKGROUND 120 started the job")?;
    until_the_turn_ended(&rig, &project)?;

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

#[test]
fn a_job_that_ignores_the_signals_that_ask_it_to_end_is_killed_once_the_grace_of_ten_seconds_is_over(
) -> Outcome {
    let rig = Rig::start(settling())?;
    let (project, job) = a_worker_with_a_job(&rig, "BACKGROUND --deaf 120 started the job")?;
    until_the_turn_ended(&rig, &project)?;

    let closing = Instant::now();
    let stopped = rig.page("task.cancel", json!({ "project": project.id(), "task": 1 }))?;
    assert_eq!(stopped["ok"], true, "{stopped}");
    rig.wait_for("the job to be killed", secs(30), || {
        Ok(!is_alive(job.pid()))
    })?;
    assert!(
        closing.elapsed() >= secs(9),
        "the job was given its grace: {:?}",
        closing.elapsed()
    );
    assert!(!job.released(), "and was killed, not asked");
    rig.close()?;
    Ok(())
}

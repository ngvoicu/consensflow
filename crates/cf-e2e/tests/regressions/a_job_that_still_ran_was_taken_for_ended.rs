//! A job that still ran was taken for ended, and a held worker's words were lost: both
//! reviews of round 4 of the turn's end (GPT-6-Astra and GPT-6.1-Sol, 2026-10-16), in the
//! records Claude Code writes.
//!
//! **A notification of a job that runs.** Claude Code 2.1.301 watches what a background
//! command writes, and when it has written nothing for 45 seconds after a line that looks
//! like a question (`Continue?`), it wakes the agent with a notification that names the job,
//! says it "appears to be waiting for interactive input" and has no status, while the
//! command runs on. ConsensFlow ended the job on any notification that named it: the turn
//! the wake-up began, which said the job still waited, was taken for the result, and the
//! window closed on the job. What should happen: the job runs, the task stays held, its
//! chief told once (turn-end's smallest rule, 2026-10-21), and the job's end is the result.
//!
//! **A record written to the transcript alone.** Claude Code 2.1.292 writes the jobs a
//! resumed session's last process left with no record of their end in one notification,
//! `stopped`, to the transcript alone (`queueTranscriptOnly`), and answers nothing to it. It
//! ended the jobs, and the task held for them was answered: but the record was read as a
//! message of the user's, after the worker's words, and the result said the agent wrote no
//! answer. What should happen: the result is the words the worker's turn ended with.
//!
//! The stand-in does what Claude Code does (`CLAUDE-JOB --asks` and `--stopped` of the fake
//! agent), in the shapes of `cf-harness`' `tests/fixtures/claude`.

use std::thread;
use std::time::Instant;

use cf_e2e::process::is_alive;
use cf_e2e::rig::{Job, Project, Rig};
use cf_e2e::Error;
use serde_json::json;

use crate::a_workers_still_going_was_given_as_its_result::{tier, told};
use crate::{config, secs, Outcome};

/// The one job the worker's window runs.
fn the_job(rig: &Rig) -> cf_e2e::Result<Job> {
    rig.wait_for("the worker's job to run", secs(60), || {
        Ok(rig.jobs("job").len() == 1)
    })?;
    rig.jobs("job")
        .into_iter()
        .next()
        .ok_or_else(|| Error::Daemon("the worker's job is gone".to_owned()))
}

#[test]
fn a_job_claude_code_says_waits_for_input_still_runs_and_the_report_its_end_wakes_is_the_result(
) -> Outcome {
    const GOING: &str = "The migration runs in the background; I will report when it is done";
    const REPORT: &str = "The migration is done: 14 tables moved.";
    let rig = Rig::start(config())?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    rig.tell(
        project.id(),
        &format!(
            "DISPATCH --tier {} CLAUDE-JOB --asks 25 {GOING} :: {REPORT}",
            tier(&added)
        ),
    )?;
    let job = the_job(&rig)?;
    let began = Instant::now();
    rig.wait_for("the chief to be told it goes on", secs(40), || {
        Ok(told(&project)?.iter().any(|(kind, body)| {
            kind == "note"
                && body.starts_with("T-1 goes on in the background: @")
                && body.ends_with(GOING)
        }))
    })?;

    // Claude Code wakes the worker while the job runs (the stand-in five seconds after it
    // began), and the turn says it still waits: the task stays held, nothing is ended, and
    // the chief is not told again.
    thread::sleep(secs(15).saturating_sub(began.elapsed()));
    assert_eq!(project.task(1)?["state"], "working");
    assert!(
        told(&project)?.iter().all(|(kind, _)| kind != "result"),
        "{:?}",
        told(&project)?
    );
    assert!(is_alive(job.pid()), "the job runs");
    assert!(!job.released(), "and was never asked to end");

    // Its end wakes the report, which is the result.
    rig.wait_for("the job to end by itself", secs(60), || Ok(job.finished()))?;
    rig.wait_for("T-1 to be done", secs(60), || {
        Ok(project.task(1)?["state"] == "done")
    })?;
    let thread = told(&project)?;
    let kinds: Vec<&str> = thread.iter().map(|(kind, _)| kind.as_str()).collect();
    assert_eq!(kinds, ["note", "result"], "{thread:?}");
    assert!(thread[0].1.ends_with(GOING), "{}", thread[0].1);
    assert_eq!(thread[1].1, REPORT);
    rig.close()?;
    Ok(())
}

#[test]
fn a_held_workers_words_are_its_result_when_its_jobs_end_is_written_to_the_transcript_alone(
) -> Outcome {
    const SERVING: &str = "The preview server is up on port 4000";
    let rig = Rig::start(config())?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    rig.tell(
        project.id(),
        &format!(
            "DISPATCH --tier {} CLAUDE-JOB --stopped 8 {SERVING} :: NEVER_SAID",
            tier(&added)
        ),
    )?;
    let job = the_job(&rig)?;
    rig.wait_for("the chief to be told it goes on", secs(40), || {
        Ok(told(&project)?
            .iter()
            .any(|(kind, body)| kind == "note" && body.ends_with(SERVING)))
    })?;

    // The job ends, and Claude Code writes that it stopped to the transcript alone: no turn
    // follows, and the words the turn ended with are the result.
    rig.wait_for("the job to end by itself", secs(60), || Ok(job.finished()))?;
    rig.wait_for("T-1 to be done", secs(60), || {
        Ok(project.task(1)?["state"] == "done")
    })?;
    let thread = told(&project)?;
    let kinds: Vec<&str> = thread.iter().map(|(kind, _)| kind.as_str()).collect();
    assert_eq!(kinds, ["note", "result"], "{thread:?}");
    assert_eq!(
        thread[1].1, SERVING,
        "the worker's words, and no word that it wrote none"
    );
    rig.close()?;
    Ok(())
}

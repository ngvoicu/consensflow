//! A job the agent detached from its shell was never seen by ConsensFlow: the chief was not
//! told it ran, a pause closed the window it ran in, and a close ended it without a word.
//!
//! Found on 2026-11-02 by two tasks. T-1126 (Muse on Windows) saw Muse detach a `ping` with
//! `Start-Process` and its PowerShell exit at once: under both settings of `servers()` the
//! chief was told nothing, and the window's close named nothing. T-1117 (a pause keeps the
//! window of a worker that runs work) wrote down its limit: the pane host named a job
//! detached with `&` (Pi and Devin on the Mac) as neither the turn's work nor anything else,
//! so a paused window with only that running was closed, and the close ended it.
//!
//! The host's tree held the job all the same: on Windows the pane's job object holds what a
//! process starts wherever it goes (`Start-Process` is no breakaway), and on the Mac the
//! window's session holds what a shell that leads none of its own leaves. What it did not do
//! was say so: only a chain of processes that run, up to a shell the harness started, made a
//! process the turn's work, and a detached job has lost the shell (`cf-panes`' `work`). The
//! host now names it as work the window's close ends (T-1145), and what already stood on
//! that (T-1117's keep, the notes of a pause and of a close) tells the chief.
//!
//! What the owner should have seen, and these cases hold on the real pane host with a stand-in
//! that detaches a job as the harnesses do (`JOB --detach-here` on the Mac, with its shell
//! staying in the window's session; `--detach` on Windows, a PowerShell's `Start-Process`):
//!
//! - a pause that the worker ignores while its detached job runs keeps the window open, the
//!   job running, and tells the chief and the human, as for a job a shell still runs;
//! - the human's delete-session then ends the window and the job with it;
//! - a worker whose turn ends with a job detached has its result at once, and its window's
//!   close ends the job and **names** it to whoever gave the task.
//!
//! The limit, written where the check is (`cf-panes`' `work`): on the Mac a job detached from a
//! shell that leads a session of its own (Pi's and Claude Code's tool shells, the stand-in's
//! `--detach`) leaves the window's session, loses its parent and, being a system binary, shows
//! no marker: nothing ties it to the window, no close ends it, and the host cannot name it.
//! `a_job_the_agent_detached_from_its_shell_is_left_running_and_nothing_is_said_of_it`
//! (`rig/background_work.rs`) holds that case.
//!
//! The plants that make these fail are `tests/plants/daemon/detached-work.toml`.

use std::thread::sleep;

use cf_e2e::process::is_alive;
use cf_e2e::rig::{Project, Rig, OPEN};
use cf_e2e::Error;
use serde_json::{json, Value};

use crate::{config, secs, Outcome};

/// How the stand-in's tool detaches its job from the shell that runs it: where the job stays
/// in the window's session on the Mac, which is what the window's close ends there; a
/// PowerShell's `Start-Process` on Windows, where the pane's job holds it.
#[cfg(unix)]
const DETACH: &str = "--detach-here";
#[cfg(windows)]
const DETACH: &str = "--detach";

/// The notes that say the worker's window did not stop: the human's, then the chief's.
fn told(rig: &Rig, project: &Project) -> cf_e2e::Result<[Vec<String>; 2]> {
    let human = rig.page("inbox.get", json!({ "project": project.id() }))?["messages"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let said = |messages: Vec<Value>| -> Vec<String> {
        messages
            .into_iter()
            .filter_map(|message| message["body"].as_str().map(str::to_owned))
            .filter(|body| body.contains("did not stop"))
            .collect()
    };
    Ok([said(human), said(project.inbox("chief")?)])
}

/// `cf` with `words`, run in the chief's window as its agent runs it.
fn cf(rig: &Rig, chief: &Value, words: &[&str]) -> cf_e2e::Result<()> {
    let ran = rig.cf_in_window(chief, words)?;
    if ran.code != Some(0) {
        return Err(Error::Daemon(format!("cf {}: {ran}", words.join(" "))));
    }
    Ok(())
}

#[test]
fn a_busy_worker_that_detached_a_job_and_ignores_its_pause_keeps_its_window_and_the_job_and_the_chief_is_told(
) -> Outcome {
    let rig = Rig::start(config())?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    let tier = added["member"]["tier"].as_str().unwrap_or_default();
    // The worker's tool detaches a job from its shell, and its turn goes on, deaf to every key
    // (a turn on a tool call that holds it): nothing of the job is a shell's that still runs.
    rig.tell(
        project.id(),
        &format!("DISPATCH --tier {tier} JOB {DETACH} 600 HANG 600 Reply with exactly: NEVER_OK"),
    )?;
    rig.wait_for("the worker's detached job to run", secs(90), || {
        Ok(project.task(1)?["state"] == "working" && rig.jobs("job").len() == 1)
    })?;
    let job = rig
        .jobs("job")
        .into_iter()
        .next()
        .ok_or_else(|| Error::Daemon("the job is gone".to_owned()))?;
    let handle = project.task(1)?["assignee"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    let pane = project.lane(&handle)?["pane"].clone();
    let chief = rig.open_frame(&format!("p{}-chief", project.id()), OPEN)?;

    // The chief pauses T-1; the window ignores every key: three rounds, then the look that asks
    // what the window runs, and one more look at it ten seconds on.
    cf(&rig, &chief, &["task", "pause", "T-1"])?;
    rig.wait_for("the chief to be told the window is kept", secs(60), || {
        Ok(!told(&rig, &project)?[1].is_empty())
    })?;
    sleep(secs(15));
    assert_eq!(project.task(1)?["state"], "paused");
    assert_eq!(
        project.lane(&handle)?["pane"],
        pane,
        "the window is open, as it was"
    );
    assert!(
        is_alive(job.pid()) && !job.released(),
        "the detached job runs on, never asked to end"
    );
    let [human, to_chief] = told(&rig, &project)?;
    assert!(
        to_chief.len() == 1
            && to_chief[0].contains("ConsensFlow keeps the window open for it")
            && to_chief[0].contains("--background-job"),
        "the chief was told, and the note names the job: {to_chief:?}"
    );
    assert!(
        human.len() == 1 && human[0].contains(&format!("delete @{handle}'s session")),
        "the human was told: {human:?}"
    );

    // The human deletes the session: that ends the window, and its work with it.
    let ended = rig.page(
        "session.end",
        json!({ "project": project.id(), "handle": handle }),
    )?;
    assert_eq!(ended["ok"], true, "{ended}");
    rig.wait_for("the window's work to end with it", secs(60), || {
        Ok(!is_alive(job.pid()))
    })?;
    Ok(())
}

#[test]
fn a_workers_close_names_the_job_it_ended_that_the_agent_had_detached_before_its_turn_ended(
) -> Outcome {
    let rig = Rig::start(config())?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    let tier = added["member"]["tier"].as_str().unwrap_or_default();
    rig.tell(
        project.id(),
        &format!("DISPATCH --tier {tier} BACKGROUND {DETACH} 600 started the job"),
    )?;
    rig.wait_for("the worker's detached job to run", secs(90), || {
        Ok(rig.jobs("job").len() == 1)
    })?;
    let job = rig
        .jobs("job")
        .into_iter()
        .next()
        .ok_or_else(|| Error::Daemon("the job is gone".to_owned()))?;

    // The turn ended with the job running: its answer is the task's result at once.
    rig.wait_for("T-1 to be done", secs(60), || {
        Ok(project.task(1)?["state"] == "done")
    })?;
    // The window closes as a finished worker's does, and its close ends what it holds: the
    // job, which the chief is told of by name.
    rig.wait_for("the worker's window to close", secs(60), || {
        Ok(!project.lane("worker")?["pane"].is_object())
    })?;
    rig.wait_for("the job to be ended", secs(60), || Ok(!is_alive(job.pid())))?;
    rig.wait_for(
        "the chief to be told what the close ended",
        secs(30),
        || {
            Ok(project.inbox("chief")?.iter().any(|message| {
                message["kind"] == "note" && message["body"].as_str().is_some_and(|body| {
                    body.starts_with(
                        "T-1's window closed with background work running, which its close ended: ",
                    ) && body.contains("--background-job")
                })
            }))
        },
    )?;
    rig.close()?;
    Ok(())
}

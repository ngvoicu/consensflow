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
//! - Nor is a server the harness starts after the first message was marked, which
//!   is where Claude Code starts its MCP servers (found on 3.0.1, where a worker's
//!   task was never done): it stays in the group of the window's program, and a
//!   command the agent's tool runs is in a group of its own. The stand-in starts
//!   its server so (`CF_TEST_MCP_SERVER=late`), well past the pass that marks the
//!   turn, and a Claude worker's turn still ends with its result; with a job of the
//!   agent's beside the server, the window is held for the job alone. Nor is what
//!   the server starts, in whatever group: the browser of `@playwright/mcp` and
//!   `chrome-devtools-mcp`, which the stand-in's server launches in a session of
//!   its own (`CF_TEST_MCP_BROWSER`).
//! - A window that closes with the job running (a cancel) asks it to end first,
//!   and whoever gave the task is told the job was running. A pause closes
//!   nothing.
//! - A job that ignores the request is killed once the grace is over.
//!
//! The asking and the grace are Unix's. On Windows a window is ended at once and
//! nothing is asked of its programs (`pty::close` of the pane host says why): its
//! job is gone with it, having written nothing, and whoever gave the task is told
//! all the same.

use std::thread;
use std::time::Instant;

#[cfg(unix)]
use cf_e2e::process::group_of;
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

/// What the board says a window held for one process is doing: the count, and
/// the command it names after it (what the line holds of a long path is no more
/// than the first of it).
fn assert_held_for_one_process(activity: &Value) {
    let reason = activity["reason"].as_str().unwrap_or_default();
    let named = reason.strip_prefix("background work running (1 process): ");
    assert!(
        named.is_some_and(|command| !command.is_empty()),
        "the board says it waits for one process, and which: {activity}"
    );
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
    let activity = project.lane("worker")?["activity"].clone();
    assert_eq!(activity["state"], "working");
    assert_held_for_one_process(&activity);
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
    // In a group of its own, as Codex starts its helpers: no group tells it from the
    // turn's work, only that the window had it before its turn.
    let rig = Rig::start(settling().var("CF_TEST_MCP_SERVER", "apart"))?;
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

/// What these cases cannot hold on Windows, where a job object has no process
/// groups for the host to tell by: a server the harness starts after the first
/// message was marked is the turn's work there, and holds the first task's result
/// (written as a limit in the engine's `background` module). So they are Unix's:
/// where the host looks at the tree at all, which is macOS.
///
/// The server begins `THINKING` after the window's first message shows in its
/// record, past the pass that marks where the turn began, and the turn outlasts it.
#[cfg(unix)]
#[test]
fn a_server_the_harness_starts_after_the_first_message_was_marked_holds_nothing_the_workers_task_is_done(
) -> Outcome {
    // Found on 3.0.1: Claude Code starts its MCP servers as it takes its first
    // message, and the user's own held a worker's task for ever.
    let rig = Rig::start(settling().var("CF_TEST_MCP_SERVER", "late"))?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    rig.tell(
        project.id(),
        &format!(
            "DISPATCH --tier {} SLEEP 15 Reply with exactly: WORKER_OK",
            tier(&added)
        ),
    )?;
    // The chief's server and the worker's run while the worker's turn is at work.
    rig.wait_for("the servers to run", secs(60), || {
        let servers = rig.jobs("server");
        Ok(servers.len() >= 2 && servers.iter().all(|server| is_alive(server.pid())))
    })?;
    assert_eq!(
        project.task(1)?["state"],
        "working",
        "the servers began while the turn was at work"
    );
    rig.wait_for("T-1 to be done", secs(60), || {
        Ok(project.task(1)?["state"] == "done")
    })?;
    rig.wait_for("the result to reach the chief", secs(30), || {
        Ok(!results(&project)?.is_empty())
    })?;
    assert_eq!(results(&project)?[0]["body"], "WORKER_OK");
    rig.close()?;
    Ok(())
}

/// The owner's own setup: `@playwright/mcp` and `chrome-devtools-mcp` launch a
/// browser when a tool is used, in a group of its own. The browser is a child of
/// the server, and what a harness's server starts is the harness's whatever group it
/// goes to: a task that touched a page is done with the browser running, not held
/// for it as long as the server lives.
#[cfg(unix)]
#[test]
fn a_browser_the_server_launches_in_a_group_of_its_own_holds_nothing_either_the_workers_task_is_done(
) -> Outcome {
    let rig = Rig::start(
        settling()
            .var("CF_TEST_MCP_SERVER", "late")
            .var("CF_TEST_MCP_BROWSER", "1"),
    )?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    rig.tell(
        project.id(),
        &format!(
            "DISPATCH --tier {} SLEEP 15 Reply with exactly: WORKER_OK",
            tier(&added)
        ),
    )?;
    // The chief's server and browser and the worker's run while the worker's turn is at work.
    rig.wait_for("the servers and their browsers to run", secs(60), || {
        let (servers, browsers) = (rig.jobs("server"), rig.jobs("browser"));
        Ok(servers.len() >= 2
            && browsers.len() >= 2
            && servers
                .iter()
                .chain(&browsers)
                .all(|job| is_alive(job.pid())))
    })?;
    // The browsers are in a group of their own, the servers are not: the shapes this
    // case stands on.
    for browser in rig.jobs("browser") {
        assert_eq!(group_of(browser.pid()), Some(browser.pid()), "it leads one");
    }
    for server in rig.jobs("server") {
        assert_ne!(
            group_of(server.pid()),
            Some(server.pid()),
            "it is in its window's"
        );
    }
    assert_eq!(
        project.task(1)?["state"],
        "working",
        "the browsers began while the turn was at work"
    );
    rig.wait_for("T-1 to be done", secs(60), || {
        Ok(project.task(1)?["state"] == "done")
    })?;
    rig.wait_for("the result to reach the chief", secs(30), || {
        Ok(!results(&project)?.is_empty())
    })?;
    assert_eq!(results(&project)?[0]["body"], "WORKER_OK");
    rig.close()?;
    Ok(())
}

/// The same worker, whose agent leaves a job beside the harness's server: the job
/// is in a group of its own and holds the result, the server is not counted, and
/// when the job has ended the turn's end is the result.
#[cfg(unix)]
#[test]
fn a_job_the_agent_leaves_holds_the_result_beside_the_harnesss_server_which_is_not_counted_or_waited_for(
) -> Outcome {
    let rig = Rig::start(settling().var("CF_TEST_MCP_SERVER", "late"))?;
    let (project, job) = a_worker_with_a_job(&rig, "BACKGROUND 20 started the job")?;
    // The harness's servers have begun beside the job, and the daemon has looked since.
    rig.wait_for("the servers to run", secs(30), || {
        let servers = rig.jobs("server");
        Ok(servers.len() >= 2 && servers.iter().all(|server| is_alive(server.pid())))
    })?;
    thread::sleep(secs(2));
    until_the_turn_ended(&rig, &project)?;

    assert_eq!(project.task(1)?["state"], "working", "held for the job");
    let activity = project.lane("worker")?["activity"].clone();
    assert_eq!(activity["state"], "working");
    assert_held_for_one_process(&activity);
    assert!(is_alive(job.pid()), "the job runs on");
    assert!(
        rig.jobs("server")
            .iter()
            .all(|server| is_alive(server.pid())),
        "and so do the harness's servers, which the task does not wait for"
    );
    assert_eq!(results(&project)?, Vec::<Value>::new());

    rig.wait_for("T-1 to be done", secs(60), || {
        Ok(project.task(1)?["state"] == "done")
    })?;
    assert!(job.finished(), "the job ran to its end");
    rig.wait_for("the result to reach the chief", secs(30), || {
        Ok(!results(&project)?.is_empty())
    })?;
    assert_eq!(results(&project)?[0]["body"], "started the job");
    rig.close()?;
    Ok(())
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
    let rig = Rig::start(settling())?;
    let (project, job) = a_worker_with_a_job(&rig, "BACKGROUND 120 started the job")?;
    until_the_turn_ended(&rig, &project)?;
    assert_eq!(project.task(1)?["state"], "working");

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

/// The signals and the grace are Unix's: on Windows nothing is asked and a job is
/// ended at once, which the cancel case above holds.
#[cfg(unix)]
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

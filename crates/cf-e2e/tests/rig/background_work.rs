//! A worker's background work outlives its turn, and its answer is its result at
//! once: the owner's decision of 2026-10-13, which reverses the one of 2026-10-09
//! (T-183's worker started a recording run under a lock tool in the background,
//! armed its harness's monitor and ended its turn; ConsensFlow took the turn's end for
//! the result and closed the window, and the job died with it, killed outright, with
//! no RELEASED) after T-7 and T-15 on Windows (a headless Edge check the harness
//! moved to the background, which never exits, kept a finished worker's task
//! working, and the chief had no result). End to end through the real pane host
//! (`npm run test:integration`): fake agents in real PTYs, and a job of the fake
//! agent's (its `background` part) that writes RELEASED when it is asked to end and
//! FINISHED when it ends by itself.
//!
//! - A turn that ends while the job it started runs is the task's result at once;
//!   the job is ended as the result is taken, the chief is told so in a note beside
//!   the result, and the window closes as any finished worker's does. A window the
//!   human keeps open stays open, with its job ended and its harness untouched.
//! - Work that was there before the turn (a server the harness started with its
//!   window) is not the turn's.
//! - Nor is a server the harness starts after the first message was marked, which is
//!   where Claude Code starts its MCP servers (found on 3.0.1): it stays in the group
//!   of the window's program, and a command the agent's tool runs is in a group of
//!   its own. The stand-in starts its server so (`CF_TEST_MCP_SERVER=late`), well
//!   past the pass that marks the turn, and a Claude worker's turn still ends with
//!   its result and nothing is said of work; with a job of the agent's beside the
//!   server, the job is ended and the server is not. Nor is what the server starts,
//!   in whatever group: the browser of `@playwright/mcp` and `chrome-devtools-mcp`,
//!   which the stand-in's server launches in a session of its own
//!   (`CF_TEST_MCP_BROWSER`).
//! - A window that closes with a job running (a cancel, mid-turn) asks it to end
//!   first, and whoever gave the task is told the job was running. A pause closes
//!   nothing.
//! - A job that ignores the request is killed once the grace is over, and the
//!   result did not wait for it.
//!
//! The asking and the grace are Unix's. On Windows a window is ended at once and
//! nothing is asked of its programs (`pty::close` of the pane host says why): its
//! job is gone with it, having written nothing, and what a turn left is ended by
//! itself, at once, where a shell the harness started after the mark started it:
//! the stand-in starts the commands of its agent in a PowerShell there, as the
//! harnesses do.

use std::sync::OnceLock;
use std::thread;
use std::time::Instant;

use cf_e2e::pattern;
#[cfg(unix)]
use cf_e2e::process::group_of;
use cf_e2e::process::is_alive;
use cf_e2e::rig::{Job, Project, Rig};
use cf_e2e::Error;
use regex::Regex;
use serde_json::{json, Value};

use crate::{config, secs, Outcome};

/// The tier the worker was given, as a word the chief's brief names.
fn tier(added: &Value) -> &str {
    added["member"]["tier"].as_str().unwrap_or_default()
}

/// A project with one worker, given `words` through the chief.
fn a_worker_given<'a>(rig: &'a Rig, words: &str) -> cf_e2e::Result<Project<'a>> {
    let project = Project::open(rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    rig.tell(
        project.id(),
        &format!("DISPATCH --tier {} {words}", tier(&added)),
    )?;
    Ok(project)
}

/// Waits until the worker's job runs: the job.
fn until_the_job_runs(rig: &Rig) -> cf_e2e::Result<Job> {
    rig.wait_for("the worker's job to run", secs(60), || {
        Ok(rig.jobs("job").len() == 1)
    })?;
    rig.jobs("job")
        .into_iter()
        .next()
        .ok_or_else(|| Error::Daemon("the worker's job is gone".to_owned()))
}

/// The human opens the worker's window as soon as it is there: it stays open once
/// its task is done, as a window they hold open does.
fn keep_the_workers_window_open(rig: &Rig, project: &Project) -> Outcome {
    rig.wait_for("the worker's window to open", secs(60), || {
        Ok(project.lane("worker")?["pane"].is_object())
    })?;
    let session = project.lane("worker")?["participant"]["handle"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    let opened = rig.page(
        "session.open",
        json!({ "project": project.id(), "handle": session }),
    )?;
    assert_eq!(opened["ok"], true, "{opened}");
    Ok(())
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

/// What the chief is told of the job its worker's turn left, once it is ended.
fn ended_note() -> &'static Regex {
    static ENDED: OnceLock<Regex> = OnceLock::new();
    pattern::once(
        &ENDED,
        r"^T-1's turn left background work running\. Ended: .*--background-job .*\.$",
    )
}

/// Whatever the chief was told of work: of an ending or of a close.
fn any_note_of_work() -> &'static Regex {
    static WORK: OnceLock<Regex> = OnceLock::new();
    pattern::once(&WORK, r"background work")
}

#[test]
fn a_workers_answer_is_its_result_at_once_the_job_it_left_is_ended_and_the_chief_is_told() -> Outcome
{
    let rig = Rig::start(config())?;
    let project = a_worker_given(&rig, "BACKGROUND 600 started the job")?;
    let job = until_the_job_runs(&rig)?;

    // Six hundred seconds of job: the task does not wait for them.
    rig.wait_for("T-1 to be done", secs(60), || {
        Ok(project.task(1)?["state"] == "done")
    })?;
    rig.wait_for("the job to be ended", secs(30), || Ok(!is_alive(job.pid())))?;
    assert!(!job.finished(), "it did not run to its end");
    #[cfg(unix)]
    assert!(job.released(), "it was asked to end, not killed");
    rig.wait_for("the result to reach the chief", secs(30), || {
        Ok(!results(&project)?.is_empty())
    })?;
    assert_eq!(results(&project)?[0]["body"], "started the job");
    rig.wait_for("the chief to be told what was ended", secs(30), || {
        Ok(!notes(&project, ended_note())?.is_empty())
    })?;
    // The window's close found nothing left to say: the ending was first.
    thread::sleep(secs(2));
    assert_eq!(
        notes(&project, any_note_of_work())?.len(),
        1,
        "{:?}",
        notes(&project, any_note_of_work())?
    );
    rig.close()?;
    Ok(())
}

#[test]
fn a_window_the_human_keeps_open_stays_open_with_its_job_ended_and_its_program_untouched() -> Outcome
{
    let rig = Rig::start(config())?;
    let project = a_worker_given(&rig, "BACKGROUND 600 started the job")?;
    keep_the_workers_window_open(&rig, &project)?;
    let job = until_the_job_runs(&rig)?;

    rig.wait_for("T-1 to be done", secs(60), || {
        Ok(project.task(1)?["state"] == "done")
    })?;
    rig.wait_for("the job to be ended", secs(30), || Ok(!is_alive(job.pid())))?;
    rig.wait_for("the chief to be told what was ended", secs(30), || {
        Ok(!notes(&project, ended_note())?.is_empty())
    })?;

    // It is theirs: the window is open, and what a window's close would say is not said.
    thread::sleep(secs(3));
    assert!(
        project.lane("worker")?["pane"].is_object(),
        "the window is open"
    );
    assert_eq!(project.task(1)?["state"], "done");
    rig.close()?;
    Ok(())
}

#[test]
fn work_the_window_had_before_its_turn_is_not_the_turns_a_server_the_harness_started_with_it_holds_nothing(
) -> Outcome {
    // In a group of its own, as Codex starts its helpers: no group tells it from the
    // turn's work, only that the window had it before its turn.
    let rig = Rig::start(config().var("CF_TEST_MCP_SERVER", "apart"))?;
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
    // The human keeps the window open, so that its close does not end the server
    // before this looks at it: only an ending could.
    keep_the_workers_window_open(&rig, &project)?;
    rig.wait_for("T-1 to be done", secs(60), || {
        Ok(project.task(1)?["state"] == "done")
    })?;
    rig.wait_for("the result to reach the chief", secs(30), || {
        Ok(!results(&project)?.is_empty())
    })?;
    assert_eq!(results(&project)?[0]["body"], "WORKER_OK");
    // What an ending would end and say is done within moments of the result.
    thread::sleep(secs(3));
    let servers = rig.jobs("server");
    assert!(
        servers.len() >= 2 && servers.iter().all(|server| is_alive(server.pid())),
        "the chief and the worker each had a server, and have it still"
    );
    assert_eq!(notes(&project, any_note_of_work())?, Vec::<Value>::new());
    rig.close()?;
    Ok(())
}

/// What these cases cannot hold on Windows, where a job object has no process
/// groups for the host to tell by: a server the harness starts after the first
/// message was marked is told from a job there only by the shell that started the
/// job, so it is left running and said so (written as a limit in the engine's
/// `background` module). So they are Unix's: where the host looks at the tree at
/// all, which is macOS.
///
/// The server begins `THINKING` after the window's first message shows in its
/// record, past the pass that marks where the turn began, and the turn outlasts it.
#[cfg(unix)]
#[test]
fn a_server_the_harness_starts_after_the_first_message_was_marked_is_no_work_of_the_turn_and_nothing_is_said(
) -> Outcome {
    // Found on 3.0.1: Claude Code starts its MCP servers as it takes its first
    // message, and the user's own held a worker's task for ever. Ending what a turn
    // left must not end them.
    let rig = Rig::start(config().var("CF_TEST_MCP_SERVER", "late"))?;
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
    thread::sleep(secs(2));
    assert_eq!(notes(&project, any_note_of_work())?, Vec::<Value>::new());
    rig.close()?;
    Ok(())
}

/// The owner's own setup: `@playwright/mcp` and `chrome-devtools-mcp` launch a
/// browser when a tool is used, in a group of its own. The browser is a child of
/// the server, and what a harness's server starts is the harness's whatever group it
/// goes to: a task that touched a page is done with the browser running, and the
/// browser is not ended as the turn's.
#[cfg(unix)]
#[test]
fn a_browser_the_server_launches_in_a_group_of_its_own_is_no_work_of_the_turn_either() -> Outcome {
    let rig = Rig::start(
        config()
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
    rig.wait_for("T-1 to be done", secs(60), || {
        Ok(project.task(1)?["state"] == "done")
    })?;
    rig.wait_for("the result to reach the chief", secs(30), || {
        Ok(!results(&project)?.is_empty())
    })?;
    assert_eq!(results(&project)?[0]["body"], "WORKER_OK");
    thread::sleep(secs(2));
    assert_eq!(notes(&project, any_note_of_work())?, Vec::<Value>::new());
    rig.close()?;
    Ok(())
}

/// The same worker, whose agent leaves a job beside the harness's server, in a window
/// the human keeps open: the job is ended, and the server, which the host does not
/// list as the turn's, is not asked to end and runs on.
#[cfg(unix)]
#[test]
fn a_job_the_agent_leaves_is_ended_beside_the_harnesss_server_which_runs_on() -> Outcome {
    let rig = Rig::start(config().var("CF_TEST_MCP_SERVER", "late"))?;
    let project = a_worker_given(&rig, "BACKGROUND 600 started the job")?;
    keep_the_workers_window_open(&rig, &project)?;
    let job = until_the_job_runs(&rig)?;
    // The harness's servers have begun beside the job.
    rig.wait_for("the servers to run", secs(30), || {
        let servers = rig.jobs("server");
        Ok(servers.len() >= 2 && servers.iter().all(|server| is_alive(server.pid())))
    })?;

    rig.wait_for("T-1 to be done", secs(60), || {
        Ok(project.task(1)?["state"] == "done")
    })?;
    rig.wait_for("the job to be ended", secs(30), || Ok(!is_alive(job.pid())))?;
    rig.wait_for("the chief to be told what was ended", secs(30), || {
        Ok(!notes(&project, ended_note())?.is_empty())
    })?;
    assert!(job.released(), "it was asked to end, not killed");
    assert!(
        rig.jobs("server")
            .iter()
            .all(|server| is_alive(server.pid())),
        "the harness's servers are left as they were"
    );
    rig.close()?;
    Ok(())
}

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

/// The signals and the grace are Unix's: on Windows nothing is asked and a job is
/// ended at once, which the cancel case above holds.
#[cfg(unix)]
#[test]
fn a_job_that_ignores_the_signals_that_ask_it_to_end_is_killed_once_the_grace_of_ten_seconds_is_over_and_the_result_did_not_wait_for_it(
) -> Outcome {
    let rig = Rig::start(config())?;
    let project = a_worker_given(&rig, "BACKGROUND --deaf 600 started the job")?;
    let job = until_the_job_runs(&rig)?;

    rig.wait_for("T-1 to be done", secs(60), || {
        Ok(project.task(1)?["state"] == "done")
    })?;
    let done = Instant::now();
    assert!(
        is_alive(job.pid()),
        "the task is done while the job is still being given its grace"
    );

    rig.wait_for("the job to be killed", secs(30), || {
        Ok(!is_alive(job.pid()))
    })?;
    assert!(
        done.elapsed() >= secs(8),
        "the job was given its grace: {:?}",
        done.elapsed()
    );
    assert!(!job.released(), "and was killed, not asked");
    rig.wait_for("the chief to be told what was ended", secs(30), || {
        Ok(!notes(&project, ended_note())?.is_empty())
    })?;
    rig.close()?;
    Ok(())
}

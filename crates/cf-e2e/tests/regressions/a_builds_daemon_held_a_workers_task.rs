//! A build's daemon held a finished worker's task.
//!
//! Seen on another Mac on 2026-10-15 (batch A of that day's field reports, A1: a project
//! with a Gradle build), on 3.0.3: T-4, T-6 and T-10 stayed `working` after their workers
//! had finished. The work was in the repository, no result reached the chief, and the
//! human cancelled the tasks. No logs were kept. The reading the owner took
//! (`issues/field-reports-2026-10-15.md` in consensflow-sme): each worker's build left
//! Gradle's daemon running, a process a build starts and leaves when it ends, and 3.0.3
//! held a worker's result while anything its turn had started still ran. 3.0.4 takes a
//! worker's answer for its result at once, whatever its turn left
//! (`decisions/2026-10-13-a-workers-answer-ends-its-turns-work.md`), which is what fixed
//! it; the same report's notices of "background java still running" (A5) were the same
//! daemons.
//!
//! What the owner should have seen: the worker's answer is its task's result within
//! moments, and the chief is told nothing of background work. The daemon is no work of
//! the turn: the build that started it was a call the worker's tool waited for, and it
//! ended. So the ending of the turn's work neither waits for it nor ends it: a build's
//! daemon is there for the builds after it.
//!
//! The stand-in Claude (`fake-agent`) runs such a build (`BUILD <seconds> <words>`): its
//! shell tool runs the build in the foreground; the build starts its daemon, a child that
//! outlives it (in a session of its own on Unix, as Gradle's daemon makes one, and with
//! the window's environment), and ends once the daemon is up; the shell ends with it, and
//! the agent answers.
//!
//! It guards the rule that a Claude Code job run in the background holds its task (the
//! owner's amendment of 2026-10-15 to the decision above): what holds a task there is the
//! harness's word that it runs a job in the background (the tool's result that says so,
//! and its notification when the job ends), never a process that outlived a call the
//! harness waited for. Were the daemon taken for such a job, T-1 would stay working until
//! the daemon ended (ten minutes here; three idle hours for Gradle's), which the first
//! wait of each case below says, and the chief would be told of work left.

use std::thread;

use cf_e2e::process::{is_alive, signal, started, Signal};
use cf_e2e::rig::{Job, Project, Rig};
use cf_e2e::Error;
use serde_json::{json, Value};

use crate::{config, secs, Outcome};

/// How long the daemon runs: past any case, which ends it.
const DAEMON_SECONDS: u64 = 600;

/// The tier the worker was given, as a word the chief's brief names.
fn tier(added: &Value) -> &str {
    added["member"]["tier"].as_str().unwrap_or_default()
}

/// A project with one worker, given T-1 through the chief: a build that leaves its
/// daemon, then the answer.
fn a_worker_given_a_build(rig: &Rig) -> cf_e2e::Result<Project<'_>> {
    let project = Project::open(rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    rig.tell(
        project.id(),
        &format!(
            "DISPATCH --tier {} BUILD {DAEMON_SECONDS} BUILD_OK",
            tier(&added)
        ),
    )?;
    Ok(project)
}

/// Waits until the build has ended and its daemon runs: the daemon.
fn until_the_build_left_its_daemon(rig: &Rig) -> cf_e2e::Result<Job> {
    rig.wait_for("the build to leave its daemon running", secs(60), || {
        let ended = rig
            .jobs("build")
            .first()
            .is_some_and(|build| build.finished() && !is_alive(build.pid()));
        Ok(ended && rig.jobs("daemon").len() == 1)
    })?;
    rig.jobs("daemon")
        .into_iter()
        .next()
        .ok_or_else(|| Error::Daemon("the build's daemon is gone".to_owned()))
}

/// The results the chief has been given, oldest first.
fn results(project: &Project) -> cf_e2e::Result<Vec<Value>> {
    Ok(project
        .inbox("chief")?
        .into_iter()
        .filter(|message| message["kind"] == "result")
        .collect())
}

/// What the chief was told of background work: of an ending, or of a close.
fn told_of_work(project: &Project) -> cf_e2e::Result<Vec<String>> {
    Ok(project
        .inbox("chief")?
        .into_iter()
        .filter_map(|message| message["body"].as_str().map(str::to_owned))
        .filter(|body| body.contains("background work"))
        .collect())
}

/// The worker's answer is T-1's result within moments, its daemon running or not.
fn the_answer_is_the_result(rig: &Rig, project: &Project) -> Outcome {
    rig.wait_for("T-1 to be done", secs(60), || {
        Ok(project.task(1)?["state"] == "done")
    })?;
    rig.wait_for("the result to reach the chief", secs(30), || {
        Ok(!results(project)?.is_empty())
    })?;
    assert_eq!(results(project)?[0]["body"], "BUILD_OK");
    Ok(())
}

/// The window is the human's, open, so that its close does not reach the daemon before
/// this looks at what the ending did with it.
#[test]
fn a_workers_build_that_leaves_its_daemon_running_has_its_task_done_and_the_daemon_is_neither_waited_for_nor_ended(
) -> Outcome {
    let rig = Rig::start(config())?;
    let project = a_worker_given_a_build(&rig)?;
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
    let daemon = until_the_build_left_its_daemon(&rig)?;
    let began = started(daemon.pid());
    assert!(began.is_some(), "the daemon is there");

    the_answer_is_the_result(&rig, &project)?;
    assert!(is_alive(daemon.pid()), "the daemon runs");
    // What an ending would end and say is done within moments of the result.
    thread::sleep(secs(3));
    assert!(
        is_alive(daemon.pid()) && started(daemon.pid()) == began,
        "the daemon runs on"
    );
    assert!(
        !daemon.released() && !daemon.finished(),
        "and was never asked to end"
    );
    assert_eq!(told_of_work(&project)?, Vec::<String>::new());
    assert_eq!(
        project.lane("worker")?["activity"]["reason"],
        Value::Null,
        "the board says no work is running"
    );
    assert!(
        project.lane("worker")?["pane"].is_object(),
        "the window is open"
    );

    // It is nobody's to end now: end it so that it does not outlive the case.
    signal(daemon.pid(), Signal::Kill)?;
    rig.wait_for("the daemon to be ended", secs(30), || {
        Ok(!is_alive(daemon.pid()))
    })?;
    rig.close()?;
    Ok(())
}

/// The same worker's window, closed as a finished worker's is once its result is taken:
/// nothing is said of the daemon at the close either. What the close does with it is the
/// close's rule for a process no chain proves the turn's (`cf-panes`' `background`): on
/// the Mac it signals what is the window's by the process that started it, by the session
/// the window's program leads, or by a look that saw it so, and a daemon in a session of
/// its own whose build has ended is none of those, so it runs on; on Windows the pane's
/// job object holds whatever a window's process starts, and the close ends it with the
/// window, whatever build of another window may be using it then.
///
/// The order the product keeps: the worker's answer is taken for T-1's result, and only
/// then is its window closed, the turn's ending having left the daemon alone. On Windows
/// that close comes at once and ends the daemon with the window, so the daemon may be
/// gone by the time T-1 is seen done (zeewin, 2026-10-16: the case asserted it ran then,
/// and lost to the close); what says there that the ending never touched it is that it
/// was never asked to end, and nothing was told of it. On the Mac it runs at T-1's end
/// and past the close.
#[test]
fn a_workers_build_daemon_is_not_told_as_work_left_when_the_finished_workers_window_closes(
) -> Outcome {
    let rig = Rig::start(config())?;
    let project = a_worker_given_a_build(&rig)?;
    let daemon = until_the_build_left_its_daemon(&rig)?;
    the_answer_is_the_result(&rig, &project)?;
    if !cfg!(windows) {
        assert!(is_alive(daemon.pid()), "the daemon runs");
    }
    rig.wait_for("the worker's window to close", secs(60), || {
        Ok(project.lane("worker")?["pane"].is_null())
    })?;
    // What the daemon says of the end and of the close is said by now.
    thread::sleep(secs(3));
    assert_eq!(told_of_work(&project)?, Vec::<String>::new());
    assert!(!daemon.released(), "it was never asked to end");
    if cfg!(windows) {
        rig.wait_for("the close to end the daemon", secs(30), || {
            Ok(!is_alive(daemon.pid()))
        })?;
    } else {
        assert!(is_alive(daemon.pid()), "the daemon runs on past the close");
        signal(daemon.pid(), Signal::Kill)?;
        rig.wait_for("the daemon to be ended", secs(30), || {
            Ok(!is_alive(daemon.pid()))
        })?;
    }
    rig.close()?;
    Ok(())
}

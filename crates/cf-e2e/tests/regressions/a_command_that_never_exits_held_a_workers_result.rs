//! A command that never exits held a finished worker's task.
//!
//! Seen by the owner on 2026-10-12 and 2026-10-13, on 3.0.x, on their Windows
//! machine: T-7 (a Devin worker) and T-15 (a Claude Code worker) were the same
//! case. T-15's worker finished: its transcript ends with its final answer, and its
//! output file was written. But T-15 stayed `working`, the chief was given no result
//! and `cf task accept` was refused. Just before the answer the worker's transcript
//! holds `Command running in background with ID: a34b16`: a headless Edge check
//! (`msedge --headless --dump-dom …`) that its harness moved to the background after
//! the agent's wait of 22 s, and that never exits. The chief could not see why: only
//! the worker's board line said "background work running". The pane host listed the
//! process as work the turn started, and the daemon held the task's result back for
//! it, for as long as the window lived. The report is the decision
//! `decisions/2026-10-13-a-workers-answer-ends-its-turns-work.md` and the issue
//! `issues/windows-devin-worker-held-for-background-work.md` in consensflow-sme.
//!
//! What the owner should have seen (their decision of 2026-10-13: "we have to end
//! everything and give the answer to the chief"): the worker's turn ends, the task
//! is done, the chief is given the result and a line that says what was ended, and
//! can accept it; the process is gone, and the board says nothing is running.

use cf_e2e::process::is_alive;
use cf_e2e::rig::{Project, Rig, OPEN};
use cf_e2e::Error;
use serde_json::{json, Value};

use crate::{config, secs, Outcome};

/// The tier the worker was given, as a word the chief's brief names.
fn tier(added: &Value) -> &str {
    added["member"]["tier"].as_str().unwrap_or_default()
}

/// The results the chief has been given or is to be given, oldest first.
fn results(project: &Project) -> cf_e2e::Result<Vec<Value>> {
    let mut results: Vec<Value> = project
        .inbox("chief")?
        .into_iter()
        .filter(|message| message["kind"] == "result")
        .collect();
    results.sort_by_key(|message| message["id"].as_i64().unwrap_or_default());
    Ok(results)
}

/// The chief accepts the task `task` from its window, as it does with `cf task
/// accept`.
fn accepts(rig: &Rig, project: &Project, task: &str) -> cf_e2e::Result<()> {
    let chief = rig.open_frame(&format!("p{}-chief", project.id()), OPEN)?;
    let ran = rig.cf_in_window(&chief, ["task", "accept", task])?;
    if ran.code == Some(0) {
        Ok(())
    } else {
        Err(Error::Daemon(format!("cf task accept {task}: {ran}")))
    }
}

/// A worker whose turn starts a command that never exits (ten hours of it), and ends
/// with its final answer: T-15's shape. The stand-in starts it as a harness's tool
/// does, in a group of its own on Unix and behind a PowerShell on Windows, after the
/// turn was marked.
#[test]
fn a_worker_that_leaves_a_command_that_never_exits_running_gives_the_chief_its_result_and_the_chief_accepts_it(
) -> Outcome {
    let rig = Rig::start(config())?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    rig.tell(
        project.id(),
        &format!(
            "DISPATCH --tier {} BACKGROUND 36000 WORKER_OK",
            tier(&added)
        ),
    )?;
    rig.wait_for("the worker's command to run", secs(60), || {
        Ok(rig.jobs("job").len() == 1)
    })?;
    let command = rig
        .jobs("job")
        .into_iter()
        .next()
        .ok_or_else(|| Error::Daemon("the worker's command is gone".to_owned()))?;

    // The worker answered with the command still running: the task is done, within
    // moments, not when the command ends (which it never does).
    rig.wait_for("T-1 to be done", secs(60), || {
        Ok(project.task(1)?["state"] == "done")
    })?;
    rig.wait_for("the result to reach the chief", secs(30), || {
        Ok(!results(&project)?.is_empty())
    })?;
    assert_eq!(results(&project)?[0]["body"], "WORKER_OK");

    // The chief is told what was ended, and the board does not say work runs.
    rig.wait_for("the command to be ended", secs(30), || {
        Ok(!is_alive(command.pid()))
    })?;
    rig.wait_for("the chief to be told what was ended", secs(30), || {
        Ok(project.inbox("chief")?.iter().any(|message| {
            message["kind"] == "note"
                && message["body"].as_str().is_some_and(|body| {
                    body.starts_with("T-1's turn left background work running. Ended: ")
                        && body.contains("--background-job")
                })
        }))
    })?;
    assert_eq!(
        project.lane("worker")?["activity"]["reason"],
        Value::Null,
        "the board says no work is running"
    );

    // And the chief can decide on it.
    accepts(&rig, &project, "T-1")?;
    assert_eq!(project.task(1)?["state"], "accepted");
    rig.close()?;
    Ok(())
}

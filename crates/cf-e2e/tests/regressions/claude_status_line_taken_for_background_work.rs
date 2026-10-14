//! Claude Code's own status line was told as background work its window left.
//!
//! Seen by the owner on 2026-10-14, on 3.0.3, on their Mac. When T-42's window closed,
//! ConsensFlow told the chief that background work was still running: two runs of `bash
//! ~/.claude/statusline-command.sh` and one `git -C …/dell-ai-system --no-optional-locks
//! branch --show-current`. None of it was the agent's. It is Claude Code's status line:
//! the user's script, which runs that `git` to show the branch. When the chief looked a
//! minute later, none of it was running. The report is the issue
//! `issues/claude-status-line-taken-for-background-work.md` in consensflow-sme.
//!
//! The pane host took for the turn's work what began after the daemon marked where the
//! turn began, but for what is in the window's group (the harness's servers, which stay in
//! it). Claude Code runs its status line in a group of its own, as it runs the commands its
//! agent asks for, so a look at the window's tree that came while a run lived found a shell
//! the harness had started after the mark and a `git` under it, and told them. A status
//! line is run every few seconds, so one is at work at almost any moment, and the look is
//! made at the close of a window, which tells the chief and the human what it found, and at
//! the end of a turn, where what is found is ended and told too.
//!
//! What the owner should have seen: nothing of it. A harness's own processes are never told
//! as work the window left: those in the window's group never are, and what the host finds
//! when it looks is told only if it is still there a moment later, which a script of a
//! second is not.
//!
//! The stand-in Claude (`fake-agent`) runs a status line as the harness does, for as long as
//! it lives (`CF_TEST_STATUS_LINE`): the user's script, in a group of its own, that runs the
//! `git` and ends a second later, a new run beginning before the last has ended. Unix's: the
//! script is a `bash`.

use std::thread;

use cf_e2e::rig::{Project, Rig};
use serde_json::{json, Value};

use crate::{config, secs, Outcome};

/// The tier the worker was given, as a word the chief's brief names.
fn tier(added: &Value) -> &str {
    added["member"]["tier"].as_str().unwrap_or_default()
}

/// A project with a worker, given T-1 through the chief: a turn of `seconds` seconds, which
/// ends with its answer.
fn a_worker_given_a_turn<'a>(rig: &'a Rig, seconds: u64) -> cf_e2e::Result<Project<'a>> {
    let project = Project::open(rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    rig.tell(
        project.id(),
        &format!(
            "DISPATCH --tier {} SLEEP {seconds} Reply with exactly: WORKER_OK",
            tier(&added)
        ),
    )?;
    Ok(project)
}

/// The status line runs while the turn is at work, as the owner's did: more than one run
/// has begun, and one is at work.
fn until_the_status_line_runs(rig: &Rig) -> Outcome {
    rig.wait_for("the status line to run", secs(60), || {
        Ok(rig.jobs("statusline").len() >= 2)
    })?;
    Ok(())
}

/// What the chief has been told that says work was left, or names the status line: the
/// notes of its inbox that do.
fn told_of_work(project: &Project) -> cf_e2e::Result<Vec<String>> {
    Ok(project
        .inbox("chief")?
        .into_iter()
        .filter_map(|message| message["body"].as_str().map(str::to_owned))
        .filter(|body| body.contains("background work") || body.contains("statusline"))
        .collect())
}

#[test]
fn a_claude_workers_status_line_in_a_group_of_its_own_is_no_work_its_turn_left_at_its_end_or_at_its_windows_close(
) -> Outcome {
    let rig = Rig::start(config().var("CF_TEST_STATUS_LINE", "own"))?;
    let project = a_worker_given_a_turn(&rig, 4)?;
    until_the_status_line_runs(&rig)?;

    // The turn ends with its answer, and the window closes as a finished worker's does.
    rig.wait_for("T-1 to be done", secs(60), || {
        Ok(project.task(1)?["state"] == "done")
    })?;
    rig.wait_for("the worker's window to close", secs(60), || {
        Ok(project.lane("worker")?["pane"].is_null())
    })?;
    // What the daemon says of the end and of the close is said by now.
    thread::sleep(secs(3));
    assert_eq!(told_of_work(&project)?, Vec::<String>::new());
    // The status line ran on all the while: the case holds nothing without it.
    assert!(
        rig.jobs("statusline").len() >= 4,
        "{:?}",
        rig.jobs("statusline")
    );
    rig.close()?;
    Ok(())
}

/// The same status line in the window's group, as a helper of the harness is, and for
/// longer than the host waits before it looks again: it is the harness's wherever and for
/// however long it runs.
#[test]
fn a_claude_workers_helper_in_its_windows_group_is_never_told_however_long_it_runs() -> Outcome {
    let rig = Rig::start(config().var("CF_TEST_STATUS_LINE", "group"))?;
    let project = a_worker_given_a_turn(&rig, 4)?;
    until_the_status_line_runs(&rig)?;

    rig.wait_for("T-1 to be done", secs(60), || {
        Ok(project.task(1)?["state"] == "done")
    })?;
    rig.wait_for("the worker's window to close", secs(60), || {
        Ok(project.lane("worker")?["pane"].is_null())
    })?;
    thread::sleep(secs(3));
    assert_eq!(told_of_work(&project)?, Vec::<String>::new());
    rig.close()?;
    Ok(())
}

/// A window that closes while its task still works (the window of a worker the human
/// closed, or one that crashed) pauses the task with the reason, which names what the
/// window left running: not the status line.
#[test]
fn a_claude_workers_window_closed_while_its_task_works_says_no_work_was_left_for_the_status_line(
) -> Outcome {
    let rig = Rig::start(config().var("CF_TEST_STATUS_LINE", "own"))?;
    let project = a_worker_given_a_turn(&rig, 120)?;
    until_the_status_line_runs(&rig)?;
    rig.wait_for("the worker's window to be at work", secs(60), || {
        Ok(project.lane("worker")?["activity"]["state"] == "working")
    })?;
    // Let the turn go on a few seconds, so that what the window starts is the turn's.
    thread::sleep(secs(6));
    let pane = project.lane("worker")?["pane"].clone();
    let killed = rig.host(
        "pane.kill",
        json!({ "id": pane["id"], "generation": pane["generation"] }),
    )?;
    assert_eq!(killed["ok"], true, "{killed}");

    // The chief is told the window closed, and what with: no work.
    rig.wait_for("the chief to be told the window closed", secs(60), || {
        Ok(project.inbox("chief")?.iter().any(|message| {
            message["body"]
                .as_str()
                .is_some_and(|body| body.contains("window closed"))
        }))
    })?;
    assert_eq!(told_of_work(&project)?, Vec::<String>::new());
    rig.close()?;
    Ok(())
}

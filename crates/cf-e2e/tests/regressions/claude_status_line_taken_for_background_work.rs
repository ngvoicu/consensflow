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
//! when it looks is told only if it is still there a moment later (`background::SETTLE`, 2
//! s), which a script of a second is not.
//!
//! That is a rule about a moment, and a machine can stretch a script's moment: at a load of 83
//! (T-1328's suite) four tenths of a second of sleeps, and the processes started for them, took
//! more than the host's two seconds, and the close named five runs of the status line as work
//! the window left. The host did what it does of any process that lasts. So the cases of the
//! status line in a group of its own do not say "the note names nothing"; they say what the
//! rule says: **the note names no run that lived less than the host's second look**. Each run
//! of the stand-in says how long it lived (`Job::lived`), and a run the note names that lived
//! two seconds or more is the rule at work on a loaded machine, and no status line taken for
//! work. (Whether a real status line that a loaded machine stretches past two seconds should
//! be named at a close is the product's question, T-1343's report; the stand-in's cannot
//! make the machine less loaded.) The helper in the window's group stays strict: it is never
//! told, however long it runs.
//!
//! The stand-in Claude (`fake-agent`) runs a status line as the harness does, for as long as
//! it lives (`CF_TEST_STATUS_LINE`): the user's script, in a group of its own, that runs the
//! `git` and ends a second later, a new run beginning before the last has ended. Unix's: the
//! script is a `bash`.

use std::collections::HashSet;
use std::thread;
use std::time::Duration;

use cf_e2e::rig::{Project, Rig};
use regex::Regex;
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

/// How long after its first look the host looks again, and names only what both looks find
/// (`background::SETTLE`): a process it names has lived at least so long.
const SETTLE: Duration = Duration::from_secs(2);

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

/// What the host named as work left that it should not have: nothing the notes of the chief's
/// inbox say of work or of the status line, except a run of the status line that lived as long
/// as the host's second look (see the head of this file), and, where such a run lived, the
/// processes a run starts. A run that was named and lived less is the rule broken, and so is a
/// process named that no run could have started.
fn named_wrongly(rig: &Rig, project: &Project) -> cf_e2e::Result<Vec<String>> {
    let told = told_of_work(project)?;
    if told.is_empty() {
        return Ok(Vec::new());
    }
    let pid =
        Regex::new(r"process (\d+)").map_err(|failed| cf_e2e::Error::Daemon(failed.to_string()))?;
    let named: HashSet<u32> = told
        .iter()
        .flat_map(|body| pid.captures_iter(body))
        .filter_map(|found| found[1].parse().ok())
        .collect();
    // The runs the note names were ended by the close that named them: each says how long it
    // lived once it is gone.
    rig.wait_for(
        "the runs the note names to say how long they lived",
        secs(60),
        || {
            Ok(rig
                .jobs("statusline")
                .iter()
                .filter(|run| named.contains(&run.pid()))
                .all(|run| run.lived().is_some()))
        },
    )?;
    let runs = rig.jobs("statusline");
    let outlived = runs
        .iter()
        .any(|run| run.lived().is_some_and(|lived| lived >= SETTLE));
    let mut wrong = Vec::new();
    for pid in &named {
        match runs.iter().find(|run| run.pid() == *pid) {
            Some(run) => {
                if let Some(lived) = run.lived().filter(|lived| *lived < SETTLE) {
                    wrong.push(format!(
                        "run {pid} of the status line lived {lived:?}, less than the host's second look"
                    ));
                }
            }
            None if !outlived => wrong.push(format!(
                "process {pid} is no run of the status line, and no run lived past the host's second look"
            )),
            None => {}
        }
    }
    if named.is_empty() && !outlived {
        wrong.extend(told);
    }
    wrong.sort();
    Ok(wrong)
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
    assert_eq!(named_wrongly(&rig, &project)?, Vec::<String>::new());
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
    assert_eq!(named_wrongly(&rig, &project)?, Vec::<String>::new());
    rig.close()?;
    Ok(())
}

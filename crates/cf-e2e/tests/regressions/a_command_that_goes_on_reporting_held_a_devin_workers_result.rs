//! A command that goes on reporting held a finished Devin worker's task.
//!
//! Seen on 2026-10-14 on zeewin (Windows), in the live check of what a worker's turn
//! leaves (`cargo xtask live finish devin`, 3.0.4's branch): the worker, a Devin, was
//! asked to start `ping -n 4711 127.0.0.1` in the background and answer, and it did
//! (`Command running in background with ID: …`, and its answer: the command is
//! running in the background). Its window drew its prompt at rest. But T-1 stayed
//! `working` for the 300 s the check waited, the board read the worker's window as
//! `working` all that time, the chief was given no result and no note, and the host
//! went on listing the command's processes: nobody had asked it to end them, for the
//! ending is asked for once the result is taken. The same check had passed on the
//! same machine earlier that day, with the same code on Windows: the task was done
//! after 6 s, and the command ended with it.
//!
//! The daemon reads that a Devin window is on a turn from its wire log, the words its
//! TUI and its agent say to each other: thoughts, words and the calls it begins since the
//! turn's last end are work. What a tool says of a call after it began was taken for work
//! too, but for its last word (the end of a shell that an Escape had stopped the turn of),
//! and a command that reports says so as long as it runs: a ping, a line a second. So the
//! window read as working for as long as the command ran, and the result, which is taken
//! when its turn has settled, was never taken, and the command, which is ended once it
//! is, ran on. The pass earlier that day fits it: the answer came before the command's
//! first line did. The failed run's wire log was not kept, so this is the closest shape
//! of what was seen (the window at work with its prompt at rest, the command listed,
//! the result never taken) that the stand-in allows; the live check now says how the wire
//! ends when a worker does not settle.
//!
//! What the owner should have seen (their decision of 2026-10-13: the worker's answer is
//! its task's result at once, whatever the turn left running): the task done within
//! moments, the chief given the result and a line that says what was ended, and the
//! command gone.
//!
//! The stand-in Devin (`fake-devin`) leaves such a command (`BACKGROUND <seconds>
//! <words>`: a shell that runs `sleep`, a child of the window's program, after the agent
//! has thought) and goes on saying a line of it on its wire log for as long as it runs,
//! once its turn has ended. Unix's: the stand-in's command is a shell of the machine's.

use cf_e2e::live::wire;
use cf_e2e::process::is_alive;
use cf_e2e::rig::{Project, Rig};
use cf_e2e::Error;
use serde_json::{json, Value};

use crate::devin_pause::write_roster;
use crate::{config, secs, Outcome, FAKE_DEVIN};

/// The tier the worker was given, as a word the chief's brief names.
fn tier(added: &Value) -> &str {
    added["member"]["tier"].as_str().unwrap_or_default()
}

/// The results the chief has been given, oldest first.
fn results(project: &Project) -> cf_e2e::Result<Vec<Value>> {
    Ok(project
        .inbox("chief")?
        .into_iter()
        .filter(|message| message["kind"] == "result")
        .collect())
}

#[test]
fn a_devin_worker_that_leaves_a_command_that_goes_on_reporting_has_its_task_done_and_the_command_ended(
) -> Outcome {
    let rig = Rig::start(config().devin(FAKE_DEVIN))?;
    write_roster(&rig)?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let added = project.add_member("devin-worker")?;
    rig.tell(
        project.id(),
        &format!(
            "DISPATCH --tier {} BACKGROUND 600 Reply with exactly: LEFT_IT",
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

    // The worker answered with the command running, and still saying what it prints: the
    // task is done within moments, not when the command ends (in ten minutes).
    rig.wait_for("T-1 to be done", secs(60), || {
        Ok(project.task(1)?["state"] == "done")
    })?;
    // The wire went on with the command's words once the turn had ended: what the case
    // is about. Without them it would hold nothing.
    let wire = wire::tail(&rig.home()).unwrap_or_default();
    assert!(
        wire.contains("cause:complete, tool_call_update"),
        "the command did not go on reporting after its turn ended: {wire}"
    );
    rig.wait_for("the result to reach the chief", secs(30), || {
        Ok(!results(&project)?.is_empty())
    })?;
    assert_eq!(results(&project)?[0]["body"], "LEFT_IT");

    // The turn's end ends nothing (turn-end's smallest rule, 2026-10-21): the worker has
    // nothing more to do, so its window closes at once, the close ends the command, and the
    // chief is told what the close ended.
    rig.wait_for("the command to be ended", secs(30), || {
        Ok(!is_alive(command.pid()))
    })?;
    rig.wait_for(
        "the chief to be told what the close ended",
        secs(30),
        || {
            Ok(project.inbox("chief")?.iter().any(|message| {
                message["kind"] == "note"
                    && message["body"].as_str().is_some_and(|body| {
                        body.starts_with(
                        "T-1's window closed with background work running, which its close ended: ",
                    ) && body.contains("sleep 600")
                    })
            }))
        },
    )?;
    rig.close()?;
    Ok(())
}

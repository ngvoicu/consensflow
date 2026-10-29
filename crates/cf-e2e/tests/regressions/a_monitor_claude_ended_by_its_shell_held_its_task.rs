//! A worker whose Monitor Claude Code ended was held until Finish on Windows.
//!
//! Seen on zeewin2 on 2026-10-28 and 2026-10-29, Claude Code 2.1.312 (T-858's live `tell`,
//! T-906): a turn armed a Monitor on `exec sleep 611`, whose time ran out, and Claude Code's
//! notice of its expiry came; the task stayed held, a paused one was never told its turn
//! was over, and Finish said "1 monitor that said it ended". Claude's adapter told the pane
//! host it did not know how Claude Code keeps its servers on Windows, so the host told none
//! of the window's commands and the engine, which takes no notice for a monitor's end blind
//! (a line of the command's could say it), held the task. And Claude Code ends a command on
//! Windows by its shell alone (its tree kill asks `ps` and signals the group, neither of
//! which native Windows has, and falls back to the shell's number): what the shell ran runs
//! on, with no parent.
//!
//! What the chief should see, on every platform: the task goes on once the monitor's
//! notice came and its shell is gone, its last answer the result, though what the shell ran
//! runs on; never a word that ConsensFlow cannot see the window's commands.
//!
//! The stand-in (`CLAUDE-MONITOR-ENDS` of the fake agent) ends the monitor as Claude Code
//! 2.1.312 does on Windows: the shell alone, then the notice, with no status.

use cf_e2e::process::{is_alive, signal, Signal};
use cf_e2e::rig::{Project, Rig};
use cf_e2e::Error;
use serde_json::json;

use crate::a_workers_still_going_was_given_as_its_result::{tier, told};
use crate::{config, secs, Outcome};

/// What the worker said as it armed the monitor and ended its turn.
const WATCHING: &str = "Watching the command with Monitor";

/// How long the monitor watches before its time runs out.
const EXPIRES: u64 = 6;

#[test]
fn a_monitor_whose_shell_claude_code_ended_lets_its_task_go_though_what_the_shell_ran_runs_on(
) -> Outcome {
    let rig = Rig::start(config())?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    rig.tell(
        project.id(),
        &format!(
            "DISPATCH --tier {} CLAUDE-MONITOR-ENDS {EXPIRES} {WATCHING}",
            tier(&added)
        ),
    )?;
    rig.wait_for("the watched command to run", secs(60), || {
        Ok(rig.jobs("watched").len() == 1)
    })?;
    let watched = rig
        .jobs("watched")
        .into_iter()
        .next()
        .ok_or_else(|| Error::Daemon("the watched command is gone".to_owned()))?;
    rig.wait_for("the chief to be told it goes on", secs(40), || {
        Ok(told(&project)?
            .iter()
            .any(|(kind, body)| kind == "note" && body.contains(WATCHING)))
    })?;
    rig.wait_for("T-1 to be done", secs(120), || {
        Ok(project.task(1)?["state"] == "done")
    })?;
    let thread = told(&project)?;
    let results: Vec<&str> = thread
        .iter()
        .filter(|(kind, _)| kind == "result")
        .map(|(_, body)| body.as_str())
        .collect();
    // What the turn the notice woke answered (the stand-in's `Saw <event>`).
    assert_eq!(
        results,
        [format!(
            "Saw [Monitor expired after {EXPIRES}s with no events delivered. Re-arm it if you still need the watch \u{2014} and widen the filter if silence was unexpected.]"
        )],
        "{thread:?}"
    );
    assert!(
        thread
            .iter()
            .all(|(_, body)| !body.contains("cannot see on this machine")),
        "the host told the window's commands: {thread:?}"
    );
    // What the ended shell ran was left running, as Claude Code leaves it. On Windows the
    // free window's close after the result ends its job object whole, that with it; on Unix
    // nothing ends it, and the case does.
    if cfg!(unix) {
        assert!(
            is_alive(watched.pid()),
            "what the ended shell ran runs on, as Claude Code leaves it"
        );
        signal(watched.pid(), Signal::Kill)?;
    }
    rig.close()?;
    Ok(())
}

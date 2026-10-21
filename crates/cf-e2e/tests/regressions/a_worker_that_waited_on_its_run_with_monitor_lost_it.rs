//! A worker that waited on its test run with Claude Code's Monitor lost the run.
//!
//! Seen on poker-lab on 2026-10-17, ConsensFlow 3.0.4, Claude Code 2.1.302 (T-442): the
//! worker started a long test run in the background, armed Monitor to wake it on the
//! run's events, and ended its turn. ConsensFlow took that turn's words for T-442's
//! result, ended the run 74 s in (`rc=143`, note m-2080) and closed the window: the chief
//! got a progress diary, and about twenty minutes were lost. The report is
//! `issues/field-reports-2026-10-18-monitor-wait-loses-the-job.md` in consensflow-sme.
//!
//! What the chief should have seen: the task stays working while the run and its monitor
//! run, each of the monitor's events wakes a turn that is no result, nothing is ended and
//! the window stays, and the turn the monitor's end wakes gives the result, once.
//!
//! The stand-in writes what Claude Code 2.1.305 writes (`CLAUDE-MONITOR` of the fake agent,
//! the shapes of `monitor-wait.jsonl`, a live run's): the job's start, the monitor's
//! (`taskId`, `timeoutMs`), its events (an `<event>` and no status), the job's end, and
//! the monitor's own end.

use cf_e2e::process::is_alive;
use cf_e2e::rig::{Project, Rig};
use cf_e2e::Error;
use serde_json::json;

use crate::a_workers_still_going_was_given_as_its_result::{tier, told};
use crate::{config, secs, Outcome};

/// What T-442's worker said as it armed the monitor and ended its turn.
const WATCHING: &str = "The suite runs in the background; Monitor wakes me on its events";

/// What it reported once the monitor's end woke it.
const REPORT: &str = "The suite passed: 412 tests, 0 failures";

/// How long the run takes, and how many lines the monitor sees in it.
const RUN_SECONDS: u64 = 16;
const EVENTS: u64 = 3;

#[test]
fn a_worker_that_armed_monitor_on_its_run_goes_on_until_the_monitor_ends_and_its_report_is_the_result(
) -> Outcome {
    let rig = Rig::start(config())?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    rig.tell(
        project.id(),
        &format!(
            "DISPATCH --tier {} CLAUDE-MONITOR {EVENTS} {RUN_SECONDS} {WATCHING} :: {REPORT}",
            tier(&added)
        ),
    )?;
    rig.wait_for("the worker's run to start", secs(60), || {
        Ok(rig.jobs("job").len() == 1)
    })?;
    let run = rig
        .jobs("job")
        .into_iter()
        .next()
        .ok_or_else(|| Error::Daemon("the worker's run is gone".to_owned()))?;
    rig.wait_for("the chief to be told it goes on", secs(40), || {
        Ok(told(&project)?
            .iter()
            .any(|(kind, body)| kind == "note" && body.contains(WATCHING)))
    })?;

    // The monitor's events wake turns, none of them a result; the run is not ended, and
    // the window stays with it.
    std::thread::sleep(secs(8));
    assert_eq!(project.task(1)?["state"], "working");
    assert!(
        told(&project)?.iter().all(|(kind, _)| kind != "result"),
        "{:?}",
        told(&project)?
    );
    assert!(is_alive(run.pid()), "the run runs");
    assert!(!run.released(), "and was never asked to end");
    assert!(
        !project.lane("worker")?["pane"].is_null(),
        "its window stays"
    );

    // The run ends by itself, the monitor after it, and the turn the monitor's end wakes is
    // the result.
    rig.wait_for("the run to end by itself", secs(60), || Ok(run.finished()))?;
    rig.wait_for("T-1 to be done", secs(60), || {
        Ok(project.task(1)?["state"] == "done")
    })?;
    let thread = told(&project)?;
    let kinds: Vec<&str> = thread.iter().map(|(kind, _)| kind.as_str()).collect();
    assert_eq!(kinds, ["note", "result"], "{thread:?}");
    assert!(
        thread[0].1.contains("2 background jobs"),
        "the run and its monitor: {}",
        thread[0].1
    );
    assert_eq!(thread[1].1, REPORT);
    assert!(!run.released(), "the run ran to its end, and none ended it");
    rig.close()?;
    Ok(())
}

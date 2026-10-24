//! A worker's "still going" was given as its result, and the job it was waiting for
//! was ended.
//!
//! Seen again on dip-agentos, 2026-10-17 to 10-19, on 3.0.4 (T-20: marked done while
//! it waited on a background build, its result only progress lines, nothing committed;
//! `issues/field-reports-2026-10-19-dip-agentos.md` #1 of consensflow-sme), and first by
//! the owner on 2026-10-15, on 3.0.4 (a chief's field reports), a Claude
//! Code worker, T-14. It started a 33-step mutation run with Claude Code's
//! `run_in_background` and wrote "The mutation run is still going in the background.
//! I'll write up the result once it finishes", and ended its turn. Claude Code was to
//! wake it when the run ended, in a turn of its own. ConsensFlow took the turn's end
//! for the result (m-131), ended the mutation runner mid-edit, which could have left a
//! source file with a security check removed, and told the chief what it had ended in
//! m-132, after the result, when the chief had already decided on it. The decision is
//! `decisions/2026-10-13-a-workers-answer-ends-its-turns-work.md` of consensflow-sme,
//! amended on 2026-10-15 for this report, and replaced by turn-end's smallest rule
//! (`decisions/2026-10-21-turn-end-ships-the-smallest-safe-rule.md`).
//!
//! What the owner should have seen: T-14 stays working while the job runs; the chief is
//! told the worker goes on in the background, with the words it said, and is given no
//! result; the job is not ended and runs to its end; the turn its end wakes is the
//! result, its final report and none of what the turn before said; and nothing is said
//! of anything ended, since nothing was.
//!
//! The stand-in does what Claude Code does (`CLAUDE-JOB` of the fake agent): the job is
//! a process of the window's tree that runs a few seconds, started after the turn was
//! marked, the record of its start says its id, and when it ends the stand-in writes
//! the notification, queued and taken as a turn of its own, whose answer is the report.

use cf_e2e::process::is_alive;
use cf_e2e::rig::{Project, Rig};
use cf_e2e::Error;
use serde_json::{json, Value};

use crate::{config, secs, Outcome};

/// What T-14's worker said as its turn ended, with its job running.
const GOING: &str = "The mutation run is still going in the background. I will write up the result once it finishes";

/// What it reported once the job's end woke it.
const REPORT: &str = "All 33 mutants are killed.";

/// How long the job runs, past the moment the chief is told: several passes of the
/// daemon, in which nothing may end the job or give the chief a result.
const JOB_SECONDS: u64 = 16;

/// The tier the worker was given, as a word the chief's brief names.
pub(crate) fn tier(added: &Value) -> &str {
    added["member"]["tier"].as_str().unwrap_or_default()
}

/// What T-1's thread gave the chief, oldest first: the kind and the words of each message.
pub(crate) fn told(project: &Project) -> cf_e2e::Result<Vec<(String, String)>> {
    let mut messages: Vec<Value> = project
        .inbox("chief")?
        .into_iter()
        .filter(|message| message["taskNumber"] == 1)
        .collect();
    messages.sort_by_key(|message| message["id"].as_i64().unwrap_or_default());
    Ok(messages
        .into_iter()
        .map(|message| {
            (
                message["kind"].as_str().unwrap_or_default().to_owned(),
                message["body"].as_str().unwrap_or_default().to_owned(),
            )
        })
        .collect())
}

/// T-14's shape: a worker whose turn starts a job in the background, says where it
/// stands and ends, and whose answer to the job's end is the report.
#[test]
fn a_worker_that_left_a_job_running_goes_on_until_the_jobs_end_wakes_it_and_its_report_is_the_result(
) -> Outcome {
    let rig = Rig::start(config())?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    rig.tell(
        project.id(),
        &format!(
            "DISPATCH --tier {} CLAUDE-JOB {JOB_SECONDS} {GOING} :: {REPORT}",
            tier(&added)
        ),
    )?;
    rig.wait_for("the worker's job to run", secs(60), || {
        Ok(rig.jobs("job").len() == 1)
    })?;
    let job = rig
        .jobs("job")
        .into_iter()
        .next()
        .ok_or_else(|| Error::Daemon("the worker's job is gone".to_owned()))?;

    // The chief is told the worker goes on, with the words it said.
    rig.wait_for("the chief to be told it goes on", secs(40), || {
        Ok(told(&project)?
            .iter()
            .any(|(kind, body)| kind == "note" && body.contains(GOING)))
    })?;

    // Passes of the daemon go by with the job running: T-14 is working, there is no result,
    // and the job is where it was: not asked to end, not ended.
    std::thread::sleep(secs(5));
    assert_eq!(project.task(1)?["state"], "working");
    assert!(
        told(&project)?.iter().all(|(kind, _)| kind != "result"),
        "{:?}",
        told(&project)?
    );
    assert!(is_alive(job.pid()), "the job runs");
    assert!(!job.released(), "and was never asked to end");

    // The job ends by itself, and the turn it wakes is the result.
    rig.wait_for("the job to end by itself", secs(60), || Ok(job.finished()))?;
    rig.wait_for("T-1 to be done", secs(60), || {
        Ok(project.task(1)?["state"] == "done")
    })?;
    let thread = told(&project)?;
    let kinds: Vec<&str> = thread.iter().map(|(kind, _)| kind.as_str()).collect();
    assert_eq!(
        kinds,
        ["note", "result"],
        "the chief was told it goes on, and then given the result, and nothing was said of anything ended: {thread:?}"
    );
    assert!(thread[0].1.starts_with("T-1 goes on in the background: @"));
    assert!(thread[0].1.contains("1 background job"), "{}", thread[0].1);
    assert_eq!(
        thread[1].1, REPORT,
        "the final answer of the turn the job's end woke, and none of what the turn before said"
    );
    assert!(!job.released(), "the job ran to its end, and none ended it");
    rig.close()?;
    Ok(())
}

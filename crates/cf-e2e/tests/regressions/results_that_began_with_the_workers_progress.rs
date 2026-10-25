//! A worker's result began with every line of progress it wrote on the way.
//!
//! Seen by the owner on 2026-10-15, on 3.0.4 (a chief's field reports): the results of
//! T-14 and T-15 began with ten to twenty lines ("Still reviewing…", "Now the tests")
//! before the report, because the result was everything the member wrote in its turn,
//! in order. The chief read a page of a worker thinking aloud before it came to what the
//! worker had found. The decision is
//! `decisions/2026-10-13-a-workers-answer-ends-its-turns-work.md` of consensflow-sme,
//! amended on 2026-10-15 for this report.
//!
//! What the owner should have seen: the result is the report, what the worker wrote after
//! its last tool call; the progress stays in the worker's transcript, which
//! `cf task get --transcript` reads.
//!
//! The stand-in writes the shape of Claude Code's record of such a turn (`STEPS` of the
//! fake agent): a message for each line of progress, each calling a tool, the tool's
//! result after it, and the answer last.

use cf_e2e::rig::{Project, Rig, OPEN};
use serde_json::{json, Value};

use crate::{config, secs, Outcome};

/// What the worker found, which is all the chief should have been given.
const REPORT: &str = "PASS: no findings in the parser";

/// How many lines of progress it wrote first, as T-14's and T-15's wrote ten to twenty.
const STEPS: u64 = 15;

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

#[test]
fn a_workers_result_is_the_report_it_wrote_after_its_last_tool_call_and_not_the_progress_before_it(
) -> Outcome {
    let rig = Rig::start(config())?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    rig.tell(
        project.id(),
        &format!("DISPATCH --tier {} STEPS {STEPS} {REPORT}", tier(&added)),
    )?;
    rig.wait_for("T-1 to be done", secs(60), || {
        Ok(project.task(1)?["state"] == "done")
    })?;
    rig.wait_for("the result to reach the chief", secs(30), || {
        Ok(!results(&project)?.is_empty())
    })?;

    let result = &results(&project)?[0];
    assert_eq!(
        result["body"], REPORT,
        "the report alone, none of its {STEPS} lines of progress"
    );

    // The worker did write them, and they are in its transcript.
    let handle = project.lane("worker")?["participant"]["handle"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    let frame = rig.open_frame(&format!("p{}-{handle}", project.id()), OPEN)?;
    let session = frame["argv"]
        .as_array()
        .and_then(|argv| {
            let at = argv.iter().position(|word| word == "--session-id")?;
            argv.get(at + 1)?.as_str().map(str::to_owned)
        })
        .unwrap_or_default();
    let transcript = rig.transcript(&session);
    assert!(
        transcript.contains(&format!("Step 1 of {STEPS}"))
            && transcript.contains(&format!("Step {STEPS} of {STEPS}")),
        "the progress stays in what the worker wrote"
    );
    rig.close()?;
    Ok(())
}

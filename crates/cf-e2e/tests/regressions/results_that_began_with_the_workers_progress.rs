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
//!
//! Seen again on 3.0.6-alpha.2 (T-262): T-593's result (m-4165) began with the line its
//! worker wrote before it asked the advisor, in the message that answers, whose result
//! Claude Code writes in that message itself; and the chief decided that a turn that wrote
//! nothing after its last tool call gives the last words it wrote before it.

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

/// The result the chief is given of a worker's turn the line `steps` (a `STEPS` line of
/// the stand-in's) asks for, and what the worker wrote in its window, whole.
fn result_and_transcript(steps: &str) -> cf_e2e::Result<(Value, String)> {
    let rig = Rig::start(config())?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    rig.tell(
        project.id(),
        &format!("DISPATCH --tier {} {steps}", tier(&added)),
    )?;
    rig.wait_for("T-1 to be done", secs(60), || {
        Ok(project.task(1)?["state"] == "done")
    })?;
    rig.wait_for("the result to reach the chief", secs(30), || {
        Ok(!results(&project)?.is_empty())
    })?;
    let result = results(&project)?.remove(0);

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
    rig.close()?;
    Ok((result, transcript))
}

#[test]
fn a_workers_result_is_the_report_it_wrote_after_its_last_tool_call_and_not_the_progress_before_it(
) -> Outcome {
    let (result, transcript) = result_and_transcript(&format!("STEPS {STEPS} {REPORT}"))?;
    assert_eq!(
        result["body"], REPORT,
        "the report alone, none of its {STEPS} lines of progress"
    );
    // The worker did write them, and they are in its transcript.
    assert!(
        transcript.contains(&format!("Step 1 of {STEPS}"))
            && transcript.contains(&format!("Step {STEPS} of {STEPS}")),
        "the progress stays in what the worker wrote"
    );
    Ok(())
}

#[test]
fn a_report_written_after_the_advisor_answered_in_its_own_message_is_the_result_alone() -> Outcome {
    // T-593's result (m-4165, 3.0.6-alpha.2) began with the line its worker wrote in its
    // answer's message before it asked the advisor, a server tool whose result Claude Code
    // writes in that message itself.
    let (result, transcript) = result_and_transcript(&format!("STEPS 3 --advised {REPORT}"))?;
    assert_eq!(
        result["body"], REPORT,
        "the report alone, not the words before the advisor's answer"
    );
    assert!(
        transcript.contains("One last check with the advisor")
            && transcript.contains("Step 3 of 3"),
        "what it wrote before stays in its transcript"
    );
    Ok(())
}

#[test]
fn a_worker_that_wrote_nothing_after_its_last_tool_call_gives_the_last_words_it_wrote() -> Outcome {
    // The chief's decision on T-262: a result is never empty.
    let (result, transcript) = result_and_transcript("STEPS 3 --quiet")?;
    assert_eq!(
        result["body"],
        "Its turn ended with no words after its last command. The last words it wrote:\n\nStep 3 of 3",
        "the last words before its last tool call"
    );
    assert!(transcript.contains("Step 1 of 3"));
    Ok(())
}

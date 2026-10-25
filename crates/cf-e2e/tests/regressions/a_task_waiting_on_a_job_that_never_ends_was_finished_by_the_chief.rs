//! A task that waits on a job that never ends, finished by the chief.
//!
//! The owner's decision of 2026-10-18 (`decisions/2026-10-18-the-chief-can-finish-any-working-task.md`
//! of consensflow-sme), after poker-lab's T-442 stuck "working" with its answer in the
//! record, and dip-agentos's T-20 (#9 of `issues/field-reports-2026-10-19-dip-agentos.md`),
//! whose verified commit could not be accepted: `cf task finish T-n` takes what the worker of
//! any working task said last as its result, and ends nothing.
//!
//! What the chief should see: a worker whose turn left a dev server running (a job that
//! never ends) holds its task working; `cf task finish T-1` makes its last complete answer
//! the result, once, says the server was not ended, and a second asks is refused.

use cf_e2e::process::is_alive;
use cf_e2e::rig::{Project, Rig, OPEN};
use cf_e2e::Error;
use serde_json::json;

use crate::a_workers_still_going_was_given_as_its_result::{tier, told};
use crate::{config, secs, Outcome};

/// What the worker said as its turn ended with the server running.
const SERVING: &str = "The docs are served on port 8000";

#[test]
fn the_chief_finishes_a_task_held_by_a_job_that_never_ends_with_its_workers_last_answer() -> Outcome
{
    let rig = Rig::start(config())?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    rig.tell(
        project.id(),
        &format!(
            "DISPATCH --tier {} CLAUDE-JOB 600 {SERVING} :: The server stopped",
            tier(&added)
        ),
    )?;
    rig.wait_for("the worker's server to run", secs(60), || {
        Ok(rig.jobs("job").len() == 1)
    })?;
    let server = rig
        .jobs("job")
        .into_iter()
        .next()
        .ok_or_else(|| Error::Daemon("the worker's server is gone".to_owned()))?;
    rig.wait_for("the chief to be told it goes on", secs(40), || {
        Ok(told(&project)?
            .iter()
            .any(|(kind, body)| kind == "note" && body.contains(SERVING)))
    })?;
    assert_eq!(project.task(1)?["state"], "working");

    let chief = rig.open_frame(&format!("p{}-chief", project.id()), OPEN)?;
    let finished = rig.cf_in_window(&chief, ["task", "finish", "T-1"])?;
    assert_eq!(finished.code, Some(0), "{finished}");
    assert!(
        finished.stdout.contains("T-1 is finished: what @")
            && finished
                .stdout
                .contains("(1 background job): it may run on"),
        "{finished}"
    );
    assert_eq!(project.task(1)?["state"], "done");
    assert!(
        is_alive(server.pid()) || server.released(),
        "finishing ended nothing by itself"
    );
    let results: Vec<String> = told(&project)?
        .into_iter()
        .filter(|(kind, _)| kind == "result")
        .map(|(_, body)| body)
        .collect();
    assert_eq!(results.len(), 1, "{results:?}");
    assert!(results[0].starts_with(SERVING), "{results:?}");
    assert!(
        results[0].contains("which finishing ends none of"),
        "{results:?}"
    );

    // Asked again, it is refused: the task is done.
    let again = rig.cf_in_window(&chief, ["task", "finish", "T-1"])?;
    assert_ne!(again.code, Some(0), "{again}");
    assert!(
        again.stderr.contains("cannot finish T-1: it is done"),
        "{again}"
    );
    rig.close()?;
    Ok(())
}

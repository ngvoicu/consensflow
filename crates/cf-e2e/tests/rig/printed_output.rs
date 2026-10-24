//! What a window prints, as the rig takes it in place of the page (T-139, 2026-10-24).
//!
//! The pane host reads no more of a window whose unread output fills its budget
//! (`backlogBytes`, 1 MiB unless the open names one), and it is the page that says each
//! frame was read (`pane.ack`). A rig has no page, and said nothing: every window of a
//! rig, a long run's real harnesses among them, stopped in the middle of its next write
//! once it had printed 1 MiB, its program blocked on its terminal. A stand-in window
//! printing as fast as it could stopped at line 15,888 of 66 bytes, 1,048,608 bytes in.
//! It may be what froze the long run's Pi windows of 2026-10-19 and 2026-10-20 (Pi
//! streams its model's reasoning to its screen; consensflow-sme
//! `issues/a-chief-stalls-after-a-compact-or-a-restart.md`, its second candidate), which
//! their evidence cannot tell: it holds no terminal's unread bytes.

use cf_e2e::rig::{Project, Rig};
use serde_json::{json, Value};

use crate::{rig, secs, Outcome};

/// The results the chief has been given or is to be given.
fn results(project: &Project) -> cf_e2e::Result<Vec<Value>> {
    Ok(project
        .inbox("chief")?
        .into_iter()
        .filter(|message| message["kind"] == "result")
        .collect())
}

/// A worker whose turn prints 4 MiB to its screen, as a harness streams its model's
/// words, and then answers: its task is done and the chief is given its result.
#[test]
fn a_worker_whose_turn_prints_four_mib_to_its_screen_ends_it_and_the_chief_has_its_result(
) -> Outcome {
    let rig: Rig = rig()?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    let tier = added["member"]["tier"].as_str().unwrap_or_default();
    rig.tell(
        project.id(),
        &format!("DISPATCH --tier {tier} PRINT 4194304 Reply with exactly: PRINTED"),
    )?;
    rig.wait_for("T-1 to be done", secs(180), || {
        Ok(project.task(1)?["state"] == "done")
    })?;
    rig.wait_for("the result to reach the chief", secs(30), || {
        Ok(!results(&project)?.is_empty())
    })?;
    assert_eq!(results(&project)?[0]["body"], "PRINTED");
    let printed = rig.open_frames().iter().any(|opened| {
        opened["id"].as_str().is_some_and(|pane| {
            pane.contains("worker") && rig.screen(pane).contains("reasoning, line")
        })
    });
    assert!(printed, "the worker's window printed it");
    rig.close()?;
    Ok(())
}

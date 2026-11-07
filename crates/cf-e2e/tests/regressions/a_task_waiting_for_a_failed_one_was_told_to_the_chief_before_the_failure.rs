//! A task waiting for a failed one was told to the chief before the failure.
//!
//! Seen on the "kluris" board on 2026-11-02, ConsensFlow 3.0.7-alpha.1 (main
//! `d63067fa8`), project 11 of the owner's ledger. The harness said the model was at
//! capacity and T-302 failed. The chief was sent "T-303 waits for T-302, which failed"
//! as m-6900 and "T-302 failed" as m-6901, in one write, a millisecond apart. A chief is
//! given its notes one at a time, the oldest first: it acted on the consequence with the
//! cause unknown (it cancelled T-303 and opened another task), and the cause reached it
//! 47 seconds later, when its turn on the first was over, as news it had already dealt
//! with. The report is the issue `a-note-of-what-a-failure-left-behind-reached-the-chief-
//! before-the-failure` in consensflow-sme (T-1113 fixed the ledger's own failure, T-1148
//! the rest).
//!
//! A Codex worker fails here three seconds into its brief, for a request its server refused
//! (a model at capacity is waited out since T-1112), with words of the stand-in's own and the
//! plain name Codex's kind has (`a_failure_said_no_reason`), and the chief has put a second task on the board that needs
//! the first.
//!
//! What the owner should have seen: the chief is sent one note for T-1, the failure first,
//! and the task that waits for it named after the failure, in the same note.

use cf_e2e::rig::{Project, Rig};
use serde_json::{json, Value};

use crate::{config_with_codex, secs, Outcome};

/// The brief whose turn fails, three seconds in.
const BRIEF: &str = "SLEEP 3 TURN-FAILS Reply with exactly: CODEX_OK";

/// What the stand-in Codex says of the turn it failed.
const SAID: &str = "The stand-in refused the request.";

/// The roster of the case: the chief on the stand-in Claude, the worker on Codex.
fn write_roster(rig: &Rig) -> cf_e2e::Result {
    let roster = json!({
        "schemaVersion": 1,
        "agents": [
            { "id": "chief", "kind": "claude-code", "model": "fake-chief" },
            { "id": "worker", "kind": "codex", "model": "fake", "workTier": "standard" },
        ],
    });
    rig.stand_ins(&roster)
}

/// The notes ConsensFlow wrote the chief that name T-1 or T-2, in the order they were
/// written: their ids and words.
fn about_the_two(project: &Project) -> cf_e2e::Result<Vec<(i64, String)>> {
    let mut notes: Vec<(i64, String)> = project
        .inbox("chief")?
        .into_iter()
        .filter(|m| m["kind"] == "note" && m.get("sender") == Some(&Value::Null))
        .filter_map(|m| Some((m["id"].as_i64()?, m["body"].as_str()?.to_owned())))
        .filter(|(_, body)| body.contains("T-1") || body.contains("T-2"))
        .collect();
    notes.sort();
    Ok(notes)
}

#[test]
fn a_task_that_waits_for_a_failed_one_is_named_in_the_note_that_says_it_failed() -> Outcome {
    let rig = Rig::start(config_with_codex())?;
    write_roster(&rig)?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    let tier = added["member"]["tier"].as_str().unwrap_or("standard");
    // One turn of the chief puts both tasks on the board: T-2 needs T-1.
    rig.tell(
        project.id(),
        &format!(
            "DISPATCH --tier {tier} {BRIEF}\nDISPATCH --tier {tier} --needs T-1 Reply with exactly: AFTER"
        ),
    )?;

    rig.wait_for("T-1 to fail", secs(90), || {
        Ok(project.task(1)?["state"] == "failed")
    })?;
    let session = project.task(1)?["assignee"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    assert!(session.starts_with("worker-"), "{session}");
    rig.wait_for("the chief to be told T-1 failed", secs(30), || {
        Ok(about_the_two(&project)?
            .iter()
            .any(|(_, body)| body.starts_with("T-1 failed: ")))
    })?;

    let told = about_the_two(&project)?;
    assert_eq!(
        told.iter().map(|(_, body)| body.as_str()).collect::<Vec<_>>(),
        [format!(
            "T-1 failed: @{session}'s harness reported a failure: the API rejected the request as malformed (bad_request); it said: \"{SAID}\". Reopen it with: cf task reopen T-1 \"…\", or accept it with what you checked: cf task accept T-1 \"…\" T-2 waits for it: cancel T-2, or reopen T-1 and let it finish."
        )],
        "the failure first, and the task that waits for it in it: {told:?}"
    );
    assert_eq!(project.task(2)?["state"], "open", "T-2 still waits");
    rig.close()?;
    Ok(())
}

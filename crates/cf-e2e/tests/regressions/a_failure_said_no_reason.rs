//! A worker's failed turn failed its task with no reason.
//!
//! Seen by the owner of dip-agentos between 2026-10-17 and 19, on 3.0.4: each
//! failure said only "the harness reported a failure", and neither the chief nor
//! the human could tell a crash from a refusal from a bad model name. The report
//! is row 4 of `issues/field-reports-2026-10-19-dip-agentos.md` in consensflow-sme;
//! the project is not on this Mac, so its records were not read.
//!
//! Codex ends a turn it could not serve with `task_complete` whose error says why,
//! with the error's words (`server_overloaded` in a real rollout: a model at capacity,
//! which a task now waits out, T-1112). A Codex worker fails here in that shape, three
//! seconds into its brief, for a request its server refused (`bad_request`), with words
//! of the stand-in's own.
//!
//! What the owner should have seen: T-1 fails, and the note that says so, to the
//! chief, quotes what Codex said, and names the kind of the error in plain words
//! (T-823: Codex's safety filter refused two turns of the kluris board, 2026-10-27, and
//! the board said only that the harness reported a failure; the second case fails a turn
//! in the shape of those records, `cyber_policy` and the filter's words).

use cf_e2e::rig::{Project, Rig};
use serde_json::{json, Value};

use crate::{config_with_codex, secs, Outcome};

/// The brief whose turn fails, three seconds in.
const BRIEF: &str = "SLEEP 3 TURN-FAILS Reply with exactly: CODEX_OK";

/// What the stand-in Codex says of the turn it failed.
const SAID: &str = "The stand-in refused the request.";

/// The brief whose turn the stand-in's safety filter refuses, three seconds in.
const FILTERED: &str = "SLEEP 3 TURN-FILTERED Reply with exactly: CODEX_OK";

/// What the stand-in Codex says of the turn its safety filter refused: a record's words.
const FILTER_SAID: &str = "This content was flagged for possible cybersecurity risk. If this seems wrong, try rephrasing your request. If you\u{2019}re doing authorized security work that requires more cyber permissive safeguards, apply for Daybreak access via https://platform.openai.com/settings/organization/status-and-access before retrying.";

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

/// The notes ConsensFlow wrote the chief that say T-1 failed.
fn failed_notes(project: &Project) -> cf_e2e::Result<Vec<String>> {
    Ok(project
        .inbox("chief")?
        .into_iter()
        .filter(|m| m["kind"] == "note" && m.get("sender") == Some(&Value::Null))
        .filter_map(|m| m["body"].as_str().map(str::to_owned))
        .filter(|body| body.starts_with("T-1 failed: "))
        .collect())
}

/// A Codex worker is given `brief`, its turn fails, and the note T-1's failure makes: the
/// worker's session, and the note.
fn the_note_of_a_failed_turn(brief: &str) -> cf_e2e::Result<(String, String)> {
    let rig = Rig::start(config_with_codex())?;
    write_roster(&rig)?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    let tier = added["member"]["tier"].as_str().unwrap_or("standard");
    rig.tell(project.id(), &format!("DISPATCH --tier {tier} {brief}"))?;

    rig.wait_for("T-1 to fail", secs(90), || {
        Ok(project.task(1)?["state"] == "failed")
    })?;
    let task = project.task(1)?;
    let session = task["assignee"].as_str().unwrap_or_default().to_owned();
    assert!(session.starts_with("worker-"), "{task}");
    rig.wait_for("the chief to be told T-1 failed", secs(30), || {
        Ok(!failed_notes(&project)?.is_empty())
    })?;
    let mut notes = failed_notes(&project)?;
    assert_eq!(notes.len(), 1, "{notes:?}");
    rig.close()?;
    Ok((session, notes.remove(0)))
}

#[test]
fn a_codex_worker_whose_turn_failed_fails_its_task_with_what_codex_said() -> Outcome {
    let (session, note) = the_note_of_a_failed_turn(BRIEF)?;
    assert_eq!(
        note,
        format!(
            "T-1 failed: @{session}'s harness reported a failure: the API rejected the request as malformed (bad_request); it said: \"{SAID}\". Reopen it with: cf task reopen T-1 \"…\", or accept it with what you checked: cf task accept T-1 \"…\""
        )
    );
    Ok(())
}

#[test]
fn a_codex_worker_whose_turn_its_safety_filter_refused_fails_its_task_saying_so() -> Outcome {
    let (session, note) = the_note_of_a_failed_turn(FILTERED)?;
    let said = FILTER_SAID.replace('"', "'");
    assert_eq!(
        note,
        format!(
            "T-1 failed: @{session}'s harness reported a failure: Codex's safety filter refused the turn as a cybersecurity risk (cyber_policy): reopen it with the work rephrased, or give security work to another harness; it said: \"{said}\". Reopen it with: cf task reopen T-1 \"…\", or accept it with what you checked: cf task accept T-1 \"…\""
        )
    );
    Ok(())
}

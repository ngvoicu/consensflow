//! A turn that fails is tried again in its window, twice, before its task fails (D3's N35): each
//! try is a hold of the task and main's own Resume. The stand-in's turn fails a given number of
//! times (`FAIL-TURNS n`), the retries' among them, and the daemon's tries run at a unit of half
//! a second (`CONSENSFLOW_TURN_RETRY_MS`) in place of its thirty: the stand-in Claude Code, Codex
//! and Devin each run both cases. Muse's echo stand-in cannot fail a turn, and the rig has no Pi.
use cf_e2e::rig::{Config, Project, Rig};
use serde_json::{json, Value};

use crate::{config, secs, Outcome};

const FAKE_CODEX: &str = env!("CARGO_BIN_EXE_fake-codex");
const FAKE_DEVIN: &str = env!("CARGO_BIN_EXE_fake-devin");

/// The rig's configuration: the daemon tries a failed turn at a unit of half a second, and a
/// worker of `kind` runs on its stand-in.
fn retrying(kind: &str) -> Config {
    let config = match kind {
        "codex" => config().codex(FAKE_CODEX),
        "devin" => crate::devin_config(FAKE_DEVIN),
        _ => config(),
    };
    // Devin's adapter reads a window at work for 30 seconds after a message of the user's its
    // agent has not answered, an error end of the turn or not (`TAKEN_IN_MS`): a Resume waits
    // that long for the window's rest, so its unit is eight times the others'.
    let unit = if kind == "devin" { "4000" } else { "500" };
    config.daemon_var("CONSENSFLOW_TURN_RETRY_MS", unit)
}

/// The project of a chief on the stand-in Claude Code and a worker on `kind`'s.
fn opened<'a>(rig: &'a Rig, kind: &str) -> cf_e2e::Result<Project<'a>> {
    let roster = json!({
        "schemaVersion": 1,
        "agents": [
            { "id": "chief", "kind": "claude-code", "model": "fake-chief" },
            { "id": "worker", "kind": kind, "model": "fake", "workTier": "standard" },
        ],
    });
    rig.stand_ins(&roster)?;
    let project = Project::open(rig, "chief", json!({}))?;
    project.add_member("worker")?;
    Ok(project)
}

fn results_of(project: &Project, number: i64) -> cf_e2e::Result<usize> {
    Ok(project.task(number)?["messages"]
        .as_array()
        .map_or(0, |messages| {
            messages
                .iter()
                .filter(|message| message["kind"] == "result")
                .count()
        }))
}

/// How many times main's own Resume was pasted for T-1: one for each try.
fn resumes_of(project: &Project, number: i64) -> cf_e2e::Result<usize> {
    Ok(project.task(number)?["messages"]
        .as_array()
        .map_or(0, |messages| {
            messages
                .iter()
                .filter(|message| {
                    message["kind"] == "task"
                        && message["body"] == "Resumed: Go on where you stopped."
                })
                .count()
        }))
}

/// The text of the result of T-1.
fn result_of(project: &Project, number: i64) -> cf_e2e::Result<String> {
    Ok(project.task(number)?["messages"]
        .as_array()
        .and_then(|messages| messages.iter().find(|message| message["kind"] == "result"))
        .and_then(|message| message["body"].as_str())
        .unwrap_or_default()
        .to_owned())
}

fn failed_notes(project: &Project) -> cf_e2e::Result<Vec<String>> {
    Ok(project
        .inbox("chief")?
        .into_iter()
        .filter(|m| m["kind"] == "note" && m.get("sender") == Some(&Value::Null))
        .filter_map(|m| m["body"].as_str().map(str::to_owned))
        .filter(|body| body.starts_with("T-1 failed: "))
        .collect())
}

fn two_failed_turns_then_an_answer(kind: &str) -> Outcome {
    let rig = Rig::start(retrying(kind))?;
    let project = opened(&rig, kind)?;
    rig.tell(
        project.id(),
        "DISPATCH --tier standard FAIL-TURNS 2 Reply with exactly: AFTER_TWO",
    )?;
    rig.wait_for("T-1 to be done", secs(150), || {
        Ok(matches!(
            project.task(1)?["state"].as_str(),
            Some("done" | "accepted")
        ))
    })?;
    assert_eq!(results_of(&project, 1)?, 1, "one result");
    assert!(
        result_of(&project, 1)?.contains("AFTER_TWO"),
        "the answer of the third turn is the result: {}",
        result_of(&project, 1)?
    );
    assert_eq!(
        resumes_of(&project, 1)?,
        2,
        "two Resumes were pasted, one for each try"
    );
    assert!(failed_notes(&project)?.is_empty(), "nothing failed");
    rig.close()?;
    Ok(())
}

fn a_turn_that_always_fails(kind: &str) -> Outcome {
    let rig = Rig::start(retrying(kind))?;
    let project = opened(&rig, kind)?;
    rig.tell(
        project.id(),
        "DISPATCH --tier standard FAIL-TURNS 99 Reply with exactly: NEVER",
    )?;
    let waited = rig.wait_for("T-1 to fail", secs(150), || {
        Ok(project.task(1)?["state"] == "failed")
    });
    waited?;
    rig.wait_for("the chief to be told T-1 failed", secs(30), || {
        Ok(!failed_notes(&project)?.is_empty())
    })?;
    let notes = failed_notes(&project)?;
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(notes[0].contains("(tried again twice)"), "{notes:?}");
    assert_eq!(resumes_of(&project, 1)?, 2, "two Resumes, never a third");
    rig.close()?;
    Ok(())
}

#[test]
fn a_claude_worker_whose_turn_fails_twice_ends_its_task_done_with_one_result() -> Outcome {
    two_failed_turns_then_an_answer("claude-code")
}

#[test]
fn a_claude_worker_whose_turn_always_fails_fails_its_task_after_its_two_tries() -> Outcome {
    a_turn_that_always_fails("claude-code")
}

#[test]
fn a_codex_worker_whose_turn_fails_twice_ends_its_task_done_with_one_result() -> Outcome {
    two_failed_turns_then_an_answer("codex")
}

#[test]
fn a_codex_worker_whose_turn_always_fails_fails_its_task_after_its_two_tries() -> Outcome {
    a_turn_that_always_fails("codex")
}

#[test]
fn a_devin_worker_whose_turn_fails_twice_ends_its_task_done_with_one_result() -> Outcome {
    // On the wire log Devin 3000.12.43 writes: the error end of a turn is read there.
    crate::on_the_installed_devin(|| two_failed_turns_then_an_answer("devin"))
}

#[test]
fn a_devin_worker_whose_turn_always_fails_fails_its_task_after_its_two_tries() -> Outcome {
    crate::on_the_installed_devin(|| a_turn_that_always_fails("devin"))
}

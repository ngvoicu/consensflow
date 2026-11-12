//! Claude Code's prompt suggestion, which a project's Project settings leaves off until the human
//! ticks it (T-1444), end to end through the daemon and the pane host: a project that never ticked
//! "Prompt suggestion" has every Claude Code window launched with the switch in its own settings
//! file (`env.CLAUDE_CODE_ENABLE_PROMPT_SUGGESTION` as `false`), the chief's as the member's, and
//! a project that ticked it launches its windows with no word about it. The stand-in Claude's
//! windows run scripts, so these cases are Unix's.

#![cfg(unix)]

use std::fs;

use cf_e2e::rig::{Project, Rig, OPEN};
use serde_json::{json, Value};

use crate::{after, config, secs, Outcome};

/// The suggestion's switch in the `env` block of the settings file a Claude window was
/// launched with: none where the file has no `env`, or none of it; None as well where it has no
/// file at all.
fn suggestion_switch(frame: &Value) -> Option<Value> {
    let text = fs::read_to_string(after(frame, "--settings")).ok()?;
    let file: Value = serde_json::from_str(&text).ok()?;
    file.get("env")?
        .get("CLAUDE_CODE_ENABLE_PROMPT_SUGGESTION")
        .cloned()
}

/// Whether the window has a settings file of its launch.
fn has_settings(frame: &Value) -> bool {
    fs::read_to_string(after(frame, "--settings")).is_ok()
}

fn write_roster(rig: &Rig) -> cf_e2e::Result {
    rig.stand_ins(&json!({
        "schemaVersion": 1,
        "agents": [
            { "id": "chief", "kind": "claude-code", "model": "fake-chief" },
            { "id": "claude-worker", "kind": "claude-code", "model": "fake", "workTier": "light" },
        ],
    }))
}

/// The worker's window: the one that is not the chief's and has a settings file, once there is one.
fn worker_window(rig: &Rig, chief: &Value) -> cf_e2e::Result<Value> {
    let is_worker = |frame: &Value| frame["id"] != chief["id"] && has_settings(frame);
    rig.wait_for("the Claude worker's window", secs(120), || {
        Ok(rig.open_frames().iter().any(is_worker))
    })?;
    Ok(rig
        .open_frames()
        .into_iter()
        .find(is_worker)
        .unwrap_or(Value::Null))
}

#[test]
fn a_project_that_never_ticked_the_prompt_suggestion_launches_its_claude_windows_with_it_switched_off_the_chiefs_too(
) -> Outcome {
    let rig = Rig::start(config())?;
    write_roster(&rig)?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let id = project.id();
    project.add_member("claude-worker")?;
    // Off is what a project has until it is ticked, and its board says nothing of it.
    let board = rig.page("board.get", json!({ "project": id }))?;
    assert_eq!(board["board"]["project"]["id"], json!(id), "{board}");
    assert!(
        board["board"]["project"].get("promptSuggestion").is_none(),
        "{board}"
    );
    rig.tell(id, "DISPATCH --tier light Reply with exactly: CLAUDE_DONE")?;
    let chief = rig.open_frame(&format!("p{id}-chief"), OPEN)?;
    let worker = worker_window(&rig, &chief)?;
    assert_eq!(suggestion_switch(&worker), Some(json!("false")), "{worker}");
    assert_eq!(suggestion_switch(&chief), Some(json!("false")), "{chief}");
    rig.wait_for("the task to be done", secs(120), || {
        Ok(project.task(1).is_ok_and(|task| task["state"] == "done"))
    })?;
    rig.close()?;
    Ok(())
}

#[test]
fn a_project_that_ticked_the_prompt_suggestion_launches_its_claude_windows_with_no_word_about_it(
) -> Outcome {
    let rig = Rig::start(config())?;
    write_roster(&rig)?;
    let project = Project::open(&rig, "chief", json!({ "promptSuggestion": true }))?;
    let id = project.id();
    project.add_member("claude-worker")?;
    let board = rig.page("board.get", json!({ "project": id }))?;
    assert_eq!(
        board["board"]["project"]["promptSuggestion"],
        json!(true),
        "{board}"
    );
    rig.tell(id, "DISPATCH --tier light Reply with exactly: CLAUDE_DONE")?;
    let chief = rig.open_frame(&format!("p{id}-chief"), OPEN)?;
    let worker = worker_window(&rig, &chief)?;
    assert!(has_settings(&chief), "{chief}: it has a file");
    assert_eq!(suggestion_switch(&worker), None, "{worker}");
    assert_eq!(suggestion_switch(&chief), None, "{chief}");
    rig.wait_for("the task to be done", secs(120), || {
        Ok(project.task(1).is_ok_and(|task| task["state"] == "done"))
    })?;
    rig.close()?;
    Ok(())
}

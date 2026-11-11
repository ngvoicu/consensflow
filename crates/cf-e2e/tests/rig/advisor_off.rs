//! Claude Code's own advisor tool, which a project's Project settings has its members run
//! without (A5-20, T-1399), end to end through the daemon and the pane host: a project that never
//! ticked "Advisor tool" has every Claude Code member's window launched with the kill switch in
//! its own settings file (`env.CLAUDE_CODE_DISABLE_ADVISOR_TOOL`), the chief's never, and a
//! project that ticked it launches its members as it always did. The stand-in Claude's windows
//! run scripts, so these cases are Unix's.

#![cfg(unix)]

use std::fs;

use cf_e2e::rig::{Project, Rig, OPEN};
use serde_json::{json, Value};

use crate::{after, config, secs, Outcome};

/// The `env` block of the settings file a Claude window was launched with, if it has a file.
fn settings_env(frame: &Value) -> Option<Value> {
    let text = fs::read_to_string(after(frame, "--settings")).ok()?;
    let file: Value = serde_json::from_str(&text).ok()?;
    Some(file.get("env").cloned().unwrap_or(Value::Null))
}

/// The advisor's switch in the `env` block of the window's settings file: none where the file
/// has no `env`, or none of it. (The block may hold the prompt suggestion's switch besides, which
/// every project has until it is ticked: `prompt_suggestion_off`.)
fn advisor_switch(frame: &Value) -> Option<Value> {
    settings_env(frame)?
        .get("CLAUDE_CODE_DISABLE_ADVISOR_TOOL")
        .cloned()
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
    let is_worker = |frame: &Value| frame["id"] != chief["id"] && settings_env(frame).is_some();
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
fn a_project_that_never_ticked_the_advisor_tool_launches_its_claude_members_with_it_switched_off_and_the_chief_as_it_was(
) -> Outcome {
    let rig = Rig::start(config())?;
    write_roster(&rig)?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let id = project.id();
    project.add_member("claude-worker")?;
    // Off is what a project has until it is ticked, and its board says nothing of it.
    let board = rig.page("board.get", json!({ "project": id }))?;
    assert!(board["project"].get("advisorTool").is_none(), "{board}");
    rig.tell(id, "DISPATCH --tier light Reply with exactly: CLAUDE_DONE")?;
    let chief = rig.open_frame(&format!("p{id}-chief"), OPEN)?;
    let worker = worker_window(&rig, &chief)?;
    assert_eq!(advisor_switch(&worker), Some(json!("1")), "{worker}");
    // The chief's window is as it was: no kill switch in its settings file, if it has one.
    assert_eq!(advisor_switch(&chief), None, "{chief}");
    rig.wait_for("the task to be done", secs(120), || {
        Ok(project.task(1).is_ok_and(|task| task["state"] == "done"))
    })?;
    rig.close()?;
    Ok(())
}

#[test]
fn a_project_that_ticked_the_advisor_tool_launches_its_claude_members_as_it_always_did() -> Outcome
{
    let rig = Rig::start(config())?;
    write_roster(&rig)?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let id = project.id();
    project.add_member("claude-worker")?;
    let chosen = rig.page(
        "project.advisor_tool",
        json!({ "project": id, "advisorTool": true }),
    )?;
    assert_eq!(chosen["project"]["advisorTool"], json!(true), "{chosen}");
    rig.tell(id, "DISPATCH --tier light Reply with exactly: CLAUDE_DONE")?;
    let chief = rig.open_frame(&format!("p{id}-chief"), OPEN)?;
    let worker = worker_window(&rig, &chief)?;
    assert_eq!(advisor_switch(&worker), None, "{worker}");
    assert!(settings_env(&worker).is_some(), "{worker}: it has a file");
    rig.wait_for("the task to be done", secs(120), || {
        Ok(project.task(1).is_ok_and(|task| task["state"] == "done"))
    })?;
    rig.close()?;
    Ok(())
}

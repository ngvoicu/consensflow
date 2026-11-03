//! A chief is a harness and a model (CHF-10), over the daemon, the pane host and the
//! stand-in Claude: the human switches the chief's model in its window, and a Close and a
//! Resume reopen it on the switched model and effort; a conversation whose record names no
//! model reopens on the model picked for it; and a model its account no longer has (its
//! first turn failing in Claude Code's own record) starts the chief on the harness's own
//! default, with one note, the project left open.

use std::time::Duration;

use cf_e2e::rig::{chief_body, Project, Rig};
use serde_json::{json, Value};

use crate::{after, config, secs, session_of, Outcome};

/// A rig whose roster saves the chief's agent on the stand-in Claude, at `fake-chief`.
fn with_chief(config: cf_e2e::rig::Config) -> cf_e2e::Result<Rig> {
    let rig = Rig::start(config)?;
    let roster = json!({
        "schemaVersion": 1,
        "agents": [{ "id": "my-chief", "kind": "claude-code", "model": "fake-chief" }],
    });
    rig.stand_ins(&roster)?;
    Ok(rig)
}

/// Opens a project whose chief runs on Claude Code at `fake-chief`, as New project picks it.
fn opened(rig: &Rig) -> cf_e2e::Result<i64> {
    let opened = rig.page(
        "project.open",
        chief_body(rig, "my-chief", json!({ "directory": rig.workspace() }))?,
    )?;
    assert_eq!(opened["ok"], true, "{opened}");
    Ok(opened["project"]["id"].as_i64().unwrap_or_default())
}

/// The windows the chief of `project` was opened in, oldest first.
fn chief_frames(rig: &Rig, project: i64) -> Vec<Value> {
    let id = format!("p{project}-chief");
    rig.open_frames()
        .into_iter()
        .filter(|frame| frame["id"] == id.as_str())
        .collect()
}

/// The chief's `count`-th window, waited for.
fn chief_window(rig: &Rig, project: i64, count: usize) -> cf_e2e::Result<Value> {
    rig.wait_for("the chief's window", secs(30), || {
        Ok(chief_frames(rig, project).len() >= count)
    })?;
    Ok(chief_frames(rig, project)[count - 1].clone())
}

/// Closes the project and resumes it, as the human does.
fn closed_and_resumed(rig: &Rig, project: i64) -> cf_e2e::Result<()> {
    let closed = rig.page("project.close", json!({ "project": project }))?;
    assert_eq!(closed["ok"], true, "{closed}");
    rig.wait_for("the chief's window to go", secs(30), || {
        Ok(Project::new(rig, project).board()?["project"]["state"] == "suspended")
    })?;
    let resumed = rig.page("project.resume", json!({ "project": project }))?;
    assert_eq!(resumed["ok"], true, "{resumed}");
    Ok(())
}

/// The chief's participant, as the board says it.
fn chief(rig: &Rig, project: i64) -> cf_e2e::Result<Value> {
    let board = Project::new(rig, project).board()?;
    Ok(board["lanes"]
        .as_array()
        .and_then(|lanes| {
            lanes
                .iter()
                .find(|lane| lane["participant"]["handle"] == "chief")
        })
        .map(|lane| lane["participant"].clone())
        .unwrap_or(Value::Null))
}

#[test]
fn a_chief_the_human_switched_in_its_window_reopens_on_the_switched_model_and_effort() -> Outcome {
    let rig = with_chief(config())?;
    let project = opened(&rig)?;
    let first = chief_window(&rig, project, 1)?;
    assert_eq!(
        after(&first, "--model"),
        "fake-chief",
        "{:?}",
        first["argv"]
    );
    let session = session_of(&first);
    rig.tell(project, "Reply with exactly: BEFORE")?;
    rig.wait_for("the chief to answer", secs(10), || {
        Ok(rig.transcript(&session).contains("\"text\":\"BEFORE\""))
    })?;
    // The human's own switch in the window, with the key that keeps it to the
    // conversation; the next answer says the model switched to.
    rig.tell(project, "/model fake-switched low")?;
    rig.wait_for("the switch in the record", secs(10), || {
        Ok(rig
            .transcript(&session)
            .contains("Set model to fake-switched"))
    })?;
    rig.tell(project, "Reply with exactly: AFTER")?;
    rig.wait_for("an answer on the switched model", secs(10), || {
        Ok(rig
            .transcript(&session)
            .contains("\"model\":\"fake-switched\""))
    })?;
    closed_and_resumed(&rig, project)?;
    let again = chief_window(&rig, project, 2)?;
    assert_eq!(after(&again, "--resume"), session, "{:?}", again["argv"]);
    assert_eq!(
        after(&again, "--model"),
        "fake-switched",
        "{:?}",
        again["argv"]
    );
    assert_eq!(after(&again, "--effort"), "low", "{:?}", again["argv"]);
    // The pick stays the human's word; the card says what the window runs on.
    let now = chief(&rig, project)?;
    assert_eq!(
        (&now["model"], &now["ranOn"]),
        (&json!("fake-chief"), &json!("fake-switched"))
    );
    rig.close()?;
    Ok(())
}

#[test]
fn a_chief_whose_record_names_no_model_reopens_on_the_model_picked_for_it() -> Outcome {
    let rig = with_chief(config())?;
    let project = opened(&rig)?;
    chief_window(&rig, project, 1)?;
    closed_and_resumed(&rig, project)?;
    let again = chief_window(&rig, project, 2)?;
    assert_eq!(
        after(&again, "--model"),
        "fake-chief",
        "{:?}",
        again["argv"]
    );
    assert_eq!(
        after(&again, "--effort"),
        "",
        "no effort is picked: {:?}",
        again["argv"]
    );
    rig.close()?;
    Ok(())
}

#[test]
fn a_chief_whose_model_is_gone_starts_on_its_harnesss_default_and_the_human_is_told_once() -> Outcome
{
    let rig = with_chief(config().var("CF_TEST_GONE_MODEL", "fake-chief"))?;
    let project = opened(&rig)?;
    let first = chief_window(&rig, project, 1)?;
    assert_eq!(after(&first, "--model"), "fake-chief");
    // Its window opens, as Claude Code's does on a model the account lacks, and its first
    // turn fails in Claude Code's words, its record a `<synthetic>` API error naming the
    // model: the window is closed and the chief opens again at once on Claude Code's own
    // default, which the stand-in's list cannot name: no `--model`.
    rig.tell(project, "Reply with exactly: HELLO")?;
    let again = chief_window(&rig, project, 2)?;
    assert_eq!(after(&again, "--model"), "", "{:?}", again["argv"]);
    let board = Project::new(&rig, project);
    assert_eq!(board.board()?["project"]["state"], "open", "nothing closed");
    let told: Vec<String> = board
        .inbox("human")?
        .iter()
        .filter_map(|message| message["body"].as_str().map(str::to_owned))
        .filter(|body| body.contains("no longer available"))
        .collect();
    assert_eq!(
        told,
        ["The chief's model, fake-chief, is no longer available in Claude Code: the chief starts on Claude Code's own default model instead. Switch the chief to pick another."]
    );
    // And no third window: the default comes up.
    std::thread::sleep(Duration::from_secs(3));
    assert_eq!(chief_frames(&rig, project).len(), 2);
    rig.close()?;
    Ok(())
}

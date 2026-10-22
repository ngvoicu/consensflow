//! Keys the human typed into a dialog of the chief's window and left with Escape held
//! every delivery to it.
//!
//! Suspected on 2026-10-20/22, on 3.0.5, on the owner's other Mac (Claude Code): results
//! stayed queued for the chief for two days. The pane host guesses what the human typed and
//! did not send from their keys (a character adds one, Backspace takes one off, Enter, Ctrl+C
//! and Ctrl+U clear it), and every paste into the window waits while the count is up; Escape
//! changes nothing. A `/login` sent, a code pasted into its dialog, then Escape: the input box
//! is empty and the count is not. The same goes for any dialog, picker or question typed into
//! and left with Escape, on Claude Code and on Devin, the two harnesses ConsensFlow pastes
//! into (`cargo xtask live escape-unsent`: Devin 3000.11.92 holds so after its `/model`
//! picker). The report is `issues/keys-cleared-with-escape-still-hold-deliveries.md` in
//! consensflow-sme (T-288).
//!
//! What should have been seen: the worker's result handed to the chief, whose input box is
//! empty; and, where the human's words are still in the box after Escape (Claude Code keeps
//! them), the result waiting for them, never pasted into them.
//!
//! The cases are that shape on the rig: the chief's stand-in draws Claude Code's input box
//! and its model picker (`CF_TEST_INPUT_BOX`), the chief hands a worker a turn, and the human
//! types into the chief's window as the app's page does (`pane.typed`, which the pane host
//! counts): `/model` and Enter, a filter into the picker, and Escape, which closes it and
//! leaves the box empty, the count up; or words at the box, and Escape, which leaves them.

use std::thread::sleep;
use std::time::Duration;

use cf_e2e::rig::{Project, Rig};
use serde_json::{json, Value};

use crate::resume_of_a_chief_with_no_turn::{after, wait_for_the_chief, windows_of};
use crate::{config, secs, Outcome};

/// The gap between two keys the human types.
const KEY_GAP: Duration = Duration::from_millis(40);

/// The human types `text` into the chief's window, a key at a time, as the page does.
fn types(rig: &Rig, project: &Project, text: &str) -> Outcome {
    let pane = project.lane("chief")?["pane"].clone();
    for key in text.chars() {
        rig.types(&pane, key.to_string().as_bytes())?;
        sleep(KEY_GAP);
    }
    Ok(())
}

/// What the pane host says of the chief's window, with its screen as drawn.
fn chiefs_window(rig: &Rig, project: &Project) -> cf_e2e::Result<Value> {
    let pane = project.lane("chief")?["pane"].clone();
    rig.host(
        "pane.snapshot",
        json!({ "id": pane["id"], "generation": pane["generation"], "bottom": 12 }),
    )
}

/// The chief's result, once it is given one: its id, and whether it is delivered.
fn result(project: &Project) -> cf_e2e::Result<Option<(i64, bool)>> {
    Ok(project
        .inbox("chief")?
        .iter()
        .find(|message| message["kind"] == "result")
        .map(|message| {
            (
                message["id"].as_i64().unwrap_or_default(),
                message["state"] == "delivered",
            )
        }))
}

/// A rig whose chief draws Claude Code's input box, a worker at work on T-1 for
/// `seconds`, and the chief idle: the chief's session.
fn a_worker_at_work(rig: &Rig, project: &Project, seconds: u64) -> cf_e2e::Result<String> {
    let added = project.add_member("worker")?;
    let session = windows_of(rig, project)
        .first()
        .and_then(|first| after(first, "--session-id"))
        .ok_or_else(|| cf_e2e::Error::Daemon("the chief's window has no session".to_owned()))?
        .to_owned();
    rig.tell(
        project.id(),
        &format!(
            "DISPATCH --tier {} SLEEP {seconds} Reply with exactly: WORKER_OK",
            added["member"]["tier"].as_str().unwrap_or_default()
        ),
    )?;
    rig.wait_for("T-1 to be at work", secs(60), || {
        Ok(project.task(1)?["state"] == "working")
    })?;
    wait_for_the_chief(rig, project)?;
    Ok(session)
}

#[test]
fn keys_typed_into_a_picker_and_left_with_escape_hold_no_result_from_a_chief_whose_box_is_empty(
) -> Outcome {
    let rig = Rig::start(config().var("CF_TEST_INPUT_BOX", "chief"))?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let session = a_worker_at_work(&rig, &project, 20)?;

    // The human opens the model picker, types into its filter, and leaves it with Escape.
    types(&rig, &project, "/model")?;
    sleep(Duration::from_millis(600));
    types(&rig, &project, "\r")?;
    let pane = project.lane("chief")?["pane"]["id"].clone();
    rig.wait_for("the picker to be drawn", secs(10), || {
        Ok(rig
            .screen(pane.as_str().unwrap_or_default())
            .contains("Select model"))
    })?;
    types(&rig, &project, "zqv\x1b")?;
    rig.wait_for(
        "the box to be drawn empty, the keys counted as unsent",
        secs(10),
        || {
            let window = chiefs_window(&rig, &project)?;
            let last = window["bottom"].as_array().and_then(|lines| {
                lines
                    .iter()
                    .rev()
                    .find(|line| line.as_str().is_some_and(|line| line.starts_with('❯')))
                    .cloned()
            });
            Ok(window["unsent"] == true && last == Some(json!("❯")))
        },
    )?;
    assert_eq!(
        project.task(1)?["state"],
        "working",
        "the worker's result came before the human's keys: the case holds nothing"
    );

    // The worker answers, and its result goes into the chief's window.
    rig.wait_for("T-1 to be done", secs(90), || {
        Ok(project.task(1)?["state"] == "done")
    })?;
    rig.wait_for(
        "the worker's result to be handed to the chief, whose box is empty",
        secs(60),
        || {
            Ok(result(&project)?.is_some_and(|(id, delivered)| {
                delivered
                    && rig
                        .transcript(&session)
                        .contains(&format!("[ConsensFlow m-{id} "))
            }))
        },
    )?;
    assert_eq!(project.lane("chief")?["holding"], false);
    rig.close()?;
    Ok(())
}

#[test]
fn words_left_in_the_box_at_escape_hold_the_result_until_the_human_clears_them() -> Outcome {
    let rig = Rig::start(config().var("CF_TEST_INPUT_BOX", "chief"))?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let session = a_worker_at_work(&rig, &project, 5)?;

    // The human types words at the box and presses Escape, which leaves them there.
    types(&rig, &project, "half a thought\x1b")?;
    rig.wait_for(
        "the worker's result to wait for the human's words",
        secs(90),
        || Ok(project.task(1)?["state"] == "done" && project.lane("chief")?["holding"] == true),
    )?;
    // Long enough for a look that read the box empty to have pasted the result.
    sleep(Duration::from_secs(5));
    assert_eq!(
        result(&project)?.map(|(_, delivered)| delivered),
        Some(false)
    );
    assert!(
        !rig.transcript(&session).contains("[ConsensFlow m-"),
        "nothing was pasted into the human's words"
    );

    // The human clears the box with Ctrl+U, and the result goes in.
    types(&rig, &project, "\u{15}")?;
    rig.wait_for(
        "the worker's result to be handed to the chief",
        secs(60),
        || Ok(result(&project)?.is_some_and(|(_, delivered)| delivered)),
    )?;
    rig.close()?;
    Ok(())
}

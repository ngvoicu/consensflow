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
//!
//! Round 2 (T-330, from Calliope's review in T-329): the box left empty shows Claude Code's
//! faint suggestion, which the cursor at the box's start says is no words typed
//! (`CF_TEST_SUGGESTION`); and a key the window has not drawn yet holds the result, though
//! the box still reads empty (`CF_TEST_SLOW_DRAW`, the box drawn 3 s after it changed).

use std::thread::sleep;
use std::time::Duration;

use cf_e2e::rig::{write_staff, Project, Rig};
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

/// The human opens the model picker in the chief's window, types into its filter,
/// and leaves it with Escape: the box is drawn again, its line `shown`, and the keys
/// are counted as unsent.
fn leaves_the_picker_with_escape(rig: &Rig, project: &Project, shown: &str) -> Outcome {
    types(rig, project, "/model")?;
    sleep(Duration::from_millis(600));
    types(rig, project, "\r")?;
    let pane = project.lane("chief")?["pane"]["id"].clone();
    rig.wait_for("the picker to be drawn", secs(10), || {
        Ok(rig
            .screen(pane.as_str().unwrap_or_default())
            .contains("Select model"))
    })?;
    types(rig, project, "zqv\x1b")?;
    rig.wait_for(
        "the box to be drawn empty, the keys counted as unsent",
        secs(10),
        || {
            let window = chiefs_window(rig, project)?;
            let last = window["bottom"].as_array().and_then(|lines| {
                lines
                    .iter()
                    .rev()
                    .find(|line| line.as_str().is_some_and(|line| line.starts_with('❯')))
                    .cloned()
            });
            Ok(window["unsent"] == true && last == Some(json!(shown)))
        },
    )?;
    Ok(())
}

#[test]
fn keys_typed_into_a_picker_and_left_with_escape_hold_no_result_from_a_chief_whose_box_is_empty(
) -> Outcome {
    let rig = Rig::start(config().var("CF_TEST_INPUT_BOX", "chief"))?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let session = a_worker_at_work(&rig, &project, 20)?;

    leaves_the_picker_with_escape(&rig, &project, "❯")?;
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

#[test]
fn a_chief_whose_empty_box_shows_its_suggestion_after_keys_left_in_a_picker_is_handed_its_workers_result(
) -> Outcome {
    let config = config()
        .var("CF_TEST_INPUT_BOX", "chief")
        .var("CF_TEST_SUGGESTION", "chief");
    let rig = Rig::start(config)?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let session = a_worker_at_work(&rig, &project, 20)?;
    leaves_the_picker_with_escape(&rig, &project, "❯ Try \"fix lint errors\"")?;
    assert_eq!(
        project.task(1)?["state"],
        "working",
        "the worker's result came before the human's keys: the case holds nothing"
    );
    rig.wait_for("T-1 to be done", secs(90), || {
        Ok(project.task(1)?["state"] == "done")
    })?;
    rig.wait_for(
        "the worker's result to be handed to the chief, its box showing only the suggestion",
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
    rig.close()?;
    Ok(())
}

#[test]
fn a_key_the_chiefs_window_has_not_drawn_yet_holds_the_result_until_it_is_drawn() -> Outcome {
    let config = config()
        .var("CF_TEST_INPUT_BOX", "chief")
        .var("CF_TEST_SLOW_DRAW", "chief");
    let rig = Rig::start(config)?;
    write_staff(&rig)?;
    // Every word one agent gives another waits for the human to pass it on: the
    // worker's result is let go the moment the human has typed a key.
    let project = Project::open_with_staff(&rig, "chief", Some(true), &[("worker", "worker")])?;
    let chief = rig.open_frame(&format!("p{}-chief", project.id()), secs(60))?;
    let session = after(&chief, "--session-id")
        .ok_or("the chief's window has no session")?
        .to_owned();
    let tier = project.tiers()?.remove("worker").unwrap_or_default();
    rig.tell(
        project.id(),
        &format!("DISPATCH --tier {tier} SLEEP 1 Reply with exactly: WORKER_OK"),
    )?;
    let brief = the_one_held(&rig, &project, "the brief to be held")?;
    approve(&rig, &brief["id"])?;
    let held = the_one_held(&rig, &project, "the worker's result to be held")?;
    assert_eq!(held["kind"], "result");
    wait_for_the_chief(&rig, &project)?;
    // The chief's box drawn as it is now, slowly.
    sleep(Duration::from_secs(4));

    // The human types a key, which the window takes 3 s to draw, and the result is
    // let go at once: its box reads empty for those seconds.
    types(&rig, &project, "a")?;
    approve(&rig, &held["id"])?;
    sleep(Duration::from_secs(8));
    let id = held["id"].as_i64().unwrap_or_default();
    assert!(
        !rig.transcript(&session)
            .contains(&format!("[ConsensFlow m-{id} ")),
        "the result was pasted behind the key the window had not drawn yet"
    );
    assert_eq!(project.lane("chief")?["holding"], true);

    // The human clears the box, and the result goes in alone.
    types(&rig, &project, "\u{15}")?;
    rig.wait_for(
        "the worker's result to be handed to the chief",
        secs(60),
        || {
            Ok(rig
                .transcript(&session)
                .contains(&format!("[ConsensFlow m-{id} ")))
        },
    )?;
    assert!(
        !rig.transcript(&session)
            .contains(&format!("a[ConsensFlow m-{id} ")),
        "the key went with the result"
    );
    rig.close()?;
    Ok(())
}

/// What waits at the gate, the one message there, waited for.
fn the_one_held(rig: &Rig, project: &Project, what: &str) -> cf_e2e::Result<Value> {
    let held = || -> cf_e2e::Result<Vec<Value>> {
        Ok(project.board()?["gated"]
            .as_array()
            .cloned()
            .unwrap_or_default())
    };
    rig.wait_for(what, secs(60), || Ok(held()?.len() == 1))?;
    Ok(held()?.remove(0))
}

/// The human passes a message on at the gate, as the page does.
fn approve(rig: &Rig, id: &Value) -> Outcome {
    let passed = rig.page("message.approve", json!({ "message": id }))?;
    assert_eq!(passed["ok"], true, "{passed}");
    Ok(())
}

//! A harness dialog in a worker's window went unseen.
//!
//! Seen by the owner on 2026-10-26, on 3.0.6 (T-1183): Claude Code offered a worker's window
//! to install an LSP plugin mid-session ("Would you like to install this LSP plugin?", four
//! choices), and the chief never heard of it; nothing of it was in the daemon's log or events.
//! The report is `issues/a-harness-dialog-in-an-unwatched-window-goes-unseen.md` in
//! consensflow-sme. Claude Code 2.1.317 draws that offer when a turn writes a file of an
//! extension a marketplace has an LSP plugin for whose server is on the `PATH`, over its
//! spinner while the turn works, its status `busy` all along; it writes nothing of it in the
//! conversation, and closes it by itself after 30 s (`cargo xtask live harness-dialogs`,
//! 2026-11-03): only the screen shows it.
//!
//! The stand-in Claude draws the offer as that run saw it, as its turn works (`LSP-OFFER
//! <seconds>`, its `offers`), for the 30 s Claude Code gives it, then answers. What the owner
//! should have seen: the chief, who gave the task, and the human are told once, in words that
//! name the window, the question and its choices; and since T-1192 the offer, on Claude Code's
//! list of safe declines, is declined after that note ("No, not now", which writes nothing),
//! by its digit, and the note says so, so that no window leaves it to close by itself and
//! count against the owner's own offers. The task goes on to its result.
//!
//! The offers a launch can turn off never appear: the stand-in draws Claude Code's session
//! survey (`SURVEY`) unless its window's environment turns surveys off, as every window's
//! launch does now (`CLAUDE_CODE_DISABLE_FEEDBACK_SURVEY`).

use std::path::Path;

use cf_e2e::files;
use cf_e2e::rig::{Project, Rig};
use serde_json::{json, Value};

use crate::{config, secs, Outcome};

/// The question of the offer, as Claude Code words it.
const QUESTION: &str = "Would you like to install this LSP plugin?";

/// The roster of the case: the chief and the worker on the stand-in Claude.
fn write_roster(rig: &Rig) -> cf_e2e::Result {
    let roster = json!({
        "schemaVersion": 1,
        "agents": [
            { "id": "chief", "kind": "claude-code", "model": "fake-chief" },
            { "id": "worker", "kind": "claude-code", "model": "fake", "workTier": "standard" },
        ],
    });
    files::write(&rig.home().join("agents.json"), format!("{roster}\n"))
}

/// The notes ConsensFlow wrote `participant` of a dialog in a window.
fn dialog_notes(project: &Project, participant: &str) -> cf_e2e::Result<Vec<String>> {
    Ok(project
        .inbox(participant)?
        .into_iter()
        .filter(|m| m["kind"] == "note" && m.get("sender").is_none_or(Value::is_null))
        .filter_map(|m| m["body"].as_str().map(str::to_owned))
        .filter(|body| body.contains("'s window shows a dialog of its harness"))
        .collect())
}

/// T-1 for the worker, its brief `brief`, given by the chief; its session's handle.
fn dispatched(rig: &Rig, project: &Project, brief: &str) -> cf_e2e::Result<()> {
    let added = project.add_member("worker")?;
    let tier = added["member"]["tier"].as_str().unwrap_or("standard");
    rig.tell(project.id(), &format!("DISPATCH --tier {tier} {brief}"))
}

#[test]
fn an_offer_to_install_an_lsp_plugin_mid_turn_is_told_once_to_the_chief_and_the_human_then_declined(
) -> Outcome {
    let rig = Rig::start(config())?;
    write_roster(&rig)?;
    let project = Project::open(&rig, "chief", json!({}))?;
    dispatched(&rig, &project, "LSP-OFFER 30 Reply with exactly: DIALOG_OK")?;

    rig.wait_for("the chief to be told of the offer", secs(90), || {
        Ok(!dialog_notes(&project, "chief")?.is_empty())
    })?;
    rig.wait_for("T-1 to be done", secs(90), || {
        Ok(project.task(1)?["state"] == "done")
    })?;
    let task = project.task(1)?;
    let session = task["assignee"].as_str().unwrap_or_default().to_owned();
    assert!(session.starts_with("worker-"), "{task}");

    let told = dialog_notes(&project, "chief")?;
    assert_eq!(told.len(), 1, "{told:?}");
    let note = &told[0];
    assert!(
        note.starts_with(&format!(
            "@{session}'s window shows a dialog of its harness that waits for a key, not its prompt: \u{201c}{QUESTION}\u{201d}"
        )),
        "{note}"
    );
    for choice in [
        "Yes, install",
        "No, not now",
        "Never for this plugin",
        "Disable all LSP recommendations",
    ] {
        assert!(
            note.contains(&format!("\u{201c}{choice}\u{201d}")),
            "{note}"
        );
    }
    assert!(
        note.ends_with("ConsensFlow declines it for the window now, choosing \u{201c}No, not now\u{201d}, which writes nothing; nothing else is typed into the window meanwhile."),
        "{note}"
    );
    assert_eq!(dialog_notes(&project, "human")?, told);
    // The offer was answered by its digit, "No, not now", before its 30 s were up.
    let config = rig.var("CLAUDE_CONFIG_DIR").unwrap_or_default();
    let answers = files::read_string(&Path::new(config).join("offer-answers")).unwrap_or_default();
    assert_eq!(answers, "2\n", "how the offer was answered");
    rig.close()?;
    Ok(())
}

#[test]
fn a_survey_the_launch_turns_off_never_appears() -> Outcome {
    let rig = Rig::start(config())?;
    write_roster(&rig)?;
    let project = Project::open(&rig, "chief", json!({}))?;
    dispatched(&rig, &project, "SURVEY Reply with exactly: SURVEY_OK")?;
    rig.wait_for("T-1 to be done", secs(90), || {
        Ok(project.task(1)?["state"] == "done")
    })?;
    let config = rig.var("CLAUDE_CONFIG_DIR").unwrap_or_default();
    let drawn = files::read_string(&Path::new(config).join("surveys")).unwrap_or_default();
    assert_eq!(drawn, "", "the survey was drawn");
    assert_eq!(dialog_notes(&project, "chief")?, Vec::<String>::new());
    rig.close()?;
    Ok(())
}

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
    rig.stand_ins(&roster)
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

/// Seen by the owner on 2026-11-03 about 19:1x, on 3.0.7-alpha.1 (T-1198): a plugin of the
/// owner's (`cc-plugin-privacy`, which members keep) has a PreToolUse hook that answered "ask"
/// for a worker's Bash call; Claude Code asks then even in full permission, and the window
/// waited for good, nobody told. The stand-in Claude asks as Claude Code 2.1.317 drew it
/// (`HOOK-ASK <seconds>`, its `offers`), its status waiting on a permission prompt. What the
/// owner should have seen: the chief and the human told once, with the hook's own words, and
/// nothing pressed in the window before its ten minutes (T-1198: the chief's decision).
#[test]
fn a_hooks_ask_mid_turn_is_told_once_with_the_hooks_own_words_and_nothing_is_pressed() -> Outcome {
    let rig = Rig::start(config())?;
    write_roster(&rig)?;
    let project = Project::open(&rig, "chief", json!({}))?;
    dispatched(&rig, &project, "HOOK-ASK 20 Reply with exactly: HOOK_OK")?;
    rig.wait_for("the chief to be told of the hook's ask", secs(90), || {
        Ok(!dialog_notes(&project, "chief")?.is_empty())
    })?;
    rig.wait_for("T-1 to be done", secs(90), || {
        Ok(project.task(1)?["state"] == "done")
    })?;
    let told = dialog_notes(&project, "chief")?;
    assert_eq!(told.len(), 1, "{told:?}");
    let note = &told[0];
    for words in [
        "\u{201c}Do you want to proceed?\u{201d}",
        "its choices \u{201c}Yes\u{201d}, \u{201c}No\u{201d}",
        "Hook tool.check requires confirmation for this command:",
        "If nobody asked to send it, go on without it.",
        "[plugin:cc-plugin-privacy]",
        "It is yours to answer in that window; if nobody does within 10 minutes, ConsensFlow answers \u{201c}No\u{201d} for it, which writes nothing, and tells it to go on without that call. It never answers yes.",
    ] {
        assert!(note.contains(words), "{words} in {note}");
    }
    assert_eq!(dialog_notes(&project, "human")?, told);
    let config = rig.var("CLAUDE_CONFIG_DIR").unwrap_or_default();
    let answers = files::read_string(&Path::new(config).join("offer-answers")).unwrap_or_default();
    assert_eq!(answers, "hook:timeout\n", "nothing was pressed in the ask");
    rig.close()?;
    Ok(())
}

/// The same ask, nobody answering it (T-1198, the chief's decision of 2026-11-03): once its time
/// is up (15 s here, `CONSENSFLOW_HOOK_ASK_MS`; ten minutes in the field) ConsensFlow answers
/// "No", once, which writes nothing and ends the worker's turn as Claude Code's "No" does; the
/// chief and the human hear it, and once the window rests the words that send it on go in, the
/// hook's reason with them, and nothing else: no note of a cut turn. Its next turn's answer is
/// the task's result.
#[test]
fn a_hooks_ask_nobody_answers_is_answered_no_and_the_worker_sent_on_to_its_result() -> Outcome {
    let rig = Rig::start(config().var("CONSENSFLOW_HOOK_ASK_MS", "15000"))?;
    write_roster(&rig)?;
    let project = Project::open(&rig, "chief", json!({}))?;
    dispatched(&rig, &project, "HOOK-ASK 600 Reply with exactly: HOOK_OK")?;
    rig.wait_for(
        "the chief to be told it answered the hook's ask",
        secs(120),
        || {
            Ok(project.inbox("chief")?.iter().any(|m| {
                m["body"]
                    .as_str()
                    .is_some_and(|body| body.starts_with("Nobody answered @worker"))
            }))
        },
    )?;
    rig.wait_for("T-1 to be done", secs(120), || {
        Ok(project.task(1)?["state"] == "done")
    })?;
    let task = project.task(1)?;
    let session = task["assignee"].as_str().unwrap_or_default().to_owned();
    let config = rig.var("CLAUDE_CONFIG_DIR").unwrap_or_default();
    let answers = files::read_string(&Path::new(config).join("offer-answers")).unwrap_or_default();
    assert_eq!(answers, "hook:2\n", "\"No\", by its digit, once");
    let pressed: Vec<String> = project
        .inbox("chief")?
        .into_iter()
        .filter_map(|m| m["body"].as_str().map(str::to_owned))
        .filter(|body| body.starts_with("Nobody answered @"))
        .collect();
    assert_eq!(
        pressed,
        [format!("Nobody answered @{session}'s hook prompt on its call in 15 seconds, so ConsensFlow answered \u{201c}No\u{201d} and told it to go on without that call.")]
    );
    // What went into the worker's window: its brief, and the words that sent it on, alone.
    let given: Vec<String> = project
        .inbox(&session)?
        .into_iter()
        .filter_map(|m| m["body"].as_str().map(str::to_owned))
        .collect();
    assert_eq!(given.len(), 2, "{given:?}");
    let sent_on = given
        .iter()
        .find(|body| body.starts_with("A hook asked for confirmation of your last call"))
        .unwrap_or_else(|| panic!("the words that sent it on: {given:?}"));
    assert!(
        given.iter().any(|body| body.starts_with("HOOK-ASK ")),
        "{given:?}"
    );
    assert!(
        sent_on.contains("The hook said: \u{201c}This call's input holds your account email address, which may leave this machine. If nobody asked to send it, go on without it.\u{201d}"),
        "{given:?}"
    );
    let notes: Vec<String> = ["chief", "human"]
        .iter()
        .map(|who| project.inbox(who))
        .collect::<cf_e2e::Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .filter_map(|m| m["body"].as_str().map(str::to_owned))
        .collect();
    assert!(
        notes
            .iter()
            .all(|body| !body.contains("cut") && !body.contains("stop")),
        "no note of a cut turn or of a stop: {notes:?}"
    );
    let result = task["messages"]
        .as_array()
        .and_then(|messages| messages.iter().find(|m| m["kind"] == "result"))
        .and_then(|m| m["body"].as_str())
        .unwrap_or_default()
        .to_owned();
    assert!(
        result.starts_with("noted: "),
        "its next turn's answer is the result: {task}"
    );
    rig.close()?;
    Ok(())
}

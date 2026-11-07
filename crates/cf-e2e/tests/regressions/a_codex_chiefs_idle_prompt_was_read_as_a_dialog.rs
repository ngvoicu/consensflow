//! A Codex chief at rest with a worker's question on its screen was read as a window in a
//! dialog of its harness, and held.
//!
//! Seen by the Mac release check of 3.0.7-alpha.2 (T-1178, 2026-11-05): the live matrix's
//! `question-ask-answer` with `chief=codex`, twice. The worker's question was delivered to the
//! chief, the chief answered it with `cf answer` and went idle, and the engine told the human and
//! the chief that "@chief's window shows a dialog of its harness that waits for a key", quoting
//! the worker's question as the dialog and Codex's own lines (`Worked for 5s • 16:00`, the
//! composer's "Ask Codex to do anything", its footer) as its choices. The chief was held as
//! waiting, so the worker's result stayed queued for good. The engine reads the pane host's
//! tail, which has no empty line: Codex's composer, its footer and the line over them were one
//! under the other in one column, as a list of choices is, and Codex has no prompt reader to rule
//! it out. The report is `issues/a-codex-idle-prompt-is-mistaken-for-a-harness-dialog.md` in
//! consensflow-sme.
//!
//! The chief is the stand-in Codex, which draws its screen as Codex draws it
//! (`CF_TEST_CODEX_SCREEN=drawn`: the message after its `›`, the answer after its `•`, `Worked
//! for`, the composer and its footer, a blank line between each); the worker is the stand-in
//! Claude, which asks through `cf ask`. What the owner should have seen: no note of a dialog to
//! the chief or the human, the chief's lane not waiting, and the worker's result delivered.

use cf_e2e::rig::{Project, Rig, OPEN};
use serde_json::{json, Value};

use crate::{config_with_codex, secs, Outcome};

/// What the worker asks, ending in a question mark.
const QUESTION: &str = "Which format should I use for my reply: JSON or text?";

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

#[test]
fn a_codex_chief_at_rest_with_a_workers_question_on_its_screen_is_not_held_for_a_dialog() -> Outcome
{
    let rig = Rig::start(config_with_codex().var("CF_TEST_CODEX_SCREEN", "drawn"))?;
    let roster = json!({
        "schemaVersion": 1,
        "agents": [
            { "id": "codex-chief", "kind": "codex", "model": "fake" },
            { "id": "worker", "kind": "claude-code", "model": "fake", "workTier": "standard" },
        ],
    });
    rig.stand_ins(&roster)?;
    let project = Project::open_with_staff(&rig, "codex-chief", None, &[("worker", "worker")])?;
    let tier = project.tiers()?.get("worker").cloned().unwrap_or_default();
    let chief = rig.open_frame(&format!("p{}-chief", project.id()), OPEN)?;
    // The chief hands out a task whose brief makes the worker ask its question.
    let ran = rig.cf_in_window(
        &chief,
        [
            "task".to_owned(),
            "add".to_owned(),
            "--tier".to_owned(),
            tier,
            format!("CF ask :: {QUESTION}"),
        ],
    )?;
    assert_eq!(ran.code, Some(0), "{ran}");
    let question = |project: &Project| -> cf_e2e::Result<Option<Value>> {
        Ok(project
            .inbox("chief")?
            .into_iter()
            .find(|message| message["kind"] == "question"))
    };
    rig.wait_for(
        "the worker's question to be typed into the chief",
        secs(120),
        || Ok(question(&project)?.is_some_and(|asked| asked["state"] == "delivered")),
    )?;
    let asked = question(&project)?.unwrap_or_default();
    // The chief's window shows the question and, under it, the composer and its footer: the
    // case holds nothing if the stand-in drew none of it.
    rig.wait_for(
        "the chief's window to be at rest on its composer",
        secs(60),
        || {
            let Some(shown) = rig.snapshot(&chief, 12) else {
                return Ok(false);
            };
            let tail: Vec<&str> = shown["tail"]
                .as_array()
                .map(|lines| lines.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            Ok(tail.iter().any(|line| line.contains(QUESTION))
                && tail
                    .last()
                    .is_some_and(|line| line.contains("? for shortcuts"))
                && tail
                    .iter()
                    .any(|line| line.starts_with("› Ask Codex to do anything"))
                && project.lane("chief")?["activity"]["state"] == "idle")
        },
    )?;
    // It stays so, and is told of no dialog, for as many looks as the engine needs to be sure of
    // one and as many seconds again (`dialogs::SURE_LOOKS`, `LOOK_EVERY_MS`).
    let until = std::time::Instant::now() + secs(30);
    while std::time::Instant::now() < until {
        for who in ["chief", "human"] {
            assert_eq!(dialog_notes(&project, who)?, Vec::<String>::new(), "{who}");
        }
        let lane = project.lane("chief")?;
        assert_ne!(lane["activity"]["state"], "waiting", "{lane}");
        std::thread::sleep(secs(1));
    }
    // The chief answers, and the worker's result reaches it: nothing was held.
    let number = asked["id"].as_i64().unwrap_or_default();
    let answered = rig.cf_in_window(
        &chief,
        [
            "answer".to_owned(),
            format!("m-{number}"),
            "JSON".to_owned(),
        ],
    )?;
    assert_eq!(answered.code, Some(0), "{answered}");
    rig.wait_for("T-1 to be done", secs(120), || {
        Ok(project.task(1)?["state"] == "done")
    })?;
    rig.wait_for("T-1's result to reach the chief", secs(120), || {
        Ok(project
            .inbox("chief")?
            .into_iter()
            .any(|message| message["kind"] == "result" && message["state"] == "delivered"))
    })?;
    for who in ["chief", "human"] {
        assert_eq!(dialog_notes(&project, who)?, Vec::<String>::new(), "{who}");
    }
    rig.close()?;
    Ok(())
}

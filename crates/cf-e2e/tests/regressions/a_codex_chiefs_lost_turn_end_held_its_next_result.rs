//! A Codex chief whose `turn/completed` its broker never heard was given nothing more
//! (T-842's P2, Calliope's review of `9aa34baf6`): the broker said the thread busy for
//! good, the window read at work, and the pacing typed nothing into it while its rollout
//! read the turn over. Once the rollout has read it over for the settle while the broker
//! says busy, the broker is asked to read the thread again (`thread/read`), Codex says it
//! is idle, and the next result goes in, once.
//!
//! The chief is the stand-in Codex, which leaves its first turn's end unsaid
//! (`CF_TEST_CODEX_DROP_COMPLETED=1`); the worker is the stand-in Claude.

use cf_e2e::files;
use cf_e2e::rig::{Project, Rig, OPEN};
use serde_json::{json, Value};

use crate::{config_with_codex, secs, Outcome};

/// The result for the task `number` in the chief's inbox.
fn result_for(project: &Project, number: i64) -> cf_e2e::Result<Option<Value>> {
    Ok(project
        .inbox("chief")?
        .into_iter()
        .find(|message| message["kind"] == "result" && message["taskNumber"] == number))
}

#[test]
fn a_codex_chief_whose_first_turn_end_was_never_heard_is_given_its_next_result_once() -> Outcome {
    let rig = Rig::start(config_with_codex().var("CF_TEST_CODEX_DROP_COMPLETED", "1"))?;
    let roster = json!({
        "schemaVersion": 1,
        "agents": [
            { "id": "codex-chief", "kind": "codex", "model": "fake" },
            { "id": "worker", "kind": "claude-code", "model": "fake", "workTier": "standard" },
        ],
    });
    files::write(&rig.home().join("agents.json"), format!("{roster}\n"))?;
    let project = Project::open_with_staff(&rig, "codex-chief", None, &[("worker", "worker")])?;
    let tier = project.tiers()?.get("worker").cloned().unwrap_or_default();
    let chief = rig.open_frame(&format!("p{}-chief", project.id()), OPEN)?;
    for (number, words) in [(1, "FIRST_OK"), (2, "SECOND_OK")] {
        let ran = rig.cf_in_window(
            &chief,
            &[
                "task",
                "add",
                "--tier",
                &tier,
                &format!("Reply with exactly: {words}"),
            ],
        )?;
        assert_eq!(ran.code, Some(0), "{ran}");
        rig.wait_for(
            &format!("T-{number}'s result to reach the chief"),
            secs(120),
            || {
                Ok(result_for(&project, number)?
                    .is_some_and(|result| result["state"] == "delivered"))
            },
        )?;
    }
    let second = result_for(&project, 2)?.unwrap_or_default();
    assert_eq!(second["attempts"], 1, "{second}");
    rig.close()?;
    Ok(())
}

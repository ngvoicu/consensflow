//! The agents of the run: one for each harness, on its cheap model, written to
//! the roster of the run's home before a project opens. Each is the chief of
//! one project and a member of others, so one row serves both.

use cf_e2e::live::Harness;
use cf_e2e::rig::Rig;
use cf_e2e::{files, Result};
use serde_json::{json, Value};

/// The tier every member of the run is of. A project has one member in each
/// seat, and a task names its seat's pool (a worker's, an advisor's, a
/// reviewer's), so the tier tells nothing apart; a tier that is not critical
/// asks for no purpose.
pub const TIER: &str = "standard";

/// The image designer, the catalog's own: a Codex agent whose image tool draws.
pub const DESIGNER: &str = "pygmalion";

/// The id of the agent that runs `harness`.
pub fn id(harness: Harness) -> String {
    format!("live-{}", harness.name())
}

/// What the human's rules call the cheap model of each harness, as the app's
/// catalog and the harness's own list of models both have it (checked on
/// 2026-10-11): Claude Code on Sonnet 5.5, OpenCode and Pi on Muse Spark 1.3
/// (OpenCode's free one, Pi's through OpenRouter, which is where the catalog
/// has it: OpenCode Go is out of ConsensFlow), Devin on SWE-2 at medium, Codex
/// on GPT-5.6 Luna.
fn model(harness: Harness) -> (&'static str, Option<&'static str>) {
    match harness {
        Harness::Claude => ("claude-sonnet-5-5", None),
        Harness::OpenCode => ("opencode/muse-spark-1.3-contributor-free", None),
        Harness::Pi => ("openrouter/meta/muse-spark-1.3", None),
        Harness::Devin => ("swe-2", Some("medium")),
        Harness::Codex => ("gpt-5.6-luna", None),
    }
}

/// The roster row of the agent that runs `harness`.
pub fn row(harness: Harness) -> Value {
    let (model, effort) = model(harness);
    let mut row = json!({
        "id": id(harness),
        "kind": harness.kind(),
        "model": model,
        "workTier": TIER,
    });
    if let (Some(effort), Some(fields)) = (effort, row.as_object_mut()) {
        fields.insert("effort".to_owned(), json!(effort));
    }
    row
}

/// Writes the roster of the rig's home: the agents of `harnesses`, over the
/// one the rig started with.
pub fn write_roster(rig: &Rig, harnesses: &[Harness]) -> Result {
    let roster = json!({
        "schemaVersion": 1,
        "agents": harnesses.iter().map(|harness| row(*harness)).collect::<Vec<_>>(),
    });
    files::write(&rig.home().join("agents.json"), format!("{roster}\n"))
}

/// What each harness calls its tool for asking the person it works for a
/// question, as a brief names it; Pi has none.
pub fn question_tool(harness: Harness) -> Option<&'static str> {
    match harness {
        Harness::Claude => Some("AskUserQuestion tool"),
        Harness::OpenCode => Some("question tool"),
        Harness::Codex => Some("request_user_input tool"),
        Harness::Devin => Some("ask_user_question tool"),
        Harness::Pi => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_harness_has_one_agent_of_its_own_kind_on_its_cheap_model() {
        let rows: Vec<Value> = Harness::ALL.into_iter().map(row).collect();
        let ids: Vec<&str> = rows.iter().map(|row| row["id"].as_str().unwrap()).collect();
        assert_eq!(
            ids,
            [
                "live-claude",
                "live-opencode",
                "live-pi",
                "live-devin",
                "live-codex"
            ]
        );
        let models: Vec<&str> = rows
            .iter()
            .map(|row| row["model"].as_str().unwrap())
            .collect();
        assert_eq!(
            models,
            [
                "claude-sonnet-5-5",
                "opencode/muse-spark-1.3-contributor-free",
                "openrouter/meta/muse-spark-1.3",
                "swe-2",
                "gpt-5.6-luna"
            ]
        );
        assert!(rows.iter().all(|row| row["workTier"] == TIER));
        assert_eq!(rows[0]["kind"], "claude-code");
        assert_eq!(rows[3]["effort"], "medium");
        assert!(rows[0].get("effort").is_none());
    }

    #[test]
    fn only_pi_has_no_tool_to_ask_a_question_with() {
        for harness in Harness::ALL {
            assert_eq!(
                question_tool(harness).is_none(),
                harness == Harness::Pi,
                "{harness:?}"
            );
        }
    }
}

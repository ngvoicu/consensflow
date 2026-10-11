//! The agents of a live test: one for each harness, on its cheap model, and the
//! one place that says which model that is. A live test spends real quota, so it
//! runs each harness on the cheapest model its human's rules allow; the approval
//! suite, the bench and the checks that open one window all read it here, and a
//! change of model is a change of this table.
//!
//! An agent is a row of the roster of the run's home ([`row`], [`write_roster`])
//! where the daemon opens a member's window, and the `agent` a window is opened
//! on without a daemon ([`agent`], which the test binary `harness-ask` is asked
//! the window's command line by).

use serde_json::{json, Value};

use super::machine::Harness;
use crate::rig::Rig;
use crate::{files, Result};

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
/// on GPT-5.6 Luna. The model, and the effort where it has one.
pub fn cheap(harness: Harness) -> (&'static str, Option<&'static str>) {
    match harness {
        Harness::Claude => ("claude-sonnet-5-5", None),
        Harness::OpenCode => ("opencode/muse-spark-1.3-contributor-free", None),
        Harness::Pi => ("openrouter/meta/muse-spark-1.3", None),
        Harness::Devin => ("swe-2", Some("medium")),
        Harness::Codex => ("gpt-5.6-luna", None),
    }
}

/// The agent that runs `harness` on its cheap model, as a window is asked for
/// (`kind`, `model` and, where it has one, `effort`).
pub fn agent(harness: Harness) -> Value {
    let (model, effort) = cheap(harness);
    let mut agent = json!({ "kind": harness.kind(), "model": model });
    if let (Some(effort), Some(fields)) = (effort, agent.as_object_mut()) {
        fields.insert("effort".to_owned(), json!(effort));
    }
    agent
}

/// The roster row of the agent that runs `harness`, of the work tier `tier`
/// (the daemon picks a member for a task by its tier: a task that names one
/// lands on the harness that has it).
pub fn row(harness: Harness, tier: &str) -> Value {
    let mut row = agent(harness);
    if let Some(fields) = row.as_object_mut() {
        fields.insert("id".to_owned(), json!(id(harness)));
        fields.insert("workTier".to_owned(), json!(tier));
    }
    row
}

/// Writes the roster of the rig's home: the agents `rows`, over the one the rig
/// started with.
pub fn write_roster(rig: &Rig, rows: &[Value]) -> Result {
    let roster = json!({ "schemaVersion": 1, "agents": rows });
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
        let rows: Vec<Value> = Harness::ALL
            .into_iter()
            .map(|harness| row(harness, "standard"))
            .collect();
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
        assert!(rows.iter().all(|row| row["workTier"] == "standard"));
        assert_eq!(rows[0]["kind"], "claude-code");
        assert_eq!(rows[3]["effort"], "medium");
        assert!(rows[0].get("effort").is_none());
    }

    #[test]
    fn a_window_is_asked_for_on_the_kind_and_model_of_the_row_and_none_of_its_roster_fields() {
        for harness in Harness::ALL {
            let agent = agent(harness);
            let row = row(harness, "light");
            assert_eq!(agent["kind"], row["kind"], "{harness:?}");
            assert_eq!(agent["model"], row["model"], "{harness:?}");
            assert_eq!(agent["effort"], row["effort"], "{harness:?}");
            assert!(agent.get("id").is_none() && agent.get("workTier").is_none());
            assert_eq!(row["workTier"], "light");
        }
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

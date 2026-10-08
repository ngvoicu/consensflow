//! The agents the human keeps: the list the screen offers (`GET /api/agents`),
//! and the routes that add, edit and remove an agent defined by hand and set
//! the human's choices about the roster, each through the roster's own
//! operations.
//!
//! Every write is followed by `onRosterChange`: the members' tiers follow the
//! agents' again, the page is told if one moved, and the dispatcher is woken.
//! A write that was refused is not followed by it, and one whose follow-up
//! failed has been made all the same, and says why it failed.

use cf_base::refusal::Refusal;
use cf_base::time::SystemClock;
use cf_catalog::efforts;
use cf_harness::detect::missing_harnesses;
use cf_proto::agents::Harness;
use serde_json::{json, Value};

use super::body::fields;
use super::Screens;
use crate::api::answer::Answer;
use crate::roster::offerable;

/// What a refusal says, which is all a screen answers with: its code and status
/// are the API's.
fn words(refusal: Refusal) -> String {
    refusal.message
}

impl Screens {
    /// `GET /api/agents`: the agents as the pickers offer them, the harnesses
    /// installed here, the efforts each accepts, and what the human chose.
    pub(super) fn list_agents(&self) -> Result<Answer, String> {
        let missing = missing_harnesses(&self.env);
        let roster = self.agents.roster();
        let agents = offerable(&roster.list().map_err(words)?, &missing)
            .map_err(|failed| failed.to_string())?;
        let installed: Vec<&str> = Harness::ALL
            .into_iter()
            .filter(|harness| !missing.contains(harness))
            .map(Harness::as_str)
            .collect();
        let efforts: serde_json::Map<String, Value> = Harness::ALL
            .into_iter()
            .map(|harness| (harness.as_str().to_owned(), json!(efforts(harness))))
            .collect();
        Ok(Answer::ok(json!({
            "agents": agents,
            "harnesses": installed,
            "efforts": efforts,
            "preferences": roster.preferences().map_err(words)?,
        })))
    }

    /// `POST /api/agents`: an agent defined by hand, 201 with it as the page lists it.
    pub(super) fn add_agent(&self, body: &Value) -> Result<Answer, String> {
        let input = fields(body, "name")?;
        let agent = self
            .agents
            .roster()
            .add(&input, &mut SystemClock)
            .map_err(words)?;
        (self.on_roster_change)()?;
        Ok(Answer::created(json!({ "agent": agent })))
    }

    /// `PATCH /api/agents/<name>`: the fields of an agent defined by hand that
    /// `body` names.
    pub(super) fn edit_agent(&self, name: &str, body: &Value) -> Result<Answer, String> {
        let patch = fields(body, "workTier")?;
        let agent = self
            .agents
            .roster()
            .edit(name, &patch, &mut SystemClock)
            .map_err(words)?;
        (self.on_roster_change)()?;
        Ok(Answer::ok(json!({ "agent": agent })))
    }

    /// `DELETE /api/agents/<name>`: an agent defined by hand is gone; 204.
    pub(super) fn remove_agent(&self, name: &str) -> Result<Answer, String> {
        self.agents.roster().remove(name).map_err(words)?;
        (self.on_roster_change)()?;
        Ok(Answer::nothing(204))
    }

    /// `POST /api/preferences`: what the human chose about the roster, as it is kept.
    pub(super) fn set_preferences(&self, body: &Value) -> Result<Answer, String> {
        let chosen = self
            .agents
            .roster()
            .set_preferences(Some(body))
            .map_err(words)?;
        (self.on_roster_change)()?;
        Ok(Answer::ok(json!({ "preferences": chosen })))
    }
}

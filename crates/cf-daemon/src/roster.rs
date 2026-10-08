//! The saved agents as the daemon reads them: the human's file over the
//! catalog, asked afresh at each use, as a launch and a page read it. This is
//! the engine's `Roster` seam, the agents' API's rows ([`AgentRows`]), and what
//! keeps a member's tier the tier of its agent.

use std::path::PathBuf;

use cf_base::refusal::Refusal;
use cf_catalog::{AgentRow, AgentView, Catalog, Harness, Roster};
use cf_engine::seams::SavedAgent;
use cf_ledger::{Ledger, TierChange};
use serde_json::Value;

use crate::api::context::AgentRows;

/// The agents of a home: the catalog's, and the human's own in the file.
pub struct Agents {
    catalog: Catalog,
    path: PathBuf,
}

impl Agents {
    /// The agents in the file at `path`, over `catalog`.
    pub fn new(catalog: Catalog, path: PathBuf) -> Self {
        Self { catalog, path }
    }

    /// The roster, over the file as it is now.
    pub fn roster(&self) -> Roster<'_> {
        Roster::new(&self.catalog, self.path.clone())
    }

    /// The agents file folded into the shape it keeps now, and every member's
    /// tier read again from its agent: what the daemon does at its start
    /// (`normalizeRoster`, then `followCatalog`). The words of why it could
    /// not, which stops no start: the agents screen says why, and the file
    /// waits for the human.
    pub fn normalize_and_follow(&self, ledger: &mut Ledger) -> Result<Vec<TierChange>, String> {
        self.roster()
            .normalize()
            .map_err(|refusal| refusal.message)?;
        self.follow(ledger)
    }

    /// Every member's tier becomes its agent's now (`followCatalog`): the
    /// members that changed. An agent the roster no longer has leaves its
    /// member's tier as it was.
    pub fn follow(&self, ledger: &mut Ledger) -> Result<Vec<TierChange>, String> {
        let agents = self.roster().list().map_err(|refusal| refusal.message)?;
        ledger
            .refresh_member_tiers(|name| {
                agents
                    .iter()
                    .find(|agent| agent.name.as_deref() == Some(name))
                    .map(|agent| agent.profile.work_tier.as_str().to_owned())
            })
            .map_err(|failed| failed.to_string())
    }
}

impl AgentRows for Agents {
    fn row(&self, agent: &str) -> Result<Option<AgentRow>, Refusal> {
        self.roster().agent_row(agent)
    }
}

/// What a launch and the chief's checks read of a saved agent.
impl cf_engine::seams::Roster for Agents {
    fn agent(&self, name: &str) -> Result<Option<SavedAgent>, Refusal> {
        Ok(self.roster().agent_row(name)?.map(|row| SavedAgent {
            model: row.model().map(str::to_owned),
            effort: row.effort().map(str::to_owned),
            thinking: row.thinking().map(str::to_owned),
            designer: row.is_designer(),
        }))
    }
}

#[cfg(test)]
mod tests;

/// The agents as the pickers offer them, for the page's `agents.list` and the
/// agents screen alike: one whose harness is not installed here is hidden, so
/// only the Harnesses screen shows that harness, where it is installed; so is
/// one saved for a harness this build does not run (Kimi, dropped), for no
/// window could open on it. Each is its view with `hidden` and `notInstalled`
/// said, in that order, as `{...agent, hidden: true, notInstalled: true}` says
/// them: a key the view already has keeps its place, a new one comes last.
pub fn offerable(agents: &[AgentView], missing: &[Harness]) -> serde_json::Result<Vec<Value>> {
    agents
        .iter()
        .map(|agent| {
            let mut view = serde_json::to_value(agent)?;
            let unavailable = agent.unsupported
                || agent
                    .harness
                    .as_deref()
                    .is_some_and(|word| missing.iter().any(|harness| harness.as_str() == word));
            if let (true, Value::Object(fields)) = (unavailable, &mut view) {
                fields.insert("hidden".to_owned(), Value::Bool(true));
                fields.insert("notInstalled".to_owned(), Value::Bool(true));
            }
            Ok(view)
        })
        .collect()
}

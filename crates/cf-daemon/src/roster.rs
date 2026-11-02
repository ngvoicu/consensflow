//! The saved agents as the daemon reads them: the human's file over the
//! catalog, asked afresh at each use, as a launch and a page read it. This is
//! the engine's `Roster` seam, the agents' API's rows ([`AgentRows`]), and what
//! keeps a member's tier the tier of its agent.

use std::path::PathBuf;

use cf_base::refusal::Refusal;
use cf_base::time::SystemClock;
use cf_catalog::{AgentRow, AgentView, Catalog, Harness, Roster, WorkTier};
use cf_engine::seams::SavedAgent;
use cf_ledger::{Ledger, TierChange};
use cf_proto::specs::AppSpecs;
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

    /// The catalog this build ships: what the presets' tier rules are read from.
    pub fn catalog(&self) -> &Catalog {
        &self.catalog
    }

    /// The saved agent a member picked by its model runs on (Add member and Edit member,
    /// T-1143): the human's own agent that runs `wanted` already, or a new one under a name
    /// made of the harness, the model and the effort (`claude-opus-5-5-high`), numbered
    /// where another of the human's agents has that name (`…-2`). The launch reads its
    /// model and effort from it, as it reads every member's; the tier the human chose is
    /// the agent's own tier too, where the agent is new. A catalog agent is never one:
    /// the catalog's agents are the presets'.
    pub fn member_agent(&self, wanted: &MemberAgent<'_>) -> Result<String, Refusal> {
        let roster = self.roster();
        let agents = roster.list()?;
        let runs_it = |agent: &&AgentView| {
            agent.custom
                && agent.harness.as_deref() == Some(wanted.harness.as_str())
                && agent.model.as_deref() == Some(wanted.model)
                && agent.effort.as_deref() == wanted.effort
                && agent.designer == wanted.designer
        };
        if let Some(name) = agents.iter().find(runs_it).and_then(|agent| agent.name.clone()) {
            return Ok(name);
        }
        let base = member_agent_name(wanted.harness, wanted.model, wanted.effort);
        let taken = |name: &str| {
            agents.iter().any(|agent| agent.name.as_deref() == Some(name))
                || self.catalog.presets().iter().any(|preset| preset.id == name)
        };
        let name = std::iter::once(base.clone())
            .chain((2..).map(|n| format!("{base}-{n}")))
            .find(|name| !taken(name))
            .unwrap_or(base);
        let mut input = serde_json::Map::new();
        input.insert("name".to_owned(), Value::from(name.as_str()));
        input.insert("harness".to_owned(), Value::from(wanted.harness.as_str()));
        input.insert("model".to_owned(), Value::from(wanted.model));
        if wanted.designer {
            input.insert("designer".to_owned(), Value::Bool(true));
        }
        if let Some(effort) = wanted.effort {
            input.insert("effort".to_owned(), Value::from(effort));
        }
        input.insert("workTier".to_owned(), Value::from(wanted.tier.as_str()));
        roster.add(&input, &mut SystemClock)?;
        Ok(name)
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

/// What a member picked by its model runs on (T-1143): the model as its launch passes it
/// (Devin's exact `model_uid` for the effort, never joined; Pi's `provider/id`), the effort
/// a launch passes beside it (none where the id holds it), whether it draws, and the tier
/// the human chose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemberAgent<'a> {
    pub harness: Harness,
    pub model: &'a str,
    pub effort: Option<&'a str>,
    pub designer: bool,
    pub tier: WorkTier,
}

/// A saved agent's name for `model` on `harness` at `effort`: the harness's name, then the
/// model's and the effort's words in lowercase letters and digits, joined by `-`, the
/// harness not said twice where the model starts with it (`claude-opus-5-5-high`, not
/// `claude-claude-…`; `pi-openrouter-anthropic-claude-fable-5-1-high`), cut to
/// [`MEMBER_AGENT_NAME_MAX`] characters.
pub fn member_agent_name(harness: Harness, model: &str, effort: Option<&str>) -> String {
    let mut words: Vec<String> = Vec::new();
    for part in std::iter::once(model).chain(effort) {
        words.extend(
            part.to_ascii_lowercase()
                .split(|c: char| !c.is_ascii_alphanumeric())
                .filter(|word| !word.is_empty())
                .map(str::to_owned),
        );
    }
    if words.first().map(String::as_str) != Some(harness.as_str()) {
        words.insert(0, harness.as_str().to_owned());
    }
    let mut name = words.join("-");
    // A member's handle is its agent's name, at most 64 characters: room is left for `-99`.
    name.truncate(MEMBER_AGENT_NAME_MAX);
    name.trim_end_matches('-').to_owned()
}

/// The longest name [`member_agent_name`] makes: a member's handle holds 64 characters, and a
/// number may follow.
const MEMBER_AGENT_NAME_MAX: usize = 60;

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

    /// The catalog's word for a ready-made agent it no longer carries: why it went.
    fn retired(&self, name: &str) -> Option<String> {
        self.catalog
            .retired(name)
            .map(|retired| retired.why.clone())
    }

    /// What the file keeps of specs for every project, read as it is now: off,
    /// naming no folder and no variant, where it says nothing, and where it
    /// cannot be read (a launch reads the saved agent from the same file, and is
    /// refused first).
    fn app_specs(&self) -> AppSpecs {
        self.roster().app_specs().unwrap_or_default()
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

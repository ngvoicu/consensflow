//! The saved agents as the page reads them (`membership`, `chiefOn`,
//! `lastStaffNow` and `agentGone` of `src/core/page.js`): each read goes to the
//! file as it is now, so an agent the human added a moment ago is there and one
//! they removed is gone. How the pickers offer them is the roster's
//! ([`crate::roster::offerable`]).

use cf_base::env::Env;
use cf_base::js;
use cf_catalog::{roster_path, AgentView, Catalog, Roster};
use cf_engine::{require_chief_agent, SwitchTo};
use cf_ledger::model::fits_role;
use cf_ledger::{Ledger, StaffMember};
use serde_json::{json, Map, Value};

use super::body::Said;
use crate::roster::Agents;

/// The agents of the home `env` names, over the catalog this build ships.
pub(super) fn saved(env: &Env) -> Result<Agents, Said> {
    let path = roster_path(env).ok_or_else(|| {
        Said::from("ConsensFlow has no folder to keep its things in: set CONSENSFLOW_HOME, or HOME")
    })?;
    let catalog = Catalog::bundled().map_err(|failed| failed.to_string())?;
    Ok(Agents::new(catalog, path))
}

/// A saved agent as the staff records it: its harness (in the word the roster
/// says it in, `claude-code`), whether it is an image agent, and its tier as
/// the roster has it now.
pub(super) struct Membership {
    pub(super) agent: String,
    pub(super) harness: String,
    pub(super) designer: bool,
    pub(super) tier: &'static str,
}

impl Membership {
    /// The member as the ledger takes it, with the roles the page gave (none
    /// when it gave none): `{roles, agent, harness, designer, tier}`.
    pub(super) fn with_roles(&self, roles: Option<&Value>) -> Value {
        let mut member = Map::new();
        if let Some(roles) = roles {
            member.insert("roles".to_owned(), roles.clone());
        }
        member.extend(self.fields());
        Value::Object(member)
    }

    /// `{agent, harness, designer, tier}`, in this order.
    fn fields(&self) -> Map<String, Value> {
        let mut fields = Map::new();
        fields.insert("agent".to_owned(), json!(self.agent));
        fields.insert("harness".to_owned(), json!(self.harness));
        fields.insert("designer".to_owned(), json!(self.designer));
        fields.insert("tier".to_owned(), json!(self.tier));
        fields
    }
}

/// `membership(agent, env)`: the agents are read first (`agents =
/// listAgents(env)` is a default parameter), then the row, which is read
/// again.
pub(super) fn membership(roster: &Roster<'_>, agent: Option<&Value>) -> Result<Membership, Said> {
    let agents = roster.list()?;
    membership_among(roster, agent, &agents)
}

/// `membership(agent, env, agents)`.
fn membership_among(
    roster: &Roster<'_>,
    agent: Option<&Value>,
    agents: &[AgentView],
) -> Result<Membership, Said> {
    // `String(name ?? '')`: the row is found by the name, one `@` off.
    let name = match agent {
        None | Some(Value::Null) => std::borrow::Cow::Borrowed(""),
        given => js::text(given),
    };
    let row = roster.agent_row(&name)?;
    // The list is searched by `===`: only text names an agent.
    let saved = agent.and_then(Value::as_str).and_then(|name| {
        agents
            .iter()
            .find(|view| view.name.as_deref() == Some(name))
    });
    let (Some(row), Some(saved)) = (row, saved) else {
        return Err(Said(format!(
            "no agent named {} in your agents",
            js::text(agent)
        )));
    };
    Ok(Membership {
        agent: js::text(agent).into_owned(),
        // A row with no kind has no harness: `undefined`, which no adapter opens.
        harness: row.kind().unwrap_or("undefined").to_owned(),
        designer: row.get("designer") == Some(&Value::Bool(true)),
        tier: saved.profile.work_tier.as_str(),
    })
}

/// A chief as the dispatcher takes it: the saved agent named, and the
/// harness it runs on (`chiefOn`).
pub(super) fn chief_on(roster: &Roster<'_>, agent: Option<&Value>) -> Result<SwitchTo, Said> {
    let named = require_chief_agent(agent.and_then(Value::as_str))?;
    let member = membership(roster, Some(&json!(named)))?;
    Ok(SwitchTo {
        harness: member.harness,
        agent: named.to_owned(),
    })
}

/// The last project's staff for a new one: the members still saved, as the
/// roster has them now, in the roles their agents fit. A role one held from
/// before an image designer had to be an image agent stays behind, and so
/// does a member left with none. Each is `{agent, harness, designer, tier,
/// roles}`.
pub(super) fn last_staff_now(ledger: &Ledger, roster: &Roster<'_>) -> Result<Vec<Value>, Said> {
    let agents = roster.list()?;
    let mut staff = Vec::new();
    for StaffMember { agent, roles, .. } in ledger.last_staff()? {
        let named = agent.as_ref().map(|agent| json!(agent));
        if !agents
            .iter()
            .any(|view| view.name.is_some() && view.name == agent)
        {
            continue;
        }
        let member = membership_among(roster, named.as_ref(), &agents)?;
        let fitting: Vec<&String> = roles
            .iter()
            .filter(|role| fits_role(member.designer, role))
            .collect();
        if fitting.is_empty() {
            continue;
        }
        let mut shown = member.fields();
        shown.insert("roles".to_owned(), json!(fitting));
        staff.push(Value::Object(shown));
    }
    Ok(staff)
}

/// Whether a saved agent is gone. While the agents file cannot be read, an
/// agent is unknown, not gone: the board still loads, and the Agents page says
/// what to fix.
pub(super) fn agent_gone(roster: &Roster<'_>, agent: &str) -> bool {
    matches!(roster.agent_row(agent), Ok(None))
}

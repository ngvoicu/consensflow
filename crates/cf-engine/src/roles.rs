//! The instructions each window the daemon opens starts with, one text per
//! role (`src/core/roles.js`, `src/skill.js`). The chief's is
//! `skill/core/chief.md`, with the work tiers and the staff it has filled in.
//! Every member's is the shared text of `skill/core/staff.md` with the role's
//! own parts, so a rule all members keep is written once.
//!
//! Both files are in the program, where Node read them at each launch.

mod member;
mod skill;

use std::path::PathBuf;

use cf_base::env::Env;
use cf_base::file::{read_file_sync, FileError};
use cf_catalog::{WorkTier, WORK_TIERS};
use cf_proto::ledger::ProjectView;

pub use skill::{team_table, work_tier_list};

/// `skill/core/chief.md`.
const CHIEF: &str = include_str!("../../../skill/core/chief.md");

/// The roles of the staff's members, who have one window a task.
const MEMBERS: [&str; 4] = ["advisor", "worker", "reviewer", "designer"];

/// Evals only: a chief measured without ConsensFlow's card gets this file's
/// text instead of all of it (tiers, staff and its cf too), so nothing tells
/// it the board exists. Nothing in the app sets it.
const CARD: &str = "CONSENSFLOW_EVAL_CHIEF_CARD";

/// A member of the staff as its chief reads it: its name, roles and work
/// tier, its sessions left out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaffRow {
    pub name: String,
    pub roles: Vec<String>,
    /// None for a tier no work tier is called: the ledger holds four, or none.
    pub work_tier: Option<WorkTier>,
}

/// Why a role has no text.
#[derive(Debug, thiserror::Error)]
pub enum RoleError {
    #[error("no role instructions for {0}")]
    Unknown(String),
    #[error("staff.md has no {slot} for the {role} role")]
    MissingSlot { role: String, slot: String },
    /// Node threw a TypeError reading a tier that is none of the four.
    #[error("{member} has no work tier to name in the staff table")]
    NoWorkTier { member: String },
    /// The card an eval named, said as Node's `readFileSync` said it.
    #[error("{0}")]
    Card(#[from] FileError),
}

/// A project's staff as its chief reads it: each member's name, roles and
/// tier, sessions left out.
pub fn staff_of(project: &ProjectView) -> Vec<StaffRow> {
    project
        .participants
        .iter()
        .filter(|member| {
            member.role != "chief" && member.agent.is_some() && member.member_id.is_none()
        })
        .map(|member| StaffRow {
            name: member.handle.clone(),
            roles: member.roles.clone(),
            work_tier: member.tier.as_deref().and_then(work_tier),
        })
        .collect()
}

/// The instructions a window of `role` starts with. `cf` is where this
/// window's `cf` is: a shell that re-reads the user's profile can find
/// another `cf` first (another ConsensFlow's, or the Cloud Foundry CLI: a
/// Devin worker's `cf` was the live app's older one, 2026-09-26), and the full
/// path always works. Without it the text says nothing of the kind.
///
/// Kept from Node: the chief's text is `chief.md` with the staff table put in
/// where JavaScript's `replace` would also read `$&` and the like in it. A
/// member's name is an agent id, and holds none.
pub fn role_instructions(
    env: &Env,
    role: &str,
    staff: &[StaffRow],
    cf: Option<&str>,
) -> Result<String, RoleError> {
    if role != "chief" && !MEMBERS.contains(&role) {
        return Err(RoleError::Unknown(role.to_owned()));
    }
    let here = cf.map_or_else(String::new, |cf| {
        format!(
            "\n## This window's cf\n\nHere `cf` is {cf}. If `cf` says a command is unknown, or answers as another program, another `cf` comes first on this shell's PATH: run {cf} instead.\n"
        )
    });
    if role != "chief" {
        return Ok(member::text(role)? + &here);
    }
    if let Some(card) = env.path(CARD) {
        // Node read its environment as UTF-8: a byte that is none was U+FFFD.
        let card = PathBuf::from(card.to_string_lossy().into_owned());
        return Ok(String::from_utf8_lossy(&read_file_sync(&card)?).into_owned());
    }
    let chief = CHIEF.replacen("{{tiers}}", &work_tier_list(), 1).replacen(
        "{{staff}}",
        &team_table(staff)?,
        1,
    );
    Ok(chief + &here)
}

/// The work tier a ledger's word names.
fn work_tier(word: &str) -> Option<WorkTier> {
    WORK_TIERS.into_iter().find(|tier| tier.as_str() == word)
}

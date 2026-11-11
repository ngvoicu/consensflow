//! The instructions each window the daemon opens starts with, one text per
//! role. The chief's is `skill/core/chief.md`, with the work tiers and the
//! staff it has filled in. Every member's is the shared text of
//! `skill/core/staff.md` with the role's own parts, so a rule all members keep
//! is written once.
//!
//! Both files are in the program, where Node read them at each launch. A chief
//! whose project has specs on has a section more ([`ProjectSpecs`]); no other
//! text has one. A project whose human wrote rules for its staff has them at the
//! end of every text of its windows, the chief's and every member's
//! ([`with_staff_rules`]). A harness that keeps the instructions a conversation
//! began with is given a text that changed since in a message of its own, which
//! says it in full (`role_change`). A chief whose window is open when its specs or the
//! staff's rules change is told in a note of the section that changed (`news`, and
//! `role_news` for when).

mod member;
pub(crate) mod news;
mod shells;
mod skill;
mod specs;

use std::path::PathBuf;

use cf_base::env::Env;
use cf_base::file::{read_file_sync, FileError};
use cf_proto::agents::{WorkTier, WORK_TIERS};
use cf_proto::cf_word::CfWord;
use cf_proto::ledger::ProjectView;

pub use skill::{team_table, work_tier_list};
pub use specs::ProjectSpecs;

/// `skill/core/chief.md`.
const CHIEF: &str = include_str!("../../../skill/core/chief.md");

/// Evals only: a chief measured without ConsensFlow's card gets this file's
/// text instead of all of it (tiers, staff and its cf too), so nothing tells
/// it the board exists. Nothing in the app sets it.
const CARD: &str = "CONSENSFLOW_EVAL_CHIEF_CARD";

/// A member of the staff as its chief reads it: its name, roles and work
/// tier, its sessions left out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaffRow {
    pub name: String,
    /// The model it runs on as its harness names it, with its effort (`Opus · high`),
    /// where its name does not say so already: a member is named by its model (T-1263),
    /// so the table says it once. A member on its harness's default, or one that needs an
    /// edit, has a name that holds no model, and this says what it runs on.
    pub model: Option<String>,
    pub roles: Vec<String>,
    /// None for a tier no work tier is called: the ledger holds four, or none. An image
    /// designer has none to be called, and its table says so.
    pub work_tier: Option<WorkTier>,
}

/// Why a role has no text.
#[derive(Debug, thiserror::Error)]
pub enum RoleError {
    #[error("no role instructions for {0}")]
    Unknown(String),
    #[error("staff.md has no {slot} for the {role} role")]
    MissingSlot { role: String, slot: String },
    #[error("shells.md has no {slot} for the {shell} shell")]
    MissingShell { shell: String, slot: String },
    /// Node threw a TypeError reading a tier that is none of the four.
    #[error("{member} has no work tier to name in the staff table")]
    NoWorkTier { member: String },
    /// The card an eval named, said as Node's `readFileSync` said it.
    #[error("{0}")]
    Card(#[from] FileError),
}

/// A project's staff as its chief reads it: each member's name, the model it runs on,
/// its roles and its tier, sessions left out, and a participant on no harness, which no
/// window could open on.
pub fn staff_of(project: &ProjectView) -> Vec<StaffRow> {
    project
        .participants
        .iter()
        .filter(|member| {
            member.role != "chief"
                && member.role != "human"
                && member.harness.is_some()
                && member.member_id.is_none()
        })
        .map(|member| StaffRow {
            name: member.handle.clone(),
            model: member
                .name_apart
                .then(|| cf_ledger::handles::runs_on(member)),
            roles: member.roles.clone(),
            // A member whose roles have no tier is said to have none: none of its tasks has one
            // (an image designer's: `--design`, no tier).
            work_tier: member
                .tier
                .as_deref()
                .and_then(work_tier)
                .filter(|_| !cf_ledger::roles::takes_no_tier(&member.roles)),
        })
        .collect()
}

/// The instructions a window of `role` starts with. `cf` is how a command of
/// this window starts, and every command the text teaches (`{{cf}}` in
/// `chief.md` and `staff.md`) starts with it: the window's own `cf` by its full
/// path, written as the window's shell takes it ([`CfWord`]). A shell that
/// re-reads the user's profile can find another `cf` first (another
/// ConsensFlow's, or the Cloud Foundry CLI: a Devin worker's `cf` was the live
/// app's older one, 2026-09-26), and no order of a `PATH` matters to a full
/// path. Where there is no path ([`CfWord::bare`]) the commands say `cf`, as a
/// shell finds it. The shell also decides the paragraphs on how a text is passed
/// to a command and how a removal is written (`shells.md`).
///
/// A chief whose project has specs on is given `specs`, and its text has the
/// section of them after the rest; a member's never does, whatever is given,
/// and a chief given none reads what it always did.
///
/// Kept from Node: the chief's text is `chief.md` with the staff table put in
/// where JavaScript's `replace` would also read `$&` and the like in it, which the
/// models' names a table holds may have: `replacen` reads none.
pub fn role_instructions(
    env: &Env,
    role: &str,
    staff: &[StaffRow],
    cf: &CfWord,
    specs: Option<&ProjectSpecs<'_>>,
) -> Result<String, RoleError> {
    // The staff's members, who have one window a task, hold the roles of the ledger's table.
    if role != "chief" && !cf_ledger::roles::is_role(role) {
        return Err(RoleError::Unknown(role.to_owned()));
    }
    if role != "chief" {
        return member::text(role, cf);
    }
    if let Some(card) = env.path(CARD) {
        // Node read its environment as UTF-8: a byte that is none was U+FFFD.
        let card = PathBuf::from(card.to_string_lossy().into_owned());
        return Ok(String::from_utf8_lossy(&read_file_sync(&card)?).into_owned());
    }
    // The `cf` goes in last, so nothing in a path is read as a slot.
    let chief = CHIEF
        .replacen(
            "{{chief_input}}",
            shells::paragraph(cf.shell(), "chief_input")?,
            1,
        )
        .replacen("{{tiers}}", &work_tier_list(), 1)
        .replacen("{{staff}}", &team_table(staff)?, 1)
        .replace("{{cf}}", cf.as_str());
    let specs = specs.map_or_else(String::new, ProjectSpecs::section);
    Ok(chief + &specs)
}

/// The heading of the project's rules for its staff, in every text that has them.
pub use cf_harness::contract::STAFF_RULES_HEADING;

/// What a member's text says before the project's rules for its staff.
const STAFF_RULES_FOR_A_MEMBER: &str = "Every member of this project's staff is given these rules after its role, as you are. Follow them in every task, as you follow your role.";

/// What the chief's text says before the same rules.
const STAFF_RULES_FOR_THE_CHIEF: &str = "Every window of your staff opens with these rules in its instructions, after its role. They are the human's, written in Project settings: to change one, propose it to the human here.";

/// `instructions`, the text a window of `role` starts with, with the project's
/// rules for its staff after it, under their own heading, where the human wrote
/// some: a member is told to follow them, and the chief that every member has
/// them. The rules are the human's own words, as they are. Where there are none
/// the text is what it was, to the byte. Every window's text is given them here,
/// at its launch, whatever its harness (`Dispatcher::role_text`), so a text
/// that says a role again (`role_change`) says them too.
pub fn with_staff_rules(instructions: String, role: &str, rules: Option<&str>) -> String {
    let Some(rules) = rules else {
        return instructions;
    };
    let said = if role == "chief" {
        STAFF_RULES_FOR_THE_CHIEF
    } else {
        STAFF_RULES_FOR_A_MEMBER
    };
    format!("{instructions}\n{STAFF_RULES_HEADING}\n\n{said}\n\n{rules}\n")
}

/// The work tier a ledger's word names.
fn work_tier(word: &str) -> Option<WorkTier> {
    WORK_TIERS.into_iter().find(|tier| tier.as_str() == word)
}

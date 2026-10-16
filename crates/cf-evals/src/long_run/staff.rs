//! The staff of a long run: every harness asked for that this machine has, each on
//! the models it has here, not only its cheap one (the owner's word of 2026-10-15:
//! "when you test with all harnesses use whatever they have there"), and the ones
//! left out, each with why. A harness not on the `PATH` is left out, and so is an
//! OpenCode of 2 or more, which ConsensFlow refuses until it supports it (OpenCode
//! stays on 1.x).
//!
//! Each model of a harness has a worker of its own, and the first has the advisor
//! and the reviewer too. The tasks reach a member by its work tier, so each harness
//! has one that the ask can name: Claude Code's members are `complex` (the dev
//! server started with `run_in_background` is Claude Code's), Devin's `light` (the
//! long command Devin moves to the background itself), and the others' `standard`.
//! A tier nobody holds goes to the nearest one somebody does (the ledger's rule), so
//! a staff without one of them still gets every task.

use std::path::PathBuf;

use super::mask::{mask, Names};
use crate::plan::{harness, Agent, Member, Staff};
use crate::{Error, Result};

/// What each harness's members run on unless `--models` says otherwise: models the
/// harness has here, a strong one beside its cheap one (the catalog's presets and
/// the evals' own cheap models).
pub const MODELS: [(&str, &[&str]); 5] = [
    ("claude", &["claude-sonnet-5-5", "claude-opus-5-5"]),
    ("codex", &["gpt-5.6-luna", "gpt-6.1-sol"]),
    (
        "pi",
        &[
            "openrouter/deepseek/deepseek-v4.1-flash",
            "openai-codex/gpt-6-astra",
        ],
    ),
    (
        "opencode",
        &[
            "openrouter/deepseek/deepseek-v4.1-flash",
            "openrouter/openai/gpt-6-astra",
        ],
    ),
    ("devin", &["swe-2-medium", "claude-sonnet-5-5"]),
];

/// The work tier of a harness's members, which the ask gives its tasks by.
pub fn tier_of(name: &str) -> &'static str {
    match name {
        "claude" => "complex",
        "devin" => "light",
        _ => "standard",
    }
}

/// The models each harness runs on, by its name.
pub type Models = Vec<(String, Vec<String>)>;

/// The models of `--models`: `claude=a+b,devin=c`, each harness named the models
/// it runs, the rest on [`MODELS`].
pub fn models_of(said: Option<&str>) -> Result<Models> {
    let mut models: Models = MODELS
        .iter()
        .map(|(name, models)| {
            (
                (*name).to_owned(),
                models.iter().map(|model| (*model).to_owned()).collect(),
            )
        })
        .collect();
    for part in said
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
    {
        let Some((name, list)) = part.split_once('=') else {
            return Err(Error::Usage(format!(
                "--models names a harness and its models: claude=a+b, not {part}"
            )));
        };
        harness(name.trim()).map_err(|_| Error::Usage(format!("no such harness: {name}")))?;
        let list: Vec<String> = list
            .split('+')
            .map(str::trim)
            .filter(|model| !model.is_empty())
            .map(str::to_owned)
            .collect();
        if list.is_empty() {
            return Err(Error::Usage(format!("--models gives {name} no model")));
        }
        if let Some((_, models)) = models.iter_mut().find(|(known, _)| known == name.trim()) {
            *models = list;
        }
    }
    Ok(models)
}

/// A harness of the run, found on this machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub name: String,
    pub command: PathBuf,
    pub version: String,
}

/// A harness asked for that the run left out, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeftOut {
    pub name: String,
    pub why: String,
}

/// The harnesses of `wanted` that `look` finds (its program and its version, or
/// why there is none), and those left out. What a harness says of its version, or
/// why it could not be found, is its own words: what looks like a key or a token in
/// them is hidden before they are kept or said.
pub fn find(
    wanted: &[String],
    look: impl Fn(&str) -> std::result::Result<(PathBuf, String), String>,
) -> (Vec<Found>, Vec<LeftOut>) {
    let (mut found, mut left) = (Vec::new(), Vec::new());
    let masked = |said: &str| mask(said, &Names::default());
    for name in wanted {
        match look(name).map(|(command, version)| (command, masked(&version))) {
            Err(why) => left.push(LeftOut {
                name: name.clone(),
                why: format!("not on this machine: {}", masked(&why)),
            }),
            Ok((_, version)) if name == "opencode" && major(&version) >= Some(2) => {
                left.push(LeftOut {
                    name: name.clone(),
                    why: format!(
                        "OpenCode {version} is 2.x, which ConsensFlow refuses (OpenCode stays on 1.x)"
                    ),
                });
            }
            Ok((command, version)) => found.push(Found {
                name: name.clone(),
                command,
                version,
            }),
        }
    }
    (found, left)
}

/// The major version a harness's version line says: the first number in it.
fn major(version: &str) -> Option<u64> {
    version
        .split(|at: char| !at.is_ascii_digit())
        .find(|part| !part.is_empty())
        .and_then(|part| part.parse().ok())
}

/// The roster rows and the project staff of the harnesses `names`: for each, a
/// worker on each of its models, and an advisor and a reviewer on the first, all of
/// the harness's tier, at the reasoning level `effort`.
pub fn staff_of(names: &[String], models: &Models, effort: &str) -> Result<Staff> {
    let mut staff = Staff {
        agents: Vec::new(),
        members: Vec::new(),
    };
    for name in names {
        let kind = harness(name)?;
        let models = models
            .iter()
            .find(|(known, _)| known == name)
            .map(|(_, models)| models.clone())
            .unwrap_or_else(|| vec![kind.model.to_owned()]);
        let mut roles: Vec<(String, &'static str, &str)> = models
            .iter()
            .enumerate()
            .map(|(at, model)| (format!("long-{name}-{}", at + 1), "worker", model.as_str()))
            .collect();
        let first = models.first().map_or(kind.model, String::as_str);
        roles.push((format!("long-{name}-advisor"), "advisor", first));
        roles.push((format!("long-{name}-reviewer"), "reviewer", first));
        for (id, role, model) in roles {
            staff.members.push(Member {
                agent: id.clone(),
                role,
            });
            staff
                .agents
                .push(Agent::new(id, kind, model, Some(effort)).with_tier(tier_of(name)));
        }
    }
    Ok(staff)
}

#[cfg(test)]
mod tests;

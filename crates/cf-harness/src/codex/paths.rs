//! Where Codex keeps its things.

use std::path::PathBuf;

use cf_base::env::Env;
use cf_base::path;
use cf_proto::specs::Variant;

use crate::shared::paths::agents_skills;
use crate::shared::record::find::{find_file, DEPTH};
use crate::shared::record::home;
use crate::shared::specmint;

/// Codex's home: `CODEX_HOME`, an empty one too, else `.codex` in the user's
/// home.
pub(crate) fn codex_home(env: &Env) -> Result<String, String> {
    match env.os("CODEX_HOME") {
        Some(root) => Ok(root.to_string_lossy().into_owned()),
        None => Ok(path::join(&[&home(env)?, ".codex"])),
    }
}

/// The `SKILL.md` of the Specmint variant the human installed for Codex, in the
/// places it reads the skills of the user's: `.agents/skills` in the home, then
/// the `skills` folder of its own home. None where it is in neither.
pub(crate) fn global_specmint(env: &Env, variant: Variant) -> Option<String> {
    let roots: Vec<String> = [
        agents_skills(env),
        codex_home(env).map(|home| path::join(&[&home, "skills"])),
    ]
    .into_iter()
    .flatten()
    .collect();
    specmint::installed(&roots, variant)
}

/// The folder of Codex's `sessions`, which holds a rollout for each thread:
/// `sessions/YYYY/MM/DD/rollout-….jsonl`.
pub(crate) fn sessions(env: &Env) -> Result<PathBuf, String> {
    Ok(PathBuf::from(path::join(&[&codex_home(env)?, "sessions"])))
}

/// Where Codex keeps the rollout of the thread `session`: the first file
/// under its `sessions` folder whose name holds the session, or none.
pub(crate) fn transcript(session: &str, env: &Env) -> Result<Option<PathBuf>, String> {
    Ok(find_file(
        &sessions(env)?,
        &|name| name.contains(session),
        DEPTH,
    ))
}

#[cfg(test)]
mod tests;

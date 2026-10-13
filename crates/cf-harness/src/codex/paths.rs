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
/// `sessions/YYYY/MM/DD/rollout-<timestamp>-<thread>.jsonl`.
pub(crate) fn sessions(env: &Env) -> Result<PathBuf, String> {
    Ok(PathBuf::from(path::join(&[&codex_home(env)?, "sessions"])))
}

/// Where Codex keeps the rollout of the thread `session`: the first file
/// under its `sessions` folder whose name holds the session, or none. The
/// record is read from it, as Node's was.
pub(crate) fn transcript(session: &str, env: &Env) -> Result<Option<PathBuf>, String> {
    Ok(find_file(
        &sessions(env)?,
        &|name| name.contains(session),
        DEPTH,
    ))
}

/// Whether Codex kept the rollout of the thread `session`: a file under its
/// `sessions` folder that is named as Codex names it, which is the question of
/// whether a thread can be resumed. A copy kept beside it (`….jsonl.bak`), or a
/// rollout of another thread whose id ends as this one does, is not it, though
/// [`transcript`] would find either. The file is found and not read: an empty or
/// corrupt rollout is kept all the same.
pub(crate) fn rollout_kept(session: &str, env: &Env) -> Result<bool, String> {
    Ok(find_file(&sessions(env)?, &|name| is_rollout_of(name, session), DEPTH).is_some())
}

/// Whether `name` is what Codex names the rollout of the thread `session`:
/// `rollout-<timestamp>-<session>.jsonl`.
fn is_rollout_of(name: &str, session: &str) -> bool {
    name.strip_prefix("rollout-")
        .and_then(|rest| rest.strip_suffix(".jsonl"))
        .and_then(|rest| rest.strip_suffix(session))
        .and_then(|rest| rest.strip_suffix('-'))
        .is_some_and(is_timestamp)
}

/// A timestamp as Codex writes it in a rollout's name: `2026-10-05T01-00-00`.
fn is_timestamp(text: &str) -> bool {
    const SHAPE: &[u8] = b"dddd-dd-ddTdd-dd-dd";
    text.len() == SHAPE.len()
        && text.bytes().zip(SHAPE).all(|(found, &shape)| match shape {
            b'd' => found.is_ascii_digit(),
            literal => found == literal,
        })
}

#[cfg(test)]
mod tests;

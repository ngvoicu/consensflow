//! Where Claude Code keeps its things.

use std::path::{Path, PathBuf};

use cf_base::env::Env;
use cf_base::path;
use cf_proto::specs::Variant;

use crate::shared::record::find::{find_file, DEPTH};
use crate::shared::record::home;
use crate::shared::specmint;

/// Claude Code's config folder: `CLAUDE_CONFIG_DIR`, an empty one too, else
/// `.claude` in the user's home.
fn config_folder(env: &Env) -> Result<String, String> {
    match env.os("CLAUDE_CONFIG_DIR") {
        Some(root) => Ok(root.to_string_lossy().into_owned()),
        None => Ok(path::join(&[&home(env)?, ".claude"])),
    }
}

/// The `SKILL.md` of the Specmint variant the human installed for Claude Code,
/// in the `skills` folder of its config folder: none where it is not there, or
/// where there is no config folder to look in.
pub(crate) fn global_specmint(env: &Env, variant: Variant) -> Option<String> {
    let skills = path::join(&[&config_folder(env).ok()?, "skills"]);
    specmint::installed(&[skills], variant)
}

/// Where Claude Code keeps the transcript of the session `session`: the
/// first file under the `projects` folder of its config folder named
/// `<session>.jsonl`, or none.
pub(crate) fn transcript(session: &str, env: &Env) -> Result<Option<PathBuf>, String> {
    let root = config_folder(env)?;
    let projects = path::join(&[&root, "projects"]);
    let file = format!("{session}.jsonl");
    Ok(find_file(Path::new(&projects), &|name| name == file, DEPTH))
}

#[cfg(test)]
mod tests;

//! Where Claude Code keeps its things.

use std::path::{Path, PathBuf};

use cf_base::env::Env;
use cf_base::path;

use crate::shared::record::find::{find_file, DEPTH};
use crate::shared::record::home;

/// Where Claude Code keeps the transcript of the session `session`: the
/// first file under the `projects` folder of its config folder named
/// `<session>.jsonl`, or none. Its config folder is `CLAUDE_CONFIG_DIR`, an
/// empty one too, else `.claude` in the user's home.
pub(crate) fn transcript(session: &str, env: &Env) -> Result<Option<PathBuf>, String> {
    let root = match env.os("CLAUDE_CONFIG_DIR") {
        Some(root) => root.to_string_lossy().into_owned(),
        None => path::join(&[&home(env)?, ".claude"]),
    };
    let projects = path::join(&[&root, "projects"]);
    let file = format!("{session}.jsonl");
    Ok(find_file(Path::new(&projects), &|name| name == file, DEPTH))
}

#[cfg(test)]
mod tests;

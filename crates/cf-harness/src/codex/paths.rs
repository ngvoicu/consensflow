//! Where Codex keeps its things.

use std::path::{Path, PathBuf};

use cf_base::env::Env;
use cf_base::path;

use crate::shared::record::find::{find_file, DEPTH};
use crate::shared::record::home;

/// Where Codex keeps the rollout of the thread `session`: the first file
/// under its `sessions` folder whose name holds the session, or none. Its
/// home is `CODEX_HOME`, an empty one too, else `.codex` in the user's home.
pub(crate) fn transcript(session: &str, env: &Env) -> Result<Option<PathBuf>, String> {
    let root = match env.os("CODEX_HOME") {
        Some(root) => root.to_string_lossy().into_owned(),
        None => path::join(&[&home(env)?, ".codex"]),
    };
    let sessions = path::join(&[&root, "sessions"]);
    Ok(find_file(
        Path::new(&sessions),
        &|name| name.contains(session),
        DEPTH,
    ))
}

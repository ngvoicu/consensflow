//! Where Pi keeps its things.
//!
//! The home is `shared::paths::home`.

use std::path::{Path, PathBuf};

use cf_base::env::Env;
use cf_base::file::read_file;
use cf_base::json::from_slice_lossy;
use cf_base::path;
use cf_proto::specs::Variant;

use crate::shared::paths::{agents_skills, home, set};
use crate::shared::record::find::{find_file, DEPTH};
use crate::shared::specmint;

/// Where Pi keeps the file of the session `session`: the first file under
/// its sessions folder whose name holds the session, or none.
pub(crate) fn transcript(session: &str, env: &Env) -> Result<Option<PathBuf>, String> {
    let sessions = session_dir(env)?;
    Ok(find_file(
        Path::new(&sessions),
        &|name| name.contains(session),
        DEPTH,
    ))
}

/// Where Pi keeps its sessions (`piSessionDir`): `PI_CODING_AGENT_SESSION_DIR`,
/// else the `sessionDir` of the user's settings, else `sessions` in the agent
/// folder. An empty variable is none.
///
/// Pi also reads `sessionDir` from the project's `.pi/settings.json`, and takes
/// a relative one from its working folder: a look from here knows neither
/// folder, so those are not followed (T-549).
fn session_dir(env: &Env) -> Result<String, String> {
    if let Some(configured) = set(env, "PI_CODING_AGENT_SESSION_DIR") {
        return expand(&configured, env);
    }
    let agent = agent_dir(env)?;
    if let Some(configured) = settings_session_dir(&agent, env)? {
        return Ok(configured);
    }
    expand(&path::join(&[&agent, "sessions"]), env)
}

/// The `sessionDir` of the user's own settings (`settings.json` in the agent
/// folder), `~` expanded: none where the file, the key or a whole path is
/// missing.
fn settings_session_dir(agent: &str, env: &Env) -> Result<Option<String>, String> {
    let file = path::join(&[agent, "settings.json"]);
    let Ok(bytes) = read_file(Path::new(&file)) else {
        return Ok(None);
    };
    let Some(configured) = from_slice_lossy(&bytes).ok().and_then(|settings| {
        let written = settings.get("sessionDir")?.as_str()?;
        (!written.is_empty()).then(|| written.to_owned())
    }) else {
        return Ok(None);
    };
    let expanded = expand(&configured, env)?;
    Ok(Path::new(&expanded).is_absolute().then_some(expanded))
}

/// The file Pi appends to its system prompt by itself
/// (`discoverAppendSystemPromptFile`), which it reads only where it is given no
/// `--append-system-prompt`: `.pi/APPEND_SYSTEM.md` in the working folder of a
/// trusted project (every window is opened `--approve`), else
/// `APPEND_SYSTEM.md` in the agent folder. None where neither is there. `.pi`
/// is Pi's `piConfig.configDir`.
pub(crate) fn append_system_prompt(env: &Env, directory: &str) -> Option<String> {
    let project = path::join(&[directory, ".pi", "APPEND_SYSTEM.md"]);
    let user = || Some(path::join(&[&agent_dir(env).ok()?, "APPEND_SYSTEM.md"]));
    [Some(project), user()]
        .into_iter()
        .flatten()
        .find(|file| Path::new(file).exists())
}

/// Pi's folder (`piAgentDir`): `PI_CODING_AGENT_DIR`, else `.pi/agent` in the
/// home. An empty variable is none.
pub(crate) fn agent_dir(env: &Env) -> Result<String, String> {
    let configured = match set(env, "PI_CODING_AGENT_DIR") {
        Some(configured) => configured,
        None => path::join(&[&home(env)?, ".pi", "agent"]),
    };
    expand(&configured, env)
}

/// The `SKILL.md` of the Specmint variant the human installed for Pi, in the
/// places it reads the skills of the user's: the `skills` folder of its own
/// folder, then `.agents/skills` in the home. None where it is in neither.
pub(crate) fn global_specmint(env: &Env, variant: Variant) -> Option<String> {
    let mut roots = Vec::new();
    roots.extend(agent_dir(env).ok().map(|dir| path::join(&[&dir, "skills"])));
    roots.extend(agents_skills(env).ok());
    specmint::installed(&roots, variant)
}

/// A configured path with its `~` expanded (`piPath`): `~` is the home, and
/// `~/` and, on Windows, `~\` begin a path under it.
fn expand(configured: &str, env: &Env) -> Result<String, String> {
    if configured == "~" {
        return home(env);
    }
    if let Some(under) = configured.strip_prefix("~/") {
        return Ok(path::join(&[&home(env)?, under]));
    }
    if cfg!(windows) {
        if let Some(under) = configured.strip_prefix("~\\") {
            return Ok(path::join(&[&home(env)?, under]));
        }
    }
    Ok(configured.to_owned())
}

#[cfg(test)]
mod tests;

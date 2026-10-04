//! Where Pi keeps its things (`piSessionDir`, `piPath` and `piAgentDir`,
//! `src/harnesses.js`, and `piTranscript`, `hosts/lib/completion/pi.js`).
//!
//! Kept from Node on purpose: Node's `home(env)` there is `HOME`, else
//! `USERPROFILE`, else `os.homedir()`, which reads the process's own
//! environment. A Rust module may not, so an environment with neither fails
//! the locator, with `missing home in env`, the sentence of
//! `shared::record::home`. An empty `HOME` is still a home, as `??` took it,
//! where that function takes it for none.

use std::path::{Path, PathBuf};

use cf_base::env::Env;
use cf_base::path;

use crate::shared::record::find::{find_file, DEPTH};

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
/// else `sessions` in the agent folder. An empty variable is none.
fn session_dir(env: &Env) -> Result<String, String> {
    let configured = match set(env, "PI_CODING_AGENT_SESSION_DIR") {
        Some(configured) => configured,
        None => path::join(&[&agent_dir(env)?, "sessions"]),
    };
    expand(&configured, env)
}

/// Pi's folder (`piAgentDir`): `PI_CODING_AGENT_DIR`, else `.pi/agent` in the
/// home. An empty variable is none.
fn agent_dir(env: &Env) -> Result<String, String> {
    let configured = match set(env, "PI_CODING_AGENT_DIR") {
        Some(configured) => configured,
        None => path::join(&[&home(env)?, ".pi", "agent"]),
    };
    expand(&configured, env)
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

/// A variable set to something, as `env.NAME ||` takes it, read as Node read
/// its environment: bytes that are no UTF-8 as U+FFFD.
fn set(env: &Env, name: &str) -> Option<String> {
    env.path(name)
        .map(|value| value.to_string_lossy().into_owned())
}

/// The user's home (`home`, `src/harnesses.js`): `HOME`, else `USERPROFILE`,
/// whichever is set, empty or not.
fn home(env: &Env) -> Result<String, String> {
    env.os("HOME")
        .or_else(|| env.os("USERPROFILE"))
        .map(|home| home.to_string_lossy().into_owned())
        .ok_or_else(|| "missing home in env".to_owned())
}

#[cfg(test)]
mod tests;

//! How an OpenCode window is opened on its channel (the `opencode` branch of
//! `launchConfiguration`, `src/channels.js`): the port its server listens on
//! and the password it asks for, the plugin's own port and token, and the
//! arguments and environment that tell the window of them.
//!
//! Kept from Node on purpose: the workspace is only checked for being
//! something. Node also made it whole against the working folder
//! (`path.resolve`), which fails only where that folder is gone, and used it
//! for no harness but Codex.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use cf_base::env::Env;
use cf_base::js;
use serde_json::json;

use super::{Bridge, Channel};
use crate::contract::LaunchId;
use crate::seams::{Entropy, Ports};
use crate::shared::paths::set;

/// How an OpenCode window is opened on its launch's channel.
#[derive(Debug, Clone)]
pub(crate) struct Launched {
    pub(crate) args: Vec<String>,
    /// The environment the window is told of its channel by, in the order
    /// JavaScript built it.
    pub(crate) env: Vec<(String, String)>,
    pub(crate) channel: Channel,
}

/// The arguments and environment that open an OpenCode window on a server of
/// its own and tell its plugin, from `settings`, where to answer: two free
/// ports, drawn in that order with a token for each. A window that runs
/// under a settings file of the human's own is refused rather than
/// overridden.
pub(crate) fn launch_configuration(
    env: &Env,
    ports: &dyn Ports,
    entropy: &dyn Entropy,
    launch: &LaunchId,
    workspace: &str,
    settings: &str,
) -> Result<Launched, String> {
    if workspace.is_empty() {
        return Err("launch configuration needs a workspace".to_owned());
    }
    if set(env, "OPENCODE_TUI_CONFIG").is_some() {
        return Err("OpenCode has a custom OPENCODE_TUI_CONFIG; its settings were preserved. Remove that launch override to enable ConsensFlow reply delivery.".to_owned());
    }
    let bridge_port = ports.free_loopback()?;
    let token = random(entropy)?;
    let port = ports.free_loopback()?;
    let password = random(entropy)?;
    let bridge = json!({ "launchId": launch.as_str(), "port": bridge_port, "token": token });
    let env = [
        ("OPENCODE_TUI_CONFIG", settings.to_owned()),
        ("CF_OPENCODE_SESSION_BRIDGE", js::stringify(&bridge)),
        ("OPENCODE_SERVER_PASSWORD", password.clone()),
        ("OPENCODE_SERVER_USERNAME", "opencode".to_owned()),
    ];
    Ok(Launched {
        args: ["--port", &port.to_string(), "--hostname", "127.0.0.1"]
            .map(str::to_owned)
            .to_vec(),
        env: env
            .into_iter()
            .map(|(name, value)| (name.to_owned(), value))
            .collect(),
        channel: Channel {
            launch_id: launch.as_str().to_owned(),
            endpoint: format!("http://127.0.0.1:{port}"),
            password,
            bridge: Bridge {
                endpoint: format!("http://127.0.0.1:{bridge_port}"),
                token,
            },
        },
    })
}

/// 24 bytes drawn, as `randomBytes(24).toString('base64url')` writes them.
fn random(entropy: &dyn Entropy) -> Result<String, String> {
    let mut drawn = [0; 24];
    entropy.fill(&mut drawn)?;
    Ok(URL_SAFE_NO_PAD.encode(drawn))
}

#[cfg(test)]
mod tests;

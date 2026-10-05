//! What a window leaves in ConsensFlow's folder under its launch id
//! (`src/core/launch-files.js`): the settings and integration files written
//! for it, in `integrations/<harness>/<launch>`. A launch id lives as long
//! as its window, and nothing else reads these folders, so they go when it
//! does.

use cf_base::env::Env;
use cf_base::home::config_root;
use cf_base::path;
use cf_proto::agents::Harness;

use crate::contract::LaunchId;

/// The folder under `integrations` a harness keeps its launches' files in:
/// Codex is given what it runs with and keeps none.
pub(crate) fn harness_folder(harness: Harness) -> Option<&'static str> {
    match harness {
        Harness::Claude => Some("claude"),
        Harness::Pi => Some("pi"),
        Harness::Devin => Some("devin"),
        Harness::Opencode => Some("opencode"),
        Harness::Codex => None,
    }
}

/// The folder of `launch`'s files in `harness_folder`, joined as
/// `path.join` joins it, or the failure of an environment that names no
/// ConsensFlow folder (Node read the process's own home there).
pub(crate) fn launch_folder(
    harness_folder: &str,
    env: &Env,
    launch: &LaunchId,
) -> Result<String, String> {
    let root = config_root(env).ok_or_else(|| "missing home in env".to_owned())?;
    Ok(path::join(&[
        &root.to_string_lossy(),
        "integrations",
        harness_folder,
        launch.as_str(),
    ]))
}

//! ConsensFlow's plugin for OpenCode, made where OpenCode is told to load it
//! from: an immutable private copy of the plugin's files, and a `tui.json`
//! beside them that names the plugin, never an edit of OpenCode's own settings.

use cf_base::env::Env;
use cf_base::home::config_root;
use cf_base::js;
use cf_base::path::{self, to_file_url};
use cf_proto::agents::Harness;
use serde_json::json;

use crate::detect::harness_path;
use crate::shared::private_bundle::{prepare_private_integration, Made};

/// The plugin's file, and the one it imports, as the bundle is hashed and
/// published by their names in the repository, and their bytes, which this
/// build holds.
const PLUGIN: (&str, &[u8]) = (
    "hosts/opencode-extension/consensflow-session.mjs",
    include_bytes!("../../../../hosts/opencode-extension/consensflow-session.mjs"),
);
const DOOR: (&str, &[u8]) = (
    "hosts/lib/question-door.js",
    include_bytes!("../../../../hosts/lib/question-door.js"),
);

/// The settings file OpenCode is told to read its plugins from, which this
/// build writes: it is not in the repository.
const TUI: &str = "hosts/opencode-extension/tui.json";

/// What preparing the plugin came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Extension {
    /// OpenCode is not installed here: nothing is made.
    NotInstalled,
    /// The plugin's files are in place, and the settings that name it.
    /// OpenCode has not loaded it yet, so it has not been seen to work.
    InstalledUnverified {
        /// The plugin's file.
        path: String,
        /// The settings file OpenCode reads its plugins from.
        config: String,
    },
    /// Why the plugin could not be made.
    Error { reason: String },
}

/// Makes the plugin for the OpenCode `env` finds, if there is one.
pub fn prepare_extension(env: &Env) -> Extension {
    if harness_path(Harness::Opencode, env).is_none() {
        return Extension::NotInstalled;
    }
    // Kept from Node on purpose: the settings name the plugin by its file
    // URL, which a folder that is not whole has none of here. Node made it
    // whole against the working folder (`pathToFileURL`), and the window,
    // which opens in another, would then never find it.
    if let Some(config) = config_root(env) {
        let root = path::join(&[&config.to_string_lossy(), "extensions", "opencode"]);
        if plugin_url(&root).is_none() {
            return Extension::Error {
                reason: "ConsensFlow's folder is not an absolute path".to_owned(),
            };
        }
    }
    match prepare_private_integration(env, "opencode", &[PLUGIN, DOOR], &settings) {
        Ok(root) => Extension::InstalledUnverified {
            path: path::join(&[&root, PLUGIN.0]),
            config: path::join(&[&root, TUI]),
        },
        Err(reason) => Extension::Error { reason },
    }
}

/// The plugin's file URL once the bundle is in `folder`.
fn plugin_url(folder: &str) -> Option<String> {
    to_file_url(&path::join(&[folder, PLUGIN.0]), cfg!(windows))
}

/// The settings file the bundle in `folder` is given: the plugin, by its file
/// URL. It reads differently in each folder, so it is made for the folder it
/// is hashed in and for the one it is written to. The folder was checked to
/// have a file URL before this is asked.
fn settings(folder: &str) -> Vec<Made> {
    plugin_url(folder).map_or_else(Vec::new, |url| {
        let text = js::stringify(&json!({ "plugin": [url] }));
        vec![(TUI.to_owned(), text.into_bytes())]
    })
}

impl Extension {
    /// The settings file the window's TUI is told to read, or the sentence a
    /// launch is refused with.
    ///
    /// An OpenCode that is not installed has no reason to give, and Node's
    /// template said what a missing one reads as, `undefined`: a launch is
    /// refused for that only where OpenCode was uninstalled since the launch
    /// found it.
    pub(super) fn into_config(self) -> Result<String, String> {
        let refused = |reason: &str| {
            format!("ConsensFlow's OpenCode plugin could not be installed: {reason}")
        };
        match self {
            Extension::InstalledUnverified { config, .. } => Ok(config),
            Extension::NotInstalled => Err(refused("undefined")),
            Extension::Error { reason } => Err(refused(&reason)),
        }
    }
}

#[cfg(test)]
mod tests;

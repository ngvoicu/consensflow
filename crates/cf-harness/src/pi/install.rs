//! ConsensFlow's extension for Pi, made where Pi is told to load it from
//! (`preparePiExtension`, `src/pi-install.js`): an immutable private copy of
//! the extension's file, never an edit of Pi's own settings.

use cf_base::env::Env;
use cf_base::path;
use cf_proto::agents::Harness;

use crate::detect::harness_path;
use crate::shared::private_bundle::prepare_private_integration;

/// The extension's file, as the bundle is hashed and published by its name in
/// the repository, and its bytes, which this build holds.
const EXTENSION: (&str, &[u8]) = (
    "hosts/pi-extension/consensflow-delivery.mjs",
    include_bytes!("../../../../hosts/pi-extension/consensflow-delivery.mjs"),
);

/// What preparing the extension came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Extension {
    /// Pi is not installed here: nothing is made.
    NotInstalled,
    /// The extension's file is in place. Pi has not loaded it yet, so it has
    /// not been seen to work.
    InstalledUnverified { path: String },
    /// Why the extension could not be made.
    Error { reason: String },
}

/// Makes the extension for the Pi `env` finds, if there is one.
pub fn prepare_extension(env: &Env) -> Extension {
    if harness_path(Harness::Pi, env).is_none() {
        return Extension::NotInstalled;
    }
    match prepare_private_integration(env, "pi", &[EXTENSION], &|_| Vec::new()) {
        Ok(root) => Extension::InstalledUnverified {
            path: path::join(&[&root, EXTENSION.0]),
        },
        Err(reason) => Extension::Error { reason },
    }
}

impl Extension {
    /// The extension's file, or the sentence a launch is refused with.
    ///
    /// A Pi that is not installed has no reason to give, and Node's template
    /// said what a missing one reads as, `undefined`: a launch is refused for
    /// that only where Pi was uninstalled since the launch found it.
    pub(super) fn into_path(self) -> Result<String, String> {
        let refused =
            |reason: &str| format!("ConsensFlow's Pi extension could not be installed: {reason}");
        match self {
            Extension::InstalledUnverified { path } => Ok(path),
            Extension::NotInstalled => Err(refused("undefined")),
            Extension::Error { reason } => Err(refused(&reason)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_launch_is_refused_in_node_s_sentence_for_each_way_the_extension_failed() {
        let installed = Extension::InstalledUnverified {
            path: "/ext/pi.mjs".to_owned(),
        };
        assert_eq!(installed.into_path(), Ok("/ext/pi.mjs".to_owned()));
        let failed = Extension::Error {
            reason: "EACCES: permission denied".to_owned(),
        };
        assert_eq!(
            failed.into_path(),
            Err(
                "ConsensFlow's Pi extension could not be installed: EACCES: permission denied"
                    .to_owned()
            )
        );
        assert_eq!(
            Extension::NotInstalled.into_path(),
            Err("ConsensFlow's Pi extension could not be installed: undefined".to_owned())
        );
    }
}

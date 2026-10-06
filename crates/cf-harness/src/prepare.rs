//! What opening the standalone app prepares (`prepareApp`, `src/install.js`):
//! its private launcher, the terminal command, and its Pi and OpenCode
//! integrations. It is what `cf setup` does, and the one place that puts the
//! launcher beside the extensions; the harnesses' own modules stay apart.
//!
//! The launcher is made by `cf-launcher`, which knows no bundle: this is
//! given the `cf` of the bundle it runs from.

use std::path::Path;

use cf_base::env::Env;
use cf_launcher::Places;

use crate::{opencode, pi};

/// What preparing the app came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prepared {
    /// The lines `cf setup` says before the rest: the launcher could not be
    /// installed, and why. None when it was, or when a command of someone
    /// else's was there to leave alone.
    pub report: Vec<String>,
    pub pi_extension: pi::Extension,
    pub opencode_extension: opencode::Extension,
}

/// Prepares the launcher for `cf`, the native one of this bundle, and the
/// extensions of the harnesses `env` finds. A launcher that cannot be
/// installed is reported and nothing more: the integrations are prepared all
/// the same, and each says for itself whether it was.
pub fn prepare_app(env: &Env, cf: &Path) -> Prepared {
    let mut report = Vec::new();
    if let Err(message) = cf_launcher::install(env, cf, &Places::default()) {
        report.push(format!("The cf launcher could not be installed: {message}"));
    }
    Prepared {
        report,
        pi_extension: pi::prepare_extension(env),
        opencode_extension: opencode::prepare_extension(env),
    }
}

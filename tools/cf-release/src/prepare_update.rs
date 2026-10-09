//! `cf-release prepare-update`: the update feed's `latest.json` for one build,
//! after the bundle, its archive and its signature have been checked against
//! the sources. Not built yet (landing S4): `app/scripts/prepare-update.mjs`
//! does it, and the release workflow still runs that.

use std::ffi::OsString;

use cf_base::env::Env;

use crate::cli::{Command, Console, Failure};

/// `cf-release prepare-update`.
pub const COMMAND: Command = Command {
    name: "prepare-update",
    about: "Write the update feed's latest.json for one build",
    usage: "--bundle APP --archive FILE --signature FILE --notes FILE --output FILE \
            --channel alpha|stable --date RFC3339 [--repo DIR]",
    run,
};

fn run(_env: &Env, _args: &[OsString], _console: &mut Console) -> Result<(), Failure> {
    Err(Failure::Failed(
        "not built yet: the release still runs node app/scripts/prepare-update.mjs".into(),
    ))
}

//! `cf-release sign-mac`: the Mac release under the Developer ID, every
//! Mach-O signed from the inside out, the app and its DMG notarized and
//! stapled. Not built yet (landing S5): `app/scripts/sign-mac.mjs` does it, and
//! the release workflow still runs that.

use std::ffi::OsString;

use cf_base::env::Env;

use crate::cli::{Command, Console, Failure};

/// `cf-release sign-mac`.
pub const COMMAND: Command = Command {
    name: "sign-mac",
    about: "Sign, notarize and staple the Mac release under the Developer ID",
    usage: "[--bundle DIR] [--adhoc]",
    run,
};

fn run(_env: &Env, _args: &[OsString], _console: &mut Console) -> Result<(), Failure> {
    Err(Failure::Failed(
        "not built yet: the release still runs node app/scripts/sign-mac.mjs".into(),
    ))
}

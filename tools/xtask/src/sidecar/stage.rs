//! `cargo xtask stage`: everything the app needs to run on its own, put where
//! Tauri bundles it from, `app/src-tauri/resources`.
//!
//! The app is the whole installation: someone who downloads it should not then
//! have to install Node, npm, or the CLI. The bundle carries the native `cf`
//! (crates/cf) as a resource, `cli/bin/cf`: it is the daemon the app starts
//! (`cf ui`) and the command every window runs, and nothing else of the CLI
//! travels, no Node and no sources. On Windows it carries Microsoft's own
//! console host beside it, in `conpty/`.

use std::ffi::OsString;
use std::fs;
use std::io::{self, Write};
use std::path::Path;

use super::conpty::{self, Package};
use super::{build_cf, files, finish, Error, Machine, Platform, System};
use crate::context::Context;
use crate::dispatch::{Console, Failure};

pub(super) fn run(
    context: &Context,
    args: &[OsString],
    console: &mut Console,
) -> Result<i32, Failure> {
    if !args.is_empty() {
        return Err(Failure::Usage("stage takes no arguments".into()));
    }
    let mut machine = Machine { env: &context.env };
    let result = stage(context, &conpty::PINNED, &mut machine, console.out);
    finish(result, console)
}

/// Builds `cf` and stages it as the bundle's `cli/bin/cf`, and on Windows
/// stages the console host, fetched in `package`'s version, as the bundle's
/// `conpty/`. Says each file it stages that is not `cf`, then where `cf` is.
fn stage(
    context: &Context,
    package: &Package,
    system: &mut dyn System,
    out: &mut dyn Write,
) -> Result<(), Error> {
    // The copy in bin/, which is signed where macOS has it signed: the bundle's is that one.
    let cf = build_cf::build(context, false, system)?;
    let resources = context.path("app/src-tauri/resources");
    let cli = resources.join("cli");
    clear(&cli)?;
    let bin = cli.join("bin");
    fs::create_dir_all(&bin).map_err(files("make", &bin))?;
    let staged = bin.join(system.platform().cf());
    fs::copy(&cf, &staged).map_err(files("copy the cf to", &staged))?;
    // Windows: Microsoft's own console host, which the Windows bundle puts beside
    // the app (tauri.windows.conf.json) and the portable exe in its runtime.
    if system.platform() == Platform::Windows {
        let cache = context.path("app/.cache");
        conpty::prepare(package, &cache, &resources.join("conpty"), system, out)?;
    }
    writeln!(out, "cf → {}", staged.display())?;
    Ok(())
}

/// Takes away what was staged before, so that nothing of an older staging stays
/// in the bundle.
fn clear(cli: &Path) -> Result<(), Error> {
    match fs::remove_dir_all(cli) {
        Ok(()) => Ok(()),
        Err(cause) if cause.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(cause) => Err(files("remove", cli)(cause)),
    }
}

#[cfg(test)]
mod tests;

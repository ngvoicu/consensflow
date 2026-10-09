//! `cargo xtask build-cf [--offline]`: the native `cf` (crates/cf) built and put
//! in `bin/`: the checkout's, the integration suite's, and through `stage` the
//! app bundle's. The copy there is replaced, never rewritten in place: macOS can
//! kill the next run of a Mach-O changed in place, and Windows refuses to delete
//! a `cf.exe` that a question hook still runs, though it lets one be renamed
//! aside. On macOS the copy is signed ad hoc, as the bundle around it is.

use std::ffi::OsString;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use super::{files, finish, ran, Error, Machine, Platform, System};
use crate::context::Context;
use crate::dispatch::{Console, Failure};
use crate::process::Invocation;

pub(super) fn run(
    context: &Context,
    args: &[OsString],
    console: &mut Console,
) -> Result<i32, Failure> {
    let offline = parse(args)?;
    let mut machine = Machine { env: &context.env };
    let result = say(context, offline, &mut machine, console.out);
    finish(result, console)
}

/// Whether `--offline` was given, which is all `build-cf` takes.
fn parse(args: &[OsString]) -> Result<bool, Failure> {
    match args {
        [] => Ok(false),
        [flag] if flag == "--offline" => Ok(true),
        _ => {
            let given: Vec<_> = args.iter().map(|arg| arg.to_string_lossy()).collect();
            Err(Failure::Usage(format!(
                "build-cf takes --offline or nothing, not {}",
                given.join(" ")
            )))
        }
    }
}

/// Builds `cf` and says where it is.
fn say(
    context: &Context,
    offline: bool,
    system: &mut dyn System,
    out: &mut dyn Write,
) -> Result<(), Error> {
    let placed = build(context, offline, system)?;
    writeln!(out, "cf → {}", placed.display())?;
    Ok(())
}

/// Builds `cf` and puts it in `bin/`: the path it is at.
pub(super) fn build(
    context: &Context,
    offline: bool,
    system: &mut dyn System,
) -> Result<PathBuf, Error> {
    ran(system, &cargo_build(context, offline))?;
    let platform = system.platform();
    // The workspace's one build folder (.cargo/config.toml) holds every crate's output.
    let built = context
        .path("app/src-tauri/target/release")
        .join(platform.cf());
    let placed = place(&built, &context.path("bin"), platform.cf(), system)?;
    if platform == Platform::MacOs {
        ran(system, &sign(context, &placed))?;
    }
    Ok(placed)
}

/// The build, from the checkout's root. The lockfile is held to, so that a
/// build never mends it behind anyone's back.
fn cargo_build(context: &Context, offline: bool) -> Invocation {
    Invocation::new("cargo", &context.root)
        .args(["build", "--release", "--locked"])
        .args(offline.then_some("--offline"))
        .args(["-p", "cf", "--bin", "cf"])
}

/// The ad hoc signature (`-` is no identity), forced over any the copy has.
fn sign(context: &Context, cf: &Path) -> Invocation {
    Invocation::new("codesign", &context.root)
        .args(["--force", "--sign", "-"])
        .arg(cf)
}

/// Puts the `built` file in `bin` as `name`: the folder made when a clone has
/// none (git keeps no empty folder, and the `cf` is all that is put in this
/// one), the copies set aside earlier taken away, and the copy there replaced.
/// The path it is at.
fn place(built: &Path, bin: &Path, name: &str, system: &mut dyn System) -> Result<PathBuf, Error> {
    // Before anything is touched: the copy there is the only `cf` until the new one is in.
    if !built.is_file() {
        return Err(Error::NotBuilt {
            path: built.to_path_buf(),
        });
    }
    fs::create_dir_all(bin).map_err(files("make", bin))?;
    sweep(bin, name, system)?;
    let placed = bin.join(name);
    clear(&placed, name, system)?;
    fs::copy(built, &placed).map_err(files("copy the cf to", &placed))?;
    Ok(placed)
}

/// Takes away the copies set aside earlier, now that nothing runs them any
/// more. One that is still run stays for the next time.
fn sweep(bin: &Path, name: &str, system: &mut dyn System) -> Result<(), Error> {
    let prefix = format!("{name}.old-");
    for entry in fs::read_dir(bin).map_err(files("list", bin))?.flatten() {
        if entry.file_name().to_string_lossy().starts_with(&prefix) {
            let _ = system.remove_file(&entry.path());
        }
    }
    Ok(())
}

/// Makes room for the new copy: the one there deleted or, when it cannot be
/// (Windows refuses while a program runs from it), renamed aside, which Windows
/// allows.
fn clear(placed: &Path, name: &str, system: &mut dyn System) -> Result<(), Error> {
    match system.remove_file(placed) {
        Ok(()) => Ok(()),
        Err(cause) if cause.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(_) => {
            let aside = placed.with_file_name(format!("{name}.old-{}", system.now_ms()));
            fs::rename(placed, &aside).map_err(files("set aside", placed))
        }
    }
}

#[cfg(test)]
mod tests;

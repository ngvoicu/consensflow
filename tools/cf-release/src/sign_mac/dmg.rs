//! The DMG Tauri built, made again around the signed app.

use std::ffi::OsString;
use std::fs;
use std::path::Path;
use std::time::Duration;

use cf_base::file;

use super::name;
use super::tools::{args, fail, Tools};
use crate::cli::Failure;

/// How a volume is detached, one try after another: Spotlight can hold a fresh
/// volume busy for a moment, and the last try does not ask.
const DETACHES: [&[&str]; 4] = [&[], &[], &[], &["-force"]];

/// How long to wait before trying to detach again.
const SETTLE: Duration = Duration::from_secs(2);

/// Makes the DMG Tauri built again around the signed `app`: the same volume, its
/// name, icon and Applications link, with the app in it replaced.
pub(super) fn rebuild(
    tools: &mut Tools,
    dmg: &Path,
    app: &Path,
    scratch: &Path,
) -> Result<(), Failure> {
    let writable = scratch.join("writable.dmg");
    let volume = scratch.join("volume");
    fs::create_dir(&volume)
        .map_err(|cause| fail(format!("could not make {}: {cause}", volume.display())))?;
    tools.run(
        "hdiutil",
        args!["convert", dmg, "-format", "UDRW", "-ov", "-o", &writable],
    )?;
    tools.run(
        "hdiutil",
        args![
            "attach",
            &writable,
            "-readwrite",
            "-noverify",
            "-noautoopen",
            "-nobrowse",
            "-mountpoint",
            &volume,
        ],
    )?;
    let replaced = replace(tools, &volume, app);
    let detached = detach(tools, &volume);
    after(replaced, detached)?;
    tools.run(
        "hdiutil",
        args![
            "convert",
            &writable,
            "-format",
            "UDZO",
            "-imagekey",
            "zlib-level=9",
            "-ov",
            "-o",
            dmg,
        ],
    )?;
    Ok(())
}

/// Puts `app` in the mounted `volume` in place of the one there.
fn replace(tools: &Tools, volume: &Path, app: &Path) -> Result<(), Failure> {
    let removing = |path: &Path| file::remove_all(path).map_err(|cause| fail(cause.to_string()));
    let inside = volume.join(name(app));
    removing(&inside)?;
    tools.run("ditto", args![app, &inside])?;
    // As Tauri leaves its volume: nothing in it writable by others, no event log.
    tools.run("chmod", args!["-R", "go-w", &inside])?;
    removing(&volume.join(".fseventsd"))
}

/// Detaches `volume`.
fn detach(tools: &Tools, volume: &Path) -> Result<(), Failure> {
    for (attempt, options) in DETACHES.into_iter().enumerate() {
        if attempt > 0 {
            tools.wait(SETTLE);
        }
        let mut words = args!["detach", volume];
        words.extend(options.iter().map(OsString::from));
        if tools.capture("hdiutil", words)?.code == 0 {
            return Ok(());
        }
    }
    Err(fail(format!(
        "hdiutil detach failed: {} stays mounted",
        volume.display()
    )))
}

/// How `first` came out, and then `then` after it: the first failure, with the
/// second told after it when there are two, so that neither hides the other.
fn after(first: Result<(), Failure>, then: Result<(), Failure>) -> Result<(), Failure> {
    match (first, then) {
        (Err(Failure::Failed(first)), Err(Failure::Failed(then))) => {
            Err(fail(format!("{first}; and {then}")))
        }
        (Err(first), _) => Err(first),
        (Ok(()), then) => then,
    }
}

//! The app an update installed, started as this one ends. Tauri's own restart starts it with a
//! bare `Command`, from whichever thread ends the app, while its other threads open sockets and
//! pipes: on macOS a program started then holds what it was left for as long as it runs, and the
//! app started again runs as long as the human keeps it. So the app ends as any app does, and
//! this starts the new one at its exit, the way Tauri would, holding only its streams.
//!
//! An app whose event loop is already gone when it is asked to end (Tauri ends it there and then)
//! is not started again: the human reopens it.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{AppHandle, Runtime};

use crate::update_install::{plist_value, Tools};

/// Whether the app is to be started again as it ends.
static ASKED: AtomicBool = AtomicBool::new(false);

/// Ends the app, to be started again at its exit ([`if_asked`]).
pub(crate) fn ask<R: Runtime>(app: &AppHandle<R>) {
    ASKED.store(true, Ordering::Release);
    app.exit(0);
}

/// At the app's exit: starts the app again, if it was asked to.
pub(crate) fn if_asked() {
    if !ASKED.load(Ordering::Acquire) {
        return;
    }
    let started = std::env::current_exe().and_then(|exe| {
        replacement(&exe, std::env::args_os().skip(1))
            .spawn()
            .map(drop)
    });
    if let Err(error) = started {
        eprintln!("ConsensFlow was updated but could not start again ({error}): reopen it.");
    }
}

/// The command that starts the app installed where `exe` runs from, with `args`: the executable
/// the bundle's Info.plist names now (an update may have named it otherwise), or `exe` itself
/// outside a bundle, as Tauri's restart has it.
fn replacement(exe: &Path, args: impl IntoIterator<Item = OsString>) -> Command {
    let mut command = Command::new(bundled(exe).unwrap_or_else(|| exe.to_path_buf()));
    command.args(args);
    cf_process::hold_only_the_streams(&mut command);
    command
}

/// The executable the bundle `exe` runs from names, when `exe` is in a bundle's `Contents/MacOS`.
fn bundled(exe: &Path) -> Option<PathBuf> {
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    if macos.file_name()? != "MacOS" || contents.file_name()? != "Contents" {
        return None;
    }
    let name = plist_value(&Tools::system(), contents.parent()?, "CFBundleExecutable").ok()?;
    Some(macos.join(name))
}

#[cfg(test)]
mod tests;

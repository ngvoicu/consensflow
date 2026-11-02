//! The program that stands for the launcher in the user's terminal on Windows.
//!
//! The launcher of every build before the shim was a `.cmd`, and cmd.exe ends a
//! command at the first line break of an argument: a PowerShell user who typed
//! `cf ask "…"` with a text of many lines sent its first line, and `cf` said it
//! was asked. Nothing a batch file does can see the rest. A real `cf.exe` has no
//! such limit, and PowerShell 5.1, PowerShell 7 and Git Bash give it an
//! argument whole.
//!
//! So the command on the user's `PATH` is a program, and it is the shim
//! ([`crate::shim`]): it reads the launch file beside it (`cf.launch` for
//! `cf.exe`, the text the `.cmd` held) and runs the `cf` that names, with every
//! argument as it was given and the home it pins. It is small, it is the same in
//! every release, and it names no version, so an update of the app changes the
//! text and never the program: the shim of the home's `bin` is a copy of the one
//! the bundle carries beside its `cf` ([`BUNDLED`]), and it is replaced only
//! when a release changes the shim.
//!
//! A copy of the bundle's `cf` itself would be a program of 20 MB for each of
//! the two names, stale after every update the app could not replace it in, and
//! one that finds its bundle by its own folder.

use std::fs;
use std::path::{Path, PathBuf};

use cf_base::file::{read_file_sync, write_file, FileError};

/// The shim as the bundle carries it: beside the `cf` it is for.
pub(crate) const BUNDLED: &str = "cf-shim.exe";

/// Where the bundle of `cf` keeps its shim.
fn bundled(cf: &Path) -> PathBuf {
    cf.with_file_name(BUNDLED)
}

/// The shim the bundle of `cf` carries, or the sentence that says it carries
/// none: a launcher without its program is no command at all.
pub(crate) fn read_bundled(cf: &Path) -> Result<Vec<u8>, String> {
    let shim = bundled(cf);
    read_file_sync(&shim).map_err(|failed| {
        format!(
            "the program for the terminal's cf is missing beside {}: {failed}",
            cf.display()
        )
    })
}

/// Whether the program at `program` is the shim `bytes`.
pub(crate) fn is_current(program: &Path, bytes: &[u8]) -> bool {
    fs::read(program).is_ok_and(|held| held == bytes)
}

/// Makes the program at `program` the shim `bytes`, unless it is.
///
/// Never written in place: Windows refuses the write, and the removal, of a
/// program that runs, which a `cf` the user is using right now is, and lets it be
/// renamed. So the one there goes aside under a name of its own, the new one is
/// written at the name, and the copy set aside is taken away if nothing runs it.
/// One that is still run stays, and the next call takes it away.
pub(crate) fn place(program: &Path, bytes: &[u8]) -> Result<(), FileError> {
    sweep(program);
    if is_current(program, bytes) {
        return Ok(());
    }
    if program.symlink_metadata().is_ok() {
        fs::rename(program, free_aside(program))
            .map_err(|failed| FileError::call(failed, "rename", Some(program)))?;
    }
    write_file(program, bytes, 0o777)?;
    sweep(program);
    Ok(())
}

/// The name a program goes aside under: the first `<name>.old-<n>` that is not
/// taken.
fn free_aside(program: &Path) -> PathBuf {
    let name = program.file_name().unwrap_or_default().to_string_lossy();
    let mut number = 1_u32;
    loop {
        let aside = program.with_file_name(format!("{name}.old-{number}"));
        if aside.symlink_metadata().is_err() {
            return aside;
        }
        number += 1;
    }
}

/// Takes away the copies set aside beside `program`, now that nothing runs them
/// any more. One that is still run stays for the next time.
pub(crate) fn sweep(program: &Path) {
    let (Some(folder), Some(name)) = (program.parent(), program.file_name()) else {
        return;
    };
    let prefix = format!("{}.old-", name.to_string_lossy());
    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.file_name().to_string_lossy().starts_with(&prefix) {
            let _ = fs::remove_file(entry.path());
        }
    }
}

#[cfg(test)]
mod tests;

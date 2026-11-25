//! The folder of stand-ins that comes first on the `PATH` of every window the daemon opens:
//! `pkill` and `killall` on a Mac, `taskkill.exe` and `tskill.exe` on Windows, each the
//! bundle's `cf`, which answers started under those names (`cf_base::member_guard`). A
//! window's program that runs one of them finds this before the real command. The person's
//! own terminals, and ConsensFlow's own processes, never have this folder on their `PATH`.
//!
//! On a Mac a login shell runs `/usr/libexec/path_helper`, which puts the system's folders
//! before the ones the window's PATH had. `path_helper` reads those folders from
//! `$PATH_HELPER_ROOT/etc` when the variable is set, so a window is given a root of ours that
//! mirrors the system's files with the guard's folder as the first line of `etc/paths`
//! ([`mirror`]). Nothing is written in or beside the person's own startup files.

use std::io;
use std::path::{Path, PathBuf};

use cf_base::member_guard::file_names;

use crate::files::Log;
use crate::seams::WindowEnv;

/// The folder under the daemon's `home` the stand-ins are in.
pub fn folder(home: &Path) -> PathBuf {
    home.join("member-guard")
}

/// The root a window's `path_helper` reads in place of `/` (`PATH_HELPER_ROOT`).
pub fn path_helper_root(home: &Path) -> PathBuf {
    home.join("member-guard-root")
}

/// Makes the stand-ins in [`folder`] of `home`, each `cf`'s own file (a link to it on a Mac,
/// a hard link or a copy of it on Windows, where a program is found by its `.exe` in every
/// shell), as they should be now: one that is there and is `cf` is left, one that is another
/// or is gone is made again. The folder, whether or not every file could be made.
pub fn install(home: &Path, cf: &Path) -> Result<PathBuf, (PathBuf, io::Error)> {
    let folder = folder(home);
    std::fs::create_dir_all(&folder).map_err(|failed| (folder.clone(), failed))?;
    let mut first = None;
    for (file, _) in file_names() {
        if let Err(failed) = put(&folder.join(&file), cf) {
            first.get_or_insert((folder.join(&file), failed));
        }
    }
    match first {
        None => Ok(folder),
        Some(failed) => Err(failed),
    }
}

/// Makes in `root` what `path_helper` reads of the system's `etc` (`/etc`): `paths` with
/// `folder` as its first line and the system's own lines after it, `manpaths`, and the files
/// of `paths.d` and `manpaths.d`, so that it answers as it would for the system with `folder`
/// first. A file that is gone from the system's goes from the root. Every file is written
/// beside and renamed into place, so a window's shell reads a whole one, and a link at a
/// name is replaced, never written through; a link where a folder of the root belongs is
/// refused. Called at each of the daemon's starts: what the system gains while it runs is
/// taken at the next.
#[cfg(unix)]
pub fn mirror(etc: &Path, root: &Path, folder: &Path) -> io::Result<()> {
    let ours = root.join("etc");
    for dir in [root, &ours, &ours.join("paths.d"), &ours.join("manpaths.d")] {
        own_folder(dir)?;
    }
    let mut paths = folder.as_os_str().as_encoded_bytes().to_vec();
    paths.push(b'\n');
    paths.extend(read_or_nothing(&etc.join("paths"))?.unwrap_or_default());
    replace(root, &ours.join("paths"), Some(&paths))?;
    replace(
        root,
        &ours.join("manpaths"),
        read_or_nothing(&etc.join("manpaths"))?.as_deref(),
    )?;
    for name in ["paths.d", "manpaths.d"] {
        let theirs = names_in(&etc.join(name))?;
        for gone in names_in(&ours.join(name))? {
            if !theirs.contains(&gone) {
                std::fs::remove_file(ours.join(name).join(gone))?;
            }
        }
        for file in theirs {
            let text = read_or_nothing(&etc.join(name).join(&file))?;
            replace(root, &ours.join(name).join(&file), text.as_deref())?;
        }
    }
    Ok(())
}

/// The folder `dir`, made where it is not: never a link that someone put there.
#[cfg(unix)]
fn own_folder(dir: &Path) -> io::Result<()> {
    match std::fs::symlink_metadata(dir) {
        Ok(found) if found.is_dir() => Ok(()),
        Ok(_) => Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} is no folder of ConsensFlow's", dir.display()),
        )),
        Err(failed) if failed.kind() == io::ErrorKind::NotFound => std::fs::create_dir_all(dir),
        Err(failed) => Err(failed),
    }
}

#[cfg(unix)]
fn read_or_nothing(file: &Path) -> io::Result<Option<Vec<u8>>> {
    match std::fs::read(file) {
        Ok(text) => Ok(Some(text)),
        Err(failed) if failed.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(failed) => Err(failed),
    }
}

/// The names in `dir`, none where there is no such folder.
#[cfg(unix)]
fn names_in(dir: &Path) -> io::Result<Vec<std::ffi::OsString>> {
    match std::fs::read_dir(dir) {
        Ok(entries) => entries
            .map(|entry| entry.map(|entry| entry.file_name()))
            .collect(),
        Err(failed) if failed.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(failed) => Err(failed),
    }
}

/// `file` holding `text`, or gone where there is none: written in `scratch` (the root, which
/// `path_helper` lists no files of) and renamed into place.
#[cfg(unix)]
fn replace(scratch: &Path, file: &Path, text: Option<&[u8]>) -> io::Result<()> {
    let Some(text) = text else {
        return match std::fs::remove_file(file) {
            Err(failed) if failed.kind() != io::ErrorKind::NotFound => Err(failed),
            _ => Ok(()),
        };
    };
    let there = std::fs::symlink_metadata(file).is_ok_and(|found| found.is_file());
    if there && std::fs::read(file).is_ok_and(|had| had == text) {
        return Ok(());
    }
    let beside = scratch.join(format!(".writing-{}", std::process::id()));
    let _ = std::fs::remove_file(&beside);
    std::fs::write(&beside, text)?;
    std::fs::rename(&beside, file)
}

#[cfg(unix)]
fn put(file: &Path, cf: &Path) -> io::Result<()> {
    if std::fs::read_link(file).is_ok_and(|target| target == cf) {
        return Ok(());
    }
    // Only a link, or nothing: never a file somebody else put there.
    if std::fs::symlink_metadata(file).is_ok_and(|meta| !meta.file_type().is_symlink()) {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "a file that is no link of ConsensFlow's is there",
        ));
    }
    let _ = std::fs::remove_file(file);
    std::os::unix::fs::symlink(cf, file)
}

/// A stand-in is `cf`'s own file or a copy of it: it is made again when its content differs
/// from `cf`'s (an update that replaced `cf.exe` leaves the old hard link on the old file, of
/// the same size and time as it may be). One that runs cannot be removed on Windows, and can
/// be renamed: it is moved aside, and removed at a later start.
#[cfg(windows)]
fn put(file: &Path, cf: &Path) -> io::Result<()> {
    let aside = file.with_extension("old");
    let _ = std::fs::remove_file(&aside);
    if let (Ok(had), Ok(wanted)) = (std::fs::read(file), std::fs::read(cf)) {
        if had == wanted {
            return Ok(());
        }
    }
    if std::fs::remove_file(file).is_err() && file.exists() {
        std::fs::rename(file, &aside)?;
    }
    std::fs::hard_link(cf, file).or_else(|_| std::fs::copy(cf, file).map(|_| ()))
}

/// `windows` with the stand-ins of the commands a member's window may not run first on their
/// PATH, and on a Mac with the root that keeps them first in a login shell. Where either cannot
/// be made the windows open without it, and the log says why: a guard that cannot stand is no
/// reason for a board not to start.
pub fn guard(windows: WindowEnv, home: &Path, cf: &Path, log: &Log) -> WindowEnv {
    let folder = match install(home, cf) {
        Ok(folder) => folder,
        Err((file, failed)) => {
            log.error(
                &format!("the member guard's {} could not be made", file.display()),
                Some(&failed.to_string()),
            );
            return windows;
        }
    };
    let windows = windows.with_guard(&folder);
    #[cfg(target_os = "macos")]
    {
        let root = path_helper_root(home);
        match mirror(Path::new("/etc"), &root, &folder) {
            Ok(()) => return windows.with_path_helper_root(&root),
            Err(failed) => log.error(
                &format!("the member guard's {} could not be made", root.display()),
                Some(&failed.to_string()),
            ),
        }
    }
    windows
}

#[cfg(test)]
mod tests;

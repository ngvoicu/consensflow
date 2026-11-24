//! The folder of stand-ins that comes first on the `PATH` of every window the daemon opens:
//! `pkill` and `killall` on a Mac, `taskkill.exe` and `tskill.exe` on Windows, each the
//! bundle's `cf`, which answers started under those names (`cf_base::member_guard`). A
//! window's program that runs one of them finds this before the real command. The person's
//! own terminals, and ConsensFlow's own processes, never have this folder on their `PATH`.

use std::io;
use std::path::{Path, PathBuf};

use cf_base::member_guard::file_names;

use crate::files::Log;
use crate::seams::WindowEnv;

/// The folder under the daemon's `home` the stand-ins are in.
pub fn folder(home: &Path) -> PathBuf {
    home.join("member-guard")
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
    #[cfg(unix)]
    if let Err(failed) = write_zshenv(&folder) {
        first.get_or_insert((folder.join("zsh").join(".zshenv"), failed));
    }
    match first {
        None => Ok(folder),
        Some(failed) => Err(failed),
    }
}

/// `text` in single quotes, for a POSIX shell.
fn quoted(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

/// The folder zsh is given as its `ZDOTDIR` in a window: its `.zshenv` puts the person's own
/// `ZDOTDIR` back (or none), reads the person's own `.zshenv`, and then defines `pkill` and
/// `killall`, so that the person's other startup files are read from their own place and
/// `/etc/zprofile`'s `path_helper` has no PATH to reorder past them. It prints nothing.
pub fn zshenv(folder: &Path) -> String {
    let mut text = String::from(
        "# ConsensFlow's member guard: makes pkill and killall refuse in a member's window.\n\
         if [[ -n ${CONSENSFLOW_ZDOTDIR_WAS+x} ]]; then\n  ZDOTDIR=$CONSENSFLOW_ZDOTDIR_WAS\nelse\n  unset ZDOTDIR\nfi\n\
         unset CONSENSFLOW_ZDOTDIR_WAS\n\
         [[ -r ${ZDOTDIR:-$HOME}/.zshenv ]] && source \"${ZDOTDIR:-$HOME}/.zshenv\"\n",
    );
    for (file, _) in file_names() {
        text.push_str(&format!(
            "{file}() {{ {} \"$@\"; }}\n",
            quoted(&folder.join(&file).to_string_lossy())
        ));
    }
    text
}

#[cfg(unix)]
fn write_zshenv(folder: &Path) -> io::Result<()> {
    let dir = folder.join("zsh");
    std::fs::create_dir_all(&dir)?;
    let file = dir.join(".zshenv");
    let text = zshenv(folder);
    if std::fs::read_to_string(&file).is_ok_and(|had| had == text) {
        return Ok(());
    }
    std::fs::write(file, text)
}

/// What a window's environment holds for the shells' function layer, on a Mac: `ZDOTDIR` the
/// folder of the `.zshenv` above and the person's own, when there was one, in
/// `CONSENSFLOW_ZDOTDIR_WAS`; and a function for each name in both forms bash reads from the
/// environment, the system's bash 3.2 (`name`) and bash 4.3's (`BASH_FUNC_name%%`). None on
/// Windows, whose shells read the PATH as it is given.
pub fn shell_vars(folder: &Path, person_zdotdir: Option<&str>) -> Vec<(String, String)> {
    if cfg!(windows) {
        return Vec::new();
    }
    let mut vars = vec![(
        "ZDOTDIR".to_owned(),
        folder.join("zsh").to_string_lossy().into_owned(),
    )];
    if let Some(person) = person_zdotdir {
        vars.push(("CONSENSFLOW_ZDOTDIR_WAS".to_owned(), person.to_owned()));
    }
    for (file, _) in file_names() {
        let body = format!(
            "() {{ {} \"$@\"; }}",
            quoted(&folder.join(&file).to_string_lossy())
        );
        vars.push((file.clone(), body.clone()));
        vars.push((format!("BASH_FUNC_{file}%%"), body));
    }
    vars
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

#[cfg(windows)]
fn put(file: &Path, cf: &Path) -> io::Result<()> {
    let same = |a: &Path, b: &Path| match (std::fs::metadata(a), std::fs::metadata(b)) {
        (Ok(a), Ok(b)) => a.len() == b.len() && a.modified().ok() == b.modified().ok(),
        _ => false,
    };
    if same(file, cf) {
        return Ok(());
    }
    let _ = std::fs::remove_file(file);
    std::fs::hard_link(cf, file).or_else(|_| std::fs::copy(cf, file).map(|_| ()))
}

/// `windows` with the stand-ins of the commands a member's window may not run first on their
/// PATH. Where they cannot be made the windows open without them, and the log says why: a
/// guard that cannot stand is no reason for a board not to start.
pub fn guard(windows: WindowEnv, home: &Path, cf: &Path, log: &Log) -> WindowEnv {
    match install(home, cf) {
        Ok(folder) => windows.with_guard(&folder),
        Err((file, failed)) => {
            log.error(
                &format!("the member guard's {} could not be made", file.display()),
                Some(&failed.to_string()),
            );
            windows
        }
    }
}

#[cfg(test)]
mod tests;

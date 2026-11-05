//! Where a launcher is looked for and made, and what it is called there.

use std::path::{Path, PathBuf};

use cf_base::env::Env;
use cf_base::file::{make_folder, Mkdir};
use cf_base::home::config_root;
use cf_base::path;

/// The names the launcher's file goes by, the one the app is called and the
/// short one the skill teaches. The first is the one a status is asked of.
///
/// On Windows the file is `.launch`: the text the program beside it reads
/// ([`program_of`]), where every build before the shim wrote a `.cmd` for
/// cmd.exe to run ([`script_of`]), and cmd.exe ends a command at the first
/// line break of an argument.
pub(crate) fn names(windows: bool) -> [&'static str; 2] {
    if windows {
        ["consensflow.launch", "cf.launch"]
    } else {
        ["consensflow", "cf"]
    }
}

/// The names a status looks for the first launcher under, in order: the
/// launcher of this build and, on Windows, the `.cmd` of every build before the
/// shim, which is a launcher until the app repairs it ([`script_of`]).
pub(crate) fn sought(windows: bool) -> &'static [&'static str] {
    if windows {
        &["consensflow.launch", "consensflow.cmd"]
    } else {
        &["consensflow"]
    }
}

/// The program that stands for the launcher at `launch` on Windows: the shim,
/// `cf.exe` for `cf.launch`, which finds its text by its own name.
pub(crate) fn program_of(launch: &Path) -> PathBuf {
    launch.with_extension("exe")
}

/// The launch file the shim at `program` reads: `cf.launch` for `cf.exe`, found
/// by the shim's own name.
pub(crate) fn launch_of(program: &Path) -> PathBuf {
    program.with_extension("launch")
}

/// The launcher the builds before the shim wrote on Windows for `launch`:
/// `cf.cmd` for `cf.launch`, which cmd.exe cuts a many-line argument of.
pub(crate) fn script_of(launch: &Path) -> PathBuf {
    launch.with_extension("cmd")
}

/// Where a launcher is looked for and made.
///
/// By default in the `bin` of ConsensFlow's home and nowhere else: runtime
/// launchers belong to ConsensFlow's home, never to a project's or a global
/// bin, whatever `CONSENSFLOW_BIN_DIR` says. A caller may name the folders
/// itself (Node's `candidates`), and the first of them that can be written
/// takes the command.
#[derive(Debug, Clone, Default)]
pub struct Places {
    given: Option<Vec<PathBuf>>,
}

impl Places {
    /// The folders given, as they are given.
    pub fn at(folders: Vec<PathBuf>) -> Self {
        Self {
            given: Some(folders),
        }
    }

    /// The folders to look in, in order. The home's `bin` is told by the
    /// environment, and where that names no home at all (Node fell back on
    /// the system's own, which a Rust module may not ask) there is none, and
    /// the sentence says so, as the harnesses' preparation says it.
    pub(crate) fn folders(&self, env: &Env) -> Result<Vec<PathBuf>, String> {
        if let Some(given) = &self.given {
            return Ok(given.clone());
        }
        let root = config_root(env).ok_or_else(|| "missing home in env".to_owned())?;
        Ok(vec![PathBuf::from(path::join(&[
            &root.to_string_lossy(),
            "bin",
        ]))])
    }
}

/// Makes the folders that are not there, except a system's: a user-owned
/// folder is created rather than reported missing, `/usr` and `/opt` never.
/// One that cannot be made is not said here; whether any can be written is
/// asked next.
pub(crate) fn make_missing(folders: &[PathBuf]) {
    for folder in folders {
        if folder.exists() || is_system(&folder.to_string_lossy()) {
            continue;
        }
        let _ = make_folder(folder, 0o777, Mkdir::Sync);
    }
}

/// Whether `folder` is a system's, by its name: whatever begins with `/usr`
/// or `/opt`, as Node told them, with no look at what the next character is.
fn is_system(folder: &str) -> bool {
    folder.starts_with("/usr") || folder.starts_with("/opt")
}

/// Whether the system lets this user write `dir` (`accessSync(dir, W_OK)`):
/// what `access` answers, for a file as for a folder, so one that is a file
/// fails later, at the write, where its error is said.
#[cfg(unix)]
pub(crate) fn writable(dir: &Path) -> bool {
    use nix::unistd::{access, AccessFlags};
    access(dir, AccessFlags::W_OK).is_ok()
}

/// Whether libuv lets this user write `dir` on Windows: it asks the
/// attributes alone, and finds a path writable that is there and is not a
/// read-only file; a folder cannot be read-only.
#[cfg(windows)]
pub(crate) fn writable(dir: &Path) -> bool {
    use std::os::windows::fs::MetadataExt;
    const READONLY: u32 = 0x1;
    const DIRECTORY: u32 = 0x10;
    std::fs::symlink_metadata(dir).is_ok_and(|found| {
        let attributes = found.file_attributes();
        attributes & READONLY == 0 || attributes & DIRECTORY != 0
    })
}

#[cfg(test)]
mod tests;

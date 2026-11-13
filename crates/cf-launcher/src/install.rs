//! Putting the launcher where the terminal finds it, and asking whether it is
//! there.

use std::path::{Path, PathBuf};

use cf_base::env::Env;
use cf_base::file::{is_missing, read_file_sync, write_file, FileError};
use cf_base::home::config_root;
use cf_base::path;

use crate::places::{make_missing, names, program_of, script_of, sought, writable, Places};
use crate::program;
use crate::text::{is_ours, launcher, spelled};

/// The launcher found installed here: the file, the folder it is in, and
/// whether that folder is on `PATH`, which is what makes it a command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    pub path: PathBuf,
    pub dir: PathBuf,
    pub on_path: bool,
}

/// Puts the launcher for `cf`, the native one of this bundle, in the first
/// of `places` that can be written, under each of its names, and says where
/// it went.
///
/// A user-owned folder is created rather than reported missing; a system one
/// never is. Someone else's command by either name is left alone, and ours is
/// replaced: a name is ours when the file holds the mark. The launcher pins
/// ConsensFlow's home when the environment names one of its own.
///
/// On Windows a launcher is a pair, the launch file and the shim that reads it
/// (`cf.exe` beside `cf.launch`), and the shim is the copy of the one beside
/// `cf` in its bundle: no shim there, and nothing is written. A `.cmd` of ours
/// from a build before the shim is taken away, for cmd.exe would end an
/// argument at its first line break; one of someone else's is a command by that
/// name, and the name is left alone.
///
/// This is what `cf setup` asks for, the user's own act, and it names the
/// `cf` it is given wherever that is. The care [`repair`](crate::repair) takes
/// not to name a place macOS takes away is for what runs unasked.
///
/// The failure is Node's sentence: no folder that can be written, or the
/// call that failed, said as Node's error says it.
pub fn install(env: &Env, cf: &Path, places: &Places) -> Result<Option<Installed>, String> {
    let folders = places.folders(env)?;
    make_missing(&folders);
    let Some(dir) = folders.iter().find(|folder| writable(folder)) else {
        let tried: Vec<_> = folders.iter().map(|each| each.to_string_lossy()).collect();
        return Err(format!(
            "no writable directory for the command (tried {}) — create one, or add it yourself",
            tried.join(", ")
        ));
    };
    let windows = env.on_windows();
    let named = spelled(cf, windows);
    // Before anything is written: a launcher without its program is no command.
    let shim = if windows {
        Some(program::read_bundled(Path::new(&named))?)
    } else {
        None
    };
    let script = launcher(windows, &named, pinned_home(env).as_deref());
    for name in names(windows) {
        let file = PathBuf::from(path::join(&[&dir.to_string_lossy(), name]));
        // Someone else's `cf` is left alone; ours is replaced.
        let free = if windows {
            is_free(&file)?
        } else {
            !file.exists() || is_ours(&read_text(&file)?)
        };
        if !free {
            continue;
        }
        write_launcher(&file, &script, windows).map_err(|failed| failed.to_string())?;
        if let Some(shim) = &shim {
            program::place(&program_of(&file), shim).map_err(|failed| failed.to_string())?;
            // The `.cmd` of ours the shim replaces: cmd.exe would run it for the name `cf.cmd`.
            // Where there is none, there is nothing to do; one that cannot be taken away
            // leaves the old command where a shell finds it first, so the install says so.
            let cmd = script_of(&file);
            if let Err(failed) = std::fs::remove_file(&cmd) {
                if !is_missing(&failed) {
                    return Err(format!("{}: {failed}", cmd.display()));
                }
            }
        }
    }
    status(env, places)
}

/// Whether a launcher on Windows may be written at the launch file `file`:
/// nothing of someone else's is under its names. The file, the `.cmd` of a build
/// before the shim, and the program each is ours or is not there, and a program
/// is ours when the file beside it is, which is how it was put there.
fn is_free(file: &Path) -> Result<bool, String> {
    let ours = |found: &Path| -> Result<Option<bool>, String> {
        if found.exists() {
            Ok(Some(is_ours(&read_text(found)?)))
        } else {
            Ok(None)
        }
    };
    let launch = ours(file)?;
    if launch == Some(false) || ours(&script_of(file))? == Some(false) {
        return Ok(false);
    }
    Ok(launch == Some(true) || !program_of(file).exists())
}

/// Whether our launcher is installed here, where, and whether that place is
/// on `PATH`: by its first name, in each of `places` in turn (on Windows, the
/// launch file, or the `.cmd` of a build before the shim that the app has not
/// yet repaired). Only ours: a `consensflow` someone else put there is not ours
/// to report as installed.
/// A file that cannot be read is the failure, as Node's read threw it, and so
/// is an environment that names no home to look in.
pub fn status(env: &Env, places: &Places) -> Result<Option<Installed>, String> {
    Ok(found(env, places)?.map(|(installed, _)| installed))
}

/// The launcher [`status`] finds, with what it says.
pub(crate) fn found(env: &Env, places: &Places) -> Result<Option<(Installed, String)>, String> {
    for dir in places.folders(env)? {
        let dir_text = dir.to_string_lossy().into_owned();
        for name in sought(env.on_windows()) {
            let file = PathBuf::from(path::join(&[&dir_text, name]));
            if !file.exists() {
                continue;
            }
            let text = read_text(&file)?;
            if !is_ours(&text) {
                continue;
            }
            let delimiter = if cfg!(windows) { ';' } else { ':' };
            let on_path = env.os("PATH").is_some_and(|value| {
                value
                    .to_string_lossy()
                    .split(delimiter)
                    .any(|at| at == dir_text)
            });
            return Ok(Some((
                Installed {
                    path: file,
                    dir,
                    on_path,
                },
                text,
            )));
        }
    }
    Ok(None)
}

/// The home the launcher pins: ConsensFlow's own when `CONSENSFLOW_HOME`
/// names one, none otherwise.
fn pinned_home(env: &Env) -> Option<String> {
    env.path("CONSENSFLOW_HOME")?;
    config_root(env).map(|root| root.to_string_lossy().into_owned())
}

/// What the file at `path` says, read as Node read it as UTF-8.
pub(crate) fn read_text(path: &Path) -> Result<String, String> {
    read_file_sync(path)
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .map_err(|failed| failed.to_string())
}

/// Writes `script` at `file`, in place, and makes it a program the system
/// runs where there is a mode to say so: Windows runs a `.cmd` for its name.
pub(crate) fn write_launcher(file: &Path, script: &str, windows: bool) -> Result<(), FileError> {
    write_file(file, script.as_bytes(), 0o666)?;
    if windows {
        return Ok(());
    }
    make_executable(file)
}

/// `chmodSync(file, 0o755)`.
#[cfg(unix)]
fn make_executable(file: &Path) -> Result<(), FileError> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o755))
        .map_err(|failed| FileError::call(failed, "chmod", Some(file)))
}

/// A mode is Unix's: nothing to make on a system that has none.
#[cfg(not(unix))]
fn make_executable(_file: &Path) -> Result<(), FileError> {
    Ok(())
}

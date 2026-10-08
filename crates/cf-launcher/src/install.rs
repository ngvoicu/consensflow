//! Putting the launcher where the terminal finds it, and asking whether it is
//! there.

use std::path::{Path, PathBuf};

use cf_base::env::Env;
use cf_base::file::{read_file_sync, write_file, FileError};
use cf_base::home::config_root;
use cf_base::path;

use crate::places::{make_missing, names, writable, Places};
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
/// never is. Someone else's command by either name is left alone, and ours
/// is replaced: a name is ours when the file holds the mark. The launcher
/// pins ConsensFlow's home when the environment names one of its own.
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
    let script = launcher(windows, &spelled(cf, windows), pinned_home(env).as_deref());
    for name in names(windows) {
        let file = PathBuf::from(path::join(&[&dir.to_string_lossy(), name]));
        // Someone else's `cf` is left alone; ours is replaced.
        if file.exists() && !is_ours(&read_text(&file)?) {
            continue;
        }
        write_launcher(&file, &script, windows).map_err(|failed| failed.to_string())?;
    }
    status(env, places)
}

/// Whether our launcher is installed here, where, and whether that place is
/// on `PATH`: by its first name, in each of `places` in turn. Only ours: a
/// `consensflow` someone else put there is not ours to report as installed.
/// A file that cannot be read is the failure, as Node's read threw it, and so
/// is an environment that names no home to look in.
pub fn status(env: &Env, places: &Places) -> Result<Option<Installed>, String> {
    Ok(found(env, places)?.map(|(installed, _)| installed))
}

/// The launcher [`status`] finds, with what it says.
pub(crate) fn found(env: &Env, places: &Places) -> Result<Option<(Installed, String)>, String> {
    let name = names(env.on_windows())[0];
    for dir in places.folders(env)? {
        let dir_text = dir.to_string_lossy().into_owned();
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

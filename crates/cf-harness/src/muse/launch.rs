//! What a Muse window runs with: the binary its launcher names, run directly with the
//! launcher's hourly update off, and the owner's hooks with ConsensFlow's added, for
//! the one run.
//!
//! The launcher (`~/.local/bin/muse`) checks for an update each hour, downloads a new
//! `muse-bin-<version>` beside the old and flips `.muse-version`, so a window opened
//! through it may run another Muse than the one before it, mid-task. A launch runs the
//! binary `.muse-version` names (`muse-bin-1.4.13-R6996.1`) with `MUSE_NO_AUTO_UPDATE`,
//! so nothing changes under a running task: the human's own `muse` updates it. Where
//! the launcher names no binary beside it, the program found is run itself.
//!
//! `-c hooks=<json>` is taken for the run in place of the hooks of the owner's
//! `settings.json`, not added to them (Muse 1.4.13, T-787's probe: the owner's
//! SessionStart hook did not run): the launch gives Muse the owner's hooks, read and
//! never written, with ConsensFlow's after them, as Devin's config is written.

use std::fs;
use std::path::{Path, PathBuf};

use cf_base::env::Env;
use cf_base::file::{make_folder, Mkdir};
use cf_base::json::from_slice_lossy;
use cf_base::{js, path};
use serde_json::{json, Map, Value};

use super::paths;
use super::version::{self, Version};
use crate::contract::LaunchId;
use crate::shared::launch_files::launch_folder;

/// The variable that keeps a run of Muse's launcher from updating it, which a window
/// is given though it runs the binary itself: a Muse it starts may read it too.
pub(crate) const NO_UPDATE: (&str, &str) = ("MUSE_NO_AUTO_UPDATE", "1");

/// The file in a launch's folder its SessionStart hook writes each session it starts
/// in (`session_hook`).
const SAID: &str = "sessions.jsonl";

/// How long Muse lets the SessionStart hook run, in seconds.
const SESSION_HOOK_SECONDS: u32 = 10;

/// What a Muse window of a launch is given.
pub(super) struct Integration {
    /// The program the window runs: the binary its launcher names.
    pub(super) program: String,
    /// `-c hooks=<json>`: the owner's hooks and ConsensFlow's.
    pub(super) args: Vec<String>,
    pub(super) env: Vec<(String, String)>,
    /// Where the hook writes the sessions the window starts.
    pub(super) said: PathBuf,
}

/// The binary the launcher `executable` runs: `muse-bin-<version>` beside it, where
/// `.muse-version` names a version and that binary is there (`.exe` on Windows); the
/// program itself where not. A Muse older than ConsensFlow reads is refused.
pub(super) fn pinned(executable: &str, env: &Env) -> Result<String, String> {
    let launcher = Path::new(executable);
    let Some(line) = version::installed(launcher) else {
        return Ok(executable.to_owned());
    };
    if Version::of(&line).is_some_and(|found| !found.supported()) {
        return Err(version::required());
    }
    let real = really(launcher);
    let suffix = if env.on_windows() { ".exe" } else { "" };
    let binary = real.with_file_name(format!("muse-bin-{line}{suffix}"));
    Ok(if binary.is_file() {
        binary.to_string_lossy().into_owned()
    } else {
        executable.to_owned()
    })
}

/// Where `launcher` really is: the file a link names (Homebrew's and the human's own
/// links), and the path as found where it is no link, as Windows' canonical form is
/// one of its own (`\\?\C:\…`) that no window was opened on before.
fn really(launcher: &Path) -> PathBuf {
    let linked = fs::symlink_metadata(launcher).is_ok_and(|meta| meta.file_type().is_symlink());
    if linked {
        fs::canonicalize(launcher).unwrap_or_else(|_| launcher.to_owned())
    } else {
        launcher.to_owned()
    }
}

/// `value` as one word of a shell's line, as `quote` quotes it.
fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

/// The hook ConsensFlow adds for a launch's SessionStart: `cf hook muse-session` of the
/// window's `cf` (`pane_cf`, named in full, as Muse's hook shell has a `PATH` of its
/// own), with the role file and the file it writes the window's sessions in. A check
/// that keeps a launch's hooks to ConsensFlow's knows it by this.
pub(crate) fn added_hook(pane_cf: &str, role_file: &str, said: &str) -> Value {
    json!({
        "matcher": "",
        "hooks": [{
            "type": "command",
            "command": format!(
                "{} hook muse-session {} {}",
                quote(pane_cf),
                quote(role_file),
                quote(said)
            ),
            "timeout": SESSION_HOOK_SECONDS,
        }],
    })
}

/// The hooks the owner's settings give Muse, in `env`'s config: none where there is no
/// settings file, or it names none. A file that is no JSON, or hooks that are no
/// object, are refused in plain words: Muse itself would not run on them.
fn owner_hooks(env: &Env) -> Result<Map<String, Value>, String> {
    let file = paths::settings(env)?;
    let bytes = match fs::read(&file) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Map::new()),
        Err(error) => return Err(format!("Muse Code's settings cannot be read: {error}")),
    };
    let settings = from_slice_lossy(&bytes).map_err(|_| {
        "Muse Code's settings.json is not JSON, so ConsensFlow cannot add its hooks to them"
            .to_owned()
    })?;
    match settings.get("hooks") {
        None | Some(Value::Null) => Ok(Map::new()),
        Some(Value::Object(hooks)) => Ok(hooks.clone()),
        Some(_) => Err("the hooks of Muse Code's settings cannot take ConsensFlow's".to_owned()),
    }
}

/// The hooks a launch runs with: the owner's, each event's in its order, and
/// ConsensFlow's SessionStart hook after the owner's own.
pub(super) fn hooks(env: &Env, added: Value) -> Result<Value, String> {
    let mut hooks = owner_hooks(env)?;
    let mut starts = match hooks.remove("SessionStart") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(kept)) => kept,
        Some(_) => {
            return Err("the hooks of Muse Code's settings cannot take ConsensFlow's".to_owned())
        }
    };
    starts.push(added);
    hooks.insert("SessionStart".to_owned(), Value::Array(starts));
    Ok(Value::Object(hooks))
}

/// What the window of `launch` runs with, its role in `role_file`: its folder made, the
/// program it runs, and the hooks it is given.
pub(super) fn integration(
    env: &Env,
    launch: &LaunchId,
    executable: &str,
    pane_cf: &str,
    role_file: &str,
) -> Result<Integration, String> {
    let program = pinned(executable, env)?;
    let root = launch_folder("muse", env, launch)?;
    make_folder(Path::new(&root), 0o700, Mkdir::Promise).map_err(|error| error.to_string())?;
    let said = path::join(&[&root, SAID]);
    let hooks = hooks(env, added_hook(pane_cf, role_file, &said))?;
    let mut given = vec![(NO_UPDATE.0.to_owned(), NO_UPDATE.1.to_owned())];
    given.extend(super::paths::data_home_given(env).map(|real| ("XDG_DATA_HOME".to_owned(), real)));
    Ok(Integration {
        program,
        args: vec!["-c".to_owned(), format!("hooks={}", js::stringify(&hooks))],
        env: given,
        said: PathBuf::from(said),
    })
}

#[cfg(test)]
mod tests;

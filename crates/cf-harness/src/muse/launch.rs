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
use crate::launch::{Check, Refused, Said};
use crate::seams::processes::{probe, Asking, Unanswered};
use crate::shared::launch_files::launch_folder;
use crate::shared::shell::cf_hook;

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

/// What [`pinned`] came to: the program the window runs, and the check of it.
pub(crate) struct Pinned {
    /// The program a window runs: the binary the pin names, else the launcher itself.
    pub(crate) program: String,
    pub(crate) check: Check,
}

/// The program the launcher `executable` runs, and the check of that program:
/// 1. `muse-bin-<version>` beside it, where `.muse-version` names a version and that
///    binary is there (`.exe` on Windows): the window runs it. Its name is its version:
///    a Muse older than ConsensFlow reads is refused, and a newer one asks nothing.
/// 2. A launcher that is a batch file (Windows' `muse.cmd`) with no such binary: refused.
///    `cmd.exe` runs one, and takes the quotes and the `|`, `&`, `<` and `>` of the hooks'
///    JSON (`-c hooks=…`) for its own, so the window would open with no hooks of
///    ConsensFlow's, or none at all. No program is run.
/// 3. Anything else (no pin, a pin that names no version, or whose binary is absent): the
///    window runs the launcher itself, so the launcher is asked `--version`, with its
///    update and login off, at every launch: the answer is never kept, as a launcher that
///    is a script may run another Muse the next time with its own file unchanged. A
///    version older than ConsensFlow reads is refused; one it reads, or none, opens.
pub(crate) async fn pinned(asking: &Asking<'_>, executable: &str) -> Pinned {
    let launcher = Path::new(executable);
    let line = version::installed(launcher);
    let suffix = if asking.env.on_windows() { ".exe" } else { "" };
    let binary = line.as_deref().and_then(|line| {
        let binary = really(launcher).with_file_name(format!("muse-bin-{line}{suffix}"));
        binary.is_file().then_some(binary)
    });
    if let (Some(binary), Some(line)) = (binary, line) {
        let old = Version::of(&line).is_some_and(|found| !found.supported());
        return Pinned {
            program: binary.to_string_lossy().into_owned(),
            check: Check {
                verdict: if old { Err(older()) } else { Ok(()) },
                said: Said::Read(line),
            },
        };
    }
    if batch_file(launcher) {
        return Pinned {
            program: executable.to_owned(),
            check: Check {
                said: Said::NotAsked,
                verdict: Err(Refused {
                    sentence: format!(
                        "Muse Code's launcher {executable} is a batch file, which cmd.exe would run with the hooks ConsensFlow gives the window mangled, and no muse-bin-<version>.exe named by its .muse-version is beside it: reinstall Muse Code or run `muse` once to finish its update"
                    ),
                    read: false,
                }),
            },
        };
    }
    Pinned {
        program: executable.to_owned(),
        check: ask(asking, launcher).await,
    }
}

/// A Muse older than ConsensFlow reads, refused.
fn older() -> Refused {
    Refused {
        sentence: version::required(),
        read: true,
    }
}

/// What the launcher's own `--version` says, asked afresh.
async fn ask(asking: &Asking<'_>, launcher: &Path) -> Check {
    asking.probes.forget(launcher);
    let mut env = asking.env.clone();
    for (name, value) in version::QUIET {
        env = env.with_var(name, value);
    }
    let said = match probe(
        asking.probes,
        asking.processes,
        launcher,
        &["--version"],
        &env,
    )
    .await
    {
        Ok(probed) => probed.stdout,
        Err(Unanswered::Unread(why)) => {
            return Check {
                said: Said::Unanswered(why),
                verdict: Ok(()),
            }
        }
        Err(Unanswered::Failed(failed)) => {
            let why = if failed.killed {
                "it did not answer in time".to_owned()
            } else {
                failed.message
            };
            return Check {
                said: Said::Unanswered(why),
                verdict: Ok(()),
            };
        }
    };
    match version::native(&said) {
        Some(found) if !found.supported() => Check {
            said: Said::Read(said),
            verdict: Err(older()),
        },
        Some(_) => Check {
            said: Said::Read(said),
            verdict: Ok(()),
        },
        None => Check {
            said: Said::Unanswered("it named no version of Muse Code's own".to_owned()),
            verdict: Ok(()),
        },
    }
}

/// Whether `program` is one Windows runs through `cmd.exe`: a `.cmd` or `.bat` file.
fn batch_file(program: &Path) -> bool {
    program.extension().is_some_and(|extension| {
        extension.eq_ignore_ascii_case("cmd") || extension.eq_ignore_ascii_case("bat")
    })
}

/// Where `launcher` really is: the file a link names (Homebrew's and the human's own
/// links), and the path as found where it is no link, as Windows' canonical form is
/// one of its own (`\\?\C:\…`) that no window was opened on before. A launcher that is
/// a link on Windows still comes back in that form, and is the program the window is
/// spawned as: no Windows install of Muse links it today (Calliope's P3-4 of T-945).
fn really(launcher: &Path) -> PathBuf {
    let linked = fs::symlink_metadata(launcher).is_ok_and(|meta| meta.file_type().is_symlink());
    if linked {
        fs::canonicalize(launcher).unwrap_or_else(|_| launcher.to_owned())
    } else {
        launcher.to_owned()
    }
}

/// The hooks ConsensFlow adds for a launch's SessionStart, `count` of them in one group
/// of the matcher's, the group put first ([`hooks`]): each `cf hook muse-session` of the window's `cf` (`pane_cf`, named
/// in full, as Muse's hook shell has a `PATH` of its own), with the role file, the file it
/// writes the window's sessions in, and the number of the part of the role it says (0 for
/// the first; a hook may say 16,384 bytes of output and a role is longer, so each part has
/// a hook, `session_hook`). Muse runs `command` in a POSIX shell; on Windows (`windows`) it
/// runs `commandWindows` in cmd.exe, through Rust's own quoting, so that every `"` of it
/// reaches cmd.exe as `\"` and no quoted word survives (T-792's probe on zeewin2: "The
/// filename, directory name, or volume label syntax is incorrect"): there each word is a
/// path cmd.exe takes as it is, its short form where it holds a blank
/// ([`cf_process::cmd_word`]), and a launch whose paths have neither is refused in plain
/// words. A check that keeps a launch's hooks to ConsensFlow's knows it by this.
pub(crate) fn added_hooks(
    pane_cf: &str,
    role_file: &str,
    said: &str,
    windows: bool,
    count: usize,
) -> Result<Value, String> {
    let words = if windows {
        let word = |path: &str| {
            cf_process::cmd_word(Path::new(path)).ok_or_else(|| {
                format!(
                    "Muse Code runs its hooks on Windows through cmd.exe with no quoted word, and {path} holds a blank or a character cmd.exe reads as its own and has no short name (8.3 names are off for its drive): ConsensFlow cannot give the window its role there"
                )
            })
        };
        Some((word(pane_cf)?, word(role_file)?, word(said)?))
    } else {
        None
    };
    let hooks: Vec<Value> = (0..count)
        .map(|part| {
            let number = part.to_string();
            let mut hook = json!({
                "type": "command",
                "command": cf_hook(pane_cf, "muse-session", &[role_file, said, &number]),
                "timeout": SESSION_HOOK_SECONDS,
            });
            if let Some((cf, role, said)) = &words {
                hook["commandWindows"] =
                    Value::String(format!("{cf} hook muse-session {role} {said} {number}"));
            }
            hook
        })
        .collect();
    Ok(json!({ "matcher": "", "hooks": hooks }))
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
/// ConsensFlow's SessionStart hooks (`added`, one group) **before** the owner's own: ours
/// always print, so each is the block of its number whatever the owner's hooks print
/// (Muse numbers the blocks of the hooks that printed, in the order of the hooks), and a
/// hook of the owner's that prints only sometimes cannot move a part of the role to
/// another block, or leave an old one in the conversation.
pub(super) fn hooks(env: &Env, added: Value) -> Result<Value, String> {
    let mut hooks = owner_hooks(env)?;
    let mut starts = match hooks.remove("SessionStart") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(kept)) => kept,
        Some(_) => {
            return Err("the hooks of Muse Code's settings cannot take ConsensFlow's".to_owned())
        }
    };
    starts.insert(0, added);
    hooks.insert("SessionStart".to_owned(), Value::Array(starts));
    Ok(Value::Object(hooks))
}

/// What the window of `launch` runs with, its role in `role_file`, said by `count` hooks (every
/// launch gives [`super::session_hook::MOST_PARTS`]: a hook for each part of the role and, past
/// them, one that writes a filler over each block an older role may have left): its folder
/// made, the program it runs, and the hooks it is given.
pub(super) async fn integration(
    asking: &Asking<'_>,
    launch: &LaunchId,
    executable: &str,
    pane_cf: &str,
    role_file: &str,
    count: usize,
) -> Result<Integration, String> {
    let env = asking.env;
    let Pinned { program, check } = pinned(asking, executable).await;
    check.verdict.map_err(|refused| refused.sentence)?;
    let root = launch_folder("muse", env, launch)?;
    make_folder(Path::new(&root), 0o700, Mkdir::Promise).map_err(|error| error.to_string())?;
    let said = path::join(&[&root, SAID]);
    let count = count.min(super::session_hook::MOST_PARTS);
    let hooks = hooks(
        env,
        added_hooks(pane_cf, role_file, &said, env.on_windows(), count)?,
    )?;
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

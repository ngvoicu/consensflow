//! Where a command is on this machine's PATH, as Windows or Unix looks for
//! one, and whether to look as Windows does: on Windows itself, or wherever
//! the environment says it is Windows's (the tests of a Windows layout). Which
//! PATH: the one what is started runs on, or, for a harness's CLI, the one the
//! environment was given.

use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use cf_base::env::Env;

/// The extensions Windows can start a program by; a PATHEXT naming another
/// (`.JS`, `.PS1`) names nothing a process can start.
const STARTABLE: [&str; 4] = [".com", ".exe", ".bat", ".cmd"];

/// The extensions of a program Windows starts as it is: `.com` and `.exe`. A script
/// ([`STARTABLE`]'s other two) is for cmd.exe, which a window or a process that is not a
/// shell cannot host.
const PROGRAMS: [&str; 2] = [".com", ".exe"];

/// The names `command` may have on disk: with each PATHEXT extension of
/// `extensions` on Windows, as it is elsewhere.
fn candidate_names(command: &str, env: &Env, extensions: &[&str]) -> Vec<String> {
    if !env.on_windows() {
        return vec![command.to_string()];
    }
    // `??`: an empty PATHEXT names no extension, and so no program.
    env.os("PATHEXT")
        .map_or_else(
            || ".COM;.EXE;.BAT;.CMD".into(),
            |value| value.to_string_lossy(),
        )
        .split(';')
        .map(str::to_lowercase)
        .filter(|extension| extensions.contains(&extension.as_str()))
        .map(|extension| format!("{command}{extension}"))
        .collect()
}

/// Where `command` resolves on `env`'s PATH, the one what it starts runs on,
/// as an absolute path: the pane host refuses a relative one, and a PATH may
/// hold relative folders. Where a program something needs is found (the Node
/// an npm shim runs on): a daemon's has the system's folders after it.
pub fn on_path(command: &str, env: &Env) -> Option<PathBuf> {
    on(command, env.os("PATH"), env, &STARTABLE)
}

/// [`on_path`] for a program that is run as it is, not through cmd.exe: on Windows a `.exe`
/// or a `.com`, never a `.cmd` or a `.bat` that PATHEXT lists before them (the Node an npm
/// shim runs on, which a `node.cmd` first on the PATH is not).
pub fn on_path_as_program(command: &str, env: &Env) -> Option<PathBuf> {
    on(command, env.os("PATH"), env, &PROGRAMS)
}

/// Where `command` resolves on the PATH `env` was given
/// ([`Env::search_path`]), before a daemon put the system's folders after it:
/// where a harness's CLI is looked for first, the places under the home and
/// those folders coming after it (`cf-harness`'s detection, and it alone).
pub fn on_search_path(command: &str, env: &Env) -> Option<PathBuf> {
    on(command, env.search_path(), env, &STARTABLE)
}

/// Where `command` resolves on `path`, read as the system reads a PATH
/// (`split_paths`: Windows takes a folder in double quotes whole, `;` and all).
fn on(command: &str, path: Option<&OsStr>, env: &Env, extensions: &[&str]) -> Option<PathBuf> {
    let folders = std::env::split_paths(path?).filter(|folder| !folder.as_os_str().is_empty());
    find(command, folders, env, extensions)
}

/// The first of `folders` that holds `command` as a file this user can start,
/// by the names `env`'s system gives it, as an absolute path.
pub fn find_in(
    command: &str,
    folders: impl IntoIterator<Item = PathBuf>,
    env: &Env,
) -> Option<PathBuf> {
    find(command, folders, env, &STARTABLE)
}

/// [`find_in`] by the PATHEXT extensions of `extensions` alone.
fn find(
    command: &str,
    folders: impl IntoIterator<Item = PathBuf>,
    env: &Env,
    extensions: &[&str],
) -> Option<PathBuf> {
    let names = candidate_names(command, env, extensions);
    folders
        .into_iter()
        .flat_map(|folder| names.iter().map(move |name| folder.join(name)))
        .filter_map(|candidate| resolved(&candidate))
        .find(|candidate| startable(candidate))
}

/// A candidate as `path.resolve` made it before Node looked at it: whole
/// against the working folder, its `.` and `..` taken off by its text, so a
/// `..` goes back above a folder that is not there, or a link, by its name.
fn resolved(candidate: &Path) -> Option<PathBuf> {
    let whole = std::path::absolute(candidate).ok()?;
    Some(PathBuf::from(cf_base::path::join(&[
        &whole.to_string_lossy()
    ])))
}

/// Whether `file` is a file this user can start: on Unix, one the system
/// lets this user run (`access(X_OK)`), as Node asked; on Windows, any file.
fn startable(file: &Path) -> bool {
    let is_file = fs::metadata(file).is_ok_and(|metadata| metadata.is_file());
    #[cfg(unix)]
    {
        use nix::unistd::{access, AccessFlags};
        is_file && access(file, AccessFlags::X_OK).is_ok()
    }
    #[cfg(not(unix))]
    {
        is_file
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn looks_as_windows_does_on_windows_or_when_the_environment_says_so() {
        let windows = Env::from_vars([("OS", "Windows_NT"), ("PATHEXT", ".JS;.EXE;.Cmd;.PS1")]);
        assert_eq!(
            candidate_names("node", &windows, &STARTABLE),
            ["node.exe", "node.cmd"]
        );
        assert_eq!(candidate_names("node", &windows, &PROGRAMS), ["node.exe"]);
        let default = Env::from_vars([("OS", "Windows_NT")]);
        assert_eq!(
            candidate_names("node", &default, &STARTABLE),
            ["node.com", "node.exe", "node.bat", "node.cmd"]
        );
        let empty = Env::from_vars([("OS", "Windows_NT"), ("PATHEXT", "")]);
        assert_eq!(
            candidate_names("node", &empty, &STARTABLE),
            Vec::<String>::new()
        );
    }

    #[cfg(unix)]
    #[test]
    fn finds_the_first_startable_one_on_path_as_an_absolute_path() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let (plain, runs) = (dir.path().join("plain"), dir.path().join("runs"));
        fs::create_dir_all(&plain).unwrap();
        fs::create_dir_all(&runs).unwrap();
        fs::write(plain.join("node"), "").unwrap();
        fs::write(runs.join("node"), "").unwrap();
        fs::set_permissions(runs.join("node"), fs::Permissions::from_mode(0o755)).unwrap();
        let path = format!("{}::{}", plain.display(), runs.display());
        let env = Env::from_vars([("PATH", path.as_str())]);
        assert_eq!(on_path("node", &env), Some(runs.join("node")));
        assert_eq!(on_path("codex", &env), None);
        assert_eq!(on_path("node", &Env::default()), None);
    }

    /// A script is startable by PATHEXT and no program of its own: the program an npm shim
    /// runs on is a `.exe` or a `.com`, whatever a `node.cmd` or `node.bat` before it is
    /// (the review of 2026-10-21).
    #[cfg(unix)]
    #[test]
    fn a_program_is_found_as_an_exe_or_a_com_and_never_as_a_script_pathext_lists_first() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let (scripts, programs) = (dir.path().join("scripts"), dir.path().join("programs"));
        for (folder, name) in [
            (&scripts, "node.bat"),
            (&scripts, "node.cmd"),
            (&programs, "node.com"),
            (&programs, "node.exe"),
        ] {
            fs::create_dir_all(folder).unwrap();
            fs::write(folder.join(name), "").unwrap();
            fs::set_permissions(folder.join(name), fs::Permissions::from_mode(0o755)).unwrap();
        }
        let windows = |folders: &[&Path]| {
            let path = std::env::join_paths(folders).unwrap();
            Env::from_vars([
                ("OS", "Windows_NT"),
                ("PATH", path.to_str().unwrap()),
                ("PATHEXT", ".BAT;.CMD;.EXE;.COM"),
            ])
        };
        let both = windows(&[&scripts, &programs]);
        assert_eq!(on_path("node", &both), Some(scripts.join("node.bat")));
        assert_eq!(
            on_path_as_program("node", &both),
            Some(programs.join("node.exe"))
        );
        // `.com` is a program too, in PATHEXT's order.
        fs::remove_file(programs.join("node.exe")).unwrap();
        assert_eq!(
            on_path_as_program("node", &both),
            Some(programs.join("node.com"))
        );
        assert_eq!(on_path_as_program("node", &windows(&[&scripts])), None);
        // Elsewhere there is no extension: the name is the program's.
        let plain = Env::from_vars([("PATH", programs.as_os_str())]);
        assert_eq!(
            on_path_as_program("node.com", &plain),
            Some(programs.join("node.com"))
        );
    }

    /// What a daemon runs has the system's folders after its PATH. A program it
    /// needs (Node, for an npm shim) is found on that PATH, the one it runs on;
    /// a harness is looked for on the PATH it was given, so the system's folders
    /// stay the last place one is found (the reviews of 2026-10-16).
    #[cfg(unix)]
    #[test]
    fn a_program_is_found_on_the_path_it_runs_on_and_a_harness_on_the_one_given() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let (own, homebrew) = (dir.path().join("own"), dir.path().join("homebrew"));
        fs::create_dir_all(&own).unwrap();
        fs::create_dir_all(&homebrew).unwrap();
        fs::write(homebrew.join("codex"), "").unwrap();
        fs::set_permissions(homebrew.join("codex"), fs::Permissions::from_mode(0o755)).unwrap();
        let running = Env::from_vars([
            ("PATH", own.as_os_str()),
            (cf_base::env::TOOL_FOLDERS, homebrew.as_os_str()),
        ])
        .with_tool_folders_on_path();
        assert!(running
            .os("PATH")
            .is_some_and(|path| std::env::split_paths(path).any(|folder| folder == homebrew)));
        assert_eq!(on_path("codex", &running), Some(homebrew.join("codex")));
        assert_eq!(
            on_path_as_program("codex", &running),
            Some(homebrew.join("codex"))
        );
        assert_eq!(on_search_path("codex", &running), None);
    }

    /// Windows takes a PATH's folder in double quotes whole, `;` and all, and a
    /// program in it is found (the reviews of 2026-10-16: the folders were the
    /// pieces between the `;`s, quotes and all).
    #[cfg(windows)]
    #[test]
    fn a_quoted_folder_holding_a_semicolon_is_one_folder() {
        let dir = tempfile::tempdir().unwrap();
        let (pinned, other) = (dir.path().join("tools;pinned"), dir.path().join("other"));
        fs::create_dir_all(&pinned).unwrap();
        fs::create_dir_all(&other).unwrap();
        fs::write(pinned.join("pi.exe"), "").unwrap();
        let path = format!("\"{}\";{}", pinned.display(), other.display());
        let env = Env::from_vars([("PATH", path.as_str())]);
        assert_eq!(on_path("pi", &env), Some(pinned.join("pi.exe")));
        assert_eq!(on_search_path("pi", &env), Some(pinned.join("pi.exe")));
    }

    #[cfg(unix)]
    #[test]
    fn takes_a_dot_dot_off_by_its_text_before_looking_as_node_resolved_it() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bin");
        fs::create_dir_all(&bin).unwrap();
        fs::write(bin.join("claude"), "").unwrap();
        fs::set_permissions(bin.join("claude"), fs::Permissions::from_mode(0o755)).unwrap();
        // `missing` is not there: the system would not go through it.
        let path = dir.path().join("missing").join("..").join("bin");
        let env = Env::from_vars([("PATH", path.as_os_str())]);
        assert_eq!(on_path("claude", &env), Some(bin.join("claude")));
    }

    #[cfg(unix)]
    #[test]
    fn a_file_this_user_may_not_run_is_passed_over_for_the_next() {
        use nix::unistd::{access, AccessFlags};
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let (others, mine) = (dir.path().join("others"), dir.path().join("mine"));
        fs::create_dir_all(&others).unwrap();
        fs::create_dir_all(&mine).unwrap();
        // Others may run it, its owner (this user) may not.
        fs::write(others.join("pi"), "").unwrap();
        fs::set_permissions(others.join("pi"), fs::Permissions::from_mode(0o641)).unwrap();
        if access(&others.join("pi"), AccessFlags::X_OK).is_ok() {
            return; // Root may run any file with an execute bit.
        }
        fs::write(mine.join("pi"), "").unwrap();
        fs::set_permissions(mine.join("pi"), fs::Permissions::from_mode(0o700)).unwrap();
        let path = format!("{}:{}", others.display(), mine.display());
        let env = Env::from_vars([("PATH", path.as_str())]);
        assert_eq!(on_path("pi", &env), Some(mine.join("pi")));
    }
}

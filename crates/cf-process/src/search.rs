//! Where a command is on this machine's PATH, as Windows or Unix looks for
//! one, and whether to look as Windows does: on Windows itself, or wherever
//! the environment says it is Windows's (the tests of a Windows layout).

use std::fs;
use std::path::{Path, PathBuf};

use cf_base::env::Env;

/// The extensions Windows can start a program by; a PATHEXT naming another
/// (`.JS`, `.PS1`) names nothing a process can start.
const STARTABLE: [&str; 4] = [".com", ".exe", ".bat", ".cmd"];

/// The names `command` may have on disk: with each startable PATHEXT
/// extension on Windows, as it is elsewhere.
fn candidate_names(command: &str, env: &Env) -> Vec<String> {
    if !env.on_windows() {
        return vec![command.to_string()];
    }
    // `??`: an empty PATHEXT names no extension, and so no program.
    env.os("PATHEXT")
        .map_or_else(|| ".COM;.EXE;.BAT;.CMD".into(), |value| value.to_string_lossy())
        .split(';')
        .map(str::to_lowercase)
        .filter(|extension| STARTABLE.contains(&extension.as_str()))
        .map(|extension| format!("{command}{extension}"))
        .collect()
}

/// Where `command` resolves on `env`'s PATH, as an absolute path: the pane
/// host refuses a relative one, and a PATH may hold relative folders.
pub fn on_path(command: &str, env: &Env) -> Option<PathBuf> {
    let separator = if cfg!(windows) { ';' } else { ':' };
    let path = env.os("PATH")?.to_string_lossy().into_owned();
    find_in(
        command,
        path.split(separator)
            .filter(|folder| !folder.is_empty())
            .map(PathBuf::from),
        env,
    )
}

/// The first of `folders` that holds `command` as a file this user can
/// start, by the names `env`'s system gives it (`pathOnPath`,
/// `src/harnesses.js`), as an absolute path.
pub fn find_in(
    command: &str,
    folders: impl IntoIterator<Item = PathBuf>,
    env: &Env,
) -> Option<PathBuf> {
    let names = candidate_names(command, env);
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
        assert_eq!(candidate_names("node", &windows), ["node.exe", "node.cmd"]);
        let default = Env::from_vars([("OS", "Windows_NT")]);
        assert_eq!(
            candidate_names("node", &default),
            ["node.com", "node.exe", "node.bat", "node.cmd"]
        );
        let empty = Env::from_vars([("OS", "Windows_NT"), ("PATHEXT", "")]);
        assert_eq!(candidate_names("node", &empty), Vec::<String>::new());
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
